//! Loopback proof of the model -> inert proposal -> Host callback ->
//! tool-role result -> resumed model request shape. Never performs real tools.
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};
use wex_api::ToolDefinition;
use wex_desktop_service::{drive_tool_rounds, ApiMessage, PreparedChat};

fn exchange_responses(
    replies: Vec<(u16, String)>,
) -> (String, thread::JoinHandle<Vec<serde_json::Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let mut captured = Vec::new();
        for (index, (status, response)) in replies.iter().enumerate() {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut received = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = socket.read(&mut chunk).unwrap();
                assert!(n > 0);
                received.extend_from_slice(&chunk[..n]);
                if let Some(sep) = received.windows(4).position(|s| s == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&received[..sep]).to_ascii_lowercase();
                    let len: usize = header
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    if received.len() >= sep + 4 + len {
                        let value =
                            serde_json::from_slice(&received[sep + 4..sep + 4 + len]).unwrap();
                        captured.push(value);
                        break;
                    }
                }
            }
            let body = response.as_bytes();
            let status = format!("{status} Test Response");
            let wire = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(wire.as_bytes()).unwrap();
            socket.write_all(body).unwrap();
            assert_eq!(captured.len(), index + 1);
        }
        captured
    });
    (url, worker)
}

fn mock_exchange_plan(
    fail_continuation: bool,
    resume: bool,
) -> (String, thread::JoinHandle<Vec<serde_json::Value>>) {
    let proposal = r#"{"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"file_name\":\"demo.txt\"}"}}]}}]}"#;
    let text = r#"{"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"verified reply"}}]}"#;
    let mut replies = vec![
        (200, proposal.into()),
        (if fail_continuation { 500 } else { 200 }, text.into()),
    ];
    if resume {
        replies.push((200, text.into()));
    }
    exchange_responses(replies)
}

fn mock_exchange(fail_continuation: bool) -> (String, thread::JoinHandle<Vec<serde_json::Value>>) {
    mock_exchange_plan(fail_continuation, false)
}

#[test]
fn approved_tool_receipt_is_bound_to_next_model_round() {
    let (base_url, worker) = mock_exchange(false);
    let chat = PreparedChat {
        turn_id: "turn_1".into(),
        conversation_id: "conversation_1".into(),
        connection_id: "connection_1".into(),
        credential_ref: "unused_mock".into(),
        model_id: "mock-model".into(),
        base_url,
        messages: vec![ApiMessage {
            role: "user".into(),
            content: "Inspect demo".into(),
        }],
    };
    let tools = vec![ToolDefinition {
        name: "read_file".into(),
        description: "Test-only inert proposal".into(),
        parameters_json: r#"{"type":"object","properties":{"file_name":{"type":"string"}},"required":["file_name"]}"#.into(),
    }];
    let mut host_invocations = 0;
    let answer = drive_tool_rounds(&chat, "mock-key", tools, |calls| {
        host_invocations += 1;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "read_file");
        Ok(vec![(
            calls[0].id.clone(),
            r#"{"evidence_ref":"ev_1","content":"approved"}"#.into(),
        )])
    })
    .unwrap();
    assert_eq!(host_invocations, 1);
    assert_eq!(answer, "verified reply");
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .all(|m| m["role"] != "tool"));
    let messages = requests[1]["messages"].as_array().unwrap();
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[2]["role"], "tool");
    assert_eq!(messages[2]["tool_call_id"], "call_1");
    assert!(messages[2]["content"].as_str().unwrap().contains("ev_1"));
}

#[test]
fn failed_model_continuation_retains_tool_result_without_redispatch() {
    use wex_desktop_service::{read_tool_results, save_tool_result};
    let directory = tempfile::tempdir().unwrap();
    let (base_url, server) = mock_exchange(true);
    let chat = PreparedChat {
        turn_id: "turn_1".into(),
        conversation_id: "conversation_1".into(),
        connection_id: "connection_1".into(),
        credential_ref: "mock".into(),
        model_id: "mock-model".into(),
        base_url,
        messages: vec![ApiMessage {
            role: "user".into(),
            content: "Inspect demo".into(),
        }],
    };
    let mut dispatches = 0;
    let result = drive_tool_rounds(&chat, "mock-key", vec![], |calls| {
        dispatches += 1;
        let body = r#"{"evidence_ref":"ev_1","content":"actual mock tool result"}"#;
        save_tool_result(
            directory.path(),
            &chat.conversation_id,
            &chat.turn_id,
            &calls[0],
            body,
        )?;
        Ok(vec![(calls[0].id.clone(), body.into())])
    });
    assert!(result.is_err());
    assert_eq!(dispatches, 1);
    assert_eq!(server.join().unwrap().len(), 2);
    let recovered =
        read_tool_results(directory.path(), &chat.conversation_id, &chat.turn_id).unwrap();
    assert_eq!(recovered.len(), 1);
    assert!(recovered[0].result.contains("actual mock tool result"));
    // Reading persisted results is a local operation with no dispatch callback.
    assert_eq!(dispatches, 1);
}

