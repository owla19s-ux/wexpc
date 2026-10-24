//! Trusted desktop-only approval bridge for the initial local read tool.
/// Selecting the exact file in a native picker is the per-call user approval.
/// No Space/project association can grant access. Nothing is auto-retried.
use std::{fs, io::Write};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use wex_api::ToolDefinition;
use wex_core::{
    access::{Access, DirectoryGrant},
    operation::{Action, ExecutionChecks, OperationIntent, OperationLedger, Outcome},
};
use wex_desktop_service::{
    digest_arguments, drive_tool_rounds, read_approved_file, PreparedChat,
    Result, ServiceError, ToolProposal,
};
use wex_local_resources::{DirectoryRegistry, Protection};

pub fn run_non_streaming(app: &AppHandle, chat: &PreparedChat, key: &str) -> Result<String> {
    let tool = ToolDefinition {
        name: "read_file".into(),
        description: "Read a single UTF-8 file only after the user selects that exact file in a native approval dialog.".into(),
        parameters_json: r#"{"type":"object","properties":{"file_name":{"type":"string"}},"required":["file_name"],"additionalProperties":false}"#.into(),
    };
    let mut tools = vec![tool];
    for (name, description) in [
        ("git_status", "Read Git status of a repository selected by the user."),
        ("cargo_test", "Run cargo test --workspace --locked only after explicit per-run approval."),
        ("cargo_fmt_check", "Run cargo fmt --all -- --check after explicit per-run approval."),
    ] {
        tools.push(ToolDefinition {
            name: name.into(), description: description.into(),
            parameters_json: r#"{"type":"object","properties":{},"additionalProperties":false}"#.into(),
        });
    }
    if crate::github_config::status(app)?.enabled {
        tools.push(ToolDefinition {
            name: "github_read_file".into(),
            description: "Read a GitHub file only after explicit user confirmation.".into(),
            parameters_json: r#"{"type":"object","properties":{"owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"}},"required":["owner","repo","path"],"additionalProperties":false}"#.into(),
        });
        tools.push(ToolDefinition {
            name: "github_write_file".into(),
            description: "Update an existing GitHub text file with its exact current blob SHA; user must review and approve all bytes.".into(),
            parameters_json: r#"{"type":"object","properties":{"owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"},"sha":{"type":"string"},"content":{"type":"string"}},"required":["owner","repo","path","sha","content"],"additionalProperties":false}"#.into(),
        });
    }
    drive_tool_rounds(chat, key, tools, |calls| {
        // Reject unsupported or malformed proposals before dispatching any
        // member of the batch. Validation does not itself authorize execution.
        for call in calls {
            if !tools_available_for_proposal(call, crate::github_config::status(app)?.enabled) {
                return Err(ServiceError::new("TOOL_UNSUPPORTED", "工具批次包含不可用的调用，未执行此批次"));
            }
        }
        calls.iter().map(|call| {
            if matches!(call.name.as_str(), "git_status" | "cargo_test" | "cargo_fmt_check") {
                return Ok((call.id.clone(), review_local_command(app, chat, call)?));
            }
            if call.name == "github_read_file" || call.name == "github_write_file" {
                return Ok((call.id.clone(), review_github(app, chat, call)?));
            }
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
            let (content, evidence_ref) = review_and_read(app, chat, leaf)?;
            let result = serde_json::json!({"content":content,"evidence_ref":evidence_ref});
            Ok((call.id.clone(), result.to_string()))
        }).collect::<Result<Vec<_>>>()
    })
}

fn tools_available_for_proposal(call: &ToolProposal, github_enabled: bool) -> bool {
    match call.name.as_str() {
        "read_file" | "git_status" | "cargo_test" | "cargo_fmt_check" => true,
        "github_read_file" | "github_write_file" => github_enabled,
        _ => false,
    }
}

