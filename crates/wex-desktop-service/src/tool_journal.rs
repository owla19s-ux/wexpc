//! Durable execution metadata. This is not replay authorization or stored tool output.
use crate::{Result, ServiceError};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolReceiptMarker {
    pub operation_id: String,
    pub attempt_id: String,
    pub state: String,
    pub content_digest: Option<String>,
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
/// Publish an immutable marker using create_new. A partial write is surfaced as
/// damaged evidence on reads; it must never be mistaken for an absent operation.
pub fn append_tool_marker(root: &Path, marker: &ToolReceiptMarker) -> Result<()> {
    validate(marker)?;
    fs::create_dir_all(root).map_err(|_| error())?;
    let path = root.join(format!("{}_{}.json", marker.attempt_id, marker.state));
    let bytes = serde_json::to_vec(marker).map_err(|_| error())?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|_| error())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| error())
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
        // Legacy unbound markers are retained on disk, never assigned to a conversation by guesswork.
        if marker.conversation_id.as_deref() != Some(conversation) {
            continue;
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
    Ok(attempts.into_values().collect())
}
