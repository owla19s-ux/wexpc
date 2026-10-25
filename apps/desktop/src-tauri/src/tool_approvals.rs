//! Trusted desktop-only approval bridge for the initial local read tool.
/// Selecting the exact file in a native picker is the per-call user approval.
/// No Space/project association can grant access. Nothing is auto-retried.
use std::fs;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use wex_api::ToolDefinition;
use wex_core::{
    access::{Access, DirectoryGrant},
    operation::{Action, ExecutionChecks, OperationIntent, OperationLedger, Outcome},
};
use wex_desktop_service::{
    digest_arguments, drive_tool_rounds_from_history, execute_model_step, read_approved_file,
    PreparedChat, Result, ServiceError, ToolProposal,
};
use wex_local_resources::{DirectoryRegistry, Protection};

pub fn run_non_streaming(
    app: &AppHandle,
    chat: &PreparedChat,
    key: &str,
    before_step: impl Fn() -> Result<()>,
) -> Result<String> {
    run_with_history(app, chat, key, Vec::new(), 0, before_step)
}
pub fn resume_non_streaming(
    app: &AppHandle,
    chat: &PreparedChat,
    key: &str,
    history: Vec<wex_api::Message>,
    sequence: u32,
    before_step: impl Fn() -> Result<()>,
) -> Result<String> {
    run_with_history(app, chat, key, history, sequence, before_step)
}
fn run_with_history(
    app: &AppHandle,
    chat: &PreparedChat,
    key: &str,
    history: Vec<wex_api::Message>,
    mut checkpoint_sequence: u32,
    before_step: impl Fn() -> Result<()>,
) -> Result<String> {
    let tool = ToolDefinition {
        name: "read_file".into(),
        description: "Read a single UTF-8 file only after the user selects that exact file in a native approval dialog.".into(),
        parameters_json: r#"{"type":"object","properties":{"file_name":{"type":"string"}},"required":["file_name"],"additionalProperties":false}"#.into(),
    };
    let mut tools = vec![tool];
    for (name, description) in [
        (
            "git_status",
            "Read Git status of a repository selected by the user.",
        ),
        (
            "git_diff",
            "Read the current unstaged Git diff without executing external diff helpers.",
        ),
        (
            "cargo_test",
            "Run cargo test --workspace --locked only after explicit per-run approval.",
        ),
        (
            "cargo_fmt_check",
            "Run cargo fmt --all -- --check after explicit per-run approval.",
        ),
    ] {
        tools.push(ToolDefinition {
            name: name.into(),
            description: description.into(),
            parameters_json: r#"{"type":"object","properties":{},"additionalProperties":false}"#
                .into(),
        });
    }
    if crate::github_config::status(app).is_ok_and(|status| status.enabled) {
        tools.push(ToolDefinition {
            name: "github_read_file".into(),
            description: "Read an existing GitHub UTF-8 file and its current blob SHA after explicit user confirmation; use that SHA for a separately approved update.".into(),
            parameters_json: r#"{"type":"object","properties":{"owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"}},"required":["owner","repo","path"],"additionalProperties":false}"#.into(),
        });
        tools.push(ToolDefinition {
            name: "github_create_file".into(),
            description: "Create a new GitHub UTF-8 text file only if no file exists at that path, with separately approved exact bytes.".into(),
            parameters_json: r#"{"type":"object","properties":{"owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"},"content":{"type":"string"}},"required":["owner","repo","path","content"],"additionalProperties":false}"#.into(),
        });
        tools.push(ToolDefinition {
            name: "github_write_file".into(),
            description: "Update an existing GitHub text file with its exact current blob SHA; user must review and approve all bytes.".into(),
            parameters_json: r#"{"type":"object","properties":{"owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"},"sha":{"type":"string"},"content":{"type":"string"}},"required":["owner","repo","path","sha","content"],"additionalProperties":false}"#.into(),
        });
    }
    drive_tool_rounds_from_history(
        history,
        |history| {
            before_step()?;
            execute_model_step(chat, key, history, tools.clone())
        },
        |calls| {
            before_step()?;
            // Reject unsupported or malformed proposals before dispatching any
            // member of the batch. Validation does not itself authorize execution.
            preflight_batch(calls, || {
                crate::github_config::status(app).map(|status| status.enabled)
            })?;
            let mut completed = 0usize;
            let result = calls
                .iter()
                .map(|call| {
                    let output: (String, String) = (|| -> Result<(String, String)> {
                        if matches!(
                            call.name.as_str(),
                            "git_status" | "git_diff" | "cargo_test" | "cargo_fmt_check"
                        ) {
                            return Ok((
                                call.id.clone(),
                                review_local_command(app, chat, call, &before_step)?,
                            ));
                        }
                        if matches!(
                            call.name.as_str(),
                            "github_read_file" | "github_write_file" | "github_create_file"
                        ) {
                            return Ok((
                                call.id.clone(),
                                review_github(app, chat, call, &before_step)?,
                            ));
                        }
                        if call.name != "read_file" {
                            return Err(ServiceError::new(
                                "TOOL_UNSUPPORTED",
                                "模型请求了未提供的工具",
                            ));
                        }
                        let value: serde_json::Value = serde_json::from_str(&call.arguments_json)
                            .map_err(|_| {
                            ServiceError::new("INVALID_INPUT", "工具参数无法解析")
                        })?;
                        let leaf = value
                            .get("file_name")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| ServiceError::new("INVALID_INPUT", "缺少文件名"))?;
                        if value.as_object().is_none_or(|v| v.len() != 1)
                            || leaf.is_empty()
                            || leaf.len() > 255
                            || leaf.contains('/')
                            || leaf.contains('\\')
                            || leaf.chars().any(char::is_control)
                        {
                            return Err(ServiceError::new(
                                "INVALID_INPUT",
                                "工具只能请求单个文件名",
                            ));
                        }
                        let (content, evidence_ref) =
                            review_and_read(app, chat, leaf, &before_step)?;
                        let result =
                            serde_json::json!({"content":content,"evidence_ref":evidence_ref});
                        Ok((call.id.clone(), result.to_string()))
                    })()?;
                    let root = app.path().app_data_dir().map_err(|_| {
                        ServiceError::new(
                            "TOOL_RESULT_STORAGE_ERROR",
                            "工具可能已执行，但无法定位回执目录；不要直接重试工具",
                        )
                    })?;
                    wex_desktop_service::save_tool_result(
                        &root.join("tool-results"),
                        &chat.conversation_id,
                        &chat.turn_id,
                        call,
                        &output.1,
                    )?;
                    Ok(output)
                })
                .inspect(|item| {
                    if item.is_ok() {
                        completed += 1;
                    }
                })
                .collect::<Result<Vec<_>>>();
            result.map_err(|error| {
            if completed == 0 {
                error
            } else {
                ServiceError::new(
                    "TOOL_BATCH_PARTIAL",
                    format!("本批已有 {completed} 项工具取得执行结果，后续失败（{}）。请核对回执后再继续，禁止自动重试", error.code),
                )
            }
        })
        },
        |history, pending| {
            let root = app.path().app_data_dir().map_err(|_| {
                ServiceError::new("TOOL_CHECKPOINT_ERROR", "无法定位工具回合检查点目录")
            })?;
            wex_desktop_service::save_tool_checkpoint(
                &root.join("tool-checkpoints"),
                chat,
                checkpoint_sequence,
                history,
                pending,
            )?;
            checkpoint_sequence += 1;
            Ok(())
        },
    )
}