fn review_and_read(app: &AppHandle, chat: &PreparedChat, leaf: &str) -> Result<(String, String)> {
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
    let internal = app.path().app_data_dir()
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法确定应用数据目录"))?;
    let directory_id = registry.register_with_exclusions(
        parent, Protection::Protected, &[internal.as_path()],
    )
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
        directory_id: &directory_id, access: Access::Read, explicit_personal: true,
    }];
    let content = read_approved_file(&dispatch, &registry, leaf, true, &grants);
    let receipt_digest = content.as_ref().ok()
        .map(|body| digest_arguments("tool_receipt", &evidence_id, &[body.as_bytes()]));
    journal(app, &attempt_id, &operation_id,
        if content.is_ok() { "succeeded" } else { "failed" },
        receipt_digest.as_deref(),
    )?;
    let outcome = if content.is_ok() {
        Outcome::Succeeded { evidence_ref: evidence_id.clone() }
    } else {
        Outcome::Failed { evidence_ref: evidence_id.clone() }
    };
    ledger.finish(&operation_id, &attempt_id, outcome).map_err(|_| core_failure())?;
    content.map(|body| (body, evidence_id))
}


/// The user chooses the target repository each time. No AI-supplied cwd,
/// shell expression or arbitrary argument enters the subprocess.
fn review_local_command(app: &AppHandle, chat: &PreparedChat, call: &ToolProposal) -> Result<String> {
    let args: serde_json::Value = serde_json::from_str(&call.arguments_json)
        .map_err(|_| ServiceError::new("INVALID_INPUT", "命令参数无效"))?;
    if !args.as_object().is_some_and(|o| o.is_empty()) {
        return Err(ServiceError::new("INVALID_INPUT", "当前命令不接受模型提供的参数"));
    }
    let (capability, action, command) = match call.name.as_str() {
        "git_status" => ("git_status", Action::Read, "git --no-optional-locks status --porcelain=v1 -uno"),
        "cargo_test" => ("terminal_cargo_test", Action::Write, "cargo test --workspace --locked"),
        "cargo_fmt_check" => ("terminal_cargo_fmt_check", Action::Write, "cargo fmt --all -- --check"),
        _ => return Err(ServiceError::new("TOOL_UNSUPPORTED", "不支持的命令")),
    };
    let selected = app.dialog().file()
        .set_title(format!("WEXPC 请求执行 {command}：请选择仓库目录"))
        .blocking_pick_folder()
        .ok_or_else(|| ServiceError::new("PERMISSION_DENIED", "未选择授权仓库目录"))?;
    let root = selected.into_path()
        .map_err(|_| ServiceError::new("INVALID_INPUT", "仓库位置不可用"))?;
    let root = fs::canonicalize(root)
        .map_err(|_| ServiceError::new("RESOURCE_UNAVAILABLE", "无法验证仓库目录"))?;
    let internal = app.path().app_data_dir()
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "应用数据目录不可用"))?;
    if root.starts_with(&internal) {
        return Err(ServiceError::new("PERMISSION_DENIED", "禁止执行于 WEXPC 内部数据目录"));
    }
    let cwd = root.to_str().ok_or_else(|| ServiceError::new("INVALID_INPUT", "目录编码不支持"))?;
    let extra = if action == Action::Write {
        "注意：Cargo 构建可能执行仓库自身的构建脚本，仅在信任此代码时批准。"
    } else {
        "仅查询 Git 状态，不执行提交或推送。"
    };
    let approved = app.dialog()
        .message(format!("请求执行：{command}\n工作目录：{cwd}\n{extra}"))
        .title("WEXPC 终端操作逐次审批")
        .buttons(MessageDialogButtons::OkCancel)
        .blocking_show();
    if !approved {
        return Err(ServiceError::new("PERMISSION_DENIED", "用户未批准命令"));
    }
    let resource_id = digest_arguments("local_repo", "selected", &[cwd.as_bytes()]);
    let operation_id = format!("op_{}", uuid::Uuid::new_v4().simple());
    let attempt_id = format!("at_{}", uuid::Uuid::new_v4().simple());
    let evidence_id = format!("ev_{}", uuid::Uuid::new_v4().simple());
    let intent = OperationIntent {
        id: operation_id.clone(), actor_id: "assistant".into(),
        conversation_id: chat.conversation_id.clone(), context_id: None,
        connection_id: "terminal".into(), capability_id: capability.into(),
        resource_id: resource_id.clone(), action, expected_version: None,
        intent_digest: digest_arguments(capability, &resource_id, &[cwd.as_bytes()]),
    };
    let mut ledger = OperationLedger::default();
    ledger.propose(intent.clone()).map_err(|_| core_failure())?;
    ledger.approve(&operation_id, &intent, 1).map_err(|_| core_failure())?;
    let dispatch = ledger.begin(&operation_id, &attempt_id, ExecutionChecks {
        authority_revision: 1, connection_enabled: true, authenticated: true,
        platform_available: true, resource_allowed: true,
    }).map_err(|_| core_failure())?;
    journal(app, &attempt_id, &operation_id, "in_flight", Some(&intent.intent_digest))?;
    let outcome = if call.name == "git_status" {
        wex_desktop_service::approved_git_status(&dispatch, &resource_id, &root)
    } else {
        let task = if call.name == "cargo_test" {
            wex_desktop_service::TerminalTask::CargoTest
        } else {
            wex_desktop_service::TerminalTask::CargoFmtCheck
        };
        wex_desktop_service::execute_approved_terminal(&dispatch, &resource_id, &root, task)
    };
    let status = match &outcome {
        Ok(_) => "succeeded",
        Err(e) if e.code == "OUTCOME_UNKNOWN" || e.code == "TOOL_TIMEOUT" => "unknown",
        Err(_) => "failed",
    };
    let evidence_hash = outcome.as_ref().ok().map(|value|
        digest_arguments("tool_receipt", &evidence_id, &[value.as_bytes()])
    );
    journal(app, &attempt_id, &operation_id, status, evidence_hash.as_deref())?;
    let core_outcome = match status {
        "succeeded" => Outcome::Succeeded { evidence_ref: evidence_id.clone() },
        "unknown" => Outcome::Unknown,
        _ => Outcome::Failed { evidence_ref: evidence_id.clone() },
    };
    ledger.finish(&operation_id, &attempt_id, core_outcome).map_err(|_| core_failure())?;
    let result = outcome?;
    Ok(serde_json::json!({"evidence_ref":evidence_id,"output":result}).to_string())
}

