//! Trusted, conversation-scoped user authorization. No model-facing mutator.
use std::path::Path;
use tauri::{AppHandle, Manager};
use wex_access_store::{Grant, Store, StoreError};
use wex_core::{
    access::Access,
    operation::{Action, Dispatch},
};
use wex_desktop_service::{digest_arguments, Result, ServiceError};

#[derive(Clone, Copy, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Read,
    Write,
    ReadWrite,
}
impl From<Scope> for Access {
    fn from(s: Scope) -> Self {
        match s {
            Scope::Read => Self::Read,
            Scope::Write => Self::Write,
            Scope::ReadWrite => Self::ReadWrite,
        }
    }
}
impl From<Access> for Scope {
    fn from(s: Access) -> Self {
        match s {
            Access::Read => Self::Read,
            Access::Write => Self::Write,
            Access::ReadWrite => Self::ReadWrite,
        }
    }
}
#[derive(serde::Serialize)]
pub struct Directory {
    pub id: String,
    pub path: String,
    pub access: Scope,
    pub available: bool,
}
#[derive(serde::Serialize)]
pub struct Status {
    pub revision: u64,
    pub directories: Vec<Directory>,
}

pub fn error(e: StoreError) -> ServiceError {
    let (code, message) = match e {
        StoreError::LimitExceeded => (
            "AUTHORIZATION_LIMIT",
            "目录授权容量已满，本次未保存；可撤销不再需要的目录后重试",
        ),
        StoreError::Conflict => ("AUTHORIZATION_CONFLICT", "目录授权已变化，请刷新后核对"),
        StoreError::InvalidGrant => (
            "DIRECTORY_UNAVAILABLE",
            "目录无法持久识别、已经授权或属于应用私有范围；未保存授权",
        ),
        StoreError::CommitUnknown => (
            "AUTHORIZATION_UNKNOWN",
            "授权保存结果未知，已停止使用此授权库，请核查后恢复",
        ),
        StoreError::Busy => (
            "AUTHORIZATION_BUSY",
            "授权库正在使用或上次写入未完成，请核查后重试",
        ),
        _ => (
            "AUTHORIZATION_UNAVAILABLE",
            "目录授权库不可用，未使用缓存或默认授权",
        ),
    };
    ServiceError::new(code, message)
}
fn open_at(root: &Path, conversation: &str) -> Result<Store> {
    if !conversation.starts_with("conv_")
        || conversation.len() > 64
        || !conversation
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(ServiceError::new("INVALID_INPUT", "对话标识无效"));
    }
    let base = root.join("local-authorizations");
    Store::open(&base).map_err(error)?;
    Store::open_with_internal_roots(base.join(conversation), &[root]).map_err(error)
}
pub fn open(app: &AppHandle, conversation: &str) -> Result<Store> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法定位本地授权目录"))?;
    open_at(&root, conversation)
}
pub fn status(store: &Store) -> Result<Status> {
    let state = store.load().map_err(error)?;
    let directories = state
        .directories
        .iter()
        .filter_map(|d| {
            state
                .grants
                .iter()
                .find(|g| g.directory_id == d.id)
                .map(|g| {
                    let available = wex_local_resources::DirectoryRegistry::new()
                        .restore_binding(d, &[])
                        .is_ok();
                    Directory {
                        id: d.id.clone(),
                        path: d.path.to_string_lossy().into_owned(),
                        access: g.access.into(),
                        available,
                    }
                })
        })
        .collect();
    Ok(Status {
        revision: state.revision,
        directories,
    })
}
pub fn edit(store: &Store, revision: u64, id: String, access: Scope) -> Result<Status> {
    let s = store.load().map_err(error)?;
    if !s.directories.iter().any(|d| d.id == id) {
        return Err(ServiceError::new("DIRECTORY_NOT_FOUND", "授权目录不存在"));
    }
    store
        .set(
            revision,
            Grant {
                directory_id: id,
                access: access.into(),
                explicit_personal: true,
            },
        )
        .map_err(error)?;
    status(store)
}
pub fn read(dispatch: &Dispatch, store: &Store, revision: u64, leaf: &str) -> Result<String> {
    if dispatch.intent.action != Action::Read
        || dispatch.intent.capability_id != "local_file_read"
        || dispatch.intent.intent_digest
            != digest_arguments(
                "local_file_read",
                &dispatch.intent.resource_id,
                &[leaf.as_bytes()],
            )
    {
        return Err(ServiceError::new(
            "DISPATCH_REJECTED",
            "读取参数与Core派发不匹配",
        ));
    }
    let bytes = store
        .read_authorized_at(Some(revision), &dispatch.intent.resource_id, leaf, true)
        .map_err(|e| {
            ServiceError::new(
                "LOCAL_READ_DENIED",
                format!("读取未完成，目录身份、当前授权或文件检查失败：{e:?}"),
            )
        })?;
    String::from_utf8(bytes)
        .map_err(|_| ServiceError::new("TOOL_UNSUPPORTED_CONTENT", "当前读取工具仅支持UTF-8文本"))
}

