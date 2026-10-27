//! Trusted desktop-only approval bridge for the initial local read tool.
/// Selecting the exact file in a native picker is the per-call user approval.
/// No Space/project association can grant access. Nothing is auto-retried.
use std::fs;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use wex_core::{
    access::{Access, DirectoryGrant},
    operation::{Action, ExecutionChecks, OperationIntent, OperationLedger, Outcome},
};
use wex_desktop_service::{
    digest_arguments, drive_tool_rounds_from_history, execute_model_step, read_approved_file,
    PreparedChat, Result, ServiceError, ToolProposal,
};
use wex_local_resources::{DirectoryRegistry, Protection};

pub trait ToolPolicyAuthority {
    fn policy(&self, capability: &str) -> Result<wex_desktop_service::ToolPolicySetting>;
    fn check(&self, setting: &wex_desktop_service::ToolPolicySetting) -> Result<()>;
}

pub fn run_non_streaming(
    app: &AppHandle,
    chat: &PreparedChat,
    key: &str,
    before_step: impl Fn() -> Result<()>,
    policies: &impl ToolPolicyAuthority,
) -> Result<String> {
    run_with_history(app, chat, key, Vec::new(), 0, before_step, policies)
}
pub fn resume_non_streaming(
    app: &AppHandle,
    chat: &PreparedChat,
    key: &str,
    history: Vec<wex_api::Message>,
    sequence: u32,
    before_step: impl Fn() -> Result<()>,
    policies: &impl ToolPolicyAuthority,
) -> Result<String> {
    run_with_history(app, chat, key, history, sequence, before_step, policies)
}
fn run_with_history(
    app: &AppHandle,
    chat: &PreparedChat,
    key: &str,
    history: Vec<wex_api::Message>,
    mut checkpoint_sequence: u32,
    before_step: impl Fn() -> Result<()>,
    policies: &impl ToolPolicyAuthority,
) -> Result<String> {
    let tools = || crate::tool_catalog::available_tools(app, chat, policies);
    drive_tool_rounds_from_history(
        history,
        |history| {
            before_step()?;
            execute_model_step(chat, key, history, tools())
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
                        if call.name == "create_authorized_file" {
                            return Ok((
                                call.id.clone(),
                                review_saved_create(app, chat, call, &before_step, policies)?,
                            ));
                        }
                        if call.name == "read_authorized_file" {
                            return Ok((
                                call.id.clone(),
                                review_saved_read(app, chat, call, &before_step, policies)?,
                            ));
                        }
                        if matches!(
                            call.name.as_str(),
                            "git_status"
                                | "git_diff"
                                | "cargo_test"
                                | "cargo_fmt_check"
                                | "run_command"
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
                                review_github(app, chat, call, &before_step, policies)?,
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
                            review_and_read(app, chat, leaf, &before_step, policies)?;
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

/// Validate a future versioned local edit *without* authorizing dispatch.
/// The edit tool is intentionally not advertised or handled until an atomic
/// Windows/Unix commit path is available.
#[allow(dead_code)]
fn preflight_local_edit_intent(call: &ToolProposal) -> Result<()> {
    if call.name != "update_authorized_file" {
        return Err(ServiceError::new("TOOL_UNSUPPORTED", "不是本地修改请求"));
    }
    let value: serde_json::Value = serde_json::from_str(&call.arguments_json)
        .map_err(|_| ServiceError::new("INVALID_INPUT", "修改参数无效"))?;
    let obj = value
        .as_object()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "修改参数必须是对象"))?;
    if obj.len() != 4
        || !["directory_id", "file_name", "expected_sha256", "content"]
            .iter()
            .all(|key| obj.get(*key).is_some_and(serde_json::Value::is_string))
    {
        return Err(ServiceError::new(
            "INVALID_INPUT",
            "修改参数字段与声明不一致",
        ));
    }
    let intent = wex_core::local_edit::EditIntent {
        directory_id: obj["directory_id"].as_str().unwrap_or_default(),
        file_name: obj["file_name"].as_str().unwrap_or_default(),
        expected_sha256: obj["expected_sha256"].as_str().unwrap_or_default(),
        replacement: obj["content"].as_str().unwrap_or_default(),
    };
    intent
        .validate()
        .map_err(|_| ServiceError::new("INVALID_INPUT", "文件修改版本或路径无效"))
}

fn preflight_proposal(call: &ToolProposal, github_enabled: bool) -> Result<()> {
    if call.name == "run_command" {
        let request: wex_desktop_service::ProcessRequest =
            serde_json::from_str(&call.arguments_json)
                .map_err(|_| ServiceError::new("INVALID_INPUT", "程序执行参数与声明不一致"))?;
        return request.validate();
    }

    let args: serde_json::Value = serde_json::from_str(&call.arguments_json)
        .map_err(|_| ServiceError::new("INVALID_INPUT", "工具参数不是有效 JSON"))?;
    let obj = args
        .as_object()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "工具参数必须是对象"))?;
    let fields: &[&str] = match call.name.as_str() {
        "read_file" => &["file_name"],
        "read_authorized_file" => &["directory_id", "file_name"],
        "create_authorized_file" => &["directory_id", "file_name", "content"],
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
    if matches!(
        call.name.as_str(),
        "read_file" | "read_authorized_file" | "create_authorized_file"
    ) {
        let leaf = obj["file_name"].as_str().unwrap_or_default();
        if leaf.len() > 255
            || leaf == "."
            || leaf == ".."
            || leaf.contains('/')
            || leaf.contains('\\')
        {
            return Err(ServiceError::new(
                "INVALID_INPUT",
                "当前读取工具仅支持目录内直接文件名",
            ));
        }
    }
    if call.name == "create_authorized_file"
        && obj["content"].as_str().is_some_and(|s| s.len() > 65536)
    {
        return Err(ServiceError::new(
            "INVALID_INPUT",
            "本地新建内容超出当前64KiB安全上限",
        ));
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
    policies: &impl ToolPolicyAuthority,
) -> Result<(String, String)> {
    let policy = policies.policy("local_file_read")?;
    policies.check(&policy)?;
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
        .approve(&operation_id, &intent, policy.revision)
        .map_err(|_| core_failure())?;
    let dispatch = begin_approved_dispatch(
        &mut ledger,
        &operation_id,
        &attempt_id,
        ExecutionChecks {
            authority_revision: policy.revision,
            connection_enabled: true,
            authenticated: true,
            platform_available: wex_local_resources::platform_supported(),
            resource_allowed: true,
        },
        &|| {
            authority()?;
            policies.check(&policy)
        },
    )?;
    // A durable "in flight" marker must exist BEFORE touching the file.
    journal(
        app,
        chat,
        &attempt_id,
        &operation_id,
        "in_flight",
        Some(&intent.intent_digest),
        None,
        Some(&policy),
    )?;
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
        Some(&intent.intent_digest),
        receipt_digest.as_deref(),
        Some(&policy),
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

/// Current per-run native approval chooses cwd and displays exact arguments.
/// This does not implement persistent terminal grants or a default shell.
fn review_local_command(
    app: &AppHandle,
    chat: &PreparedChat,
    call: &ToolProposal,
    authority: &impl Fn() -> Result<()>,
) -> Result<String> {
    let args: serde_json::Value = serde_json::from_str(&call.arguments_json)
        .map_err(|_| ServiceError::new("INVALID_INPUT", "命令参数无效"))?;
    let process_request = if call.name == "run_command" {
        let request: wex_desktop_service::ProcessRequest = serde_json::from_value(args)
            .map_err(|_| ServiceError::new("INVALID_INPUT", "程序执行参数无效"))?;
        request.validate()?;
        Some(request)
    } else {
        if !args.as_object().is_some_and(|o| o.is_empty()) {
            return Err(ServiceError::new("INVALID_INPUT", "此工具不接受额外参数"));
        }
        None
    };
    let generic_summary = process_request
        .as_ref()
        .map(|r| serde_json::to_string(r).unwrap_or_default())
        .unwrap_or_default();
    let (capability, action, command) = match call.name.as_str() {
        "run_command" => ("terminal_execute",Action::Write,generic_summary.as_str()),
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
        "程序可按当前系统账号权限产生文件、网络或其它副作用；工作目录选择不构成操作系统沙箱。仅在信任程序与项目时批准。"
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
        intent_digest: if let Some(request) = &process_request {
            request.intent_digest(&resource_id, &root)?
        } else {
            digest_arguments(capability, &resource_id, &[cwd.as_bytes()])
        },
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
        None,
        None,
    )?;
    let cargo_task = match call.name.as_str() {
        "cargo_test" => Some(wex_desktop_service::TerminalTask::CargoTest),
        "cargo_fmt_check" => Some(wex_desktop_service::TerminalTask::CargoFmtCheck),
        _ => None,
    };
    let monitored_request = process_request
        .clone()
        .or_else(|| cargo_task.map(|task| task.process_request()));
    let supervisor = app.state::<crate::Processes>();
    let lease = monitored_request
        .as_ref()
        .map(|request| supervisor.start(&chat.conversation_id, &attempt_id, &operation_id, request))
        .transpose()?;
    let mut process_details = None;
    let mut interrupted_process = false;
    let process_result = if let Some(request) = &process_request {
        Some(
            wex_desktop_service::execute_approved_process_report_with_control(
                &dispatch,
                &resource_id,
                &root,
                request,
                lease.as_ref().expect("process lease").control(),
            ),
        )
    } else {
        cargo_task.map(|task| {
            wex_desktop_service::execute_approved_terminal_process_report_with_control(
                &dispatch,
                &resource_id,
                &root,
                task,
                lease.as_ref().expect("cargo lease").control(),
            )
        })
    };
    let outcome: Result<(bool, Option<i32>, String)> = if let Some(result) = process_result {
        result.and_then(|report| {
            interrupted_process = report.interrupted();
            let output = serde_json::to_string(&report).map_err(|_| {
                ServiceError::new("OUTCOME_UNKNOWN", "进程结果编码失败，须核查副作用")
            })?;
            let result = (report.success, report.exit_code, output);
            process_details = Some(serde_json::to_value(report).map_err(|_| {
                ServiceError::new("OUTCOME_UNKNOWN", "进程回执编码失败，须核查副作用")
            })?);
            Ok(result)
        })
    } else if call.name == "git_diff" {
        wex_desktop_service::approved_git_diff(&dispatch, &resource_id, &root)
            .map(|text| (true, None, text))
    } else {
        wex_desktop_service::approved_git_status(&dispatch, &resource_id, &root)
            .map(|text| (true, None, text))
    };
    let status = match &outcome {
        _ if interrupted_process => "unknown",
        Ok((true, _, _)) => "succeeded",
        Ok((false, _, _)) => "failed",
        Err(e) if e.code == "OUTCOME_UNKNOWN" || e.code == "TOOL_TIMEOUT" => "unknown",
        Err(_) => "failed",
    };
    let evidence_hash = outcome
        .as_ref()
        .ok()
        .map(|(_, _, value)| digest_arguments("tool_receipt", &evidence_id, &[value.as_bytes()]));
    if !interrupted_process {
        journal(
            app,
            chat,
            &attempt_id,
            &operation_id,
            status,
            Some(&intent.intent_digest),
            evidence_hash.as_deref(),
            None,
        )?;
    }
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
    let result = if process_details.is_some() {
        "进程结果见 process 字段；截断标记不改变实际退出状态".to_string()
    } else {
        result
    };
    let body=serde_json::json!({"evidence_ref":evidence_id,"success":success,"exit_code":exit_code,"output":result,"process":process_details}).to_string();
    if interrupted_process {
        let root = app
            .path()
            .app_data_dir()
            .map_err(|_| ServiceError::new("OUTCOME_UNKNOWN", "进程已中断，无法定位回执目录"))?;
        return persist_interrupted_process(
            &root,
            chat,
            call,
            &wex_desktop_service::ToolReceiptMarker {
                operation_id,
                attempt_id,
                state: "unknown".into(),
                content_digest: evidence_hash,
                conversation_id: Some(chat.conversation_id.clone()),
                turn_id: Some(chat.turn_id.clone()),
                policy: None,
            },
            &body,
        );
    }
    Ok(body)
}

/// Unknown is durable before the partial body. Returning an error deliberately
/// prevents the tool driver from issuing another model/tool step automatically.
fn persist_interrupted_process(
    root: &std::path::Path,
    chat: &PreparedChat,
    call: &ToolProposal,
    marker: &wex_desktop_service::ToolReceiptMarker,
    body: &str,
) -> Result<String> {
    wex_desktop_service::append_tool_marker(&root.join("tool-receipts"), marker)?;
    wex_desktop_service::save_tool_result(
        &root.join("tool-results"),
        &chat.conversation_id,
        &chat.turn_id,
        call,
        body,
    )?;
    Err(ServiceError::new(
        "OUTCOME_UNKNOWN",
        "本地进程已取消或中断；已保存可取得的输出，须核查副作用后再继续，未自动重跑",
    ))
}

/// Ask policy displays the exact repo/path and, for writes, every byte
/// of the bounded text update. The model never sees the GitHub PAT.
fn review_github(
    app: &AppHandle,
    chat: &PreparedChat,
    call: &ToolProposal,
    authority: &impl Fn() -> Result<()>,
    policies: &impl ToolPolicyAuthority,
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
    let capability = if write {
        "github_file_write"
    } else if create {
        "github_file_create"
    } else {
        "github_file_read"
    };
    let policy = policies.policy(capability)?;
    policies.check(&policy)?;
    let summary = if write {
        format!("WEXPC 请求修改 GitHub 文件\n仓库：{owner}/{repo}（默认分支）\n文件：{path}\n原 SHA：{sha}\n写入字节数：{}\n完整写入内容：\n{content}",content.len())
    } else if create {
        format!("WEXPC 请求新建 GitHub 文件（仅当路径尚不存在）\n仓库：{owner}/{repo}（默认分支）\n文件：{path}\n写入字节数：{}\n完整写入内容：\n{content}",content.len())
    } else {
        format!("WEXPC 请求读取 GitHub 文件\n仓库：{owner}/{repo}（默认分支）\n文件：{path}")
    };
    authorize_execution_policy(policy.policy, || {
        app.dialog()
            .message(summary)
            .title("GitHub 工具授权")
            .buttons(MessageDialogButtons::OkCancel)
            .blocking_show()
    })?;
    let token = crate::github_config::credential(app)?;
    let resource_id = digest_arguments(
        "github_repo",
        "global",
        &[owner.as_bytes(), repo.as_bytes()],
    );
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
        .approve(&operation_id, &intent, policy.revision)
        .map_err(|_| core_failure())?;
    let dispatch = begin_approved_dispatch(
        &mut ledger,
        &operation_id,
        &attempt_id,
        ExecutionChecks {
            authority_revision: policy.revision,
            connection_enabled: true,
            authenticated: true,
            platform_available: true,
            resource_allowed: true,
        },
        &|| {
            authority()?;
            policies.check(&policy)?;
            if crate::github_config::credential(app)? != token {
                return Err(ServiceError::new(
                    "CONNECTION_CHANGED",
                    "GitHub凭证配置已变化，本次未派发操作",
                ));
            }
            Ok(())
        },
    )?;
    journal(
        app,
        chat,
        &attempt_id,
        &operation_id,
        "in_flight",
        Some(&intent.intent_digest),
        None,
        Some(&policy),
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
        Some(&intent.intent_digest),
        evidence_hash.as_deref(),
        Some(&policy),
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

fn authorize_execution_policy(
    policy: wex_desktop_service::PolicyChoice,
    ask: impl FnOnce() -> bool,
) -> Result<()> {
    use wex_core::execution_policy::{decide, PolicyDecision};
    match decide(policy.into()) {
        PolicyDecision::Execute => Ok(()),
        PolicyDecision::RequestApproval if ask() => Ok(()),
        PolicyDecision::RequestApproval => {
            Err(ServiceError::new("PERMISSION_DENIED", "用户拒绝工具操作"))
        }
        PolicyDecision::Blocked => Err(ServiceError::new(
            "POLICY_BLOCKED",
            "本对话已拦截该工具能力",
        )),
    }
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
    intent_digest: Option<&str>,
    content_digest: Option<&str>,
    policy: Option<&wex_desktop_service::ToolPolicySetting>,
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
            content_digest: content_digest.map(str::to_owned),
            intent_digest: intent_digest.map(str::to_owned),
            conversation_id: Some(chat.conversation_id.clone()),
            turn_id: Some(chat.turn_id.clone()),
            policy: policy.cloned(),
        },
    )
}

/// Saved resource scope and execution strategy are independent authorities.
fn review_saved_read(
    app: &AppHandle,
    chat: &PreparedChat,
    call: &ToolProposal,
    authority: &impl Fn() -> Result<()>,
    policies: &impl ToolPolicyAuthority,
) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(&call.arguments_json)
        .map_err(|_| ServiceError::new("INVALID_INPUT", "读取参数无效"))?;
    let id = value["directory_id"]
        .as_str()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "缺少授权目录"))?;
    let leaf = value["file_name"]
        .as_str()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "缺少文件名"))?;
    let policy = policies.policy("local_file_read")?;
    policies.check(&policy)?;
    let store = crate::local_access::open(app, &chat.conversation_id)?;
    let state = store.load().map_err(crate::local_access::error)?;
    let directory = state
        .directories
        .iter()
        .find(|d| d.id == id)
        .ok_or_else(|| ServiceError::new("PERMISSION_DENIED", "该目录未授权给当前对话"))?;
    let grant = state
        .grants
        .iter()
        .find(|g| g.directory_id == id)
        .ok_or_else(|| ServiceError::new("PERMISSION_DENIED", "目录授权已撤销"))?;
    if !matches!(grant.access, Access::Read | Access::ReadWrite) {
        return Err(ServiceError::new("PERMISSION_DENIED", "当前目录未授权读取"));
    }
    authorize_execution_policy(policy.policy, || {
        app.dialog()
            .message(format!(
                "读取已授权目录中的文件？\n目录：{}\n文件：{leaf}\n对话：{}",
                directory.path.display(),
                chat.conversation_id
            ))
            .title("WEXPC 本地读取确认")
            .buttons(MessageDialogButtons::OkCancel)
            .blocking_show()
    })?;
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
        resource_id: id.into(),
        action: Action::Read,
        expected_version: None,
        intent_digest: digest_arguments("local_file_read", id, &[leaf.as_bytes()]),
    };
    let mut ledger = OperationLedger::default();
    ledger.propose(intent.clone()).map_err(|_| core_failure())?;
    ledger
        .approve(&operation_id, &intent, policy.revision)
        .map_err(|_| core_failure())?;
    let dispatch = begin_approved_dispatch(
        &mut ledger,
        &operation_id,
        &attempt_id,
        ExecutionChecks {
            authority_revision: policy.revision,
            connection_enabled: true,
            authenticated: true,
            platform_available: wex_local_resources::platform_supported(),
            resource_allowed: true,
        },
        &|| {
            authority()?;
            policies.check(&policy)?;
            if store.load().map_err(crate::local_access::error)?.revision != state.revision {
                return Err(ServiceError::new(
                    "AUTHORIZATION_CHANGED",
                    "目录授权已变化，本次未派发读取",
                ));
            }
            Ok(())
        },
    )?;
    journal(
        app,
        chat,
        &attempt_id,
        &operation_id,
        "in_flight",
        Some(&intent.intent_digest),
        None,
        Some(&policy),
    )?;
    // The store checks its revision and live grant under the same lock as I/O.
    let body = crate::local_access::read(&dispatch, &store, state.revision, leaf);
    let digest = body
        .as_ref()
        .ok()
        .map(|b| digest_arguments("tool_receipt", &evidence_id, &[b.as_bytes()]));
    journal(
        app,
        chat,
        &attempt_id,
        &operation_id,
        if body.is_ok() { "succeeded" } else { "failed" },
        Some(&intent.intent_digest),
        digest.as_deref(),
        Some(&policy),
    )?;
    ledger
        .finish(
            &operation_id,
            &attempt_id,
            if body.is_ok() {
                Outcome::Succeeded {
                    evidence_ref: evidence_id.clone(),
                }
            } else {
                Outcome::Failed {
                    evidence_ref: evidence_id.clone(),
                }
            },
        )
        .map_err(|_| core_failure())?;
    let content = body?;
    let sha256 = wex_desktop_service::file_content_sha256(content.as_bytes());
    Ok(serde_json::json!({"content":content,"sha256":sha256,"evidence_ref":evidence_id,"directory_id":id,"authorization_revision":state.revision}).to_string())
}

