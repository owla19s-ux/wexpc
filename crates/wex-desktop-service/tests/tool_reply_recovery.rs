use wex_desktop_service::*;
fn setup(app: &mut DesktopService) -> (AiConnection, Conversation, PreparedChat) {
    let c = app
        .add_connection(
            NewConnection {
                name: "API".into(),
                base_url: "https://api.deepseek.com".into(),
                models: vec!["deepseek-chat".into()],
            },
            "current_vault_ref".into(),
        )
        .unwrap();
    let conv = app
        .create_conversation(None, "A".into(), c.id.clone(), "deepseek-chat".into())
        .unwrap();
    let request = app
        .prepare_chat(&conv.id, "original user request".into())
        .unwrap();
    app.fail_chat(
        &request.turn_id,
        &ServiceError::new("NETWORK_ERROR", "mock model failed"),
    )
    .unwrap();
    (c, conv, request)
}
#[test]
fn failed_turn_can_resume_once_without_creating_a_new_turn() {
    let mut app = DesktopService::memory().unwrap();
    let (_, conv, request) = setup(&mut app);
    let before = app.snapshot().unwrap();
    let candidate = app
        .tool_reply_candidate(&conv.id, &request.turn_id)
        .unwrap();
    assert_eq!(
        app.snapshot().unwrap().revision,
        before.revision,
        "preflight must be read-only"
    );
    assert_eq!(candidate.messages[0].content, "original user request");
    let running = app.begin_tool_reply_resume(request.clone()).unwrap();
    app.check_tool_reply_binding(&running).unwrap();
    assert!(app.begin_tool_reply_resume(request).is_err());
    assert_eq!(app.snapshot().unwrap().turns.len(), before.turns.len());
    app.finish_chat(&running.turn_id, "recovered response".into())
        .unwrap();
    assert!(app
        .tool_reply_candidate(&conv.id, &running.turn_id)
        .is_err());
    assert_eq!(
        app.snapshot().unwrap().turns[0].assistant.as_deref(),
        Some("recovered response")
    );
}
#[test]
fn connection_revocation_is_checked_before_admission_and_network() {
    let mut app = DesktopService::memory().unwrap();
    let (c, conv, request) = setup(&mut app);
    app.set_connection_enabled(&c.id, false).unwrap();
    assert!(app
        .tool_reply_candidate(&conv.id, &request.turn_id)
        .is_err());
    assert!(app.begin_tool_reply_resume(request.clone()).is_err());
    app.set_connection_enabled(&c.id, true).unwrap();
    let running = app.begin_tool_reply_resume(request).unwrap();
    app.set_connection_enabled(&c.id, false).unwrap();
    assert!(app.check_tool_reply_binding(&running).is_err());
}
#[test]
fn foreign_conversation_changed_binding_and_parallel_request_leave_state_unchanged() {
    let mut app = DesktopService::memory().unwrap();
    let (_, conv, request) = setup(&mut app);
    let rev = app.snapshot().unwrap().revision;
    assert!(app
        .tool_reply_candidate("other_conversation", &request.turn_id)
        .is_err());
    let mut different = request.clone();
    different.base_url = "https://other.test".into();
    assert!(app.begin_tool_reply_resume(different).is_err());
    assert_eq!(app.snapshot().unwrap().revision, rev);
    let other = app.prepare_chat(&conv.id, "new request".into()).unwrap();
    assert_eq!(
        app.tool_reply_candidate(&conv.id, &request.turn_id)
            .unwrap_err()
            .code,
        "CONVERSATION_BUSY"
    );
    app.fail_chat(
        &other.turn_id,
        &ServiceError::new("NETWORK_ERROR", "failed"),
    )
    .unwrap();
    assert!(app.tool_reply_candidate(&conv.id, &request.turn_id).is_ok());
}
#[test]
fn restart_allows_explicit_recovery_of_interrupted_turn() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("desktop.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let (_, conv, _) = setup(&mut app);
    let running = app
        .prepare_chat(&conv.id, "interrupted request".into())
        .unwrap();
    drop(app);
    let mut app = DesktopService::open(&path).unwrap();
    assert_eq!(
        app.snapshot().unwrap().turns.last().unwrap().status,
        TurnStatus::Interrupted
    );
    let candidate = app
        .tool_reply_candidate(&conv.id, &running.turn_id)
        .unwrap();
    assert_eq!(candidate.messages[0].content, "interrupted request");
    let request = app.begin_tool_reply_resume(running).unwrap();
    app.check_tool_reply_binding(&request).unwrap();
    app.finish_chat(&request.turn_id, "recovered after restart".into())
        .unwrap();
    drop(app);
    let app = DesktopService::open(&path).unwrap();
    assert_eq!(
        app.snapshot()
            .unwrap()
            .turns
            .last()
            .unwrap()
            .assistant
            .as_deref(),
        Some("recovered after restart")
    );
}

#[test]
fn unsaved_response_is_saved_without_requesting_model_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("desktop.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let (_, conv, request) = setup(&mut app);
    let running = app.begin_tool_reply_resume(request).unwrap();
    let disk = rusqlite::Connection::open(&path).unwrap();
    disk.execute_batch("CREATE TRIGGER simulate_write_failure BEFORE UPDATE ON desktop_state BEGIN SELECT RAISE(FAIL, 'storage fault'); END;").unwrap();
    assert!(app
        .finish_chat(&running.turn_id, "already recovered model answer".into())
        .is_err());
    assert_eq!(
        app.tool_reply_candidate(&conv.id, &running.turn_id)
            .unwrap_err()
            .code,
        "UNSAVED_RESULT"
    );
    disk.execute_batch("DROP TRIGGER simulate_write_failure;")
        .unwrap();
    app.retry_chat_result(&running.turn_id).unwrap();
    assert_eq!(
        app.snapshot().unwrap().turns[0].assistant.as_deref(),
        Some("already recovered model answer")
    );
}
