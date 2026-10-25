//! One non-streaming model step. No tool execution occurs here.
use super::tool_proposals::{parse_tool_proposals, ToolProposal};
use crate::{PreparedChat, Result, ServiceError};
use reqwest::{blocking::Client, redirect::Policy};
use std::{io::Read, time::Duration};
use wex_api::{EncodeOptions, Message, Role, ToolCall, ToolDefinition};

#[derive(Debug, Clone)]
pub enum ModelStep {
    Text(String),
    Proposal {
        assistant_text: Option<String>,
        calls: Vec<ToolProposal>,
    },
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
        return Err(ServiceError::new(
            "CREDENTIAL_UNAVAILABLE",
            "API 密钥不可用",
        ));
    }
    let mut request = super::openai_compatible::protocol_request(prepared)?;
    if history.len() > 64 || tools.len() > 32 {
        return Err(ServiceError::new(
            "INVALID_INPUT",
            "工具回合或工具清单超过上限",
        ));
    }
    request.messages.extend(history);
    request.tools = tools;
    let mut body = wex_api::encode_request(
        &request,
        EncodeOptions {
            stream: false,
            include_usage: false,
            replay_continuation: false,
        },
    )
    .map_err(|e| ServiceError::new(e.code(), e.detail))?;
    body.as_object_mut()
        .expect("protocol JSON object")
        .insert("stream".into(), false.into());
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(120))
        .redirect(Policy::none())
        .build()
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "无法创建 API 连接"))?;
    let response = client
        .post(request.endpoint)
        .bearer_auth(key)
        .json(&body)
        .send()
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "API 请求未成功；未自动重试"))?;
    if !response.status().is_success() {
        let e = wex_api::error_for_status(response.status().as_u16(), None);
        return Err(ServiceError::new(e.code(), e.detail));
    }
    let mut bytes = Vec::new();
    response
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ServiceError::new("API_RESPONSE_ERROR", "读取回复失败"))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(ServiceError::new("API_RESPONSE_ERROR", "响应超出上限"));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| ServiceError::new("API_RESPONSE_ERROR", "回复不是有效 JSON"))?;
    parse_model_step(&value)
}

pub fn parse_model_step(value: &serde_json::Value) -> Result<ModelStep> {
    let choices = value["choices"]
        .as_array()
        .filter(|v| v.len() == 1)
        .ok_or_else(|| ServiceError::new("API_RESPONSE_ERROR", "回复候选数量无效"))?;
    let choice = &choices[0];
    let msg = choice
        .get("message")
        .filter(|m| m.is_object())
        .ok_or_else(|| ServiceError::new("API_RESPONSE_ERROR", "回复缺少消息"))?;
    // Never treat a non-assistant message as model text or an executable tool proposal.
    if msg.get("role").and_then(|v| v.as_str()) != Some("assistant") {
        return Err(ServiceError::new("API_RESPONSE_ERROR", "回复消息角色无效"));
    }
    if msg.get("function_call").is_some_and(|v| !v.is_null()) {
        return Err(ServiceError::new(
            "TOOLS_NOT_CONNECTED",
            "不支持旧版函数调用",
        ));
    }
    let calls = parse_tool_proposals(msg)?;
    let content = msg
        .get("content")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(str::to_owned);
    match choice.get("finish_reason").and_then(|v| v.as_str()) {
        Some("tool_calls") if !calls.is_empty() => Ok(ModelStep::Proposal {
            assistant_text: content,
            calls,
        }),
        Some("stop") if calls.is_empty() => content
            .map(ModelStep::Text)
            .ok_or_else(|| ServiceError::new("API_RESPONSE_ERROR", "回复文本为空")),
        Some("length") => Err(ServiceError::new("API_RESPONSE_INCOMPLETE", "回复被截断")),
        _ => Err(ServiceError::new(
            "API_RESPONSE_ERROR",
            "结束状态与工具调用不匹配",
        )),
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
        role: Role::Assistant,
        content: assistant_text,
        tool_calls: calls
            .iter()
            .map(|c| ToolCall {
                id: c.id.clone(),
                name: c.name.clone(),
                arguments_json: c.arguments_json.clone(),
            })
            .collect(),
        tool_call_id: None,
    }];
    for (call, (id, body)) in calls.iter().zip(results) {
        if &call.id != id || body.len() > 512 * 1024 {
            return Err(ServiceError::new("INVALID_INPUT", "工具回执 ID 或大小无效"));
        }
        messages.push(Message {
            role: Role::Tool,
            content: Some(body.clone()),
            tool_calls: Vec::new(),
            tool_call_id: Some(id.clone()),
        });
    }
    Ok(messages)
}

