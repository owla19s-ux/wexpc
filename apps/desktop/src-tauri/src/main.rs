#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod github_config;
mod local_access;
mod tool_approvals;
mod tool_catalog;
mod vault;

use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};
use wex_desktop_service::*;

type Shared = Arc<Mutex<DesktopService>>;
type Processes = Arc<ProcessSupervisor>;

#[tauri::command]
fn active_processes(
    processes: State<'_, Processes>,
    conversation_id: String,
) -> Result<Vec<ActiveProcess>> {
    processes.list(&conversation_id)
}
#[tauri::command]
fn cancel_process(
    processes: State<'_, Processes>,
    conversation_id: String,
    attempt_id: String,
) -> Result<()> {
    processes.cancel(&conversation_id, &attempt_id)
}
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatDelta {
    turn_id: String,
    conversation_id: String,
    delta: String,
}
fn locked(shared: &Shared) -> Result<std::sync::MutexGuard<'_, DesktopService>> {
    shared
        .lock()
        .map_err(|_| ServiceError::new("SERVICE_UNAVAILABLE", "桌面服务暂不可用，请重新打开应用"))
}

#[tauri::command]
fn rename_space(
    state: State<'_, Shared>,
    revision: u64,
    space_id: String,
    name: String,
) -> Result<()> {
    locked(&state)?.rename_space(revision, &space_id, name)
}
#[tauri::command]
fn rename_group(
    state: State<'_, Shared>,
    revision: u64,
    group_id: String,
    name: String,
) -> Result<()> {
    locked(&state)?.rename_group(revision, &group_id, name)
}
#[tauri::command]
fn update_context(
    state: State<'_, Shared>,
    revision: u64,
    context_id: String,
    name: String,
    goal: String,
    resources: Vec<String>,
) -> Result<()> {
    locked(&state)?.update_context(revision, &context_id, name, goal, resources)
}
#[tauri::command]
fn update_project(
    state: State<'_, Shared>,
    revision: u64,
    project_id: String,
    name: String,
    goal: String,
    resources: Vec<String>,
) -> Result<()> {
    locked(&state)?.update_project(revision, &project_id, name, goal, resources)
}

#[tauri::command]
fn bind_conversation_context(
    state: State<'_, Shared>,
    conversation_id: String,
    context_id: Option<String>,
) -> Result<()> {
    locked(&state)?.bind_conversation_context(&conversation_id, context_id)
}

#[tauri::command]
fn snapshot(state: State<'_, Shared>) -> Result<DesktopSnapshot> {
    locked(&state)?.snapshot()
}

#[tauri::command]
fn create_project(
    state: State<'_, Shared>,
    name: String,
    goal: String,
    resources: Vec<String>,
) -> Result<Project> {
    locked(&state)?.create_project(name, goal, resources)
}

#[tauri::command]
fn create_space(state: State<'_, Shared>, name: String) -> Result<Space> {
    locked(&state)?.create_space(name)
}
#[tauri::command]
fn create_group(state: State<'_, Shared>, space_id: String, name: String) -> Result<Group> {
    locked(&state)?.create_group(&space_id, name)
}
#[tauri::command]
fn create_context(
    state: State<'_, Shared>,
    name: String,
    goal: String,
    resources: Vec<String>,
    space_id: String,
    group_id: Option<String>,
) -> Result<Context> {
    locked(&state)?.create_context(name, goal, resources, &space_id, group_id)
}
#[tauri::command]
fn move_context(
    state: State<'_, Shared>,
    context_id: String,
    space_id: String,
    group_id: Option<String>,
) -> Result<()> {
    locked(&state)?.move_context(&context_id, &space_id, group_id)
}
#[tauri::command]
fn bind_project_context(
    state: State<'_, Shared>,
    project_id: String,
    context_id: Option<String>,
) -> Result<()> {
    locked(&state)?.bind_project_context(&project_id, context_id)
}

#[tauri::command]
fn add_connection(
    state: State<'_, Shared>,
    input: NewConnection,
    api_key: String,
) -> Result<AiConnection> {
    chat_endpoint(&input.base_url)?;
    if api_key.trim().is_empty() {
        return Err(ServiceError::new("INVALID_INPUT", "请填写 API 密钥"));
    }
    let reference = format!("vault_{}", uuid::Uuid::new_v4().simple());
    let mut app = locked(&state)?;
    vault::save(&reference, &api_key)?;
    match app.add_connection(input, reference.clone()) {
        Ok(connection) => Ok(connection),
        Err(error) => {
            vault::remove(&reference);
            Err(error)
        }
    }
}

