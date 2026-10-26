//! Regression for approved local Git reads with hostile launcher GIT_* variables.
//! The child test runs in its own process so environment mutation cannot
//! interfere with unrelated tests running in parallel.
use std::{path::PathBuf, process::Command};
use wex_core::operation::{Action, Dispatch, OperationIntent};
use wex_desktop_service::{approved_git_status, digest_arguments};

fn init_repo(path: &std::path::Path) {
    let output = Command::new("git")
        .args(["init", "--quiet"])
        .arg(path)
        .output()
        .expect("Git must be installed for local Git adapter tests");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn approved_git_status_ignores_inherited_git_environment() {
    let expected = tempfile::tempdir().unwrap();
    let alternate = tempfile::tempdir().unwrap();
    init_repo(expected.path());
    init_repo(alternate.path());
    std::fs::write(expected.path().join("expected_only.txt"), "expected").unwrap();
    std::fs::write(alternate.path().join("alternate_only.txt"), "alternate").unwrap();

    // Git honors GIT_DIR/GIT_WORK_TREE even when -C points to another repo.
    // GIT_INDEX_FILE and numbered config are other ambient overrides that
    // must not cross the approved tool boundary.
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_git_environment_isolation", "--nocapture"])
        .env("WEX_GIT_EXPECTED_ROOT", expected.path())
        .env("GIT_DIR", alternate.path().join(".git"))
        .env("GIT_WORK_TREE", alternate.path())
        .env("GIT_INDEX_FILE", alternate.path().join("alternate.index"))
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "status.showUntrackedFiles")
        .env("GIT_CONFIG_VALUE_0", "no")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "approved Git read failed with inherited GIT_* variables:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn child_git_environment_isolation() {
    let Some(root) = std::env::var_os("WEX_GIT_EXPECTED_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let root_bytes = root.to_str().unwrap().as_bytes();
    let dispatch = Dispatch {
        intent: OperationIntent {
            id: "op_git_env".into(),
            actor_id: "agent".into(),
            conversation_id: "conversation".into(),
            context_id: None,
            connection_id: "local".into(),
            capability_id: "git_status".into(),
            resource_id: "approved_repo".into(),
            action: Action::Read,
            expected_version: None,
            intent_digest: digest_arguments("git_status", "approved_repo", &[root_bytes]),
        },
        attempt_id: "attempt_git_env".into(),
    };
    let status = approved_git_status(&dispatch, "approved_repo", &root).unwrap();
    assert!(
        status.contains("?? expected_only.txt"),
        "wrong repository: {status}"
    );
    assert!(
        !status.contains("alternate_only.txt"),
        "ambient repo leaked: {status}"
    );
}