fn preflight_batch(
    calls: &[ToolProposal],
    mut github_enabled: impl FnMut() -> Result<bool>,
) -> Result<()> {
    for call in calls {
        let enabled = if matches!(
            call.name.as_str(),
            "github_read_file" | "github_write_file" | "github_create_file"
        ) {
            github_enabled()?
        } else {
            false
        };
        preflight_proposal(call, enabled)?;
    }
    Ok(())
}

fn preflight_proposal(call: &ToolProposal, github_enabled: bool) -> Result<()> {
    let args: serde_json::Value = serde_json::from_str(&call.arguments_json)
        .map_err(|_| ServiceError::new("INVALID_INPUT", "工具参数不是有效 JSON"))?;
    let obj = args
        .as_object()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "工具参数必须是对象"))?;
    let fields: &[&str] = match call.name.as_str() {
        "read_file" => &["file_name"],
        "git_status" | "git_diff" | "cargo_test" | "cargo_fmt_check" => &[],
        "github_read_file" if github_enabled => &["owner", "repo", "path"],
        "github_write_file" if github_enabled => &["owner", "repo", "path", "sha", "content"],
        "github_create_file" if github_enabled => &["owner", "repo", "path", "content"],
        _ => return Err(ServiceError::new("TOOL_UNSUPPORTED", "工具当前不可用")),
    };
    if obj.len() != fields.len()
        || fields.iter().any(|field| {
            !obj.get(*field)
                .is_some_and(|value| value.as_str().is_some())
        })
    {
        return Err(ServiceError::new(
            "INVALID_INPUT",
            "工具参数字段与声明不一致",
        ));
    }
    for field in fields {
        let value = obj.get(*field).and_then(|v| v.as_str()).unwrap_or_default();
        if (*field != "content" && value.is_empty())
            || value.contains('\0')
            || (*field != "content" && value.chars().any(char::is_control))
        {
            return Err(ServiceError::new(
                "INVALID_INPUT",
                "工具参数包含空值或控制字符",
            ));
        }
    }
    if call.name == "github_create_file"
        && obj
            .get("content")
            .and_then(|v| v.as_str())
            .is_some_and(|v| v.len() > 4096)
    {
        return Err(ServiceError::new(
            "INVALID_INPUT",
            "GitHub 新文件内容超过上限",
        ));
    }
    if call.name == "github_write_file" {
        let sha = obj.get("sha").and_then(|v| v.as_str()).unwrap_or_default();
        let content = obj
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if sha.len() != 40 || !sha.bytes().all(|c| c.is_ascii_hexdigit()) || content.len() > 4096 {
            return Err(ServiceError::new(
                "INVALID_INPUT",
                "GitHub 写入版本或大小不合法",
            ));
        }
    }
    Ok(())
}

