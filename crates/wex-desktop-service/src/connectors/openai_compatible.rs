use crate::{PreparedChat, Result, ServiceError};
use reqwest::{blocking::Client, redirect::Policy, Url};
use std::{io::Read, time::Duration};

pub fn chat_endpoint(base: &str) -> Result<String> {
    let mut url =
        Url::parse(base).map_err(|_| ServiceError::new("INVALID_ENDPOINT", "API 地址无效"))?;
    let local = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if (url.scheme() != "https" && !(url.scheme() == "http" && local))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ServiceError::new(
            "INVALID_ENDPOINT",
            "API 使用 HTTPS（本机服务可用 HTTP），密钥单独保存",
        ));
    }
    let path = url.path().trim_end_matches('/');
    let path = if path.is_empty() {
        "/v1/chat/completions".into()
    } else if path.ends_with("/chat/completions") {
        path.into()
    } else {
        format!("{path}/chat/completions")
    };
    url.set_path(&path);
    Ok(url.to_string())
}

fn encode_prepared_chat(request: &PreparedChat) -> Result<serde_json::Value> {
    use wex_api::{ApiRequest, EncodeOptions, Message, Role};
    let messages = request.messages.iter().map(|message| {
        let role = match message.role.as_str() {
            "system" => Role::System,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            _ => return Err(ServiceError::new("INVALID_INPUT", "不支持的消息角色")),
        };
        Ok(Message { role, content: Some(message.content.clone()), tool_calls: Vec::new(), tool_call_id: None })
    }).collect::<Result<Vec<_>>>()?;
    let api_request = ApiRequest {
        request_id: request.turn_id.clone(),
        connection_id: request.connection_id.clone(),
        conversation_id: request.conversation_id.clone(),
        model: request.model_id.clone(),
        endpoint: chat_endpoint(&request.base_url)?,
        messages, tools: Vec::new(), parameters_json: "{}".into(), continuation: None,
    };
    wex_api::encode_request(&api_request, EncodeOptions { stream: false, include_usage: false, replay_continuation: false })
        .map_err(|error| ServiceError::new(error.code(), error.detail))
}

pub fn execute_chat(request: &PreparedChat, key: &str) -> Result<String> {
    if key.is_empty() {
        return Err(ServiceError::new(
            "CREDENTIAL_UNAVAILABLE",
            "API 密钥不可用",
        ));
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(20))
        .redirect(Policy::none())
        .build()
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "无法创建 API 连接"))?;
    let body = encode_prepared_chat(request)?;
    let response=client.post(chat_endpoint(&request.base_url)?).bearer_auth(key).json(&body).send().map_err(|e|ServiceError::new(if e.is_timeout(){"API_TIMEOUT"}else{"NETWORK_ERROR"},"API 请求未取得完整回复，请检查网络；未自动重试"))?;
    if !response.status().is_success() {
        return Err(ServiceError::new(
            "API_HTTP_ERROR",
            format!(
                "服务商返回 HTTP {}，请检查连接、模型和额度",
                response.status().as_u16()
            ),
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ServiceError::new("API_RESPONSE_ERROR", "无法读取完整回复"))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(ServiceError::new(
            "API_RESPONSE_ERROR",
            "服务商回复超过当前接收上限",
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| ServiceError::new("API_RESPONSE_ERROR", "服务商未返回有效 JSON"))?;
    let message = &value["choices"][0]["message"];
    if message
        .get("tool_calls")
        .and_then(|v| v.as_array())
        .is_some_and(|v| !v.is_empty())
    {
        return Err(ServiceError::new(
            "TOOLS_NOT_CONNECTED",
            "本批尚未接入工具执行，未执行服务商提出的工具调用",
        ));
    }
    message["content"]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ServiceError::new("API_RESPONSE_ERROR", "服务商未返回文本回复"))
}
