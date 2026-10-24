//! One non-streaming model step. No tool execution occurs here.
use crate::{PreparedChat, Result, ServiceError};
use reqwest::{blocking::Client, redirect::Policy};
use std::{io::Read, time::Duration};
use wex_api::{EncodeOptions, Message, Role, ToolDefinition, ToolCall};
use super::tool_proposals::{parse_tool_proposals, ToolProposal};

#[derive(Debug, Clone)]
pub enum ModelStep {
    Text(String),
    Proposal { assistant_text: Option<String>, calls: Vec<ToolProposal> },
}

/// The caller supplies only trusted tool definitions and committed prior tool
/// messages. An LLM result is inert until the Host approves and dispatches it.
pub fn execute_model_step(
    prepared: &PreparedChat,
    key: &str,
    history: Vec<Message>,
    tools: Vec<ToolDefinition>,
) -> Result<ModelStep> {
    if key.is_empty() {
        return Err(ServiceError::new("CREDENTIAL_UNAVAILABLE", "API 密钥不可用"));
    }
    let mut request = super::openai_compatible::protocol_request(prepared)?;
    if history.len() > 64 || tools.len() > 32 {
        return Err(ServiceError::new("INVALID_INPUT", "工具回合或工具清单超过上限"));
    }
    request.messages.extend(history);
    request.tools = tools;
    let mut body = wex_api::encode_request(&request, EncodeOptions {
        stream: false, include_usage: false, replay_continuation: false,
    }).map_err(|e| ServiceError::new(e.code(), e.detail))?;
    body.as_object_mut().expect("protocol JSON object").insert("stream".into(), false.into());
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(120))
        .redirect(Policy::none()).build()
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "无法创建 API 连接"))?;
    let response = client.post(request.endpoint)
        .bearer_auth(key).json(&body).send()
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "API 请求未成功；未自动重试"))?;
    if !response.status().is_success() {
        let e = wex_api::error_for_status(response.status().as_u16(), None);
        return Err(ServiceError::new(e.code(), e.detail));
    }
    let mut bytes = Vec::new();
    response.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)
        .map_err(|_| ServiceError::new("API_RESPONSE_ERROR", "读取回复失败"))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(ServiceError::new("API_RESPONSE_ERROR", "响应超出上限"));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| ServiceError::new("API_RESPONSE_ERROR", "回复不是有效 JSON"))?;
    parse_model_step(&value)
}

pub fn parse_model_step(value: &serde_json::Value) -> Result<ModelStep> {
    let choices = value["choices"].as_array()
        .filter(|v| v.len() == 1)
        .ok_or_else(|| ServiceError::new("API_RESPONSE_ERROR", "回复候选数量无效"))?;
    let choice = &choices[0];
    let msg = choice.get("message").filter(|m| m.is_object())
        .ok_or_else(|| ServiceError::new("API_RESPONSE_ERROR", "回复缺少消息"))?;
    if msg.get("function_call").is_some_and(|v| !v.is_null()) {
        return Err(ServiceError::new("TOOLS_NOT_CONNECTED", "不支持旧版函数调用"));
    }
    let calls = parse_tool_proposals(msg)?;
    let content = msg.get("content").and_then(|v| v.as_str())
        .filter(|v| !v.is_empty()).map(str::to_owned);
    match choice.get("finish_reason").and_then(|v| v.as_str()) {
        Some("tool_calls") if !calls.is_empty() => Ok(ModelStep::Proposal { assistant_text: content, calls }),
        Some("stop") if calls.is_empty() => content.map(ModelStep::Text)
            .ok_or_else(|| ServiceError::new("API_RESPONSE_ERROR", "回复文本为空")),
        Some("length") => Err(ServiceError::new("API_RESPONSE_INCOMPLETE", "回复被截断")),
        _ => Err(ServiceError::new("API_RESPONSE_ERROR", "结束状态与工具调用不匹配")),
    }
}

/// Build assistant-call + tool-result messages only from verified Host results.
pub fn committed_tool_messages(
    calls: &[ToolProposal],
    results: &[(String, String)],
    assistant_text: Option<String>,
) -> Result<Vec<Message>> {
    if calls.is_empty() || calls.len() != results.len() {
        return Err(ServiceError::new("INVALID_INPUT", "工具回执数量不匹配"));
    }
    let mut messages = vec![Message {
        role: Role::Assistant, content: assistant_text,
        tool_calls: calls.iter().map(|c| ToolCall {
            id: c.id.clone(), name: c.name.clone(),
            arguments_json: c.arguments_json.clone(),
        }).collect(),
        tool_call_id: None,
    }];
    for (call, (id, body)) in calls.iter().zip(results) {
        if &call.id != id || body.len() > 64 * 1024 {
            return Err(ServiceError::new("INVALID_INPUT", "工具回执 ID 或大小无效"));
        }
        messages.push(Message { role: Role::Tool, content: Some(body.clone()),
            tool_calls: Vec::new(), tool_call_id: Some(id.clone()) });
    }
    Ok(messages)
}

/// Drive bounded model/tool turns. The callback is owned by a trusted Host:
/// it MUST await human approval and durably settle tool evidence before
/// returning. Neither this function nor the model can authorize a tool.
pub fn drive_tool_rounds(
    prepared: &PreparedChat,
    key: &str,
    tools: Vec<ToolDefinition>,
    mut settle_approved: impl FnMut(&[ToolProposal]) -> Result<Vec<(String, String)>>,
) -> Result<String> {
    let mut transcript = Vec::new();
    for _ in 0..8 {
        match execute_model_step(prepared, key, transcript.clone(), tools.clone())? {
            ModelStep::Text(text) => return Ok(text),
            ModelStep::Proposal { assistant_text, calls } => {
                // The Host callback is the *only* path to execution.
                // Rejection or interrupted approval ends the turn here.
                let results = settle_approved(&calls)?;
                transcript.extend(committed_tool_messages(&calls, &results, assistant_text)?);
            }
        }
    }
    Err(ServiceError::new("TOOL_ROUND_LIMIT", "工具回合已达到上限，未自动继续"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn proposal_is_not_a_successful_text_reply() {
        let value = json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"x","type":"function","function":{"name":"read_file","arguments":"{}"}}]}}]});
        assert!(matches!(parse_model_step(&value).unwrap(), ModelStep::Proposal { .. }));
    }
    #[test]
    fn tool_receipt_ids_must_match_exactly() {
        let calls = vec![ToolProposal{id:"x".into(), name:"read_file".into(),arguments_json:"{}".into()}];
        assert_eq!(committed_tool_messages(&calls, &[("y".into(), "ok".into())], None).unwrap_err().code, "INVALID_INPUT");
        let msgs = committed_tool_messages(&calls, &[("x".into(), "ok".into())], None).unwrap();
        assert_eq!(msgs[1].tool_call_id.as_deref(), Some("x"));
    }
}