#[test]
fn checkpointed_receipts_resume_model_reply_without_a_tool_callback() {
    use wex_desktop_service::{
        drive_tool_rounds_checkpointed, execute_model_step, load_tool_checkpoint,
        save_tool_checkpoint, ModelStep,
    };
    let dir = tempfile::tempdir().unwrap();
    let (base_url, server) = mock_exchange_plan(true, true);
    let chat = PreparedChat {
        turn_id: "turn_1".into(),
        conversation_id: "conversation_1".into(),
        connection_id: "connection_1".into(),
        credential_ref: "mock".into(),
        model_id: "mock-model".into(),
        base_url,
        messages: vec![ApiMessage {
            role: "user".into(),
            content: "Inspect demo".into(),
        }],
    };
    let mut dispatches = 0;
    let mut sequence = 0;
    let result = drive_tool_rounds_checkpointed(
        &chat,
        "mock-key",
        vec![],
        |calls| {
            dispatches += 1;
            Ok(vec![(calls[0].id.clone(), "durable result".into())])
        },
        |history, pending| {
            save_tool_checkpoint(dir.path(), &chat, sequence, history, pending)?;
            sequence += 1;
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(dispatches, 1);
    let restored = load_tool_checkpoint(dir.path(), &chat).unwrap();
    let reply = execute_model_step(&chat, "mock-key", restored, vec![]).unwrap();
    assert!(matches!(reply,ModelStep::Text(ref text) if text=="verified reply"));
    assert_eq!(dispatches, 1);
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1]["messages"], requests[2]["messages"]);
}

#[test]
fn restarted_service_restores_frozen_request_and_settles_original_turn() {
    use wex_desktop_service::{
        drive_tool_rounds_checkpointed, execute_model_step, restore_tool_checkpoint,
        save_tool_checkpoint, DesktopService, ModelStep, NewConnection, ServiceError, TurnStatus,
    };
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("desktop.sqlite");
    let checkpoints = dir.path().join("checkpoints");
    let (base_url, server) = mock_exchange_plan(true, true);
    let mut app = DesktopService::open(&database).unwrap();
    let connection = app
        .add_connection(
            NewConnection {
                name: "Loopback".into(),
                base_url,
                models: vec!["mock-model".into()],
            },
            "current_vault".into(),
        )
        .unwrap();
    let conversation = app
        .create_conversation(None, "Recovery".into(), connection.id, "mock-model".into())
        .unwrap();
    let original = app
        .prepare_chat(&conversation.id, "Inspect demo".into())
        .unwrap();
    let mut sequence = 0;
    let mut dispatches = 0;
    let failed = drive_tool_rounds_checkpointed(
        &original,
        "mock-key",
        vec![],
        |calls| {
            dispatches += 1;
            Ok(vec![(calls[0].id.clone(), "saved tool result".into())])
        },
        |history, pending| {
            save_tool_checkpoint(&checkpoints, &original, sequence, history, pending)?;
            sequence += 1;
            Ok(())
        },
    );
    app.fail_chat(&original.turn_id, &failed.unwrap_err())
        .unwrap();
    drop(app);
    let mut restored = DesktopService::open(&database).unwrap();
    let candidate = restored
        .tool_reply_candidate(&conversation.id, &original.turn_id)
        .unwrap();
    let (frozen, history) = restore_tool_checkpoint(&checkpoints, &candidate).unwrap();
    let running = restored.begin_tool_reply_resume(frozen).unwrap();
    restored.check_tool_reply_binding(&running).unwrap();
    let reply = execute_model_step(&running, "mock-key", history, vec![]).unwrap();
    match reply {
        ModelStep::Text(text) => restored.finish_chat(&running.turn_id, text).unwrap(),
        _ => restored
            .fail_chat(
                &running.turn_id,
                &ServiceError::new("TEST_ERROR", "unexpected proposal"),
            )
            .unwrap(),
    }
    drop(restored);
    let final_state = DesktopService::open(&database).unwrap().snapshot().unwrap();
    assert_eq!(final_state.turns.len(), 1);
    assert_eq!(final_state.turns[0].id, original.turn_id);
    assert_eq!(final_state.turns[0].status, TurnStatus::Succeeded);
    assert_eq!(
        final_state.turns[0].assistant.as_deref(),
        Some("verified reply")
    );
    assert_eq!(dispatches, 1);
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1]["messages"], requests[2]["messages"]);
}

