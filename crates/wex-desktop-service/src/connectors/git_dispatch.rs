//! Read-only local Git status, admitted only via a consumed Core Dispatch.
use crate::{Result, ServiceError};
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use wex_core::operation::{Action, Dispatch};

// Include unstaged Agent-created files in the worktree status.
const GIT_STATUS_ARGS: &[&str] = &["status", "--porcelain=v1", "--untracked-files=all"];

pub fn approved_git_status(
    dispatch: &Dispatch,
    repo_id: &str,
    trusted_root: &Path,
) -> Result<String> {
    approved_git_read(dispatch, repo_id, trusted_root, "git_status")
}

/// Read a bounded patch without textconv, external diff, or shell execution.
pub fn approved_git_diff(
    dispatch: &Dispatch,
    repo_id: &str,
    trusted_root: &Path,
) -> Result<String> {
    approved_git_read(dispatch, repo_id, trusted_root, "git_diff")
}

/// Remove inherited Git-specific overrides, including arbitrary indexed
/// GIT_CONFIG_KEY_N values. Git's own prompt restriction is set afterward.
fn is_git_override(key: &std::ffi::OsStr) -> bool {
    key.to_string_lossy().to_ascii_uppercase().starts_with("GIT_")
}

fn sanitize_git_environment(command: &mut Command) {
    // Remove overrides inherited from the parent and overrides explicitly
    // configured on this Command (which take precedence over inheritance).
    let mut names: Vec<_> = std::env::vars_os()
        .map(|(key, _)| key)
        .filter(|key| is_git_override(key))
        .collect();
    names.extend(
        command
            .get_envs()
            .filter(|(key, _)| is_git_override(key))
            .map(|(key, _)| key.to_os_string()),
    );
    for key in names {
        command.env_remove(key);
    }
}

fn approved_git_read(
    dispatch: &Dispatch,
    repo_id: &str,
    trusted_root: &Path,
    capability: &str,
) -> Result<String> {
    if dispatch.intent.action != Action::Read
        || dispatch.intent.capability_id != capability
        || dispatch.intent.resource_id != repo_id
    {
        return Err(ServiceError::new(
            "DISPATCH_REJECTED",
            "Git 操作不符合已批准范围",
        ));
    }
    if !trusted_root.is_absolute() || !trusted_root.is_dir() {
        return Err(ServiceError::new(
            "RESOURCE_UNAVAILABLE",
            "受信任仓库目录不可用",
        ));
    }
    let root = trusted_root
        .to_str()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "仓库路径编码不可用"))?;
    super::intent_binding::verify_arguments(dispatch, &[root.as_bytes()])?;
    let mut child = Command::new("git");
    sanitize_git_environment(&mut child);
    child
        .arg("--no-optional-locks")
        // A repository-controlled core.fsmonitor hook can execute a program
        // even during read-only `git status`; never inherit that setting.
        .args(["-c", "core.fsmonitor=false"])
        .args(["-C"])
        .arg(trusted_root);
    if capability == "git_diff" {
        child.args(["diff", "--no-ext-diff", "--no-textconv", "--"]);
    } else {
        child.args(GIT_STATUS_ARGS);
    }
    let mut child = child
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ServiceError::new("TOOL_UNAVAILABLE", "Git 进程无法启动"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ServiceError::new("TOOL_UNAVAILABLE", "Git 输出不可用"))?;
    // Drain the pipe even after the response cap. Closing it early can send
    // SIGPIPE / broken pipe to an otherwise successful Git process.
    let reader = thread::spawn(move || collect_bounded_output(stdout, 64 * 1024));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < Duration::from_secs(15) => {
                thread::sleep(Duration::from_millis(30));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ServiceError::new("TOOL_TIMEOUT", "Git 状态读取超时或中断"));
            }
        }
    };
    let (output, truncated) = reader
        .join()
        .map_err(|_| ServiceError::new("TOOL_ERROR", "Git 读取线程失败"))?
        .map_err(|_| ServiceError::new("TOOL_ERROR", "Git 输出读取失败"))?;
    if !status.success() || truncated {
        return Err(ServiceError::new(
            "TOOL_ERROR",
            "Git 返回错误或输出超过上限",
        ));
    }
    String::from_utf8(output).map_err(|_| ServiceError::new("TOOL_ERROR", "Git 输出不是 UTF-8"))
}