/// Approval always displays the exact repo/path and, for writes, every byte
/// of the bounded text update. The model never sees the GitHub PAT.
fn review_github(app: &AppHandle, chat: &PreparedChat, call: &ToolProposal) -> Result<String> {
    let args: serde_json::Value = serde_json::from_str(&call.arguments_json)
        .map_err(|_| ServiceError::new("INVALID_INPUT", "GitHub 工具参数无效"))?;
    fn field<'a>(value: &'a serde_json::Value, name: &str) -> Result<&'a str> {
        value.get(name).and_then(|v| v.as_str())
            .ok_or_else(|| ServiceError::new("INVALID_INPUT", "GitHub 缺少必要参数"))
    }
    let owner = field(&args, "owner")?;
    let repo = field(&args, "repo")?;
    let path = field(&args, "path")?;
    let write = call.name == "github_write_file";
    let sha = if write { field(&args, "sha")? } else { "" };
    let content = if write { field(&args, "content")? } else { "" };
    if owner.is_empty() || repo.is_empty() || path.is_empty()
        || owner.len() > 255 || repo.len() > 255 || path.len() > 4096
        || owner.chars().any(char::is_control)
        || repo.chars().any(char::is_control)
        || path.chars().any(char::is_control)
        || (write && (sha.len() != 40 || content.len() > 4096))
    {
        return Err(ServiceError::new("INVALID_INPUT", "GitHub 审批参数或文件内容超过限制"));
    }
    let summary = if write {
        format!("WEXPC 请求修改 GitHub 文件\n仓库：{owner}/{repo}\n文件：{path}\n原 SHA：{sha}\n写入字节数：{}\n完整写入内容：\n{content}",content.len())
    } else {
        format!("WEXPC 请求读取 GitHub 文件\n仓库：{owner}/{repo}\n文件：{path}")
    };
    let approved = app.dialog().message(summary)
        .title("逐次 GitHub 工具授权")
        .buttons(MessageDialogButtons::OkCancel)
        .blocking_show();
    if !approved {
        return Err(ServiceError::new("PERMISSION_DENIED", "用户拒绝 GitHub 操作"));
    }
    let token = crate::github_config::credential(app)?;
    let resource_id = digest_arguments("github_repo", "global", &[owner.as_bytes(), repo.as_bytes()]);
    let capability = if write { "github_file_write" } else { "github_file_read" };
    let action = if write { Action::Write } else { Action::Read };
    let parameters: Vec<&[u8]> = if write {
        vec![owner.as_bytes(), repo.as_bytes(), path.as_bytes(), sha.as_bytes(), content.as_bytes()]
    } else {
        vec![owner.as_bytes(), repo.as_bytes(), path.as_bytes()]
    };
    let operation_id = format!("op_{}", uuid::Uuid::new_v4().simple());
    let attempt_id = format!("at_{}", uuid::Uuid::new_v4().simple());
    let evidence_id = format!("ev_{}", uuid::Uuid::new_v4().simple());
    let intent = OperationIntent {
        id: operation_id.clone(), actor_id: "assistant".into(),
        conversation_id: chat.conversation_id.clone(), context_id: None,
        connection_id: "github".into(), capability_id: capability.into(),
        resource_id: resource_id.clone(), action,
        expected_version: if write { Some(sha.into()) } else { None },
        intent_digest: digest_arguments(capability, &resource_id, &parameters),
    };
    let mut ledger = OperationLedger::default();
    ledger.propose(intent.clone()).map_err(|_| core_failure())?;
    ledger.approve(&operation_id, &intent, 1).map_err(|_| core_failure())?;
    let dispatch = ledger.begin(&operation_id, &attempt_id, ExecutionChecks {
        authority_revision: 1, connection_enabled: true,
        authenticated: true, platform_available: true, resource_allowed: true,
    }).map_err(|_| core_failure())?;
    journal(app, &attempt_id, &operation_id, "in_flight", Some(&intent.intent_digest))?;
    let outcome = if write {
        wex_desktop_service::update_approved_github_file(
            &dispatch, &resource_id, owner, repo, path, sha, content.as_bytes(), &token,
        )
    } else {
        wex_desktop_service::read_approved_github_file(
            &dispatch, &resource_id, owner, repo, path, &token,
        )
    };
    let status = match &outcome {
        Ok(_) => "succeeded",
        Err(error) if error.code == "OUTCOME_UNKNOWN" => "unknown",
        Err(_) => "failed",
    };
    let evidence_hash = outcome.as_ref().ok().map(|value|
        digest_arguments("tool_receipt", &evidence_id, &[value.as_bytes()])
    );
    journal(app, &attempt_id, &operation_id, status, evidence_hash.as_deref())?;
    let core_outcome = match status {
        "succeeded" => Outcome::Succeeded { evidence_ref: evidence_id.clone() },
        "unknown" => Outcome::Unknown,
        _ => Outcome::Failed { evidence_ref: evidence_id.clone() },
    };
    ledger.finish(&operation_id, &attempt_id, core_outcome).map_err(|_| core_failure())?;
    let body = outcome?;
    Ok(serde_json::json!({"evidence_ref": evidence_id, "result":body}).to_string())
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
