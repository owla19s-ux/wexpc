//! Trusted desktop-only approval bridge for the initial local read tool.
/// Selecting the exact file in a native picker is the per-call user approval.
/// No Space/project association can grant access. Nothing is auto-retried.
use std::{fs, io::Write};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use wex_api::ToolDefinition;
use wex_core::{
    access::{Access, DirectoryGrant},
    operation::{Action, ExecutionChecks, OperationIntent, OperationLedger, Outcome},
};
use wex_desktop_service::{
    digest_arguments, drive_tool_rounds, read_approved_file, PreparedChat,
    Result, ServiceError,
};
use wex_local_resources::{DirectoryRegistry, Protection};

pub fn run_non_streaming(app: &AppHandle, chat: &PreparedChat, key: &str) -> Result<String> {
    let tool = ToolDefinition {
        name: "read_file".into(),
        description: "Read a single UTF-8 file only after the user selects that exact file in a native approval dialog.".into(),
        parameters_json: r#"{"type":"object","properties":{"file_name":{"type":"string"}},"required":["file_name"],"additionalProperties":false}"#.into(),
    };
    drive_tool_rounds(chat, key, vec![tool], |calls| {
        calls.iter().map(|call| {
            if call.name != "read_file" {
                return Err(ServiceError::new("TOOL_UNSUPPORTED", "模型请求了未提供的工具"));
            }
            let value: serde_json::Value = serde_json::from_str(&call.arguments_json)
                .map_err(|_| ServiceError::new("INVALID_INPUT", "工具参数无法解析"))?;
            let leaf = value.get("file_name").and_then(|v| v.as_str())
                .ok_or_else(|| ServiceError::new("INVALID_INPUT", "缺少文件名"))?;
            if value.as_object().is_none_or(|v| v.len() != 1)
                || leaf.is_empty() || leaf.len() > 255
                || leaf.contains('/') || leaf.contains('\\')
                || leaf.chars().any(char::is_control)
            {
                return Err(ServiceError::new("INVALID_INPUT", "工具只能请求单个文件名"));
            }
            let content = review_and_read(app, chat, leaf)?;
            let result = serde_json::json!({"content":content,"approved":true});
            Ok((call.id.clone(), result.to_string()))
        }).collect::<Result<Vec<_>>>()
    })
}

fn review_and_read(app: &AppHandle, chat: &PreparedChat, leaf: &str) -> Result<String> {
    let selected = app.dialog().file()
        .set_title(format!("WEXPC 请求读取文件「{leaf}」：请选择同名文件以批准本次读取"))
        .blocking_pick_file()
        .ok_or_else(|| ServiceError::new("PERMISSION_DENIED", "用户未批准读取文件"))?;
    let path = selected.into_path()
        .map_err(|_| ServiceError::new("INVALID_INPUT", "选择的文件位置不可用"))?;
    if path.file_name().and_then(|v| v.to_str()) != Some(leaf) {
        return Err(ServiceError::new("PERMISSION_DENIED", "选择文件与模型提议不一致"));
    }
    let parent = path.parent().ok_or_else(|| ServiceError::new("INVALID_INPUT", "文件目录无效"))?;
    let mut registry = DirectoryRegistry::new();
    let directory_id = registry.register(parent, Protection::General)
        .map_err(|_| ServiceError::new("RESOURCE_UNAVAILABLE", "无法安全注册所选目录"))?;
    let operation_id = format!("op_{}", uuid::Uuid::new_v4().simple());
    let attempt_id = format!("at_{}", uuid::Uuid::new_v4().simple());
    let evidence_id = format!("ev_{}", uuid::Uuid::new_v4().simple());
    let intent = OperationIntent {
        id: operation_id.clone(), actor_id: "assistant".into(),
        conversation_id: chat.conversation_id.clone(), context_id: None,
        connection_id: "local".into(), capability_id: "local_file_read".into(),
        resource_id: directory_id.clone(), action: Action::Read,
        expected_version: None,
        intent_digest: digest_arguments("local_file_read", &directory_id, &[leaf.as_bytes()]),
    };
    let mut ledger = OperationLedger::default();
    ledger.propose(intent.clone()).map_err(|_| core_failure())?;
    ledger.approve(&operation_id, &intent, 1).map_err(|_| core_failure())?;
    let dispatch = ledger.begin(&operation_id, &attempt_id, ExecutionChecks {
        authority_revision: 1,
        connection_enabled: true, authenticated: true,
        platform_available: wex_local_resources::platform_supported(),
        resource_allowed: true,
    }).map_err(|_| core_failure())?;
    // A durable "in flight" marker must exist BEFORE touching the file.
    journal(app, &attempt_id, &operation_id, "in_flight", None)?;
    let grants = [DirectoryGrant {
        directory_id: &directory_id, access: Access::Read, explicit_personal: false,
    }];
    let content = read_approved_file(&dispatch, &registry, leaf, true, &grants);
    let receipt_digest = content.as_ref().ok()
        .map(|body| digest_arguments("tool_receipt", &evidence_id, &[body.as_bytes()]));
    journal(app, &attempt_id, &operation_id,
        if content.is_ok() { "succeeded" } else { "failed" },
        receipt_digest.as_deref(),
    )?;
    let outcome = if content.is_ok() {
        Outcome::Succeeded { evidence_ref: evidence_id }
    } else {
        Outcome::Failed { evidence_ref: evidence_id }
    };
    ledger.finish(&operation_id, &attempt_id, outcome).map_err(|_| core_failure())?;
    content
}

fn core_failure() -> ServiceError {
    ServiceError::new("OPERATION_STATE_ERROR", "Core 审批或派发状态无效")
}

/// Crash-safe per-attempt marker; records metadata/digest only, no file bytes.
fn journal(app: &AppHandle, attempt: &str, operation: &str, status: &str, digest: Option<&str>) -> Result<()> {
    let root = app.path().app_data_dir()
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法定位工具回执目录"))?
        .join("tool-receipts");
    fs::create_dir_all(&root)
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法创建工具回执目录"))?;
    let temp = root.join(format!("{attempt}_{}.tmp", uuid::Uuid::new_v4().simple()));
    // Append-only state markers avoid replacing a receipt file on Windows.
    let destination = root.join(format!("{attempt}_{status}.json"));
    let value = serde_json::json!({
        "operation_id":operation, "attempt_id":attempt, "state":status,
        "content_digest":digest,
    });
    let mut file = fs::OpenOptions::new().write(true).create_new(true)
        .open(&temp).map_err(|_| ServiceError::new("STORAGE_ERROR", "无法开始记录回执"))?;
    file.write_all(value.to_string().as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法持久化工具回执"))?;
    fs::rename(&temp, &destination)
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法提交工具回执"))?;
    Ok(())
}
