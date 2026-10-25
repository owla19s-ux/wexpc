use wex_desktop_service::{append_tool_marker, read_tool_markers, ToolReceiptMarker};
fn marker(state: &str) -> ToolReceiptMarker {
    ToolReceiptMarker {
        operation_id: "op_1".into(),
        attempt_id: "attempt_1".into(),
        state: state.into(),
        content_digest: None,
        conversation_id: Some("conversation_1".into()),
        turn_id: Some("turn_1".into()),
        policy: None,
    }
}
#[test]
fn restart_recovers_terminal_state_and_isolates_conversations() {
    let dir = tempfile::tempdir().unwrap();
    append_tool_marker(dir.path(), &marker("in_flight")).unwrap();
    assert_eq!(
        read_tool_markers(dir.path(), "conversation_1").unwrap()[0].state,
        "in_flight"
    );
    assert!(read_tool_markers(dir.path(), "conversation_2")
        .unwrap()
        .is_empty());
    append_tool_marker(dir.path(), &marker("failed")).unwrap();
    assert_eq!(
        read_tool_markers(dir.path(), "conversation_1").unwrap(),
        vec![marker("failed")]
    );
    assert!(append_tool_marker(dir.path(), &marker("failed")).is_err());
    assert_eq!(
        read_tool_markers(dir.path(), "conversation_1").unwrap(),
        vec![marker("failed")]
    );
}
#[test]
fn unknown_and_conflicting_results_are_never_reported_as_success() {
    let dir = tempfile::tempdir().unwrap();
    append_tool_marker(dir.path(), &marker("unknown")).unwrap();
    assert_eq!(
        read_tool_markers(dir.path(), "conversation_1").unwrap()[0].state,
        "unknown"
    );
    append_tool_marker(dir.path(), &marker("succeeded")).unwrap();
    assert!(read_tool_markers(dir.path(), "conversation_1").is_err());
}
#[test]
fn corrupt_legacy_and_invalid_paths_do_not_invent_execution_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut old = marker("succeeded");
    old.conversation_id = None;
    old.turn_id = None;
    append_tool_marker(dir.path(), &old).unwrap();
    assert!(read_tool_markers(dir.path(), "conversation_1")
        .unwrap()
        .is_empty());
    let mut invalid = marker("failed");
    invalid.attempt_id = "../escape".into();
    assert!(append_tool_marker(dir.path(), &invalid).is_err());
    std::fs::write(dir.path().join("damaged.json"), "{").unwrap();
    assert!(read_tool_markers(dir.path(), "conversation_1").is_err());
}
#[test]
fn missing_directory_is_empty_and_conflicting_turn_binding_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        read_tool_markers(&dir.path().join("missing"), "conversation_1")
            .unwrap()
            .is_empty()
    );
    append_tool_marker(dir.path(), &marker("in_flight")).unwrap();
    let mut result = marker("succeeded");
    result.turn_id = Some("turn_other".into());
    append_tool_marker(dir.path(), &result).unwrap();
    assert!(read_tool_markers(dir.path(), "conversation_1").is_err());
}

#[test]
fn execution_records_keep_the_policy_used_at_dispatch_not_current_preferences() {
    use wex_desktop_service::{PolicyChoice, ToolPolicySetting};
    let dir = tempfile::tempdir().unwrap();
    let mut pending = marker("in_flight");
    pending.policy = Some(ToolPolicySetting {
        conversation_id: "conversation_1".into(),
        capability_id: "github_file_write".into(),
        policy: PolicyChoice::Automatic,
        revision: 2,
    });
    append_tool_marker(dir.path(), &pending).unwrap();
    let mut complete = pending.clone();
    complete.state = "succeeded".into();
    append_tool_marker(dir.path(), &complete).unwrap();
    assert_eq!(
        read_tool_markers(dir.path(), "conversation_1").unwrap()[0].policy,
        complete.policy
    );
}
