//! Final replies survive a failed SQLite settlement and application restart.
//! Local recovery does not access a provider or authorize any tool.
use crate::{PreparedChat, Result, ServiceError};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
const MAX_BYTES: u64 = 64 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    version: u32,
    conversation_id: String,
    turn_id: String,
    user: String,
    response: String,
}
fn error() -> ServiceError {
    ServiceError::new(
        "SAVED_REPLY_ERROR",
        "最终回复保存或读取失败；请先核查本地记录，不要直接重新请求模型",
    )
}
fn location(root: &Path, conversation: &str, turn: &str) -> Result<PathBuf> {
    for id in [conversation, turn] {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(error());
        }
    }
    Ok(root.join(conversation).join(turn).join("reply.json"))
}
pub fn save_chat_reply(root: &Path, request: &PreparedChat, response: &str) -> Result<()> {
    let last = request
        .messages
        .last()
        .filter(|m| m.role == "user")
        .ok_or_else(error)?;
    if response.is_empty() {
        return Err(error());
    }
    let path = location(root, &request.conversation_id, &request.turn_id)?;
    let bytes = serde_json::to_vec(&Reply {
        version: 1,
        conversation_id: request.conversation_id.clone(),
        turn_id: request.turn_id.clone(),
        user: last.content.clone(),
        response: response.into(),
    })
    .map_err(|_| error())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(error());
    }
    fs::create_dir_all(path.parent().ok_or_else(error)?).map_err(|_| error())?;
    crate::infrastructure::immutable_file::publish(&path, &bytes).map_err(|_| error())
}
pub fn read_chat_reply(
    root: &Path,
    conversation: &str,
    turn: &str,
    original_user: &str,
) -> Result<Option<String>> {
    let path = location(root, conversation, turn)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(error()),
    };
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(error());
    }
    let mut bytes = vec![];
    fs::File::open(&path)
        .map_err(|_| error())?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(error());
    }
    let saved: Reply = serde_json::from_slice(&bytes).map_err(|_| error())?;
    if saved.version != 1
        || saved.conversation_id != conversation
        || saved.turn_id != turn
        || saved.user != original_user
        || saved.response.is_empty()
    {
        return Err(error());
    }
    Ok(Some(saved.response))
}