fn review_and_read(
    app: &AppHandle,
    chat: &PreparedChat,
    leaf: &str,
    authority: &impl Fn() -> Result<()>,
) -> Result<(String, String)> {
    let selected = app
        .dialog()
        .file()
        .set_title(format!(
            "WEXPC 请求读取文件「{leaf}」：请选择同名文件以批准本次读取"
        ))
        .blocking_pick_file()
        .ok_or_else(|| ServiceError::new("PERMISSION_DENIED", "用户未批准读取文件"))?;
    let path = selected
        .into_path()
        .map_err(|_| ServiceError::new("INVALID_INPUT", "选择的文件位置不可用"))?;
    if path.file_name().and_then(|v| v.to_str()) != Some(leaf) {
        return Err(ServiceError::new(
            "PERMISSION_DENIED",
            "选择文件与模型提议不一致",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "文件目录无效"))?;
    let mut registry = DirectoryRegistry::new();
    let internal = app
        .path()
        .app_data_dir()
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法确定应用数据目录"))?;
    let directory_id = registry
        .register_with_exclusions(parent, Protection::Protected, &[internal.as_path()])
        .map_err(|_| ServiceError::new("RESOURCE_UNAVAILABLE", "无法安全注册所选目录"))?;
    let operation_id = format!("op_{}", uuid::Uuid::new_v4().simple());
    let attempt_id = format!("at_{}", uuid::Uuid::new_v4().simple());
    let evidence_id = format!("ev_{}", uuid::Uuid::new_v4().simple());
    let intent = OperationIntent {
        id: operation_id.clone(),
        actor_id: "assistant".into(),
        conversation_id: chat.conversation_id.clone(),
        context_id: None,
        connection_id: "local".into(),
        capability_id: "local_file_read".into(),
        resource_id: directory_id.clone(),
        action: Action::Read,
        expected_version: None,
        intent_digest: digest_arguments("local_file_read", &directory_id, &[leaf.as_bytes()]),
    };
    let mut ledger = OperationLedger::default();
    ledger.propose(intent.clone()).map_err(|_| core_failure())?;
    ledger
        .approve(&operation_id, &intent, 1)
        .map_err(|_| core_failure())?;
    let dispatch = begin_approved_dispatch(
        &mut ledger,
        &operation_id,
        &attempt_id,
        ExecutionChecks {
            authority_revision: 1,
            connection_enabled: true,
            authenticated: true,
            platform_available: wex_local_resources::platform_supported(),
            resource_allowed: true,
        },
        authority,
    )?;
    // A durable "in flight" marker must exist BEFORE touching the file.
    journal(app, chat, &attempt_id, &operation_id, "in_flight", None)?;
    let grants = [DirectoryGrant {
        directory_id: &directory_id,
        access: Access::Read,
        explicit_personal: true,
    }];
    let content = read_approved_file(&dispatch, &registry, leaf, true, &grants);
    let receipt_digest = content
        .as_ref()
        .ok()
        .map(|body| digest_arguments("tool_receipt", &evidence_id, &[body.as_bytes()]));
    journal(
        app,
        chat,
        &attempt_id,
        &operation_id,
        if content.is_ok() {
            "succeeded"
        } else {
            "failed"
        },
        receipt_digest.as_deref(),
    )?;
    let outcome = if content.is_ok() {
        Outcome::Succeeded {
            evidence_ref: evidence_id.clone(),
        }
    } else {
        Outcome::Failed {
            evidence_ref: evidence_id.clone(),
        }
    };
    ledger
        .finish(&operation_id, &attempt_id, outcome)
        .map_err(|_| core_failure())?;
    content.map(|body| (body, evidence_id))
}

/// The user chooses the target repository each time. No AI-supplied cwd,
/// shell expression or arbitrary argument enters the subprocess.
fn review_local_command(
    app: &AppHandle,
    chat: &PreparedChat,
    call: &ToolProposal,
    authority: &impl Fn() -> Result<()>,
) -> Result<String> {
    let args: serde_json::Value = serde_json::from_str(&call.arguments_json)
        .map_err(|_| ServiceError::new("INVALID_INPUT", "命令参数无效"))?;
    if !args.as_object().is_some_and(|o| o.is_empty()) {
        return Err(ServiceError::new(
            "INVALID_INPUT",
            "当前命令不接受模型提供的参数",
        ));
    }
    let (capability, action, command) = match call.name.as_str() {
        "git_status" => (
            "git_status",
            Action::Read,
            "git --no-optional-locks -c core.fsmonitor=false -C <所选仓库目录> status --porcelain=v1 --untracked-files=all",
        ),
        "git_diff" => (
            "git_diff",
            Action::Read,
            "git diff --no-ext-diff --no-textconv --",
        ),
        "cargo_test" => (
            "terminal_cargo_test",
            Action::Write,
            "cargo test --workspace --locked",
        ),
        "cargo_fmt_check" => (
            "terminal_cargo_fmt_check",
            Action::Write,
            "cargo fmt --all -- --check",
        ),
        _ => return Err(ServiceError::new("TOOL_UNSUPPORTED", "不支持的命令")),
    };
    let selected = app
        .dialog()
        .file()
        .set_title(format!("WEXPC 请求执行 {command}：请选择仓库目录"))
        .blocking_pick_folder()
        .ok_or_else(|| ServiceError::new("PERMISSION_DENIED", "未选择授权仓库目录"))?;
    let root = selected
        .into_path()
        .map_err(|_| ServiceError::new("INVALID_INPUT", "仓库位置不可用"))?;
    let root = fs::canonicalize(root)
        .map_err(|_| ServiceError::new("RESOURCE_UNAVAILABLE", "无法验证仓库目录"))?;
    let internal = app
        .path()
        .app_data_dir()
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "应用数据目录不可用"))?;
    if root.starts_with(&internal) {
        return Err(ServiceError::new(
            "PERMISSION_DENIED",
            "禁止执行于 WEXPC 内部数据目录",
        ));
    }
    let cwd = root
        .to_str()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "目录编码不支持"))?;
    let extra = if action == Action::Write {
        "注意：Cargo 构建可能执行仓库自身的构建脚本，仅在信任此代码时批准。"
    } else {
        "仅查询 Git 状态，不执行提交或推送。"
    };
    let approved = app
        .dialog()
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
        id: operation_id.clone(),
        actor_id: "assistant".into(),
        conversation_id: chat.conversation_id.clone(),
        context_id: None,
        connection_id: "terminal".into(),
        capability_id: capability.into(),
        resource_id: resource_id.clone(),
        action,
        expected_version: None,
        intent_digest: digest_arguments(capability, &resource_id, &[cwd.as_bytes()]),
    };
    let mut ledger = OperationLedger::default();
    ledger.propose(intent.clone()).map_err(|_| core_failure())?;
    ledger
        .approve(&operation_id, &intent, 1)
        .map_err(|_| core_failure())?;
    let dispatch = begin_approved_dispatch(
        &mut ledger,
        &operation_id,
        &attempt_id,
        ExecutionChecks {
            authority_revision: 1,
            connection_enabled: true,
            authenticated: true,
            platform_available: true,
            resource_allowed: true,
        },
        authority,
    )?;
    journal(
        app,
        chat,
        &attempt_id,
        &operation_id,
        "in_flight",
        Some(&intent.intent_digest),
    )?;
    let outcome: Result<(bool, Option<i32>, String)> = if call.name == "git_status"
        || call.name == "git_diff"
    {
        if call.name == "git_diff" {
            wex_desktop_service::approved_git_diff(&dispatch, &resource_id, &root)
                .map(|text| (true, None, text))
        } else {
            wex_desktop_service::approved_git_status(&dispatch, &resource_id, &root)
                .map(|text| (true, None, text))
        }
    } else {
        let task = if call.name == "cargo_test" {
            wex_desktop_service::TerminalTask::CargoTest
        } else {
            wex_desktop_service::TerminalTask::CargoFmtCheck
        };
        wex_desktop_service::execute_approved_terminal_report(&dispatch, &resource_id, &root, task)
            .map(|report| (report.success, report.exit_code, report.output))
    };
    let status = match &outcome {
        Ok((true, _, _)) => "succeeded",
        Ok((false, _, _)) => "failed",
        Err(e) if e.code == "OUTCOME_UNKNOWN" || e.code == "TOOL_TIMEOUT" => "unknown",
        Err(_) => "failed",
    };
    let evidence_hash = outcome
        .as_ref()
        .ok()
        .map(|(_, _, value)| digest_arguments("tool_receipt", &evidence_id, &[value.as_bytes()]));
    journal(
        app,
        chat,
        &attempt_id,
        &operation_id,
        status,
        evidence_hash.as_deref(),
    )?;
    let core_outcome = match status {
        "succeeded" => Outcome::Succeeded {
            evidence_ref: evidence_id.clone(),
        },
        "unknown" => Outcome::Unknown,
        _ => Outcome::Failed {
            evidence_ref: evidence_id.clone(),
        },
    };
    ledger
        .finish(&operation_id, &attempt_id, core_outcome)
        .map_err(|_| core_failure())?;
    let (success, exit_code, result) = outcome?;
    Ok(serde_json::json!({"evidence_ref":evidence_id,"success":success,"exit_code":exit_code,"output":result}).to_string())
}

