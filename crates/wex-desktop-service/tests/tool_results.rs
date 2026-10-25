use wex_desktop_service::{read_tool_results, save_tool_result, ToolProposal};
fn call() -> ToolProposal {
    ToolProposal {
        id: "call/../1".into(),
        name: "cargo_test".into(),
        arguments_json: "{}".into(),
    }
}
#[test]
fn persisted_failure_survives_reopen_and_retains_exact_id_and_output() {
    let dir = tempfile::tempdir().unwrap();
    let body = r#"{"success":false,"exit_code":101,"output":"error\n请修复"}"#;
    save_tool_result(dir.path(), "conv_1", "turn_1", &call(), body).unwrap();
    let restored = read_tool_results(dir.path(), "conv_1", "turn_1").unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].result, body);
    assert_eq!(restored[0].call_id, call().id);
    assert_eq!(restored[0].arguments_json, "{}");
    assert!(read_tool_results(dir.path(), "conv_2", "turn_1")
        .unwrap()
        .is_empty());
    assert!(read_tool_results(dir.path(), "conv_1", "turn_2")
        .unwrap()
        .is_empty());
}
#[test]
fn repeated_provider_call_id_does_not_overwrite_earlier_result() {
    let dir = tempfile::tempdir().unwrap();
    save_tool_result(dir.path(), "conv_1", "turn_1", &call(), "first").unwrap();
    save_tool_result(dir.path(), "conv_1", "turn_1", &call(), "second").unwrap();
    let rows = read_tool_results(dir.path(), "conv_1", "turn_1").unwrap();
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].record_id, rows[1].record_id);
    assert!(rows.iter().any(|r| r.result == "first"));
    assert!(rows.iter().any(|r| r.result == "second"));
}
#[test]
fn invalid_binding_corruption_and_moved_record_fail_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    assert!(save_tool_result(dir.path(), "../escape", "turn_1", &call(), "x").is_err());
    save_tool_result(dir.path(), "conv_1", "turn_1", &call(), "saved").unwrap();
    let source = std::fs::read_dir(dir.path().join("conv_1/turn_1"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let other = dir.path().join("conv_2/turn_1");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::copy(&source, other.join(source.file_name().unwrap())).unwrap();
    assert!(read_tool_results(dir.path(), "conv_2", "turn_1").is_err());
    std::fs::write(source, "{").unwrap();
    assert!(read_tool_results(dir.path(), "conv_1", "turn_1").is_err());
}
#[test]
fn failed_storage_does_not_delete_previously_saved_results() {
    let dir = tempfile::tempdir().unwrap();
    save_tool_result(
        dir.path(),
        "conv_1",
        "turn_1",
        &call(),
        "completed before next tool failed",
    )
    .unwrap();
    let blocked = dir.path().join("blocked");
    std::fs::write(&blocked, "not a directory").unwrap();
    assert!(save_tool_result(&blocked, "conv_1", "turn_1", &call(), "x").is_err());
    assert_eq!(
        read_tool_results(dir.path(), "conv_1", "turn_1").unwrap()[0].result,
        "completed before next tool failed"
    );
}