fn review_saved_create(
    app: &AppHandle,
    chat: &PreparedChat,
    call: &ToolProposal,
    authority: &impl Fn() -> Result<()>,
    policies: &impl ToolPolicyAuthority,
) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(&call.arguments_json)
        .map_err(|_| ServiceError::new("INVALID_INPUT", "新建参数无效"))?;
    let id = value["directory_id"]
        .as_str()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "缺少授权目录"))?;
    let leaf = value["file_name"]
        .as_str()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "缺少文件名"))?;
    let content = value["content"]
        .as_str()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "缺少文件内容"))?;
    let policy = policies.policy("local_file_create")?;
    policies.check(&policy)?;
    let store = crate::local_access::open(app, &chat.conversation_id)?;
    let state = store.load().map_err(crate::local_access::error)?;
    let directory = state
        .directories
        .iter()
        .find(|d| d.id == id)
        .ok_or_else(|| ServiceError::new("PERMISSION_DENIED", "该目录未授权给当前对话"))?;
    let grant = state
        .grants
        .iter()
        .find(|g| g.directory_id == id)
        .ok_or_else(|| ServiceError::new("PERMISSION_DENIED", "目录授权已撤销"))?;
    if !matches!(grant.access, Access::Write | Access::ReadWrite) {
        return Err(ServiceError::new("PERMISSION_DENIED", "当前目录未授权写入"));
    }
    authorize_execution_policy(policy.policy, || {
        app.dialog()
            .message(format!(
                "在授权目录中新建文件（不覆盖已有文件）？\n目录：{}\n文件：{leaf}\n对话：{}",
                directory.path.display(),
                chat.conversation_id
            ))
            .title("WEXPC 本地新建确认")
            .buttons(MessageDialogButtons::OkCancel)
            .blocking_show()
    })?;
    let operation_id = format!("op_{}", uuid::Uuid::new_v4().simple());
    let attempt_id = format!("at_{}", uuid::Uuid::new_v4().simple());
    let evidence_id = format!("ev_{}", uuid::Uuid::new_v4().simple());
    let intent = OperationIntent {
        id: operation_id.clone(),
        actor_id: "assistant".into(),
        conversation_id: chat.conversation_id.clone(),
        context_id: None,
        connection_id: "local".into(),
        capability_id: "local_file_create".into(),
        resource_id: id.into(),
        action: Action::Write,
        expected_version: None,
        intent_digest: digest_arguments(
            "local_file_create",
            id,
            &[leaf.as_bytes(), content.as_bytes()],
        ),
    };
    let mut ledger = OperationLedger::default();
    ledger.propose(intent.clone()).map_err(|_| core_failure())?;
    ledger
        .approve(&operation_id, &intent, policy.revision)
        .map_err(|_| core_failure())?;
    let dispatch = begin_approved_dispatch(
        &mut ledger,
        &operation_id,
        &attempt_id,
        ExecutionChecks {
            authority_revision: policy.revision,
            connection_enabled: true,
            authenticated: true,
            platform_available: wex_local_resources::platform_supported(),
            resource_allowed: true,
        },
        &|| {
            authority()?;
            policies.check(&policy)?;
            if store.load().map_err(crate::local_access::error)?.revision != state.revision {
                return Err(ServiceError::new(
                    "AUTHORIZATION_CHANGED",
                    "目录授权已变化，本次未派发新建",
                ));
            }
            Ok(())
        },
    )?;
    journal(
        app,
        chat,
        &attempt_id,
        &operation_id,
        "in_flight",
        Some(&intent.intent_digest),
        None,
        Some(&policy),
    )?;
    // The store checks its revision and live grant under the same lock as I/O.
    let body = crate::local_access::create(&dispatch, &store, state.revision, leaf, content);
    let digest = body.as_ref().ok().map(|_| {
        digest_arguments(
            "tool_receipt",
            &evidence_id,
            &[leaf.as_bytes(), content.as_bytes()],
        )
    });
    journal(
        app,
        chat,
        &attempt_id,
        &operation_id,
        if body.is_ok() { "succeeded" } else { "unknown" },
        Some(&intent.intent_digest),
        digest.as_deref(),
        Some(&policy),
    )?;
    ledger
        .finish(
            &operation_id,
            &attempt_id,
            if body.is_ok() {
                Outcome::Succeeded {
                    evidence_ref: evidence_id.clone(),
                }
            } else {
                Outcome::Unknown
            },
        )
        .map_err(|_| core_failure())?;
    body?;
    Ok(serde_json::json!({"created":true,"file_name":leaf,"evidence_ref":evidence_id,"directory_id":id,"authorization_revision":state.revision}).to_string())
}