/// Approval always displays the exact repo/path and, for writes, every byte
/// of the bounded text update. The model never sees the GitHub PAT.
fn review_github(
    app: &AppHandle,
    chat: &PreparedChat,
    call: &ToolProposal,
    authority: &impl Fn() -> Result<()>,
) -> Result<String> {
    let args: serde_json::Value = serde_json::from_str(&call.arguments_json)
        .map_err(|_| ServiceError::new("INVALID_INPUT", "GitHub 工具参数无效"))?;
    fn field<'a>(value: &'a serde_json::Value, name: &str) -> Result<&'a str> {
        value
            .get(name)
            .and_then(|v| v.as_str())
            .ok_or_else(|| ServiceError::new("INVALID_INPUT", "GitHub 缺少必要参数"))
    }
    let owner = field(&args, "owner")?;
    let repo = field(&args, "repo")?;
    let path = field(&args, "path")?;
    let write = call.name == "github_write_file";
    let create = call.name == "github_create_file";
    let sha = if write { field(&args, "sha")? } else { "" };
    let content = if write || create {
        field(&args, "content")?
    } else {
        ""
    };
    if owner.is_empty()
        || repo.is_empty()
        || path.is_empty()
        || owner.len() > 255
        || repo.len() > 255
        || path.len() > 4096
        || owner.chars().any(char::is_control)
        || repo.chars().any(char::is_control)
        || path.chars().any(char::is_control)
        || (write && (sha.len() != 40 || content.len() > 4096))
        || (create && content.len() > 4096)
    {
        return Err(ServiceError::new(
            "INVALID_INPUT",
            "GitHub 审批参数或文件内容超过限制",
        ));
    }
    let summary = if write {
        format!("WEXPC 请求修改 GitHub 文件\n仓库：{owner}/{repo}（默认分支）\n文件：{path}\n原 SHA：{sha}\n写入字节数：{}\n完整写入内容：\n{content}",content.len())
    } else if create {
        format!("WEXPC 请求新建 GitHub 文件（仅当路径尚不存在）\n仓库：{owner}/{repo}（默认分支）\n文件：{path}\n写入字节数：{}\n完整写入内容：\n{content}",content.len())
    } else {
        format!("WEXPC 请求读取 GitHub 文件\n仓库：{owner}/{repo}（默认分支）\n文件：{path}")
    };
    let approved = app
        .dialog()
        .message(summary)
        .title("逐次 GitHub 工具授权")
        .buttons(MessageDialogButtons::OkCancel)
        .blocking_show();
    if !approved {
        return Err(ServiceError::new(
            "PERMISSION_DENIED",
            "用户拒绝 GitHub 操作",
        ));
    }
    let token = crate::github_config::credential(app)?;
    let resource_id = digest_arguments(
        "github_repo",
        "global",
        &[owner.as_bytes(), repo.as_bytes()],
    );
    let capability = if write {
        "github_file_write"
    } else if create {
        "github_file_create"
    } else {
        "github_file_read"
    };
    let action = if write || create {
        Action::Write
    } else {
        Action::Read
    };
    let parameters: Vec<&[u8]> = if write {
        vec![
            owner.as_bytes(),
            repo.as_bytes(),
            path.as_bytes(),
            sha.as_bytes(),
            content.as_bytes(),
        ]
    } else if create {
        vec![
            owner.as_bytes(),
            repo.as_bytes(),
            path.as_bytes(),
            content.as_bytes(),
        ]
    } else {
        vec![owner.as_bytes(), repo.as_bytes(), path.as_bytes()]
    };
    let operation_id = format!("op_{}", uuid::Uuid::new_v4().simple());
    let attempt_id = format!("at_{}", uuid::Uuid::new_v4().simple());
    let evidence_id = format!("ev_{}", uuid::Uuid::new_v4().simple());
    let intent = OperationIntent {
        id: operation_id.clone(),
        actor_id: "assistant".into(),
        conversation_id: chat.conversation_id.clone(),
        context_id: None,
        connection_id: "github".into(),
        capability_id: capability.into(),
        resource_id: resource_id.clone(),
        action,
        expected_version: if write { Some(sha.into()) } else { None },
        intent_digest: digest_arguments(capability, &resource_id, &parameters),
    };
    let mut ledger = OperationLedger::default();
    ledger.propose(intent.clone()).map_err(|_| core_failure())?;
    ledger
        .approve(&operation_id, &intent, 1)
        .map_err(|_| core_failure())?;
    let dispatch = begin_approved_dispatch(
        &mut ledger,
        &operation_id,
        &attempt_id,
        ExecutionChecks {
            authority_revision: 1,
            connection_enabled: true,
            authenticated: true,
            platform_available: true,
            resource_allowed: true,
        },
        authority,
    )?;
    journal(
        app,
        chat,
        &attempt_id,
        &operation_id,
        "in_flight",
        Some(&intent.intent_digest),
    )?;
    let outcome = if write {
        wex_desktop_service::update_approved_github_file(
            &dispatch,
            &resource_id,
            owner,
            repo,
            path,
            sha,
            content.as_bytes(),
            &token,
        )
    } else if create {
        wex_desktop_service::create_approved_github_file(
            &dispatch,
            &resource_id,
            owner,
            repo,
            path,
            content.as_bytes(),
            &token,
        )
    } else {
        wex_desktop_service::read_approved_github_file(
            &dispatch,
            &resource_id,
            owner,
            repo,
            path,
            &token,
        )
    };
    let status = match &outcome {
        Ok(_) => "succeeded",
        Err(error) if error.code == "OUTCOME_UNKNOWN" => "unknown",
        Err(_) => "failed",
    };
    let evidence_hash = outcome
        .as_ref()
        .ok()
        .map(|value| digest_arguments("tool_receipt", &evidence_id, &[value.as_bytes()]));
    journal(
        app,
        chat,
        &attempt_id,
        &operation_id,
        status,
        evidence_hash.as_deref(),
    )?;
    let core_outcome = match status {
        "succeeded" => Outcome::Succeeded {
            evidence_ref: evidence_id.clone(),
        },
        "unknown" => Outcome::Unknown,
        _ => Outcome::Failed {
            evidence_ref: evidence_id.clone(),
        },
    };
    ledger
        .finish(&operation_id, &attempt_id, core_outcome)
        .map_err(|_| core_failure())?;
    let body = outcome?;
    Ok(serde_json::json!({"evidence_ref": evidence_id, "result":body}).to_string())
}

