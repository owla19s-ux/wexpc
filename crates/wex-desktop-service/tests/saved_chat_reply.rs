use wex_desktop_service::*;
fn request() -> PreparedChat {
    PreparedChat {
        turn_id: "turn_1".into(),
        conversation_id: "conv_1".into(),
        connection_id: "conn_1".into(),
        credential_ref: "vault_secret_reference".into(),
        model_id: "model".into(),
        base_url: "https://example.test".into(),
        messages: vec![ApiMessage {
            role: "user".into(),
            content: "original request".into(),
        }],
    }
}
#[test]
fn final_reply_survives_restart_without_credential_data_or_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let request = request();
    save_chat_reply(dir.path(), &request, "已完成\nexact reply").unwrap();
    assert_eq!(
        read_chat_reply(dir.path(), "conv_1", "turn_1", "original request")
            .unwrap()
            .as_deref(),
        Some("已完成\nexact reply")
    );
    assert!(save_chat_reply(dir.path(), &request, "overwrite").is_err());
    assert!(
        read_chat_reply(dir.path(), "conv_other", "turn_1", "original request")
            .unwrap()
            .is_none()
    );
    assert!(read_chat_reply(dir.path(), "conv_1", "turn_1", "different request").is_err());
    let moved = dir.path().join("conv_other/turn_1");
    std::fs::create_dir_all(&moved).unwrap();
    std::fs::copy(
        dir.path().join("conv_1/turn_1/reply.json"),
        moved.join("reply.json"),
    )
    .unwrap();
    assert!(read_chat_reply(dir.path(), "conv_other", "turn_1", "original request").is_err());

    let text = std::fs::read_to_string(dir.path().join("conv_1/turn_1/reply.json")).unwrap();
    assert!(!text.contains("vault_secret_reference"));
}
#[test]
fn corrupt_and_invalid_paths_do_not_appear_as_missing_answers() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_chat_reply(dir.path(), "../escape", "turn_1", "x").is_err());
    save_chat_reply(dir.path(), &request(), "reply").unwrap();
    std::fs::write(dir.path().join("conv_1/turn_1/reply.json"), "{").unwrap();
    assert!(read_chat_reply(dir.path(), "conv_1", "turn_1", "original request").is_err());
}
#[test]
fn local_reply_recovery_after_database_failure_needs_no_enabled_provider() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("desktop.sqlite");
    let replies = dir.path().join("chat-replies");
    let mut service = DesktopService::open(&path).unwrap();
    let connection = service
        .add_connection(
            NewConnection {
                name: "API".into(),
                base_url: "https://api.deepseek.com".into(),
                models: vec!["deepseek-chat".into()],
            },
            "vault".into(),
        )
        .unwrap();
    let conversation = service
        .create_conversation(
            None,
            "A".into(),
            connection.id.clone(),
            "deepseek-chat".into(),
        )
        .unwrap();
    let request = service
        .prepare_chat(&conversation.id, "original request".into())
        .unwrap();
    service
        .set_connection_enabled(&connection.id, false)
        .unwrap();
    let disk = rusqlite::Connection::open(&path).unwrap();
    disk.execute_batch("CREATE TRIGGER fail_save BEFORE UPDATE ON desktop_state BEGIN SELECT RAISE(FAIL,'disk fault'); END;").unwrap();
    assert!(service
        .finish_chat_with_backup(&replies, &request, "model already replied".into())
        .is_err());
    disk.execute_batch("DROP TRIGGER fail_save;").unwrap();
    drop(disk);
    drop(service);
    let mut service = DesktopService::open(&path).unwrap();
    assert!(service
        .tool_reply_candidate(&conversation.id, &request.turn_id)
        .is_err());
    let user = service
        .saved_reply_candidate(&conversation.id, &request.turn_id)
        .unwrap();
    let answer = read_chat_reply(&replies, &conversation.id, &request.turn_id, &user)
        .unwrap()
        .unwrap();
    service
        .restore_saved_reply(&conversation.id, &request.turn_id, &user, answer)
        .unwrap();
    assert!(service
        .saved_reply_candidate(&conversation.id, &request.turn_id)
        .is_err());
    drop(service);
    let state = DesktopService::open(&path).unwrap().snapshot().unwrap();
    assert_eq!(state.turns.len(), 1);
    assert_eq!(state.turns[0].status, TurnStatus::Succeeded);
    assert_eq!(
        state.turns[0].assistant.as_deref(),
        Some("model already replied")
    );
    assert!(!state.connections[0].enabled);
}
#[test]
fn local_recovery_rejects_foreign_turn_or_changed_prompt_without_state_changes() {
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
    let request = service
        .prepare_chat(&conv.id, "user request".into())
        .unwrap();
    service
        .fail_chat(
            &request.turn_id,
            &ServiceError::new("NETWORK_ERROR", "failed"),
        )
        .unwrap();
    let revision = service.snapshot().unwrap().revision;
    assert!(service
        .saved_reply_candidate("foreign_conv", &request.turn_id)
        .is_err());
    assert!(service
        .restore_saved_reply(&conv.id, &request.turn_id, "wrong user", "reply".into())
        .is_err());
    assert_eq!(service.snapshot().unwrap().revision, revision);
}

fn live_request(service: &mut DesktopService) -> PreparedChat {
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
    service.prepare_chat(&conv.id, "request".into()).unwrap()
}
#[test]
fn failed_backup_does_not_discard_a_successful_database_settlement() {
    let dir = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("blocked");
    std::fs::write(&blocked, "not a directory").unwrap();
    let mut service = DesktopService::memory().unwrap();
    let request = live_request(&mut service);
    service
        .finish_chat_with_backup(&blocked, &request, "saved to database".into())
        .unwrap();
    assert_eq!(
        service.snapshot().unwrap().turns[0].assistant.as_deref(),
        Some("saved to database")
    );
}
#[test]
fn both_storage_failures_keep_pending_reply_for_save_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("desktop.sqlite");
    let blocked = dir.path().join("blocked");
    std::fs::write(&blocked, "not a directory").unwrap();
    let mut service = DesktopService::open(&path).unwrap();
    let request = live_request(&mut service);
    let disk = rusqlite::Connection::open(&path).unwrap();
    disk.execute_batch("CREATE TRIGGER fail_save BEFORE UPDATE ON desktop_state BEGIN SELECT RAISE(FAIL,'disk fault'); END;").unwrap();
    let error = service
        .finish_chat_with_backup(&blocked, &request, "complete reply still in memory".into())
        .unwrap_err();
    assert_eq!(error.code, "STORAGE_ERROR");
    assert!(error.message.contains("内存"));
    assert_eq!(
        service.snapshot().unwrap().unsaved_turn_ids,
        vec![request.turn_id.clone()]
    );
    disk.execute_batch("DROP TRIGGER fail_save;").unwrap();
    service.retry_chat_result(&request.turn_id).unwrap();
    assert_eq!(
        service.snapshot().unwrap().turns[0].assistant.as_deref(),
        Some("complete reply still in memory")
    );
}
