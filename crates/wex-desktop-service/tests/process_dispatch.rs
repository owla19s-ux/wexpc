use std::{io::Write, path::Path};
use wex_core::operation::{Action, Dispatch, OperationIntent};
use wex_desktop_service::{execute_approved_process_report, ProcessRequest};

fn approved(root: &Path, request: &ProcessRequest) -> Dispatch {
    Dispatch {
        intent: OperationIntent {
            id: "op_process".into(),
            actor_id: "agent_1".into(),
            conversation_id: "chat_1".into(),
            context_id: None,
            connection_id: "terminal".into(),
            capability_id: "terminal_execute".into(),
            resource_id: "selected".into(),
            action: Action::Write,
            expected_version: None,
            intent_digest: request.intent_digest("selected", root).unwrap(),
        },
        attempt_id: "attempt_1".into(),
    }
}

#[test]
fn process_helper() {
    if !std::env::args().any(|a| a == "--exact") {
        return;
    }
    std::io::stdout().write_all(&vec![b'x'; 180_000]).unwrap();
    std::io::stderr()
        .write_all(b"diagnostic preserved")
        .unwrap();
    std::process::exit(17);
}

// Use the real test binary as a portable child instead of assuming Python,
// a shell or Cargo is installed in the environment under test.
fn request() -> ProcessRequest {
    ProcessRequest {
        program: std::env::current_exe().unwrap().to_str().unwrap().into(),
        arguments: vec![
            "--exact".into(),
            "process_helper".into(),
            "--nocapture".into(),
        ],
        timeout_ms: 5_000,
    }
}

