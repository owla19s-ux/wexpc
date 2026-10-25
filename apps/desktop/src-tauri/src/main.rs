#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod github_config;
mod tool_approvals;
mod vault;

use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};
use wex_desktop_service::*;

type Shared = Arc<Mutex<DesktopService>>;
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
    let request = locked(&shared)?.prepare_chat(&conversation_id, prompt)?;
    // The SQLite mutex is released before network I/O. Each conversation can
    // progress independently; only one request at a time is admitted per session.
    let turn_id = request.turn_id.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
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
            tool_approvals::run_non_streaming(&app, &request, &key)
        }
    })
    .await
    .unwrap_or_else(|_| {
        Err(ServiceError::new(
            "REQUEST_INTERRUPTED",
            "请求线程中断，未自动重试",
        ))
    });
    let mut app = locked(&shared)?;
    match outcome {
        Ok(response) => app.finish_chat(&turn_id, response),
        Err(error) => {
            app.fail_chat(&turn_id, &error)?;
            Err(error)
        }
    }
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

fn restored_reply(
    app: &tauri::AppHandle,
    shared: &Shared,
    conversation: &str,
    turn: &str,
) -> Result<(PreparedChat, Vec<wex_api::Message>)> {
    let current = locked(shared)?.tool_reply_candidate(conversation, turn)?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| ServiceError::new("TOOL_CHECKPOINT_ERROR", "无法定位检查点目录"))?;
    restore_tool_checkpoint(&root.join("tool-checkpoints"), &current)
}
#[derive(serde::Serialize)]
struct ReplyRecovery {
    available: bool,
    reason: String,
}
#[tauri::command]
fn tool_reply_recovery(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    conversation_id: String,
    turn_id: String,
) -> ReplyRecovery {
    match restored_reply(&app, &state, &conversation_id, &turn_id) {
        Ok(_) => ReplyRecovery {
            available: true,
            reason: "将原上下文和已保存工具回执发回原连接恢复回复，不重新执行工具".into(),
        },
        Err(error) => ReplyRecovery {
            available: false,
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
) -> Result<()> {
    let shared = state.inner().clone();
    let (frozen, history) = restored_reply(&app, &shared, &conversation_id, &turn_id)?;
    let request = locked(&shared)?.begin_tool_reply_resume(frozen)?;
    let guard = shared.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        locked(&guard)?.check_tool_reply_binding(&request)?;
        let key = vault::read(&request.credential_ref)?;
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
    let mut service = locked(&shared)?;
    match outcome {
        Ok(text) => service.finish_chat(&turn_id, text),
        Err(error) => {
            service.fail_chat(&turn_id, &error)?;
            Err(error)
        }
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            let service = DesktopService::open(&root.join("desktop.sqlite"))?;
            app.manage(Arc::new(Mutex::new(service)));
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
            add_agent,
            assign_agent_duty,
            member_brief,
            create_task,
            update_task,
            append_task_record,
            create_conversation,
            send_message,
            tool_receipts,
            tool_results,
            tool_reply_recovery,
            resume_tool_reply,
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
