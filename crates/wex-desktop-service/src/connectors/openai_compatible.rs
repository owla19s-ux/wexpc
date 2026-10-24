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


fn protocol_error(error: wex_api::ApiError) -> ServiceError {
    ServiceError::new(error.code(), error.detail)
}

/// Streaming transport using the same credential and endpoint safeguards as
/// non-streaming chat. Tool calls are rejected until dispatch is connected.
pub fn execute_chat_stream(request: &PreparedChat, key: &str) -> Result<String> {
    use wex_api::{ApiRequest, Decoder, EncodeOptions, Message, RequestOptions, Role};
    if key.is_empty() {
        return Err(ServiceError::new("CREDENTIAL_UNAVAILABLE", "API 密钥不可用"));
    }
    let endpoint = chat_endpoint(&request.base_url)?;
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
        endpoint: endpoint.clone(),
        messages,
        tools: Vec::new(),
        parameters_json: "{}".into(),
        continuation: None,
    };
    let body = wex_api::encode_request(&api_request, EncodeOptions::default()).map_err(protocol_error)?;
    let options = RequestOptions::default();
    let client = Client::builder()
        .timeout(options.total_timeout)
        .connect_timeout(options.connect_timeout)
        .redirect(Policy::none())
        .build()
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "无法创建 API 连接"))?;
    let mut response = client.post(endpoint).bearer_auth(key).json(&body).send()
        .map_err(|error| ServiceError::new(
            if error.is_timeout() { "API_TIMEOUT" } else { "NETWORK_ERROR" },
            "API 请求失败；未自动重试"
        ))?;
    if !response.status().is_success() {
        return Err(protocol_error(wex_api::error_for_status(response.status().as_u16(), None)));
    }
    let is_sse = response.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"));
    if !is_sse {
        return Err(ServiceError::new("API_RESPONSE_ERROR", "流式请求未返回 SSE 内容类型"));
    }
    let mut decoder = Decoder::new(options);
    let mut chunk = [0u8; 8192];
    loop {
        let size = response.read(&mut chunk).map_err(|_| {
            ServiceError::new("API_RESPONSE_ERROR", "流式响应读取失败；未自动重试")
        })?;
        if size == 0 { break; }
        decoder.push(&chunk[..size]).map_err(protocol_error)?;
    }
    let completion = decoder.finish().map_err(protocol_error)?;
    if !completion.tool_calls.is_empty() {
        return Err(ServiceError::new("TOOLS_NOT_CONNECTED", "工具执行尚未接通；没有执行工具调用"));
    }
    if completion.continuation.is_some() {
        return Err(ServiceError::new("CONTINUATION_NOT_CONNECTED", "续传状态尚未接入应用层"));
    }
    if completion.text.is_empty() {
        return Err(ServiceError::new("API_RESPONSE_ERROR", "服务商未返回文本回复"));
    }
    Ok(completion.text)
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
        let status = response.status().as_u16();
        let mapped = wex_api::error_for_status(status, None);
        return Err(ServiceError::new("API_HTTP_ERROR", format!("{}: {}", mapped.code(), mapped.detail)));
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
