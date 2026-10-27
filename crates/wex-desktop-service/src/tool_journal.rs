//! Durable execution metadata. This is not replay authorization or stored tool output.
use crate::{Result, ServiceError};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
type AttemptIdentity = (String, Option<String>, Option<String>, Option<String>, Option<crate::ToolPolicySetting>);

pub struct ToolReceiptMarker {
    pub operation_id: String,
    pub attempt_id: String,
    pub state: String,
    pub content_digest: Option<String>,
    /// Stable digest of the approved operation intent, unchanged across states.
    #[serde(default)]
    pub intent_digest: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub policy: Option<crate::ToolPolicySetting>,
}
fn error() -> ServiceError {
    ServiceError::new(
        "TOOL_JOURNAL_ERROR",
        "工具执行记录不可用，请核查后再决定是否重试",
    )
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn validate(marker: &ToolReceiptMarker) -> Result<()> {
    if !valid_id(&marker.operation_id)
        || !valid_id(&marker.attempt_id)
        || !matches!(
            marker.state.as_str(),
            "in_flight" | "succeeded" | "failed" | "unknown"
        )
        || marker
            .content_digest
            .as_ref()
            .is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        || marker
            .intent_digest
            .as_ref()
            .is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        || marker
            .conversation_id
            .as_ref()
            .is_some_and(|s| !valid_id(s))
        || marker.turn_id.as_ref().is_some_and(|s| !valid_id(s))
    {
        return Err(error());
    }
    if marker.policy.as_ref().is_some_and(|p| {
        Some(p.conversation_id.as_str()) != marker.conversation_id.as_deref()
            || !valid_id(&p.capability_id)
    }) {
        return Err(error());
    }
    Ok(())
}
/// Publish a fully written immutable marker. The caller must not execute a tool
/// until its in_flight marker has been successfully published.
pub fn append_tool_marker(root: &Path, marker: &ToolReceiptMarker) -> Result<()> {
    validate(marker)?;
    fs::create_dir_all(root).map_err(|_| error())?;
    let path = root.join(format!("{}_{}.json", marker.attempt_id, marker.state));
    let bytes = serde_json::to_vec(marker).map_err(|_| error())?;
    crate::infrastructure::immutable_file::publish(&path, &bytes).map_err(|_| error())
}
/// Return one state per attempt. An unfinished attempt stays in_flight, meaning
/// execution may have happened. No reader dispatches or retries an operation.
pub fn read_tool_markers(root: &Path, conversation: &str) -> Result<Vec<ToolReceiptMarker>> {
    if !valid_id(conversation) {
        return Err(error());
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err(error()),
    };
    let mut attempts: BTreeMap<String, ToolReceiptMarker> = BTreeMap::new();
    let mut identities: BTreeMap<String, AttemptIdentity> = BTreeMap::new();
    let mut observed_in_flight = std::collections::BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|_| error())?;
        if entry.path().extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|_| error())?;
        if !metadata.is_file() || metadata.len() > 16384 {
            return Err(error());
        }
        let marker: ToolReceiptMarker =
            serde_json::from_slice(&fs::read(entry.path()).map_err(|_| error())?)
                .map_err(|_| error())?;
        validate(&marker)?;
        if entry.file_name().to_str()
            != Some(format!("{}_{}.json", marker.attempt_id, marker.state).as_str())
        {
            return Err(error());
        }
        let identity = (
            marker.operation_id.clone(),
            marker.conversation_id.clone(),
            marker.turn_id.clone(),
            marker.intent_digest.clone(),
            marker.policy.clone(),
        );
        if identities
            .insert(marker.attempt_id.clone(), identity.clone())
            .is_some_and(|previous| previous != identity)
        {
            return Err(error());
        }
        // Legacy unbound markers are retained on disk, never assigned to a conversation by guesswork.
        if marker.conversation_id.as_deref() != Some(conversation) {
            continue;
        }
        if marker.state == "in_flight" {
            observed_in_flight.insert(marker.attempt_id.clone());
        }
        if let Some(previous) = attempts.get(&marker.attempt_id) {
            if previous.operation_id != marker.operation_id
                || previous.turn_id != marker.turn_id
                || previous.policy != marker.policy
            {
                return Err(error());
            }
            if previous.state != "in_flight" && marker.state != "in_flight" && previous != &marker {
                return Err(error());
            }
            if marker.state == "in_flight" {
                continue;
            }
        }
        attempts.insert(marker.attempt_id.clone(), marker);
    }
    // A terminal marker without its pre-dispatch fence cannot prove that the
    // operation followed the journal protocol. Fail closed instead of treating
    // a lone success/failure file as trustworthy execution history.
    if attempts.values().any(|marker| {
        marker.state != "in_flight" && !observed_in_flight.contains(&marker.attempt_id)
    }) {
        return Err(error());
    }
    Ok(attempts.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(state: &str) -> ToolReceiptMarker {
        ToolReceiptMarker {
            operation_id: "op_1".into(),
            attempt_id: "attempt_1".into(),
            state: state.into(),
            content_digest: None,
            intent_digest: Some("a".repeat(64)),
            conversation_id: Some("chat_1".into()),
            turn_id: Some("turn_1".into()),
            policy: None,
        }
    }

    #[test]
    fn attempt_identity_cannot_be_reused_across_conversations() {
        let dir = tempfile::tempdir().unwrap();
        append_tool_marker(dir.path(), &marker("in_flight")).unwrap();
        let mut foreign = marker("succeeded");
        foreign.conversation_id = Some("chat_2".into());
        append_tool_marker(dir.path(), &foreign).unwrap();
        assert_eq!(
            read_tool_markers(dir.path(), "chat_1").unwrap_err().code,
            "TOOL_JOURNAL_ERROR"
        );
    }

    #[test]
    fn terminal_receipt_requires_durable_pre_dispatch_marker() {
        let dir = tempfile::tempdir().unwrap();
        append_tool_marker(dir.path(), &marker("succeeded")).unwrap();
        assert_eq!(
            read_tool_markers(dir.path(), "chat_1").unwrap_err().code,
            "TOOL_JOURNAL_ERROR"
        );
    }

    #[test]
    fn terminal_receipt_is_accepted_when_pre_dispatch_marker_exists() {
        let dir = tempfile::tempdir().unwrap();
        append_tool_marker(dir.path(), &marker("in_flight")).unwrap();
        append_tool_marker(dir.path(), &marker("succeeded")).unwrap();
        let markers = read_tool_markers(dir.path(), "chat_1").unwrap();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].state, "succeeded");
    }
}
