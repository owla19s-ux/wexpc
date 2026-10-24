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
    let response=client.post(chat_endpoint(&request.base_url)?).bearer_auth(key).json(&serde_json::json!({"model":request.model_id,"messages":request.messages,"stream":false})).send().map_err(|e|ServiceError::new(if e.is_timeout(){"API_TIMEOUT"}else{"NETWORK_ERROR"},"API 请求未取得完整回复，请检查网络；未自动重试"))?;
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
    parse_chat_response(&value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedToolCall {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatResponse {
    Text(String),
    ToolCalls(Vec<ProposedToolCall>),
}

pub fn decode_chat_response(value: &serde_json::Value) -> Result<ChatResponse> {
    let choice = value.get("choices").and_then(|v| v.as_array()).and_then(|v| v.first())
        .ok_or_else(|| ServiceError::new("API_RESPONSE_ERROR", "缺少回复候选"))?;
    let message = choice.get("message")
        .ok_or_else(|| ServiceError::new("API_RESPONSE_ERROR", "缺少回复消息"))?;
    if let Some(calls) = message.get("tool_calls") {
        let calls = calls.as_array().filter(|v| !v.is_empty())
            .ok_or_else(|| ServiceError::new("API_RESPONSE_ERROR", "工具调用结构无效"))?;
        let mut result = Vec::with_capacity(calls.len());
        for call in calls {
            let id = call.get("id").and_then(|v| v.as_str()).filter(|v| !v.trim().is_empty());
            let kind = call.get("type").and_then(|v| v.as_str());
            let function = call.get("function");
            let name = function.and_then(|f| f.get("name")).and_then(|v| v.as_str()).filter(|v| !v.trim().is_empty());
            let args = function.and_then(|f| f.get("arguments")).and_then(|v| v.as_str());
            if kind != Some("function") || id.is_none() || name.is_none() || args.is_none() {
                return Err(ServiceError::new("API_RESPONSE_ERROR", "工具调用字段无效"));
            }
            result.push(ProposedToolCall {
                id: id.unwrap().into(),
                name: name.unwrap().into(),
                arguments_json: args.unwrap().into(),
            });
        }
        return Ok(ChatResponse::ToolCalls(result));
    }
    let content = message.get("content").and_then(|v| v.as_str()).filter(|v| !v.is_empty())
        .ok_or_else(|| ServiceError::new("API_RESPONSE_ERROR", "服务商未返回文本回复"))?;
    Ok(ChatResponse::Text(content.into()))
}

pub fn parse_chat_response(value: &serde_json::Value) -> Result<String> {
    match decode_chat_response(value)? {
        ChatResponse::Text(content) => Ok(content),
        ChatResponse::ToolCalls(_) => Err(ServiceError::new(
            "TOOLS_NOT_CONNECTED",
            "工具调用已解析，但执行器尚未连接；没有执行任何工具",
        )),
    }
}

#[cfg(test)]
mod response_tests {
    use super::*;

    #[test]
    fn parses_ordinary_compatible_chat_reply() {
        let value = serde_json::json!({"choices":[{"message":{"role":"assistant","content":"hello"}}]});
        assert_eq!(parse_chat_response(&value).unwrap(), "hello");
    }

    #[test]
    fn tool_calls_are_not_misrepresented_as_text() {
        let value = serde_json::json!({"choices":[{"message":{
            "role":"assistant",
            "content":"I will run the tool",
            "tool_calls":[{"id":"call_1","type":"function","function":{"name":"example","arguments":"{}"}}]
        }}]});
        assert_eq!(parse_chat_response(&value).unwrap_err().code, "TOOLS_NOT_CONNECTED");
    }

    #[test]
    fn structured_tool_call_preserves_provider_arguments() {
        let value = serde_json::json!({"choices":[{"message":{
            "tool_calls":[{"id":"call_1","type":"function",
                "function":{"name":"github_read","arguments":"{\\"path\\":\\"README.md\\"}"}}]
        }}]});
        assert_eq!(decode_chat_response(&value).unwrap(),
            ChatResponse::ToolCalls(vec![ProposedToolCall {
                id: "call_1".into(),
                name: "github_read".into(),
                arguments_json: r#"{"path":"README.md"}"#.into(),
            }]));
    }

    #[test]
    fn malformed_tool_calls_are_rejected_before_dispatch() {
        for value in [
            serde_json::json!({"choices":[{"message":{"tool_calls":[]}}]}),
            serde_json::json!({"choices":[{"message":{"tool_calls":[{"id":"a","type":"function","function":{"name":"read"}}]}}]}),
            serde_json::json!({"choices":[{"message":{"tool_calls":[{"id":"","type":"function","function":{"name":"read","arguments":"{}"}}]}}]})
        ] {
            assert_eq!(decode_chat_response(&value).unwrap_err().code, "API_RESPONSE_ERROR");
        }
    }

    #[test]
    fn malformed_reply_does_not_succeed() {
        for value in [serde_json::json!({}), serde_json::json!({"choices":[]}),
            serde_json::json!({"choices":[{"message":{"content":null}}]})] {
            assert_eq!(parse_chat_response(&value).unwrap_err().code, "API_RESPONSE_ERROR");
        }
    }
}
