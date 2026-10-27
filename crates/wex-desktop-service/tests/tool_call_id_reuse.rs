//! Regression coverage for the tool-call identity fence on the combined
//! desktop recovery path. No network or real tool dispatch is required.
use wex_api::{Message, Role, ToolCall};
use wex_desktop_service::{drive_tool_rounds_from_history, ModelStep, ToolProposal};

fn settled_history() -> Vec<Message> {
    vec![
        Message {
            role: Role::Assistant,
            content: None,
            tool_calls: vec![ToolCall {
                id: "settled_call".into(),
                name: "read_file".into(),
                arguments_json: "{}".into(),
            }],
            tool_call_id: None,
        },
        Message {
            role: Role::Tool,
            content: Some("previously saved result".into()),
            tool_calls: vec![],
            tool_call_id: Some("settled_call".into()),
        },
    ]
}

fn proposal(ids: &[&str]) -> ModelStep {
    ModelStep::Proposal {
        assistant_text: None,
        calls: ids
            .iter()
            .map(|id| ToolProposal {
                id: (*id).into(),
                name: "read_file".into(),
                arguments_json: "{}".into(),
            })
            .collect(),
    }
}

#[test]
fn restored_call_id_cannot_be_reused_before_checkpoint_or_dispatch() {
    let mut requests = 0;
    let mut dispatches = 0;
    let mut checkpoints = 0;
    let error = drive_tool_rounds_from_history(
        settled_history(),
        |messages| {
            requests += 1;
            assert_eq!(messages.len(), 2);
            Ok(proposal(&["settled_call"]))
        },
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
    assert_eq!(error.code, "TOOL_CALL_ID_REUSED");
    assert_eq!((requests, dispatches, checkpoints), (1, 0, 0));
}

#[test]
fn duplicate_ids_within_new_batch_are_rejected_without_side_effects() {
    let mut dispatches = 0;
    let mut checkpoints = 0;
    let error = drive_tool_rounds_from_history(
        settled_history(),
        |_| Ok(proposal(&["new_call", "new_call"])),
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
    assert_eq!(error.code, "TOOL_CALL_ID_REUSED");
    assert_eq!((dispatches, checkpoints), (0, 0));
}

#[test]
fn newly_settled_call_id_cannot_be_reused_in_next_model_step() {
    let mut model_steps = 0;
    let mut dispatches = 0;
    let mut phases = vec![];
    let error = drive_tool_rounds_from_history(
        settled_history(),
        |messages| {
            model_steps += 1;
            if model_steps == 1 {
                assert_eq!(messages.len(), 2);
                Ok(proposal(&["fresh_call"]))
            } else {
                assert_eq!(messages.len(), 4);
                assert_eq!(messages.last().unwrap().tool_call_id.as_deref(), Some("fresh_call"));
                Ok(proposal(&["fresh_call"]))
            }
        },
        |calls| {
            dispatches += 1;
            Ok(vec![(calls[0].id.clone(), "recorded result".into())])
        },
        |messages, pending| {
            phases.push((messages.len(), pending));
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "TOOL_CALL_ID_REUSED");
    assert_eq!(model_steps, 2);
    assert_eq!(dispatches, 1);
    assert_eq!(phases, vec![(3, true), (4, false)]);
}

#[test]
fn failed_pending_checkpoint_rejects_dispatch_and_model_continuation() {
    let mut dispatches = 0;
    let mut model_steps = 0;
    let error = drive_tool_rounds_from_history(
        settled_history(),
        |_| {
            model_steps += 1;
            Ok(proposal(&["uncommitted_call"]))
        },
        |_| {
            dispatches += 1;
            Ok(vec![])
        },
        |messages, pending| {
            assert_eq!(messages.len(), 3);
            assert!(pending);
            Err(wex_desktop_service::ServiceError::new("STORAGE_ERROR", "failed before dispatch"))
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "STORAGE_ERROR");
    assert_eq!((model_steps, dispatches), (1, 0));
}