fn begin_approved_dispatch(
    ledger: &mut OperationLedger,
    operation: &str,
    attempt: &str,
    checks: ExecutionChecks,
    authority: &impl Fn() -> Result<()>,
) -> Result<wex_core::operation::Dispatch> {
    authority()?;
    ledger
        .begin(operation, attempt, checks)
        .map_err(|_| core_failure())
}

fn core_failure() -> ServiceError {
    ServiceError::new("OPERATION_STATE_ERROR", "Core 审批或派发状态无效")
}

/// Persist bound execution metadata before dispatch and before returning a result.
fn journal(
    app: &AppHandle,
    chat: &PreparedChat,
    attempt: &str,
    operation: &str,
    status: &str,
    digest: Option<&str>,
) -> Result<()> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法定位工具回执目录"))?
        .join("tool-receipts");
    wex_desktop_service::append_tool_marker(
        &root,
        &wex_desktop_service::ToolReceiptMarker {
            operation_id: operation.into(),
            attempt_id: attempt.into(),
            state: status.into(),
            content_digest: digest.map(str::to_owned),
            conversation_id: Some(chat.conversation_id.clone()),
            turn_id: Some(chat.turn_id.clone()),
        },
    )
}

#[cfg(test)]
mod proposal_preflight_tests {
    use super::*;

