use std::cell::Cell;
use wex_api::{Message, Role, ToolCall};
use wex_desktop_service::{drive_tool_rounds_from_history, ModelStep, ServiceError, ToolProposal};
fn history() -> Vec<Message> {
    vec![
        Message {
            role: Role::Assistant,
            content: None,
            tool_calls: vec![ToolCall {
                id: "old_call".into(),
                name: "read_file".into(),
                arguments_json: "{}".into(),
            }],
            tool_call_id: None,
        },
        Message {
            role: Role::Tool,
            content: Some("old receipt".into()),
            tool_calls: vec![],
            tool_call_id: Some("old_call".into()),
        },
    ]
}
fn proposal() -> ModelStep {
    ModelStep::Proposal {
        assistant_text: None,
        calls: vec![ToolProposal {
            id: "new_call".into(),
            name: "read_file".into(),
            arguments_json: "{}".into(),
        }],
    }
}
#[test]
fn incomplete_restored_history_never_requests_model_or_dispatch() {
    let mut model_requests = 0;
    let mut dispatches = 0;
    let error = drive_tool_rounds_from_history(
        history()[..1].to_vec(),
        |_| {
            model_requests += 1;
            Ok(ModelStep::Text("wrong".into()))
        },
        |_| {
            dispatches += 1;
            Ok(vec![])
        },
        |_, _| Ok(()),
    )
    .unwrap_err();
    assert_eq!(error.code, "TOOL_CHECKPOINT_ERROR");
    assert_eq!((model_requests, dispatches), (0, 0));
}
#[test]
fn newly_rejected_tool_keeps_only_pending_phase_and_never_replays_history() {
    let mut requests = 0;
    let mut phases = vec![];
    let error = drive_tool_rounds_from_history(
        history(),
        |_| {
            requests += 1;
            Ok(proposal())
        },
        |calls| {
            assert_eq!(calls[0].id, "new_call");
            Err(ServiceError::new("PERMISSION_DENIED", "user rejected"))
        },
        |messages, pending| {
            phases.push(pending);
            assert_eq!(messages[1].content.as_deref(), Some("old receipt"));
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "PERMISSION_DENIED");
    assert_eq!(requests, 1);
    assert_eq!(phases, vec![true]);
}
#[test]
fn revocation_after_tool_completion_prevents_the_next_model_request() {
    let enabled = Cell::new(true);
    let mut requests = 0;
    let mut dispatches = 0;
    let error = drive_tool_rounds_from_history(
        history(),
        |_| {
            if !enabled.get() {
                return Err(ServiceError::new("CONNECTION_UNAVAILABLE", "revoked"));
            }
            requests += 1;
            Ok(proposal())
        },
        |calls| {
            dispatches += 1;
            enabled.set(false);
            Ok(vec![(calls[0].id.clone(), "new receipt".into())])
        },
        |_, _| Ok(()),
    )
    .unwrap_err();
    assert_eq!(error.code, "CONNECTION_UNAVAILABLE");
    assert_eq!((requests, dispatches), (1, 1));
}
#[test]
fn existing_transport_history_limit_is_checked_before_a_new_dispatch() {
    let large = (0..32).flat_map(|_| history()).collect();
    let mut dispatches = 0;
    let mut checkpoints = 0;
    let error = drive_tool_rounds_from_history(
        large,
        |_| Ok(proposal()),
        |_| {
            dispatches += 1;
            Ok(vec![])
        },
        |_, _| {
            checkpoints += 1;
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "TOOL_HISTORY_LIMIT");
    assert_eq!((dispatches, checkpoints), (0, 0));
}

fn request() -> wex_desktop_service::PreparedChat {
    wex_desktop_service::PreparedChat {
        turn_id: "turn_1".into(),
        conversation_id: "conv_1".into(),
        connection_id: "conn_1".into(),
        credential_ref: "vault".into(),
        model_id: "model".into(),
        base_url: "https://example.test".into(),
        messages: vec![wex_desktop_service::ApiMessage {
            role: "user".into(),
            content: "continue work".into(),
        }],
    }
}
#[test]
fn rejected_new_batch_blocks_loading_the_older_ready_checkpoint() {
    use wex_desktop_service::{
        load_tool_checkpoint, restore_tool_checkpoint_with_cursor, save_tool_checkpoint,
    };
    let dir = tempfile::tempdir().unwrap();
    let request = request();
    let h = history();
    save_tool_checkpoint(dir.path(), &request, 0, &h[..1], true).unwrap();
    save_tool_checkpoint(dir.path(), &request, 1, &h, false).unwrap();
    let (_, restored, mut cursor) =
        restore_tool_checkpoint_with_cursor(dir.path(), &request).unwrap();
    let result = drive_tool_rounds_from_history(
        restored,
        |_| Ok(proposal()),
        |_| Err(ServiceError::new("PERMISSION_DENIED", "user rejected")),
        |messages, pending| {
            save_tool_checkpoint(dir.path(), &request, cursor, messages, pending)?;
            cursor += 1;
            Ok(())
        },
    );
    assert_eq!(result.unwrap_err().code, "PERMISSION_DENIED");
    assert_eq!(cursor, 3);
    assert!(load_tool_checkpoint(dir.path(), &request).is_err());
}
#[test]
fn stale_checkpoint_cursor_never_repeats_a_dispatch() {
    use wex_desktop_service::{restore_tool_checkpoint_with_cursor, save_tool_checkpoint};
    let dir = tempfile::tempdir().unwrap();
    let request = request();
    let h = history();
    save_tool_checkpoint(dir.path(), &request, 0, &h[..1], true).unwrap();
    save_tool_checkpoint(dir.path(), &request, 1, &h, false).unwrap();
    let (_, restored, cursor) = restore_tool_checkpoint_with_cursor(dir.path(), &request).unwrap();
    let mut other_pending = h.clone();
    let ModelStep::Proposal { calls, .. } = proposal() else {
        unreachable!()
    };
    other_pending.push(Message {
        role: Role::Assistant,
        content: None,
        tool_calls: calls
            .iter()
            .map(|c| ToolCall {
                id: c.id.clone(),
                name: c.name.clone(),
                arguments_json: c.arguments_json.clone(),
            })
            .collect(),
        tool_call_id: None,
    });
    save_tool_checkpoint(dir.path(), &request, cursor, &other_pending, true).unwrap();
    let mut dispatches = 0;
    let result = drive_tool_rounds_from_history(
        restored,
        |_| Ok(proposal()),
        |_| {
            dispatches += 1;
            Ok(vec![])
        },
        |messages, pending| save_tool_checkpoint(dir.path(), &request, cursor, messages, pending),
    );
    assert_eq!(result.unwrap_err().code, "TOOL_CHECKPOINT_ERROR");
    assert_eq!(dispatches, 0);
}

#[test]
fn restored_receipts_are_sent_to_model_but_only_new_proposals_are_dispatched() {
    let mut requests = 0;
    let mut dispatched = Vec::new();
    let mut phases = Vec::new();
    let answer = drive_tool_rounds_from_history(
        history(),
        |messages| {
            requests += 1;
            assert_eq!(messages[0].tool_calls[0].id, "old_call");
            assert_eq!(messages[1].content.as_deref(), Some("old receipt"));
            if requests == 1 {
                assert_eq!(messages.len(), 2);
                Ok(proposal())
            } else {
                assert_eq!(messages.len(), 4);
                assert_eq!(messages[2].tool_calls[0].id, "new_call");
                assert_eq!(messages[3].tool_call_id.as_deref(), Some("new_call"));
                assert_eq!(messages[3].content.as_deref(), Some("new receipt"));
                Ok(ModelStep::Text("continued successfully".into()))
            }
        },
        |calls| {
            assert_eq!(calls.len(), 1);
            dispatched.push(calls[0].id.clone());
            Ok(vec![(calls[0].id.clone(), "new receipt".into())])
        },
        |messages, pending| {
            assert_eq!(messages[1].content.as_deref(), Some("old receipt"));
            phases.push((messages.len(), pending));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(answer, "continued successfully");
    assert_eq!(requests, 2);
    assert_eq!(dispatched, vec!["new_call"]);
    assert_eq!(phases, vec![(3, true), (4, false)]);
}
