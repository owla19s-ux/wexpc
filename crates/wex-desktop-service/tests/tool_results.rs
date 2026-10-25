use wex_desktop_service::{read_tool_results, save_tool_result, ToolProposal};
fn call() -> ToolProposal {
    ToolProposal {
        id: "call/../1".into(),
        name: "cargo_test".into(),
        arguments_json: "{}".into(),
    }
}
#[test]
fn reopened_results_follow_save_order_not_random_record_ids() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..32 {
        save_tool_result(dir.path(), "conv_1", "turn_1", &call(), &index.to_string()).unwrap();
    }
    let rows = read_tool_results(dir.path(), "conv_1", "turn_1").unwrap();
    assert_eq!(
        rows.iter().map(|r| r.result.clone()).collect::<Vec<_>>(),
        (0..32).map(|n| n.to_string()).collect::<Vec<_>>()
    );
}
#[test]
fn concurrent_writers_get_unique_durable_positions() {
    let dir = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        for index in 0..8 {
            let root = dir.path();
            scope.spawn(move || {
                save_tool_result(root, "conv_1", "turn_1", &call(), &index.to_string()).unwrap()
            });
        }
    });
    let rows = read_tool_results(dir.path(), "conv_1", "turn_1").unwrap();
    assert_eq!(rows.len(), 8);
    assert_eq!(
        rows.iter().map(|r| r.sequence.unwrap()).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6, 7, 8]
    );
}
#[test]
fn separate_processes_preserve_every_output_with_unique_positions() {
    const CHILD_ROOT: &str = "WEXPC_RESULT_TEST_CHILD_ROOT";
    const CHILD_ID: &str = "WEXPC_RESULT_TEST_CHILD_ID";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let id = std::env::var(CHILD_ID).unwrap();
        for index in 0..4 {
            save_tool_result(
                std::path::Path::new(&root),
                "conv_1",
                "turn_1",
                &call(),
                &format!("{id}:{index}"),
            )
            .unwrap();
        }
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut children = (0..4)
        .map(|id| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "separate_processes_preserve_every_output_with_unique_positions",
                ])
                .env(CHILD_ROOT, dir.path())
                .env(CHILD_ID, id.to_string())
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    for child in &mut children {
        assert!(child.wait().unwrap().success());
    }
    let rows = read_tool_results(dir.path(), "conv_1", "turn_1").unwrap();
    assert_eq!(
        rows.iter().map(|r| r.sequence.unwrap()).collect::<Vec<_>>(),
        (1..=16).collect::<Vec<_>>()
    );
    let actual = rows
        .into_iter()
        .map(|r| r.result)
        .collect::<std::collections::BTreeSet<_>>();
    let expected = (0..4)
        .flat_map(|id| (0..4).map(move |n| format!("{id}:{n}")))
        .collect();
    assert_eq!(actual, expected);
}
#[test]
fn legacy_outputs_keep_unknown_order_and_orphan_staging_is_not_a_receipt() {
    let dir = tempfile::tempdir().unwrap();
    save_tool_result(dir.path(), "conv_1", "turn_1", &call(), "legacy").unwrap();
    let first = read_tool_results(dir.path(), "conv_1", "turn_1")
        .unwrap()
        .remove(0);
    let path = dir
        .path()
        .join("conv_1/turn_1")
        .join(format!("{}.json", first.record_id));
    let mut value = serde_json::to_value(first).unwrap();
    value.as_object_mut().unwrap().remove("sequence");
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    std::fs::write(dir.path().join("conv_1/turn_1/.pending-orphan"), "{").unwrap();
    save_tool_result(dir.path(), "conv_1", "turn_1", &call(), "new").unwrap();
    let rows = read_tool_results(dir.path(), "conv_1", "turn_1").unwrap();
    assert_eq!(rows[0].sequence, None);
    assert_eq!(rows[0].result, "legacy");
    assert_eq!(rows[1].sequence, Some(1));
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
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .unwrap();
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
