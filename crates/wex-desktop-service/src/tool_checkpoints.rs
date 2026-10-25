//! Immutable checkpoints. Ready histories contain complete, ordered tool batches.
//! A pending checkpoint blocks replay; this module never dispatches operations.
use crate::{committed_tool_messages, PreparedChat, Result, ServiceError, ToolProposal};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use wex_api::{Message, Role, ToolCall};
const MAX_BYTES: u64 = 64 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    id: String,
    name: String,
    arguments_json: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    role: String,
    content: Option<String>,
    calls: Vec<Call>,
    call_id: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u32,
    sequence: u32,
    pending: bool,
    conversation_id: String,
    turn_id: String,
    connection_id: String,
    model_id: String,
    base_url: String,
    request_messages: serde_json::Value,
    history: Vec<Entry>,
}
fn error() -> ServiceError {
    ServiceError::new(
        "TOOL_CHECKPOINT_ERROR",
        "工具回合检查点不可用或未完整结算；请核查已执行结果，不得自动重跑工具",
    )
}
fn directory(root: &Path, request: &PreparedChat) -> Result<PathBuf> {
    for id in [&request.conversation_id, &request.turn_id] {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(error());
        }
    }
    Ok(root.join(&request.conversation_id).join(&request.turn_id))
}
fn encode(history: &[Message]) -> Vec<Entry> {
    history
        .iter()
        .map(|m| Entry {
            role: match m.role {
                Role::Assistant => "assistant",
                Role::Tool => "tool",
                Role::User => "user",
                Role::System => "system",
            }
            .into(),
            content: m.content.clone(),
            call_id: m.tool_call_id.clone(),
            calls: m
                .tool_calls
                .iter()
                .map(|c| Call {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    arguments_json: c.arguments_json.clone(),
                })
                .collect(),
        })
        .collect()
}
fn decode(history: Vec<Entry>) -> Result<Vec<Message>> {
    history
        .into_iter()
        .map(|m| {
            Ok(Message {
                role: match m.role.as_str() {
                    "assistant" => Role::Assistant,
                    "tool" => Role::Tool,
                    _ => return Err(error()),
                },
                content: m.content,
                tool_call_id: m.call_id,
                tool_calls: m
                    .calls
                    .into_iter()
                    .map(|c| ToolCall {
                        id: c.id,
                        name: c.name,
                        arguments_json: c.arguments_json,
                    })
                    .collect(),
            })
        })
        .collect()
}
fn validate(history: &[Message], pending: bool) -> Result<()> {
    if history.is_empty() || history.len() > 64 {
        return Err(error());
    }
    let mut offset = 0;
    // IDs identify receipts across the whole turn, not merely within a batch.
    // Reject ambiguous restored checkpoints before any new model request.
    let mut seen_all = std::collections::HashSet::new();
    while offset < history.len() {
        let assistant = &history[offset];
        if assistant.role != Role::Assistant
            || assistant.tool_call_id.is_some()
            || assistant.tool_calls.is_empty()
            || assistant.tool_calls.len() > 4
        {
            return Err(error());
        }
        let mut seen = std::collections::HashSet::new();
        let calls: Vec<ToolProposal> = assistant
            .tool_calls
            .iter()
            .map(|c| ToolProposal {
                id: c.id.clone(),
                name: c.name.clone(),
                arguments_json: c.arguments_json.clone(),
            })
            .collect();
        for call in &calls {
            if call.id.is_empty()
                || call.id.len() > 128
                || call.name.is_empty()
                || call.name.len() > 128
                || call.arguments_json.len() > 1024 * 1024
                || !seen.insert(&call.id)
                || !seen_all.insert(&call.id)
                || !serde_json::from_str::<serde_json::Value>(&call.arguments_json)
                    .is_ok_and(|v| v.is_object())
            {
                return Err(error());
            }
        }
        if pending && offset + 1 == history.len() {
            return Ok(());
        }
        let end = offset + 1 + calls.len();
        if end > history.len() {
            return Err(error());
        }
        let mut results = vec![];
        for tool in &history[offset + 1..end] {
            if tool.role != Role::Tool || !tool.tool_calls.is_empty() {
                return Err(error());
            }
            results.push((
                tool.tool_call_id.clone().ok_or_else(error)?,
                tool.content.clone().ok_or_else(error)?,
            ));
        }
        if committed_tool_messages(&calls, &results, assistant.content.clone())
            .map_err(|_| error())?
            != history[offset..end]
        {
            return Err(error());
        }
        offset = end;
    }
    if pending {
        Err(error())
    } else {
        Ok(())
    }
}
/// Sequence is local to a turn and starts at zero. create_new prevents overwriting
/// another writer's checkpoint. A damaged write causes future reads to fail closed.
pub fn save_tool_checkpoint(
    root: &Path,
    request: &PreparedChat,
    sequence: u32,
    history: &[Message],
    pending: bool,
) -> Result<()> {
    validate(history, pending)?;
    if (sequence % 2 == 0) != pending {
        return Err(error());
    }
    let dir = directory(root, request)?;
    fs::create_dir_all(&dir).map_err(|_| error())?;
    if sequence > 0 && !dir.join(format!("{:08}.json", sequence - 1)).is_file() {
        return Err(error());
    }
    let snapshot = Snapshot {
        version: 1,
        sequence,
        pending,
        conversation_id: request.conversation_id.clone(),
        turn_id: request.turn_id.clone(),
        connection_id: request.connection_id.clone(),
        model_id: request.model_id.clone(),
        base_url: request.base_url.clone(),
        request_messages: serde_json::to_value(&request.messages).map_err(|_| error())?,
        history: encode(history),
    };
    let bytes = serde_json::to_vec(&snapshot).map_err(|_| error())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(error());
    }
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(dir.join(format!("{sequence:08}.json")))
        .map_err(|_| error())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| error())
}
/// Load only the latest complete checkpoint for this exact frozen request.
/// Current connection/permission checks remain the application's responsibility.
pub fn load_tool_checkpoint(root: &Path, request: &PreparedChat) -> Result<Vec<Message>> {
    load_tool_checkpoint_state(root, request).map(|(history, _)| history)
}
fn load_tool_checkpoint_state(root: &Path, request: &PreparedChat) -> Result<(Vec<Message>, u32)> {
    let dir = directory(root, request)?;
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|_| error())?
        .map(|entry| entry.map(|e| e.path()).map_err(|_| error()))
        .collect::<Result<_>>()?;
    paths.retain(|p| p.extension().is_some_and(|e| e == "json"));
    paths.sort();
    if paths.is_empty() {
        return Err(error());
    }
    let next_sequence = u32::try_from(paths.len()).map_err(|_| error())?;
    let expected = serde_json::to_value(&request.messages).map_err(|_| error())?;
    let mut latest: Option<(Vec<Message>, bool)> = None;
    for (index, path) in paths.into_iter().enumerate() {
        let meta = fs::symlink_metadata(&path).map_err(|_| error())?;
        if !meta.is_file()
            || meta.len() > MAX_BYTES
            || path.file_name().and_then(|s| s.to_str())
                != Some(format!("{index:08}.json").as_str())
        {
            return Err(error());
        }
        let mut bytes = vec![];
        fs::File::open(path)
            .map_err(|_| error())?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| error())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(error());
        }
        let saved: Snapshot = serde_json::from_slice(&bytes).map_err(|_| error())?;
        if saved.version != 1
            || saved.sequence as usize != index
            || (index % 2 == 0) != saved.pending
            || saved.conversation_id != request.conversation_id
            || saved.turn_id != request.turn_id
            || saved.connection_id != request.connection_id
            || saved.model_id != request.model_id
            || saved.base_url != request.base_url
            || saved.request_messages != expected
        {
            return Err(error());
        }
        let history = decode(saved.history)?;
        validate(&history, saved.pending)?;
        if let Some((previous, previous_pending)) = &latest {
            if !history.starts_with(previous)
                || history.len() <= previous.len()
                || (!previous_pending && history.len() != previous.len() + 1)
            {
                return Err(error());
            }
        } else if history.len() != 1 {
            return Err(error());
        }
        latest = Some((history, saved.pending));
    }
    match latest {
        Some((history, false)) => Ok((history, next_sequence)),
        _ => Err(error()),
    }
}