const MAX_MODEL_STEPS: usize = 8;

fn ensure_followup_budget(model_step_index: usize) -> Result<()> {
    if model_step_index >= MAX_MODEL_STEPS - 1 {
        return Err(ServiceError::new(
            "TOOL_ROUND_LIMIT",
            "工具回合已达到上限；本轮未派发新的工具操作",
        ));
    }
    Ok(())
}

/// Drive bounded model/tool turns. The callback is owned by a trusted Host:
/// it MUST await human approval and durably settle tool evidence before
/// returning. Neither this function nor the model can authorize a tool.
pub fn drive_tool_rounds(
    prepared: &PreparedChat,
    key: &str,
    tools: Vec<ToolDefinition>,
    settle_approved: impl FnMut(&[ToolProposal]) -> Result<Vec<(String, String)>>,
) -> Result<String> {
    drive_tool_rounds_with(
        |history| execute_model_step(prepared, key, history, tools.clone()),
        settle_approved,
    )
}

// Keep the same dispatch path injectable for deterministic, network-free
// tests. Production always supplies execute_model_step as the model source.
fn drive_tool_rounds_with(
    mut next_model_step: impl FnMut(Vec<Message>) -> Result<ModelStep>,
    mut settle_approved: impl FnMut(&[ToolProposal]) -> Result<Vec<(String, String)>>,
) -> Result<String> {
    let mut transcript = Vec::new();
    for model_step_index in 0..MAX_MODEL_STEPS {
        match next_model_step(transcript.clone())? {
            ModelStep::Text(text) => return Ok(text),
            ModelStep::Proposal {
                assistant_text,
                calls,
            } => {
                // A tool dispatch must leave a model request to consume its receipt.
                // Never execute the final step's proposal and then report a bare limit error.
                ensure_followup_budget(model_step_index)?;
                // Cap the batch BEFORE any potentially irreversible dispatch.
                if calls.len() > 4 {
                    return Err(ServiceError::new(
                        "TOOL_BATCH_LIMIT",
                        "一次工具回合最多执行四项操作",
                    ));
                }
                // The Host callback is the *only* path to execution.
                // Rejection or interrupted approval ends the turn here.
                let results = settle_approved(&calls)?;
                transcript.extend(committed_tool_messages(&calls, &results, assistant_text)?);
            }
        }
    }
    Err(ServiceError::new(
        "TOOL_ROUND_LIMIT",
        "工具回合已达到上限，未自动继续",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn proposal_is_not_a_successful_text_reply() {
        let value = json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","tool_calls":[{"id":"x","type":"function","function":{"name":"read_file","arguments":"{}"}}]}}]});
        assert!(matches!(
            parse_model_step(&value).unwrap(),
            ModelStep::Proposal { .. }
        ));
    }
    #[test]
    fn rejects_non_assistant_tool_proposals_and_text_replies() {
        let tool = json!({"id":"call_x","type":"function","function":{"name":"read_file","arguments":"{}"}});
        for role in [Some("user"), Some("tool"), None] {
            let mut message = json!({"tool_calls":[tool.clone()]});
            if let Some(role) = role {
                message["role"] = json!(role);
            }
            let value = json!({"choices":[{"finish_reason":"tool_calls","message":message}]});
            assert_eq!(
                parse_model_step(&value).unwrap_err().code,
                "API_RESPONSE_ERROR"
            );
        }
        for message in [
            json!({"role":"user","content":"spoofed"}),
            json!({"content":"missing role"}),
        ] {
            let value = json!({"choices":[{"finish_reason":"stop","message":message}]});
            assert_eq!(
                parse_model_step(&value).unwrap_err().code,
                "API_RESPONSE_ERROR"
            );
        }
        let valid = json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"ok"}}]});
        assert!(matches!(parse_model_step(&valid), Ok(ModelStep::Text(text)) if text == "ok"));
    }

    #[test]
    fn escaped_tool_output_can_exceed_plain_file_byte_limit() {
        let calls = vec![ToolProposal {
            id: "a".into(),
            name: "github_read_file".into(),
            arguments_json: "{}".into(),
        }];
        let output = serde_json::json!({"content": "\\n".repeat(40_000)}).to_string();
        assert!(output.len() > 64 * 1024);
        assert!(output.len() < 512 * 1024);
        let result = committed_tool_messages(&calls, &[("a".into(), output)], None);
        assert!(result.is_ok());
        let oversized = "a".repeat(512 * 1024 + 1);
        assert_eq!(
            committed_tool_messages(&calls, &[("a".into(), oversized)], None)
                .unwrap_err()
                .code,
            "INVALID_INPUT"
        );
    }

    #[test]
    fn failed_test_receipt_is_preserved_for_model_continuation() {
        let calls = vec![ToolProposal {
            id: "cargo_1".into(),
            name: "cargo_test".into(),
            arguments_json: "{}".into(),
        }];
        let payload = serde_json::json!({
            "evidence_ref": "ev_failed",
            "success": false,
            "exit_code": 101,
            "output": "test failed"
        })
        .to_string();
        let messages =
            committed_tool_messages(&calls, &[("cargo_1".into(), payload.clone())], None).unwrap();
        assert_eq!(messages[0].role, Role::Assistant);
        assert_eq!(messages[1].role, Role::Tool);
        assert_eq!(messages[1].content.as_deref(), Some(payload.as_str()));
        assert_eq!(messages[1].tool_call_id.as_deref(), Some("cargo_1"));
    }

    #[test]
    fn final_model_step_cannot_dispatch_without_a_followup() {
        for step in 0..MAX_MODEL_STEPS - 1 {
            assert!(ensure_followup_budget(step).is_ok());
        }
        assert_eq!(
            ensure_followup_budget(MAX_MODEL_STEPS - 1)
                .unwrap_err()
                .code,
            "TOOL_ROUND_LIMIT"
        );
    }

    #[test]
    fn tool_receipt_ids_must_match_exactly() {
        let calls = vec![ToolProposal {
            id: "x".into(),
            name: "read_file".into(),
            arguments_json: "{}".into(),
        }];
        assert_eq!(
            committed_tool_messages(&calls, &[("y".into(), "ok".into())], None)
                .unwrap_err()
                .code,
            "INVALID_INPUT"
        );
        let msgs = committed_tool_messages(&calls, &[("x".into(), "ok".into())], None).unwrap();
        assert_eq!(msgs[1].tool_call_id.as_deref(), Some("x"));
    }
    #[test]
    fn final_step_proposal_does_not_reach_host_dispatch() {
        let mut model_calls = 0;
        let mut dispatches = 0;
        let result = drive_tool_rounds_with(
            |history| {
                assert_eq!(history.len(), model_calls * 2);
                model_calls += 1;
                Ok(ModelStep::Proposal {
                    assistant_text: None,
                    calls: vec![ToolProposal {
                        id: format!("call_{model_calls}"),
                        name: "read_file".into(),
                        arguments_json: "{}".into(),
                    }],
                })
            },
            |calls| {
                dispatches += 1;
                Ok(vec![(calls[0].id.clone(), "ok".into())])
            },
        );
        assert_eq!(result.unwrap_err().code, "TOOL_ROUND_LIMIT");
        assert_eq!(model_calls, MAX_MODEL_STEPS);
        assert_eq!(dispatches, MAX_MODEL_STEPS - 1);
    }

    #[test]
    fn final_step_text_consumes_prior_receipts_and_completes() {
        let mut model_calls = 0;
        let mut dispatches = 0;
        let result = drive_tool_rounds_with(
            |history| {
                model_calls += 1;
                if model_calls == MAX_MODEL_STEPS {
                    assert_eq!(history.len(), (MAX_MODEL_STEPS - 1) * 2);
                    assert_eq!(history.last().unwrap().role, Role::Tool);
                    assert_eq!(
                        history.last().unwrap().tool_call_id.as_deref(),
                        Some("call_7")
                    );
                    return Ok(ModelStep::Text("completed".into()));
                }
                Ok(ModelStep::Proposal {
                    assistant_text: None,
                    calls: vec![ToolProposal {
                        id: format!("call_{model_calls}"),
                        name: "read_file".into(),
                        arguments_json: "{}".into(),
                    }],
                })
            },
            |calls| {
                dispatches += 1;
                Ok(vec![(calls[0].id.clone(), "ok".into())])
            },
        );
        assert_eq!(result.unwrap(), "completed");
        assert_eq!(model_calls, MAX_MODEL_STEPS);
        assert_eq!(dispatches, MAX_MODEL_STEPS - 1);
    }
}
