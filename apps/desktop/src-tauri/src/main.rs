#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod vault;

use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use wex_desktop_service::*;

type Shared = Arc<Mutex<DesktopService>>;
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
fn set_connection_enabled(
    state: State<'_, Shared>,
    connection_id: String,
    enabled: bool,
) -> Result<()> {
    locked(&state)?.set_connection_enabled(&connection_id, enabled)
}

#[tauri::command]
fn add_agent(state: State<'_, Shared>, input: NewAgent) -> Result<Agent> {
    locked(&state)?.add_agent(input)
}

#[tauri::command]
fn create_task(state: State<'_, Shared>, input: NewTask) -> Result<Task> {
    locked(&state)?.create_task(input)
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
    state: State<'_, Shared>,
    conversation_id: String,
    prompt: String,
) -> Result<()> {
    let shared = state.inner().clone();
    let request = locked(&shared)?.prepare_chat(&conversation_id, prompt)?;
    // The SQLite mutex is released before network I/O. Each conversation can
    // progress independently; only one request at a time is admitted per session.
    let turn_id = request.turn_id.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let key = vault::read(&request.credential_ref)?;
        execute_chat(&request, &key)
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
            add_agent,
            create_task,
            create_conversation,
            send_message,
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
