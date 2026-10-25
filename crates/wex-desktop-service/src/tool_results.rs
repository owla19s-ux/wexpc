//! User-visible local tool results, separate from ordinary logs and credentials.
//! Reading saved data never authorizes or executes a tool.
use crate::{Result, ServiceError, ToolProposal};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
const MAX_RECORD_BYTES: u64 = 16 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedToolResult {
    /// Durable publication order within one turn; absent in legacy records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    pub record_id: String,
    pub conversation_id: String,
    pub turn_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub arguments_json: String,
    pub result: String,
}
fn error() -> ServiceError {
    ServiceError::new(
        "TOOL_RESULT_STORAGE_ERROR",
        "工具可能已执行，但回执正文保存或读取失败；请核查执行记录，不要直接重试工具",
    )
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn location(root: &Path, conversation: &str, turn: &str) -> Result<PathBuf> {
    if !valid_id(conversation) || !valid_id(turn) {
        return Err(error());
    }
    Ok(root.join(conversation).join(turn))
}
fn validate(record: &SavedToolResult) -> Result<()> {
    if !valid_id(&record.record_id)
        || record.call_id.is_empty()
        || record.call_id.len() > 128
        || record.tool_name.is_empty()
        || record.tool_name.len() > 128
        || record.arguments_json.len() > 1024 * 1024
        || record.result.len() > 512 * 1024
        || !serde_json::from_str::<serde_json::Value>(&record.arguments_json)
            .is_ok_and(|v| v.is_object())
    {
        return Err(error());
    }
    Ok(())
}
pub fn save_tool_result(
    root: &Path,
    conversation: &str,
    turn: &str,
    call: &ToolProposal,
    result: &str,
) -> Result<()> {
    let dir = location(root, conversation, turn)?;
    fs::create_dir_all(&dir).map_err(|_| error())?;
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(dir.join(".write.lock")).map_err(|_| error())?;
    fs2::FileExt::lock_exclusive(&lock).map_err(|_| error())?;
    let previous = read_tool_results(root, conversation, turn)?;
    let sequence = previous
        .iter()
        .filter_map(|r| r.sequence)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(error)?;
    let record = SavedToolResult {
        sequence: Some(sequence),
        record_id: format!("result_{}", uuid::Uuid::new_v4().simple()),
        conversation_id: conversation.into(),
        turn_id: turn.into(),
        call_id: call.id.clone(),
        tool_name: call.name.clone(),
        arguments_json: call.arguments_json.clone(),
        result: result.into(),
    };
    validate(&record)?;
    let bytes = serde_json::to_vec(&record).map_err(|_| error())?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(error());
    }
    crate::infrastructure::immutable_file::publish(
        &dir.join(format!("{}.json", record.record_id)),
        &bytes,
    )
    .map_err(|_| error())
}
pub fn read_tool_results(
    root: &Path,
    conversation: &str,
    turn: &str,
) -> Result<Vec<SavedToolResult>> {
    let dir = location(root, conversation, turn)?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err(error()),
    };
    let mut results = vec![];
    let mut sequences = std::collections::HashSet::new();
    for entry in entries {
        let path = entry.map_err(|_| error())?.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let meta = fs::symlink_metadata(&path).map_err(|_| error())?;
        if !meta.is_file() || meta.len() > MAX_RECORD_BYTES {
            return Err(error());
        }
        let mut bytes = vec![];
        fs::File::open(&path)
            .map_err(|_| error())?
            .take(MAX_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| error())?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(error());
        }
        let record: SavedToolResult = serde_json::from_slice(&bytes).map_err(|_| error())?;
        validate(&record)?;
        if record
            .sequence
            .is_some_and(|n| n == 0 || !sequences.insert(n))
        {
            return Err(error());
        }
        if record.conversation_id != conversation
            || record.turn_id != turn
            || path.file_stem().and_then(|s| s.to_str()) != Some(record.record_id.as_str())
        {
            return Err(error());
        }
        results.push(record);
    }
    // Legacy UUID ordering is only a stable display order, not execution evidence.
    results.sort_by(|a, b| {
        a.sequence
            .cmp(&b.sequence)
            .then(a.record_id.cmp(&b.record_id))
    });
    Ok(results)
}