/// Bind Core's exact create intent to the live conversation grant revision.
/// The store never exposes a path-based write token to a model.
pub fn create(
    dispatch: &Dispatch,
    store: &Store,
    revision: u64,
    leaf: &str,
    content: &str,
) -> Result<()> {
    if dispatch.intent.action != Action::Write
        || dispatch.intent.capability_id != "local_file_create"
        || dispatch.intent.intent_digest
            != digest_arguments("local_file_create", &dispatch.intent.resource_id,
                &[leaf.as_bytes(), content.as_bytes()])
    {
        return Err(ServiceError::new("DISPATCH_REJECTED", "新建参数与Core派发不匹配"));
    }
    store.create_authorized_at(Some(revision), &dispatch.intent.resource_id,
        leaf, content.as_bytes(), true)
        .map_err(|e| ServiceError::new("LOCAL_CREATE_UNKNOWN",
            format!("文件新建可能已产生副作用，请核对后再决定是否重试：{e:?}")))
}

fn conversation_exists(shared: &crate::Shared, id: &str) -> Result<()> {
    if !crate::locked(shared)?
        .snapshot()?
        .conversations
        .iter()
        .any(|c| c.id == id)
    {
        return Err(ServiceError::new("CONVERSATION_NOT_FOUND", "对话不存在"));
    }
    Ok(())
}
#[tauri::command]
pub fn local_directory_status(
    app: AppHandle,
    state: tauri::State<'_, crate::Shared>,
    conversation_id: String,
) -> Result<Status> {
    conversation_exists(&state, &conversation_id)?;
    status(&open(&app, &conversation_id)?)
}
#[tauri::command]
pub async fn authorize_local_directory(
    app: AppHandle,
    state: tauri::State<'_, crate::Shared>,
    conversation_id: String,
    expected_revision: u64,
    access: Scope,
) -> Result<Status> {
    use tauri_plugin_dialog::DialogExt;
    let shared = state.inner().clone();
    conversation_exists(&shared, &conversation_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let store = open(&app, &conversation_id)?;
        if store.load().map_err(error)?.revision != expected_revision {
            return Err(error(StoreError::Conflict));
        }
        let selected = app
            .dialog()
            .file()
            .set_title("选择授权本对话访问的目录（将保存至撤销）")
            .blocking_pick_folder()
            .ok_or_else(|| ServiceError::new("PERMISSION_CANCELLED", "未选择目录，未增加授权"))?;
        let path = selected
            .into_path()
            .map_err(|_| ServiceError::new("INVALID_INPUT", "目录位置不可用"))?;
        conversation_exists(&shared, &conversation_id)?;
        store
            .authorize_directory(expected_revision, path, access.into())
            .map_err(error)?;
        status(&store)
    })
    .await
    .map_err(|_| {
        ServiceError::new(
            "AUTHORIZATION_INTERRUPTED",
            "目录授权线程中断，请刷新核对保存状态",
        )
    })?
}
#[tauri::command]
pub fn edit_local_directory(
    app: AppHandle,
    state: tauri::State<'_, crate::Shared>,
    conversation_id: String,
    expected_revision: u64,
    directory_id: String,
    access: Scope,
) -> Result<Status> {
    conversation_exists(&state, &conversation_id)?;
    edit(
        &open(&app, &conversation_id)?,
        expected_revision,
        directory_id,
        access,
    )
}
#[tauri::command]
pub fn revoke_local_directory(
    app: AppHandle,
    state: tauri::State<'_, crate::Shared>,
    conversation_id: String,
    expected_revision: u64,
    directory_id: String,
) -> Result<Status> {
    conversation_exists(&state, &conversation_id)?;
    let store = open(&app, &conversation_id)?;
    store
        .revoke(expected_revision, &directory_id)
        .map_err(error)?;
    status(&store)
}
#[tauri::command]
pub fn local_read_policy(
    state: tauri::State<'_, crate::Shared>,
    conversation_id: String,
) -> Result<wex_desktop_service::ToolPolicySetting> {
    crate::locked(&state)?.tool_policy(&conversation_id, "local_file_read")
}
#[tauri::command]
pub fn set_local_read_policy(
    state: tauri::State<'_, crate::Shared>,
    conversation_id: String,
    expected_revision: u64,
    policy: wex_desktop_service::PolicyChoice,
) -> Result<wex_desktop_service::ToolPolicySetting> {
    crate::locked(&state)?.set_tool_policy(
        &conversation_id,
        "local_file_read",
        expected_revision,
        policy,
    )
}