    fn call(name: &str, arguments_json: &str) -> ToolProposal {
        ToolProposal {
            id: "call_1".into(),
            name: name.into(),
            arguments_json: arguments_json.into(),
        }
    }

    #[test]
    fn local_tools_do_not_depend_on_github_configuration_health() {
        let mut github_checks = 0;
        let calls = vec![
            call("git_status", "{}"),
            call("cargo_test", "{}"),
            call("read_file", r#"{"file_name":"demo.txt"}"#),
        ];
        preflight_batch(&calls, || {
            github_checks += 1;
            Err(ServiceError::new(
                "GITHUB_CONFIG_ERROR",
                "damaged github configuration",
            ))
        })
        .unwrap();
        assert_eq!(github_checks, 0);
        let github = call(
            "github_read_file",
            r#"{"owner":"a","repo":"b","path":"README.md"}"#,
        );
        let error = preflight_batch(&[github], || {
            Err(ServiceError::new(
                "GITHUB_CONFIG_ERROR",
                "damaged github configuration",
            ))
        })
        .unwrap_err();
        assert_eq!(error.code, "GITHUB_CONFIG_ERROR");
    }
    #[test]
    fn revoked_live_authority_prevents_consuming_a_core_approval() {
        let intent = OperationIntent {
            id: "op_1".into(),
            actor_id: "assistant".into(),
            conversation_id: "conv_1".into(),
            context_id: None,
            connection_id: "local".into(),
            capability_id: "local_file_read".into(),
            resource_id: "directory_1".into(),
            action: Action::Read,
            expected_version: None,
            intent_digest: "a".repeat(64),
        };
        let mut ledger = OperationLedger::default();
        ledger.propose(intent.clone()).unwrap();
        ledger.approve("op_1", &intent, 1).unwrap();
        let checks = ExecutionChecks {
            authority_revision: 1,
            connection_enabled: true,
            authenticated: true,
            platform_available: true,
            resource_allowed: true,
        };
        let before = ledger.view();
        let result = begin_approved_dispatch(&mut ledger, "op_1", "attempt_1", checks, &|| {
            Err(ServiceError::new(
                "CONNECTION_UNAVAILABLE",
                "revoked after dialog",
            ))
        });
        assert_eq!(result.unwrap_err().code, "CONNECTION_UNAVAILABLE");
        assert_eq!(ledger.view(), before);
        let dispatch =
            begin_approved_dispatch(&mut ledger, "op_1", "attempt_1", checks, &|| Ok(())).unwrap();
        assert_eq!(dispatch.attempt_id, "attempt_1");
        assert!(
            begin_approved_dispatch(&mut ledger, "op_1", "attempt_2", checks, &|| Ok(())).is_err()
        );
    }
    #[test]
    fn invalid_batch_arguments_are_rejected_before_execution() {
        assert!(preflight_proposal(&call("git_status", "{}"), false).is_ok());
        assert_eq!(
            preflight_proposal(&call("cargo_test", r#"{"command":"shell"}"#), false)
                .unwrap_err()
                .code,
            "INVALID_INPUT"
        );
        assert_eq!(
            preflight_proposal(&call("read_file", "{}"), false)
                .unwrap_err()
                .code,
            "INVALID_INPUT"
        );
        assert_eq!(
            preflight_proposal(&call("unknown", "{}"), false)
                .unwrap_err()
                .code,
            "TOOL_UNSUPPORTED"
        );
    }

    #[test]
    fn github_write_preflight_preserves_multiline_text_and_checks_sha() {
        let body = serde_json::json!({
            "owner":"owner", "repo":"repo", "path":"src/main.rs",
            "sha":"a".repeat(40), "content":"first\\nsecond"
        });
        let proposed = call("github_write_file", &body.to_string());
        assert!(preflight_proposal(&proposed, true).is_ok());
        let bad = serde_json::json!({
            "owner":"owner", "repo":"repo", "path":"file",
            "sha":"invalid", "content":"text"
        });
        assert_eq!(
            preflight_proposal(&call("github_write_file", &bad.to_string()), true)
                .unwrap_err()
                .code,
            "INVALID_INPUT"
        );
    }

    #[test]
    fn github_create_is_distinct_from_versioned_update() {
        let create = call(
            "github_create_file",
            r#"{"owner":"owner","repo":"repo","path":"new.txt","content":"new"}"#,
        );
        assert_eq!(
            preflight_proposal(&create, false).unwrap_err().code,
            "TOOL_UNSUPPORTED"
        );
        assert!(preflight_proposal(&create, true).is_ok());
        let wrong = call(
            "github_create_file",
            r#"{"owner":"owner","repo":"repo","path":"new.txt","sha":"abc","content":"new"}"#,
        );
        assert_eq!(
            preflight_proposal(&wrong, true).unwrap_err().code,
            "INVALID_INPUT"
        );
    }

    #[test]
    fn github_calls_require_enabled_connection_and_exact_schema() {
        let read = call(
            "github_read_file",
            r#"{"owner":"a","repo":"b","path":"README.md"}"#,
        );
        assert_eq!(
            preflight_proposal(&read, false).unwrap_err().code,
            "TOOL_UNSUPPORTED"
        );
        assert!(preflight_proposal(&read, true).is_ok());
        let unexpected = call(
            "github_write_file",
            r#"{"owner":"a","repo":"b","path":"x","sha":"a","content":"x","extra":1}"#,
        );
        assert_eq!(
            preflight_proposal(&unexpected, true).unwrap_err().code,
            "INVALID_INPUT"
        );
    }
}
