use wex_desktop_service::*;

fn api() -> NewConnection {
    NewConnection {
        name: "DeepSeek".into(),
        base_url: "https://api.deepseek.com".into(),
        models: vec!["deepseek-chat".into(), "deepseek-reasoner".into()],
    }
}

#[test]
fn projects_agents_and_task_drafts_survive_restart_without_starting_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("desktop.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let connection = app.add_connection(api(), "vault_ref_1".into()).unwrap();
    let p = app
        .create_project(
            "WEX Android".into(),
            "打通工具链".into(),
            vec!["D:\\WEX".into(), "https://github.com/owla19s-ux/WEX".into()],
        )
        .unwrap();
    let mut agents = vec![];
    for (name, role) in [
        ("主 AI", AgentRole::Lead),
        ("施工 A", AgentRole::Worker),
        ("施工 B", AgentRole::Worker),
        ("验证", AgentRole::Verifier),
        ("审查", AgentRole::Reviewer),
    ] {
        agents.push(
            app.add_agent(NewAgent {
                project_id: p.id.clone(),
                name: name.into(),
                role,
                connection_id: connection.id.clone(),
                model_id: "deepseek-chat".into(),
            })
            .unwrap(),
        );
    }
    let task = app
        .create_task(NewTask {
            project_id: p.id.clone(),
            title: "工具接线".into(),
            goal: "按正式规范接线".into(),
            scope: vec!["API 模块".into()],
            acceptance: "真实工具回执可见".into(),
            agent_ids: agents.iter().map(|a| a.id.clone()).collect(),
        })
        .unwrap();
    assert_eq!(task.status, TaskStatus::Draft);
    drop(app);
    let app = DesktopService::open(&path).unwrap();
    let snapshot = app.snapshot().unwrap();
    assert_eq!(snapshot.projects[0].resources.len(), 2);
    assert_eq!(snapshot.agents.len(), 5);
    assert_eq!(snapshot.tasks[0].agent_ids.len(), 5);
    assert_eq!(snapshot.tasks[0].status, TaskStatus::Draft);
    assert_eq!(snapshot.conversations.len(), 5);
    let sessions: std::collections::HashSet<_> =
        snapshot.agents.iter().map(|a| &a.conversation_id).collect();
    assert_eq!(
        sessions.len(),
        5,
        "worker and reviewer never share their work session"
    );
    let raw = std::fs::read(&path).unwrap();
    assert!(!raw.windows(10).any(|w| w == b"secret-key"));
}

#[test]
fn task_cannot_bind_an_agent_from_another_project() {
    let mut app = DesktopService::memory().unwrap();
    let c = app.add_connection(api(), "vault".into()).unwrap();
    let a = app.create_project("A".into(), "".into(), vec![]).unwrap();
    let b = app.create_project("B".into(), "".into(), vec![]).unwrap();
    let agent = app
        .add_agent(NewAgent {
            project_id: a.id,
            name: "A worker".into(),
            role: AgentRole::Worker,
            connection_id: c.id,
            model_id: "deepseek-chat".into(),
        })
        .unwrap();
    let result = app.create_task(NewTask {
        project_id: b.id,
        title: "task".into(),
        goal: "".into(),
        scope: vec![],
        acceptance: "".into(),
        agent_ids: vec![agent.id],
    });
    assert_eq!(result.unwrap_err().code, "INVALID_BINDING");
    assert!(app.snapshot().unwrap().tasks.is_empty());
}

#[test]
fn ordinary_conversation_needs_no_project_and_disabled_connection_has_no_fallback() {
    let mut app = DesktopService::memory().unwrap();
    let c = app.add_connection(api(), "vault".into()).unwrap();
    let conv = app
        .create_conversation(
            None,
            "普通对话".into(),
            c.id.clone(),
            "deepseek-chat".into(),
        )
        .unwrap();
    app.set_connection_enabled(&c.id, false).unwrap();
    assert_eq!(
        app.prepare_chat(&conv.id, "你好".into()).unwrap_err().code,
        "CONNECTION_UNAVAILABLE"
    );
    assert!(app.snapshot().unwrap().turns.is_empty());
    app.set_connection_enabled(&c.id, true).unwrap();
    assert_eq!(
        app.prepare_chat(&conv.id, "你好".into()).unwrap().model_id,
        "deepseek-chat"
    );
}