#[test]
fn approved_process_runs_in_selected_directory_and_returns_real_status() {
    let root = tempfile::tempdir().unwrap();
    let request = request();
    // The helper's normal test invocation does nothing; use an explicit child
    // environment selected by the test wrapper process below.
    let request = ProcessRequest {
        program: std::env::current_exe().unwrap().to_str().unwrap().into(),
        arguments: vec![
            "--exact".into(),
            "workspace_writer".into(),
            "--nocapture".into(),
        ],
        ..request
    };
    let result = execute_approved_process_report(
        &approved(root.path(), &request),
        "selected",
        root.path(),
        &request,
    )
    .unwrap();
    assert!(result.success);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(
        std::fs::read_to_string(root.path().join("created-by-process.txt")).unwrap(),
        "exact workspace"
    );
}
#[test]
fn workspace_writer() {
    if !std::env::args().any(|a| a == "--exact") {
        return;
    }
    std::fs::write("created-by-process.txt", "exact workspace").unwrap();
}
#[test]
fn modified_arguments_are_rejected_before_the_child_starts() {
    let root = tempfile::tempdir().unwrap();
    let mut request = request();
    let dispatch = approved(root.path(), &request);
    request.arguments.push("tampered".into());
    assert_eq!(
        execute_approved_process_report(&dispatch, "selected", root.path(), &request)
            .unwrap_err()
            .code,
        "INTENT_MISMATCH"
    );
    assert!(!root.path().join("created-by-process.txt").exists());
}
#[test]
fn large_output_keeps_the_real_nonzero_exit_and_both_streams() {
    let root = tempfile::tempdir().unwrap();
    let request = request();
    let report = execute_approved_process_report(
        &approved(root.path(), &request),
        "selected",
        root.path(),
        &request,
    )
    .unwrap();
    assert!(!report.success);
    assert_eq!(report.exit_code, Some(17));
    assert!(report.stdout_truncated);
    assert!(report.stdout_complete && report.stderr_complete);
    assert!(!report.stderr_truncated);
    assert!(report.stdout.len() <= 64 * 1024);
    assert!(report.stderr.contains("diagnostic preserved"));
}
#[test]
fn sleeping_helper() {
    if std::env::args().any(|a| a == "--exact") {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}
#[test]
fn timeout_is_unknown_and_does_not_retry_the_process() {
    let root = tempfile::tempdir().unwrap();
    let request = ProcessRequest {
        arguments: vec!["--exact".into(), "sleeping_helper".into()],
        timeout_ms: 100,
        ..request()
    };
    let start = std::time::Instant::now();
    let error = execute_approved_process_report(
        &approved(root.path(), &request),
        "selected",
        root.path(),
        &request,
    )
    .unwrap_err();
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}
#[test]
fn changed_working_directory_or_timeout_cannot_reuse_approval() {
    let first = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let mut request = request();
    let dispatch = approved(first.path(), &request);
    assert_eq!(
        execute_approved_process_report(&dispatch, "selected", other.path(), &request)
            .unwrap_err()
            .code,
        "INTENT_MISMATCH"
    );
    request.timeout_ms += 1;
    assert_eq!(
        execute_approved_process_report(&dispatch, "selected", first.path(), &request)
            .unwrap_err()
            .code,
        "INTENT_MISMATCH"
    );
}
#[test]
fn control_output_helper() {
    if !std::env::args().any(|a| a == "--exact") {
        return;
    }
    std::io::stdout().write_all(&vec![0; 128 * 1024]).unwrap();
    std::io::stderr().write_all(&vec![0; 128 * 1024]).unwrap();
}
#[test]
fn escaped_bounded_process_output_survives_the_real_receipt_store() {
    use wex_desktop_service::{read_tool_results, save_tool_result, ToolProposal};
    let root = tempfile::tempdir().unwrap();
    let request = ProcessRequest {
        arguments: vec![
            "--exact".into(),
            "control_output_helper".into(),
            "--nocapture".into(),
        ],
        ..request()
    };
    let report = execute_approved_process_report(
        &approved(root.path(), &request),
        "selected",
        root.path(),
        &request,
    )
    .unwrap();
    assert!(report.success);
    assert!(report.stdout_truncated && report.stderr_truncated);
    let body = serde_json::json!({"success":report.success,"exit_code":report.exit_code,
        "evidence_ref":"ev_1","output":"进程结果见 process 字段","process":report})
    .to_string();
    let proposal = ToolProposal {
        id: "call_1".into(),
        name: "run_command".into(),
        arguments_json: serde_json::to_string(&request).unwrap(),
    };
    save_tool_result(root.path(), "conv_1", "turn_1", &proposal, &body).unwrap();
    assert_eq!(
        read_tool_results(root.path(), "conv_1", "turn_1").unwrap()[0].result,
        body
    );
    let messages =
        wex_desktop_service::committed_tool_messages(&[proposal], &[("call_1".into(), body)], None)
            .unwrap();
    let chat = wex_desktop_service::PreparedChat {
        conversation_id: "conv_1".into(),
        turn_id: "turn_1".into(),
        connection_id: "conn".into(),
        model_id: "model".into(),
        base_url: "https://example.test".into(),
        credential_ref: "vault".into(),
        messages: vec![wex_desktop_service::ApiMessage {
            role: "user".into(),
            content: "test".into(),
        }],
    };
    let checkpoints = root.path().join("checkpoints");
    wex_desktop_service::save_tool_checkpoint(&checkpoints, &chat, 0, &messages[..1], true)
        .unwrap();
    wex_desktop_service::save_tool_checkpoint(&checkpoints, &chat, 1, &messages, false).unwrap();
    assert_eq!(
        wex_desktop_service::load_tool_checkpoint(&checkpoints, &chat).unwrap(),
        messages
    );
}
#[cfg(unix)]
#[test]
fn delayed_descendant() {
    if !std::env::args().any(|a| a == "--exact") {
        return;
    }
    std::thread::sleep(std::time::Duration::from_millis(700));
    std::fs::write("late-side-effect", "unexpected").unwrap();
}
#[cfg(unix)]
#[test]
fn parent_with_inherited_pipe() {
    if !std::env::args().any(|a| a == "--exact") {
        return;
    }
    #[allow(clippy::zombie_processes)] // This test deliberately outlives its parent.
    let _child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "delayed_descendant"])
        .spawn()
        .unwrap();
    std::process::exit(0);
}
#[cfg(unix)]
#[test]
fn inherited_output_timeout_stops_the_unix_process_group() {
    let root = tempfile::tempdir().unwrap();
    let request = ProcessRequest {
        arguments: vec![
            "--exact".into(),
            "parent_with_inherited_pipe".into(),
            "--nocapture".into(),
        ],
        timeout_ms: 150,
        ..request()
    };
    let start = std::time::Instant::now();
    let error = execute_approved_process_report(
        &approved(root.path(), &request),
        "selected",
        root.path(),
        &request,
    )
    .unwrap_err();
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    std::thread::sleep(std::time::Duration::from_millis(900));
    assert!(!root.path().join("late-side-effect").exists());
}
#[test]
fn cancellable_writer() {
    if !std::env::args().any(|a| a == "--exact") {
        return;
    }
    std::io::stdout()
        .write_all(b"output before cancellation")
        .unwrap();
    std::io::stdout().flush().unwrap();
    std::fs::write("process-started", "started").unwrap();
    std::thread::sleep(std::time::Duration::from_secs(30));
}
#[test]
fn cancelling_a_running_process_preserves_partial_output_and_unknown_effects() {
    use wex_desktop_service::{
        execute_approved_process_report_with_control, ProcessCancellation, ProcessTermination,
    };
    let root = tempfile::tempdir().unwrap();
    let request = ProcessRequest {
        arguments: vec![
            "--exact".into(),
            "cancellable_writer".into(),
            "--nocapture".into(),
        ],
        timeout_ms: 30_000,
        ..request()
    };
    let control = ProcessCancellation::default();
    let dispatch = approved(root.path(), &request);
    let report = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            execute_approved_process_report_with_control(
                &dispatch,
                "selected",
                root.path(),
                &request,
                &control,
            )
            .unwrap()
        });
        let start = std::time::Instant::now();
        while !root.path().join("process-started").exists() {
            assert!(start.elapsed() < std::time::Duration::from_secs(5));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        control.cancel();
        worker.join().unwrap()
    });
    assert_eq!(report.termination, ProcessTermination::Cancelled);
    assert!(!report.success);
    assert!(report.interrupted());
    assert!(report.stdout.contains("output before cancellation"));
}
#[test]
fn cancellation_before_spawn_has_no_filesystem_side_effect() {
    use wex_desktop_service::{execute_approved_process_report_with_control, ProcessCancellation};
    let root = tempfile::tempdir().unwrap();
    let request = ProcessRequest {
        arguments: vec!["--exact".into(), "workspace_writer".into()],
        ..request()
    };
    let control = ProcessCancellation::default();
    control.cancel();
    assert_eq!(
        execute_approved_process_report_with_control(
            &approved(root.path(), &request),
            "selected",
            root.path(),
            &request,
            &control
        )
        .unwrap_err()
        .code,
        "PROCESS_CANCELLED"
    );
    assert!(!root.path().join("created-by-process.txt").exists());
}
#[test]
fn cancellation_cannot_claim_acceptance_after_process_completion() {
    use wex_desktop_service::{execute_approved_process_report_with_control, ProcessSupervisor};
    let root = tempfile::tempdir().unwrap();
    let request = ProcessRequest {
        arguments: vec!["--exact".into(), "workspace_writer".into()],
        ..request()
    };
    let supervisor = std::sync::Arc::new(ProcessSupervisor::default());
    let lease = supervisor
        .start("chat_1", "attempt_1", "op_process", &request)
        .unwrap();
    let report = execute_approved_process_report_with_control(
        &approved(root.path(), &request),
        "selected",
        root.path(),
        &request,
        lease.control(),
    )
    .unwrap();
    assert!(report.success);
    assert!(supervisor.cancel("chat_1", "attempt_1").is_err());
    assert!(!lease.control().is_cancelled());
}