#[tauri::command]
pub fn local_create_policy(
    state: tauri::State<'_, crate::Shared>,
    conversation_id: String,
) -> Result<wex_desktop_service::ToolPolicySetting> {
    crate::locked(&state)?.tool_policy(&conversation_id, "local_file_create")
}
#[tauri::command]
pub fn set_local_create_policy(
    state: tauri::State<'_, crate::Shared>,
    conversation_id: String,
    expected_revision: u64,
    policy: wex_desktop_service::PolicyChoice,
) -> Result<wex_desktop_service::ToolPolicySetting> {
    crate::locked(&state)?.set_tool_policy(
        &conversation_id, "local_file_create", expected_revision, policy,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    #[test]
    fn conversation_stores_are_private_independent_and_live_revoke_blocks_saved_dispatch() {
        let root = std::env::temp_dir().join(format!(
            "wex-host-local-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let private = root.join("private");
        fs::create_dir(&private).unwrap();
        let docs = root.join("docs");
        fs::create_dir(&docs).unwrap();
        fs::write(docs.join("a"), b"body").unwrap();
        let a = open_at(&private, "conv_A").unwrap();
        let b = open_at(&private, "conv_B").unwrap();
        let saved = a.authorize_directory(0, &docs, Access::Read).unwrap();
        assert!(b.read_authorized(&saved.id, "a", true).is_err());
        let dispatch = Dispatch {
            intent: wex_core::operation::OperationIntent {
                id: "op".into(),
                actor_id: "assistant".into(),
                conversation_id: "conv_A".into(),
                context_id: None,
                connection_id: "local".into(),
                capability_id: "local_file_read".into(),
                resource_id: saved.id.clone(),
                action: Action::Read,
                expected_version: None,
                intent_digest: digest_arguments("local_file_read", &saved.id, &[b"a"]),
            },
            attempt_id: "attempt".into(),
        };
        assert_eq!(read(&dispatch, &a, 1, "a").unwrap(), "body");
        assert!(read(&dispatch, &a, 1, "other").is_err());
        a.revoke(1, &saved.id).unwrap();
        assert!(read(&dispatch, &a, 1, "a").is_err());
        assert!(status(&a).unwrap().directories.is_empty());
        drop((a, b));
        let _ = fs::remove_dir_all(root);
    }
}
