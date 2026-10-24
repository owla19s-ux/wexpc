//! Request encoding for OpenAI-compatible Chat Completions.
//!
//! Pure function: builds the JSON body the transport will POST. No I/O,
//! no retries, no credentials. Reserved keys cannot be overridden.

use crate::types::*;
use serde_json::{json, Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodeOptions {
    pub stream: bool,
    pub include_usage: bool,
    /// Replay the opaque continuation payload into the originating assistant message.
    pub replay_continuation: bool,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            stream: true,
            include_usage: true,
            replay_continuation: true,
        }
    }
}

pub fn encode_request(request: &ApiRequest, options: EncodeOptions) -> Result<Value, ApiError> {
    request.validate()?;
    let mut body = Map::new();
    body.insert("model".into(), Value::String(request.model.clone()));
    body.insert(
        "messages".into(),
        Value::Array(encode_messages(request, options)?),
    );
    if !request.tools.is_empty() {
        body.insert("tools".into(), Value::Array(encode_tools(&request.tools)?));
    }
    if options.stream {
        body.insert("stream".into(), Value::Bool(true));
        if options.include_usage {
            body.insert("stream_options".into(), json!({ "include_usage": true }));
        }
    }
    let parameters: Value = serde_json::from_str(&request.parameters_json)
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest, "parameters must be valid JSON"))?;
    if let Value::Object(map) = parameters {
        for (key, value) in map {
            body.insert(key, value);
        }
    }
    Ok(Value::Object(body))
}

fn encode_messages(request: &ApiRequest, options: EncodeOptions) -> Result<Vec<Value>, ApiError> {
    let mut messages: Vec<Value> = request.messages.iter().map(encode_message).collect();
    if options.replay_continuation {
        if let Some(continuation) = request.continuation.as_ref() {
            replay_continuation(request, &mut messages, continuation)?;
        }
    }
    Ok(messages)
}

fn encode_message(message: &Message) -> Value {
    let mut object = Map::new();
    object.insert("role".into(), Value::String(role_name(message.role).into()));
    match message.content.as_ref() {
        Some(content) => {
            object.insert("content".into(), Value::String(content.clone()));
        }
        None => {
            object.insert("content".into(), Value::Null);
        }
    }
    if !message.tool_calls.is_empty() {
        let calls: Vec<Value> = message
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": { "name": call.name, "arguments": call.arguments_json }
                })
            })
            .collect();
        object.insert("tool_calls".into(), Value::Array(calls));
    }
    if let Some(tool_call_id) = message.tool_call_id.as_ref() {
        object.insert("tool_call_id".into(), Value::String(tool_call_id.clone()));
    }
    Value::Object(object)
}

fn encode_tools(tools: &[ToolDefinition]) -> Result<Vec<Value>, ApiError> {
    tools
        .iter()
        .map(|tool| {
            let parameters: Value = serde_json::from_str(&tool.parameters_json).map_err(|_| {
                ApiError::new(
                    ErrorKind::InvalidRequest,
                    "tool parameters must be valid JSON",
                )
            })?;
            Ok(json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": parameters
                }
            }))
        })
        .collect()
}

fn replay_continuation(
    request: &ApiRequest,
    messages: &mut [Value],
    continuation: &Continuation,
) -> Result<(), ApiError> {
    let origin = continuation.origin();
    if origin.connection_id != request.connection_id
        || origin.conversation_id != request.conversation_id
        || origin.model != request.model
    {
        return Err(ApiError::new(
            ErrorKind::InvalidRequest,
            "continuation scope does not match this request",
        ));
    }
    let index = messages
        .iter()
        .rposition(|message| {
            message.get("role").and_then(|value| value.as_str()) == Some("assistant")
        })
        .ok_or_else(|| {
            ApiError::new(
                ErrorKind::InvalidRequest,
                "continuation requires an assistant message",
            )
        })?;
    let assistant = request.messages.get(index).ok_or_else(|| {
        ApiError::new(
            ErrorKind::InvalidRequest,
            "continuation message index out of range",
        )
    })?;
    let expected: Vec<&str> = assistant
        .tool_calls
        .iter()
        .map(|call| call.id.as_str())
        .collect();
    let actual: Vec<&str> = origin.tool_call_ids.iter().map(String::as_str).collect();
    if expected != actual {
        return Err(ApiError::new(
            ErrorKind::InvalidRequest,
            "continuation tool call ids do not match the assistant message",
        ));
    }
    let message = messages[index]
        .as_object_mut()
        .ok_or_else(|| ApiError::new(ErrorKind::Protocol, "assistant message is not an object"))?;
    message.insert(
        "reasoning_content".into(),
        Value::String(continuation.payload().to_string()),
    );
    Ok(())
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}