/// Recover original context using current, application-validated connection metadata.
/// Neither the credential nor a new user prompt is recovered from disk.
pub fn restore_tool_checkpoint(
    root: &Path,
    current: &PreparedChat,
) -> Result<(PreparedChat, Vec<Message>)> {
    restore_tool_checkpoint_with_cursor(root, current)
        .map(|(request, history, _)| (request, history))
}
/// Return the next immutable checkpoint slot together with its validated history.
pub fn restore_tool_checkpoint_with_cursor(
    root: &Path,
    current: &PreparedChat,
) -> Result<(PreparedChat, Vec<Message>, u32)> {
    let dir = directory(root, current)?;
    let mut paths = fs::read_dir(&dir)
        .map_err(|_| error())?
        .map(|entry| entry.map(|e| e.path()).map_err(|_| error()))
        .collect::<Result<Vec<_>>>()?;
    paths.retain(|p| p.extension().is_some_and(|e| e == "json"));
    paths.sort();
    let path = paths.last().ok_or_else(error)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| error())?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(error());
    }
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|_| error())?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(error());
    }
    let saved: Snapshot = serde_json::from_slice(&bytes).map_err(|_| error())?;
    let mut restored = current.clone();
    restored.messages = serde_json::from_value(saved.request_messages).map_err(|_| error())?;
    let last = restored.messages.last().ok_or_else(error)?;
    let original = current.messages.last().ok_or_else(error)?;
    if last.role != "user" || original.role != "user" || last.content != original.content {
        return Err(error());
    }
    let (history, next_sequence) = load_tool_checkpoint_state(root, &restored)?;
    Ok((restored, history, next_sequence))
}

pub(crate) fn validate_complete_tool_history(history: &[Message]) -> Result<()> {
    validate(history, false)
}