#[tauri::command]
fn configure_github(app: tauri::AppHandle, token: String) -> Result<()> {
    github_config::configure(&app, &token)
}
#[tauri::command]
fn set_github_enabled(app: tauri::AppHandle, enabled: bool) -> Result<()> {
    github_config::set_enabled(&app, enabled)
}
#[tauri::command]
fn github_status(app: tauri::AppHandle) -> Result<github_config::GithubStatus> {
    github_config::status(&app)
}
#[tauri::command]
fn revoke_github(app: tauri::AppHandle) -> Result<()> {
    github_config::revoke(&app)
}

#[tauri::command]
fn set_connection_enabled(
    state: State<'_, Shared>,
    connection_id: String,
    enabled: bool,
) -> Result<()> {
    locked(&state)?.set_connection_enabled(&connection_id, enabled)
}

#[tauri::command]
fn member_brief(state: State<'_, Shared>, agent_id: String) -> Result<MemberWorkBrief> {
    locked(&state)?.member_brief(&agent_id)
}

#[tauri::command]
fn add_agent(state: State<'_, Shared>, input: NewAgent) -> Result<Agent> {
    locked(&state)?.add_agent(input)
}

#[tauri::command]
fn assign_agent_duty(
    state: State<'_, Shared>,
    agent_id: String,
    project_id: Option<String>,
    duty_id: String,
) -> Result<AgentDutyAssignment> {
    locked(&state)?.assign_agent_duty(&agent_id, project_id, duty_id)
}

#[tauri::command]
fn create_task(state: State<'_, Shared>, input: NewTask) -> Result<Task> {
    locked(&state)?.create_task(input)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskAdmissionPreview {
    task_id: String,
    project_id: String,
    agent_id: String,
    assignment_epoch: u64,
}

#[tauri::command]
fn queue_dispatch_attempt(
    state: State<'_, Shared>,
    revision: u64,
    task_id: String,
    agent_id: String,
) -> Result<DispatchAttempt> {
    locked(&state)?.queue_dispatch_attempt(revision, &task_id, &agent_id)
}

#[tauri::command]
fn preview_task_dispatch(
    state: State<'_, Shared>,
    revision: u64,
    task_id: String,
    agent_id: String,
) -> Result<TaskAdmissionPreview> {
    let ticket = locked(&state)?.task_dispatch_candidate(revision, &task_id, &agent_id)?;
    Ok(TaskAdmissionPreview {
        task_id: ticket.task_id,
        project_id: ticket.project_id,
        agent_id: ticket.agent_id,
        assignment_epoch: ticket.assignment_epoch,
    })
}

#[tauri::command]
fn publish_task(state: State<'_, Shared>, revision: u64, task_id: String) -> Result<Task> {
    locked(&state)?.publish_task(revision, &task_id)
}

#[tauri::command]
fn update_task(
    state: State<'_, Shared>,
    revision: u64,
    task_id: String,
    input: NewTask,
) -> Result<()> {
    locked(&state)?.update_task(revision, &task_id, input)
}

#[tauri::command]
fn append_task_record(
    state: State<'_, Shared>,
    revision: u64,
    input: NewTaskRecord,
) -> Result<TaskRecord> {
    locked(&state)?.append_task_record(revision, input)
}

#[tauri::command]
fn retry_chat_result(state: State<'_, Shared>, turn_id: String) -> Result<()> {
    locked(&state)?.retry_chat_result(&turn_id)
}

#[tauri::command]
fn create_conversation(
    state: State<'_, Shared>,
    project_id: Option<String>,
    title: String,
    connection_id: String,
    model_id: String,
) -> Result<Conversation> {
    locked(&state)?.create_conversation(project_id, title, connection_id, model_id)
}

#[tauri::command]
async fn send_message(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    conversation_id: String,
    prompt: String,
    stream: bool,
) -> Result<()> {
    let shared = state.inner().clone();
    let reply_root = app
        .path()
        .app_data_dir()
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法定位最终回复目录"))?
        .join("chat-replies");
    let request = locked(&shared)?.prepare_chat(&conversation_id, prompt)?;
    let backup_request = request.clone();
    let guard = shared.clone();
    // The SQLite mutex is released before network I/O. Each conversation can
    // progress independently; only one request at a time is admitted per session.
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        locked(&guard)?.check_tool_reply_binding(&request)?;
        let key = vault::read(&request.credential_ref)?;
        if stream {
            execute_chat_stream_with_progress(&request, &key, |delta| {
                let _ = app.emit(
                    "chat-delta",
                    ChatDelta {
                        turn_id: request.turn_id.clone(),
                        conversation_id: request.conversation_id.clone(),
                        delta: delta.to_owned(),
                    },
                );
            })
        } else {
            tool_approvals::run_non_streaming(
                &app,
                &request,
                &key,
                || locked(&guard)?.check_tool_reply_binding(&request),
                &ConversationPolicies {
                    shared: &guard,
                    conversation: &request.conversation_id,
                },
            )
        }
    })
    .await
    .unwrap_or_else(|_| {
        Err(ServiceError::new(
            "REQUEST_INTERRUPTED",
            "请求线程中断，未自动重试",
        ))
    });
    settle_chat_result(&shared, &backup_request, &reply_root, outcome)
}