#[cfg(test)]
mod proposal_preflight_tests {
    use super::*;

    #[test]
    fn interrupted_process_persists_unknown_and_output_without_model_continuation() {
        use wex_desktop_service::*;
        let root =
            std::env::temp_dir().join(format!("wex-interrupted-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).unwrap();
        let chat = PreparedChat {
            conversation_id: "conv_1".into(),
            turn_id: "turn_1".into(),
            connection_id: "conn_1".into(),
            model_id: "model".into(),
            base_url: "https://example.test".into(),
            credential_ref: "vault".into(),
            messages: vec![ApiMessage {
                role: "user".into(),
                content: "run".into(),
            }],
        };
        let proposal = call(
            "run_command",
            r#"{"program":"compiler","arguments":[],"timeout_ms":1000}"#,
        );
        let marker = ToolReceiptMarker {
            operation_id: "op_1".into(),
            attempt_id: "at_1".into(),
            state: "unknown".into(),
            content_digest: None,
            conversation_id: Some(chat.conversation_id.clone()),
            turn_id: Some(chat.turn_id.clone()),
            policy: None,
        };
        let body =
            r#"{"success":false,"process":{"termination":"cancelled","stdout":"partial output"}}"#;
        let mut requests = 0;
        let mut sequence = 0;
        let result = drive_tool_rounds_from_history(
            vec![],
            |_| {
                requests += 1;
                Ok(ModelStep::Proposal {
                    assistant_text: None,
                    calls: vec![proposal.clone()],
                })
            },
            |calls| {
                persist_interrupted_process(&root, &chat, &calls[0], &marker, body)
                    .map(|result| vec![(calls[0].id.clone(), result)])
            },
            |history, pending| {
                save_tool_checkpoint(&root.join("checkpoints"), &chat, sequence, history, pending)?;
                sequence += 1;
                Ok(())
            },
        );
        assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
        assert_eq!(requests, 1);
        assert_eq!(sequence, 1);
        assert_eq!(
            read_tool_markers(&root.join("tool-receipts"), "conv_1").unwrap()[0].state,
            "unknown"
        );
        assert_eq!(
            read_tool_results(&root.join("tool-results"), "conv_1", "turn_1").unwrap()[0].result,
            body
        );
        assert!(load_tool_checkpoint(&root.join("checkpoints"), &chat).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generic_process_arguments_are_typed_and_do_not_depend_on_github() {
        assert!(preflight_proposal(
            &call(
                "run_command",
                r#"{"program":"python","arguments":["-m","pytest"],"timeout_ms":120000}"#
            ),
            false
        )
        .is_ok());
        for args in [
            r#"{"program":"python","arguments":"-m pytest","timeout_ms":120000}"#,
            r#"{"program":"python","arguments":[],"timeout_ms":0}"#,
            r#"{"program":"python","arguments":[],"timeout_ms":120000,"cwd":"C:/unapproved"}"#,
        ] {
            assert!(preflight_proposal(&call("run_command", args), false).is_err());
        }
    }

    fn call(name: &str, arguments_json: &str) -> ToolProposal {
        ToolProposal {
            id: "call_1".into(),
            name: name.into(),
            arguments_json: arguments_json.into(),
        }
    }

    #[test]
    fn persisted_policy_change_after_approval_blocks_the_real_dispatch_boundary() {
        use wex_desktop_service::{DesktopService, NewConnection, PolicyChoice};
        let mut service = DesktopService::memory().unwrap();
        let c = service
            .add_connection(
                NewConnection {
                    name: "API".into(),
                    base_url: "https://api.deepseek.com".into(),
                    models: vec!["deepseek-chat".into()],
                },
                "vault".into(),
            )
            .unwrap();
        let conv = service
            .create_conversation(None, "A".into(), c.id, "deepseek-chat".into())
            .unwrap();
        let selected = service
            .set_tool_policy(&conv.id, "github_file_write", 0, PolicyChoice::Automatic)
            .unwrap();
        let shared = std::sync::Arc::new(std::sync::Mutex::new(service));
        let policies = crate::ConversationPolicies {
            shared: &shared,
            conversation: &conv.id,
        };
        let intent = OperationIntent {
            id: "op_1".into(),
            actor_id: "assistant".into(),
            conversation_id: conv.id.clone(),
            context_id: None,
            connection_id: "github".into(),
            capability_id: "github_file_write".into(),
            resource_id: "repo".into(),
            action: Action::Write,
            expected_version: Some("a".repeat(40)),
            intent_digest: "a".repeat(64),
        };
        let mut ledger = OperationLedger::default();
        ledger.propose(intent.clone()).unwrap();
        ledger.approve("op_1", &intent, selected.revision).unwrap();
        authorize_execution_policy(selected.policy, || {
            panic!("automatic policy must not open a dialog")
        })
        .unwrap();
        shared
            .lock()
            .unwrap()
            .set_tool_policy(
                &conv.id,
                "github_file_write",
                selected.revision,
                PolicyChoice::Block,
            )
            .unwrap();
        let before = ledger.view();
        let checks = ExecutionChecks {
            authority_revision: selected.revision,
            connection_enabled: true,
            authenticated: true,
            platform_available: true,
            resource_allowed: true,
        };
        let result = begin_approved_dispatch(&mut ledger, "op_1", "at_1", checks, &|| {
            policies.check(&selected)
        });
        assert_eq!(result.unwrap_err().code, "POLICY_CHANGED");
        assert_eq!(ledger.view(), before);
    }
    #[test]
    fn execution_modes_do_not_conflate_policy_with_resource_authorization() {
        use wex_desktop_service::PolicyChoice;
        let mut prompts = 0;
        authorize_execution_policy(PolicyChoice::Automatic, || {
            prompts += 1;
            false
        })
        .unwrap();
        assert_eq!(prompts, 0);
        assert_eq!(
            authorize_execution_policy(PolicyChoice::Block, || {
                prompts += 1;
                true
            })
            .unwrap_err()
            .code,
            "POLICY_BLOCKED"
        );
        assert_eq!(prompts, 0);
        authorize_execution_policy(PolicyChoice::Ask, || {
            prompts += 1;
            true
        })
        .unwrap();
        assert_eq!(prompts, 1);
        assert_eq!(
            authorize_execution_policy(PolicyChoice::Ask, || false)
                .unwrap_err()
                .code,
            "PERMISSION_DENIED"
        );
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
    fn saved_directory_read_has_exact_parameters_and_cannot_become_a_path_authorization_tool() {
        let valid = call(
            "read_authorized_file",
            r#"{"directory_id":"opaque","file_name":"file.txt"}"#,
        );
        assert!(preflight_proposal(&valid, false).is_ok());
        for json in [
            r#"{"directory_id":"opaque","file_name":"../secret"}"#,
            r#"{"directory_id":"opaque","file_name":"sub/file"}"#,
            r#"{"directory_id":"opaque","file_name":"file","path":"/new/root"}"#,
        ] {
            assert_eq!(
                preflight_proposal(&call("read_authorized_file", json), false)
                    .unwrap_err()
                    .code,
                "INVALID_INPUT"
            );
        }
    }

    #[test]
    fn existing_file_edit_schema_is_validated_but_never_dispatched() {
        let sha = "a".repeat(64);
        let valid = call(
            "update_authorized_file",
            &serde_json::json!({
                "directory_id":"saved", "file_name":"main.rs",
                "expected_sha256": sha, "content":"new content"
            })
            .to_string(),
        );
        assert!(preflight_local_edit_intent(&valid).is_ok());
        assert_eq!(
            preflight_proposal(&valid, false).unwrap_err().code,
            "TOOL_UNSUPPORTED"
        );
        for payload in [
            serde_json::json!({"directory_id":"saved","file_name":"../secret","expected_sha256":sha,"content":"x"}),
            serde_json::json!({"directory_id":"saved","file_name":"main.rs","expected_sha256":"old","content":"x"}),
            serde_json::json!({"directory_id":"saved","file_name":"main.rs","expected_sha256":sha,"content":"x","path":"/tmp"}),
        ] {
            assert_eq!(
                preflight_local_edit_intent(&call("update_authorized_file", &payload.to_string()))
                    .unwrap_err()
                    .code,
                "INVALID_INPUT"
            );
        }
    }

    #[test]
    fn local_create_requires_exact_scope_and_no_traversal_or_extra_arguments() {
        assert!(preflight_proposal(
            &call(
                "create_authorized_file",
                r#"{"directory_id":"saved","file_name":"new.rs","content":"fn main() {}"}"#
            ),
            false
        )
        .is_ok());
        for bad in [
            r#"{"directory_id":"saved","file_name":"../escape","content":"x"}"#,
            r#"{"directory_id":"saved","file_name":"nested/file","content":"x"}"#,
            r#"{"directory_id":"saved","file_name":"new.rs","content":"x","path":"/tmp/other"}"#,
            r#"{"directory_id":"saved","file_name":"new.rs"}"#,
        ] {
            assert_eq!(
                preflight_proposal(&call("create_authorized_file", bad), false)
                    .unwrap_err()
                    .code,
                "INVALID_INPUT"
            );
        }
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
