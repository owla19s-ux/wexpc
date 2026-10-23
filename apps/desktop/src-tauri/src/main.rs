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
            snapshot,
            create_project,
            add_connection,
            set_connection_enabled,
            add_agent,
            create_task,
            create_conversation,
            send_message,
            retry_chat_result
        ])
        .run(tauri::generate_context!())
        .expect("WEXPC desktop startup failed");
}