#[tauri::command]
fn tool_receipts(app: tauri::AppHandle, conversation_id: String) -> Result<Vec<ToolReceiptMarker>> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法定位工具回执目录"))?;
    read_tool_markers(&root.join("tool-receipts"), &conversation_id)
}

#[tauri::command]
fn tool_results(
    app: tauri::AppHandle,
    conversation_id: String,
    turn_id: String,
) -> Result<Vec<SavedToolResult>> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| ServiceError::new("TOOL_RESULT_STORAGE_ERROR", "无法定位工具回执目录"))?;
    read_tool_results(&root.join("tool-results"), &conversation_id, &turn_id)
}

fn settle_chat_result(
    shared: &Shared,
    request: &PreparedChat,
    root: &std::path::Path,
    outcome: Result<String>,
) -> Result<()> {
    match outcome {
        Ok(text) => locked(shared)?.finish_chat_with_backup(root, request, text),
        Err(error) => {
            locked(shared)?.fail_chat(&request.turn_id, &error)?;
            Err(error)
        }
    }
}
fn saved_reply(
    app: &tauri::AppHandle,
    shared: &Shared,
    conversation: &str,
    turn: &str,
) -> Result<Option<(String, String)>> {
    let user = locked(shared)?.saved_reply_candidate(conversation, turn)?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| ServiceError::new("SAVED_REPLY_ERROR", "无法定位最终回复目录"))?;
    Ok(
        read_chat_reply(&root.join("chat-replies"), conversation, turn, &user)?
            .map(|text| (user, text)),
    )
}
#[tauri::command]
fn restore_saved_chat_reply(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    conversation_id: String,
    turn_id: String,
) -> Result<()> {
    let (user, text) = saved_reply(&app, &state, &conversation_id, &turn_id)?
        .ok_or_else(|| ServiceError::new("NO_SAVED_REPLY", "没有已保存的最终回复"))?;
    locked(&state)?.restore_saved_reply(&conversation_id, &turn_id, &user, text)
}

fn restored_reply(
    app: &tauri::AppHandle,
    shared: &Shared,
    conversation: &str,
    turn: &str,
) -> Result<(PreparedChat, Vec<wex_api::Message>, u32)> {
    if saved_reply(app, shared, conversation, turn)?.is_some() {
        return Err(ServiceError::new(
            "SAVED_REPLY_AVAILABLE",
            "已有完整回复，请先本地恢复，不需要再次请求模型",
        ));
    }
    let current = locked(shared)?.tool_reply_candidate(conversation, turn)?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| ServiceError::new("TOOL_CHECKPOINT_ERROR", "无法定位检查点目录"))?;
    restore_tool_checkpoint_with_cursor(&root.join("tool-checkpoints"), &current)
}
#[derive(serde::Serialize)]
struct ReplyRecovery {
    available: bool,
    mode: String,
    reason: String,
}
#[tauri::command]
fn tool_reply_recovery(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    conversation_id: String,
    turn_id: String,
) -> ReplyRecovery {
    match saved_reply(&app, &state, &conversation_id, &turn_id) {
        Ok(Some(_)) => {
            return ReplyRecovery {
                available: true,
                mode: "local".into(),
                reason: "已有完整回复，只恢复本地记录，不请求API，不依赖连接启用".into(),
            }
        }
        Err(error) => {
            return ReplyRecovery {
                available: false,
                mode: "none".into(),
                reason: error.message,
            }
        }
        Ok(None) => {}
    }
    match restored_reply(&app, &state, &conversation_id, &turn_id) {
        Ok(_) => ReplyRecovery {
            available: true,
            mode: "model".into(),
            reason: "将原上下文和已保存工具回执发回原连接，可选择继续工具流程；新的工具请求仍按当前审批，历史回执不会自动派发".into(),
        },
        Err(error) => ReplyRecovery {
            available: false,
            mode: "none".into(),
            reason: error.message,
        },
    }
}
#[tauri::command]
async fn resume_tool_reply(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    conversation_id: String,
    turn_id: String,
    continue_tools: bool,
) -> Result<()> {
    let shared = state.inner().clone();
    let (frozen, history, sequence) = restored_reply(&app, &shared, &conversation_id, &turn_id)?;
    let reply_root = app
        .path()
        .app_data_dir()
        .map_err(|_| ServiceError::new("SAVED_REPLY_ERROR", "无法定位最终回复目录"))?
        .join("chat-replies");
    let request = locked(&shared)?.begin_tool_reply_resume(frozen)?;
    let backup_request = request.clone();
    let guard = shared.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        locked(&guard)?.check_tool_reply_binding(&request)?;
        let key = vault::read(&request.credential_ref)?;
        if continue_tools {
            return tool_approvals::resume_non_streaming(
                &app,
                &request,
                &key,
                history,
                sequence,
                || locked(&guard)?.check_tool_reply_binding(&request),
                &ConversationPolicies {
                    shared: &guard,
                    conversation: &request.conversation_id,
                },
            );
        }
        match execute_model_step(&request, &key, history, vec![])? {
            ModelStep::Text(text) => Ok(text),
            ModelStep::Proposal { .. } => Err(ServiceError::new(
                "RECOVERY_NEEDS_NEW_TOOLS",
                "模型请求了新工具；本次仅恢复回复，未派发工具。请查看已保存回执并在对话中继续处理",
            )),
        }
    })
    .await
    .unwrap_or_else(|_| {
        Err(ServiceError::new(
            "REQUEST_INTERRUPTED",
            "恢复请求中断，未重跑工具",
        ))
    });
    settle_chat_result(&shared, &backup_request, &reply_root, outcome)
}