#[test]
fn restart_continues_new_tools_and_appends_checkpoints_without_replaying_old_tools() {
    use wex_desktop_service::*;
    fn proposal(id: &str, file: &str) -> String {
        serde_json::json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","tool_calls":[{"id":id,"type":"function","function":{"name":"read_file","arguments":serde_json::json!({"file_name":file}).to_string()}}]}}]}).to_string()
    }
    let (base_url,server)=exchange_responses(vec![
        (200,proposal("old_call","old.txt")),(500,"{}".into()),
        (200,proposal("new_call","new.txt")),
        (200,r#"{"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"continued work complete"}}]}"#.into()),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("desktop.sqlite");
    let checkpoints = dir.path().join("checkpoints");
    let outputs = dir.path().join("outputs");
    let mut app = DesktopService::open(&database).unwrap();
    let c = app
        .add_connection(
            NewConnection {
                name: "Mock".into(),
                base_url,
                models: vec!["mock-model".into()],
            },
            "vault".into(),
        )
        .unwrap();
    let conv = app
        .create_conversation(None, "A".into(), c.id, "mock-model".into())
        .unwrap();
    let request = app
        .prepare_chat(&conv.id, "inspect both files".into())
        .unwrap();
    let mut old_dispatches = 0;
    let mut cursor = 0;
    let failed = drive_tool_rounds_checkpointed(
        &request,
        "mock-key",
        vec![],
        |calls| {
            assert_eq!(calls[0].id, "old_call");
            old_dispatches += 1;
            save_tool_result(
                &outputs,
                &request.conversation_id,
                &request.turn_id,
                &calls[0],
                "old receipt",
            )?;
            Ok(vec![(calls[0].id.clone(), "old receipt".into())])
        },
        |history, pending| {
            save_tool_checkpoint(&checkpoints, &request, cursor, history, pending)?;
            cursor += 1;
            Ok(())
        },
    );
    app.fail_chat(&request.turn_id, &failed.unwrap_err())
        .unwrap();
    drop(app);
    let mut app = DesktopService::open(&database).unwrap();
    let candidate = app
        .tool_reply_candidate(&conv.id, &request.turn_id)
        .unwrap();
    let (frozen, history, mut cursor) =
        restore_tool_checkpoint_with_cursor(&checkpoints, &candidate).unwrap();
    assert_eq!(cursor, 2);
    let resumed = app.begin_tool_reply_resume(frozen).unwrap();
    let tools = vec![ToolDefinition {
        name: "read_file".into(),
        description: "Mock tool".into(),
        parameters_json: "{}".into(),
    }];
    let mut new_dispatches = 0;
    let answer = drive_tool_rounds_from_history(
        history,
        |history| {
            app.check_tool_reply_binding(&resumed)?;
            execute_model_step(&resumed, "mock-key", history, tools.clone())
        },
        |calls| {
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].id, "new_call");
            new_dispatches += 1;
            save_tool_result(
                &outputs,
                &resumed.conversation_id,
                &resumed.turn_id,
                &calls[0],
                "new receipt",
            )?;
            Ok(vec![(calls[0].id.clone(), "new receipt".into())])
        },
        |history, pending| {
            save_tool_checkpoint(&checkpoints, &resumed, cursor, history, pending)?;
            cursor += 1;
            Ok(())
        },
    )
    .unwrap();
    app.finish_chat_with_backup(&dir.path().join("replies"), &resumed, answer)
        .unwrap();
    drop(app);
    let final_state = DesktopService::open(&database).unwrap().snapshot().unwrap();
    assert_eq!(final_state.turns.len(), 1);
    assert_eq!(
        final_state.turns[0].assistant.as_deref(),
        Some("continued work complete")
    );
    assert_eq!((old_dispatches, new_dispatches), (1, 1));
    let (_, history, cursor) = restore_tool_checkpoint_with_cursor(&checkpoints, &resumed).unwrap();
    assert_eq!(cursor, 4);
    assert_eq!(history.len(), 4);
    assert_eq!(
        read_tool_results(&outputs, &request.conversation_id, &request.turn_id)
            .unwrap()
            .len(),
        2
    );
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[1]["messages"], requests[2]["messages"]);
    let messages = requests[3]["messages"].as_array().unwrap();
    assert_eq!(messages[2]["content"], "old receipt");
    assert_eq!(messages[4]["content"], "new receipt");
}