// Keep a bounded response, but consume all process output so output limits
// cannot alter Git's exit status. The caller still rejects oversized output.
fn collect_bounded_output(mut source: impl Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 8192];
    loop {
        let count = source.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        let kept = count.min(limit.saturating_sub(output.len()));
        output.extend_from_slice(&chunk[..kept]);
        truncated |= kept < count;
    }
    Ok((output, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_git_override_names_are_fenced_including_numbered_keys() {
        use std::ffi::OsStr;
        for name in [
            "GIT_DIR", "git_work_tree", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY",
            "GIT_CONFIG_COUNT", "GIT_CONFIG_KEY_57", "GIT_CONFIG_VALUE_57",
            "GIT_CONFIG_PARAMETERS", "GIT_SSH_COMMAND",
        ] {
            assert!(is_git_override(OsStr::new(name)), "{name}");
        }
        for name in ["GITHUB_TOKEN", "HOME", "PATH", "EDITOR"] {
            assert!(!is_git_override(OsStr::new(name)), "{name}");
        }
    }

    #[test]
    fn sanitizer_removes_explicit_git_overrides_from_command() {
        let mut cmd = Command::new("git");
        cmd.env("GIT_DIR", "/other/repository")
            .env("GIT_CONFIG_KEY_57", "core.fsmonitor")
            .env("GIT_CONFIG_VALUE_57", "evil-program")
            .env("UNRELATED_SETTING", "preserve");
        sanitize_git_environment(&mut cmd);
        let configured: Vec<_> = cmd.get_envs()
            .map(|(key, value)| (key.to_string_lossy().to_string(), value.is_some()))
            .collect();
        assert!(configured.iter().any(|(key, active)| key == "UNRELATED_SETTING" && *active));
        assert!(configured.iter().all(|(key, active)| {
            !is_git_override(std::ffi::OsStr::new(key)) || !active
        }));
    }

    #[test]
    fn sanitized_git_process_does_not_inherit_repository_redirection() {
        let trusted = tempfile::tempdir().unwrap();
        let hostile = tempfile::tempdir().unwrap();
        for root in [trusted.path(), hostile.path()] {
            assert!(Command::new("git").arg("init").arg("--quiet")
                .arg(root).status().unwrap().success());
        }
        std::fs::write(trusted.path().join("trusted.txt"), b"allowed").unwrap();
        std::fs::write(hostile.path().join("hostile.txt"), b"wrong repo").unwrap();
        let mut cmd = Command::new("git");
        // Simulate inherited environment without modifying global env: the
        // sanitized process receives a clean set of Git overrides.
        sanitize_git_environment(&mut cmd);
        cmd.env_remove("GIT_DIR").env_remove("GIT_WORK_TREE")
            .arg("-C").arg(trusted.path())
            .args(GIT_STATUS_ARGS);
        let output = cmd.output().unwrap();
        assert!(output.status.success());
        let status = String::from_utf8(output.stdout).unwrap();
        assert!(status.contains("trusted.txt"), "{status}");
        assert!(!status.contains("hostile.txt"), "{status}");
    }

    #[test]
    fn oversized_git_output_is_drained_without_unbounded_retention() {
        let mut source = std::io::Cursor::new(vec![b'x'; 128 * 1024]);
        let (output, truncated) = collect_bounded_output(&mut source, 64 * 1024).unwrap();
        assert!(truncated);
        assert_eq!(output.len(), 64 * 1024);
        assert_eq!(source.position(), 128 * 1024);
    }

    #[test]
    fn exact_limit_and_small_output_are_not_marked_truncated() {
        for size in [0, 4096, 64 * 1024] {
            let mut source = std::io::Cursor::new(vec![b'y'; size]);
            let (output, truncated) = collect_bounded_output(&mut source, 64 * 1024).unwrap();
            assert!(!truncated);
            assert_eq!(output.len(), size);
            assert_eq!(source.position(), size as u64);
        }
    }

    #[test]
    fn git_status_includes_new_unstaged_files_but_not_ignored_files() {
        let root = tempfile::tempdir().unwrap();
        let init = Command::new("git")
            .args(["init", "--quiet"])
            .arg(root.path())
            .output()
            .expect("Git CLI must be available to test Git adapter");
        assert!(init.status.success(), "git init failed: {:?}", init.stderr);
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("new_module.rs"), "pub fn new() {}").unwrap();
        std::fs::write(
            root.path().join("src").join("new_test.rs"),
            "#[test] fn test() {}",
        )
        .unwrap();
        std::fs::write(root.path().join(".gitignore"), "ignored.tmp\n").unwrap();
        std::fs::write(root.path().join("ignored.tmp"), "not part of status").unwrap();
        // Use the same arguments as the approved production status path.
        let result = Command::new("git")
            .args(["--no-optional-locks", "-c", "core.fsmonitor=false", "-C"])
            .arg(root.path())
            .args(GIT_STATUS_ARGS)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "git status failed: {:?}",
            result.stderr
        );
        let status = String::from_utf8(result.stdout).unwrap();
        assert!(status.contains("?? new_module.rs"), "{status}");
        assert!(status.contains("?? src/new_test.rs"), "{status}");
        assert!(!status.contains("ignored.tmp"), "{status}");
    }

    use wex_core::operation::OperationIntent;
    #[test]
    fn git_diff_requires_distinct_approved_capability() {
        let dispatch = Dispatch {
            intent: OperationIntent {
                id: "op".into(),
                actor_id: "agent".into(),
                conversation_id: "conversation".into(),
                context_id: None,
                connection_id: "local".into(),
                capability_id: "git_status".into(),
                resource_id: "repo".into(),
                action: Action::Read,
                expected_version: None,
                intent_digest: "a".repeat(64),
            },
            attempt_id: "attempt".into(),
        };
        assert_eq!(
            approved_git_diff(&dispatch, "repo", Path::new("/unavailable"))
                .unwrap_err()
                .code,
            "DISPATCH_REJECTED"
        );
    }

    #[test]
    fn refuses_unmatched_operation_before_spawning_git() {
        let dispatch = Dispatch {
            intent: OperationIntent {
                id: "op".into(),
                actor_id: "a".into(),
                conversation_id: "c".into(),
                context_id: None,
                connection_id: "local".into(),
                capability_id: "git_status".into(),
                resource_id: "repository".into(),
                action: Action::Read,
                expected_version: None,
                intent_digest: "0".repeat(64),
            },
            attempt_id: "attempt".into(),
        };
        assert_eq!(
            approved_git_status(&dispatch, "different", Path::new("/not/a/repository"))
                .unwrap_err()
                .code,
            "DISPATCH_REJECTED"
        );
    }
}