struct ConversationPolicies<'a> {
    shared: &'a Shared,
    conversation: &'a str,
}
impl tool_approvals::ToolPolicyAuthority for ConversationPolicies<'_> {
    fn policy(&self, capability: &str) -> Result<ToolPolicySetting> {
        locked(self.shared)?.tool_policy(self.conversation, capability)
    }
    fn check(&self, setting: &ToolPolicySetting) -> Result<()> {
        if setting.conversation_id != self.conversation {
            return Err(ServiceError::new(
                "POLICY_CHANGED",
                "执行策略不属于当前对话",
            ));
        }
        locked(self.shared)?.check_tool_policy(setting)
    }
}
#[tauri::command]
fn github_tool_policies(
    state: State<'_, Shared>,
    conversation_id: String,
) -> Result<Vec<ToolPolicySetting>> {
    let service = locked(&state)?;
    [
        "github_file_read",
        "github_file_create",
        "github_file_write",
    ]
    .iter()
    .map(|cap| service.tool_policy(&conversation_id, cap))
    .collect()
}
#[tauri::command]
fn set_github_tool_policy(
    state: State<'_, Shared>,
    conversation_id: String,
    capability_id: String,
    expected_revision: u64,
    policy: PolicyChoice,
) -> Result<ToolPolicySetting> {
    locked(&state)?.set_tool_policy(&conversation_id, &capability_id, expected_revision, policy)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            let service = DesktopService::open(&root.join("desktop.sqlite"))?;
            app.manage(Arc::new(Mutex::new(service)));
            app.manage(Arc::new(ProcessSupervisor::default()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            rename_space,
            rename_group,
            update_context,
            update_project,
            bind_conversation_context,
            snapshot,
            create_project,
            add_connection,
            set_connection_enabled,
            configure_github,
            set_github_enabled,
            github_status,
            revoke_github,
            local_access::local_directory_status,
            local_access::authorize_local_directory,
            local_access::edit_local_directory,
            local_access::revoke_local_directory,
            local_access::local_read_policy,
            local_access::set_local_read_policy,
            local_access::local_create_policy,
            local_access::set_local_create_policy,
            github_tool_policies,
            set_github_tool_policy,
            add_agent,
            assign_agent_duty,
            member_brief,
            create_task,
            update_task,
            append_task_record,
            publish_task,
            preview_task_dispatch,
            queue_dispatch_attempt,
            create_conversation,
            send_message,
            active_processes,
            cancel_process,
            tool_receipts,
            tool_results,
            tool_reply_recovery,
            resume_tool_reply,
            restore_saved_chat_reply,
            retry_chat_result,
            create_space,
            create_group,
            create_context,
            move_context,
            bind_project_context
        ])
        .run(tauri::generate_context!())
        .expect("WEXPC desktop startup failed");
}