#[test]
fn requests_are_isolated_and_interrupted_turns_are_never_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("desktop.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let c = app.add_connection(api(), "vault".into()).unwrap();
    let a = app
        .create_conversation(None, "A".into(), c.id.clone(), "deepseek-chat".into())
        .unwrap();
    let b = app
        .create_conversation(None, "B".into(), c.id, "deepseek-reasoner".into())
        .unwrap();
    let pending = app.prepare_chat(&a.id, "第一条".into()).unwrap();
    assert_eq!(
        app.prepare_chat(&a.id, "不应重叠".into()).unwrap_err().code,
        "CONVERSATION_BUSY"
    );
    let other = app.prepare_chat(&b.id, "另一对话".into()).unwrap();
    app.finish_chat(&other.turn_id, "另一回复".into()).unwrap();
    drop(app);
    let mut app = DesktopService::open(&path).unwrap();
    assert_eq!(
        app.snapshot()
            .unwrap()
            .turns
            .iter()
            .find(|t| t.id == pending.turn_id)
            .unwrap()
            .status,
        TurnStatus::Interrupted
    );
    let next = app.prepare_chat(&a.id, "新消息".into()).unwrap();
    assert_eq!(
        next.messages.len(),
        1,
        "interrupted user prompt is visible in history but not silently resent"
    );
    app.finish_chat(&next.turn_id, "收到".into()).unwrap();
    assert_eq!(
        app.finish_chat(&next.turn_id, "重复回复".into())
            .unwrap_err()
            .code,
        "INVALID_STATE"
    );
}

#[test]
fn unknown_models_and_empty_project_names_are_rejected_without_partial_records() {
    let mut app = DesktopService::memory().unwrap();
    let c = app.add_connection(api(), "vault".into()).unwrap();
    assert!(app.create_project("  ".into(), "".into(), vec![]).is_err());
    assert!(app
        .create_conversation(None, "hello".into(), c.id, "missing-model".into())
        .is_err());
    let s = app.snapshot().unwrap();
    assert!(s.projects.is_empty());
    assert!(s.conversations.is_empty());
}

#[test]
fn agent_request_carries_project_duties_and_only_its_assigned_task_context() {
    let mut app = DesktopService::memory().unwrap();
    let c = app.add_connection(api(), "vault".into()).unwrap();
    let p = app
        .create_project("WEX".into(), "按正式需求施工".into(), vec![])
        .unwrap();
    let a = app
        .add_agent(NewAgent {
            project_id: p.id.clone(),
            name: "施工 A".into(),
            role: AgentRole::Worker,
            connection_id: c.id,
            model_id: "deepseek-chat".into(),
        })
        .unwrap();
    app.create_task(NewTask {
        project_id: p.id.clone(),
        title: "API 接线".into(),
        goal: "打通真实回执".into(),
        scope: vec!["API 模块".into()],
        acceptance: "测试回读".into(),
        agent_ids: vec![a.id.clone()],
    })
    .unwrap();
    app.create_task(NewTask {
        project_id: p.id,
        title: "无关的未分配任务".into(),
        goal: "".into(),
        scope: vec![],
        acceptance: "".into(),
        agent_ids: vec![],
    })
    .unwrap();
    let request = app
        .prepare_chat(&a.conversation_id, "先分析任务".into())
        .unwrap();
    assert_eq!(request.messages[0].role, "system");
    assert!(request.messages[0].content.contains("按正式需求施工"));
    assert!(request.messages[0].content.contains("API 接线"));
    assert!(request.messages[0].content.contains("草案"));
    assert!(!request.messages[0].content.contains("无关的未分配任务"));
}

#[test]
fn second_process_cannot_mark_the_first_process_request_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("desktop.sqlite");
    let _app = DesktopService::open(&path).unwrap();
    assert!(matches!(DesktopService::open(&path),Err(e) if e.code=="APP_IN_USE"));
}

#[test]
fn completed_response_can_retry_persistence_without_repeating_the_api_request() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("desktop.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let c = app.add_connection(api(), "vault".into()).unwrap();
    let conv = app
        .create_conversation(None, "A".into(), c.id, "deepseek-chat".into())
        .unwrap();
    let pending = app.prepare_chat(&conv.id, "hello".into()).unwrap();
    let disk = rusqlite::Connection::open(&path).unwrap();
    disk.execute_batch("CREATE TRIGGER simulate_write_failure BEFORE UPDATE ON desktop_state BEGIN SELECT RAISE(FAIL, 'storage fault'); END;").unwrap();
    assert_eq!(
        app.finish_chat(&pending.turn_id, "received response".into())
            .unwrap_err()
            .code,
        "STORAGE_ERROR"
    );
    assert_eq!(
        app.snapshot().unwrap().unsaved_turn_ids,
        vec![pending.turn_id.clone()]
    );
    disk.execute_batch("DROP TRIGGER simulate_write_failure;")
        .unwrap();
    app.retry_chat_result(&pending.turn_id).unwrap();
    let s = app.snapshot().unwrap();
    assert_eq!(s.turns.len(), 1);
    assert_eq!(s.turns[0].assistant.as_deref(), Some("received response"));
    assert_eq!(s.turns[0].status, TurnStatus::Succeeded);
    assert!(s.unsaved_turn_ids.is_empty());
    assert!(app.prepare_chat(&conv.id, "next".into()).is_ok());
}