#[cfg(target_os = "linux")]
#[test]
fn escaped_pipe_holder() {
    if !std::env::args().any(|a| a == "--exact") {
        return;
    }
    // Deliberately leave the runtime's group, retaining inherited pipe writers.
    assert!(unsafe { libc::setsid() } >= 0);
    std::fs::write(
        "escaped-pipe",
        std::fs::read_link("/proc/self/fd/1")
            .unwrap()
            .to_str()
            .unwrap(),
    )
    .unwrap();
    std::fs::write("escaped-pid", std::process::id().to_string()).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(15));
}
#[cfg(target_os = "linux")]
#[test]
fn parent_of_escaped_pipe_holder() {
    if !std::env::args().any(|a| a == "--exact") {
        return;
    }
    #[allow(clippy::zombie_processes)] // Outlives this helper; test owner kills it.
    let _child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "escaped_pipe_holder", "--nocapture"])
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while !Path::new("escaped-pid").exists() {
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    std::process::exit(0);
}
#[cfg(target_os = "linux")]
#[test]
fn timeout_releases_read_handles_even_when_an_escaped_descendant_retains_writers() {
    use wex_desktop_service::{
        execute_approved_process_report_with_control, ProcessCancellation, ProcessTermination,
    };
    let root = tempfile::tempdir().unwrap();
    let request = ProcessRequest {
        arguments: vec![
            "--exact".into(),
            "parent_of_escaped_pipe_holder".into(),
            "--nocapture".into(),
        ],
        timeout_ms: 400,
        ..request()
    };
    let report = execute_approved_process_report_with_control(
        &approved(root.path(), &request),
        "selected",
        root.path(),
        &request,
        &ProcessCancellation::default(),
    )
    .unwrap();
    let pid: i32 = std::fs::read_to_string(root.path().join("escaped-pid"))
        .unwrap()
        .parse()
        .unwrap();
    struct Stop(i32);
    impl Drop for Stop {
        fn drop(&mut self) {
            unsafe {
                libc::kill(self.0, libc::SIGKILL);
            }
        }
    }
    let _stop = Stop(pid);
    let pipe = std::path::PathBuf::from(
        std::fs::read_to_string(root.path().join("escaped-pipe")).unwrap(),
    );
    let leaked = std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| std::fs::read_link(e.path()).ok().as_ref() == Some(&pipe));
    assert_eq!(report.termination, ProcessTermination::TimedOut);
    assert!(!report.stdout_complete && !report.stderr_complete);
    assert_eq!(report.exit_code, Some(0));
    assert!(
        !leaked,
        "Host retained the escaped descendant's output read handle after reporting completion"
    );
}
