use crate::{Result, ServiceError};
use serde_json::Value;
use std::collections::HashSet;

/// An untrusted model proposal. This is never an approval or execution receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolProposal {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

pub fn parse_tool_proposals(message: &Value) -> Result<Vec<ToolProposal>> {
    let Some(value) = message.get("tool_calls").filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let items = value.as_array().ok_or_else(invalid)?;
    if items.len() > 32 {
        return Err(invalid());
    }
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let id = item.get("id").and_then(Value::as_str).ok_or_else(invalid)?;
        let name = item.pointer("/function/name").and_then(Value::as_str).ok_or_else(invalid)?;
        let args = item.pointer("/function/arguments").and_then(Value::as_str).ok_or_else(invalid)?;
        if item.get("type").and_then(Value::as_str) != Some("function")
            || id.is_empty() || id.len() > 128
            || name.is_empty() || name.len() > 128
            || args.len() > 1024 * 1024
            || !seen.insert(id)
            || !serde_json::from_str::<Value>(args).is_ok_and(|v| v.is_object())
        {
            return Err(invalid());
        }
        out.push(ToolProposal {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments_json: args.to_owned(),
        });
    }
    Ok(out)
}

fn invalid() -> ServiceError {
    ServiceError::new("API_RESPONSE_ERROR", "工具调用结构、数量或参数无效")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_complete_calls_without_executing_them() {
        let input = json!({"tool_calls":[{"id":"one","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a.txt\"}"}}]});
        let calls = parse_tool_proposals(&input).unwrap();
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments_json, "{\"path\":\"a.txt\"}");
    }

    #[test]
    fn rejects_duplicate_ids_and_invalid_json() {
        let call = json!({"id":"one","type":"function","function":{"name":"read_file","arguments":"{}"}});
        assert!(parse_tool_proposals(&json!({"tool_calls":[call.clone(), call]})).is_err());
        assert!(parse_tool_proposals(&json!({"tool_calls":[{"id":"x","type":"function","function":{"name":"read_file","arguments":"[]"}}]})).is_err());
    }
}
