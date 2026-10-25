//! Post-dispatch commit-object reader. A successful read is only object
//! evidence: Host must still verify repository identity, HEAD, tree and paths.
use super::{
    git_commit_binding::verify_git_commit_approval,
    git_commit_evidence::{parse_commit_object, CommitObject, PreparedCommit},
    process_capture::collect,
    process_pipe::PipeReader,
};
use crate::{Result, ServiceError};
use std::{
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use wex_core::operation::Dispatch;

pub fn read_approved_git_commit_object(
    dispatch: &Dispatch,
    resource_id: &str,
    root: &Path,
    prepared: &PreparedCommit,
    trusted_git_program: &Path,
    oid: &str,
) -> Result<CommitObject> {
    verify_git_commit_approval(dispatch, resource_id, root, prepared)?;
    if !matches!(oid.len(), 40 | 64)
        || !oid
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || !trusted_git_program.is_absolute()
        || !trusted_git_program.is_file()
    {
        return Err(ServiceError::new(
            "INVALID_INPUT",
            "Git 程序或提交对象标识无效",
        ));
    }
    let mut command = object_command(trusted_git_program, root, oid);
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            if !matches!(self.0.try_wait(), Ok(Some(_))) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
    let mut child = Child(command.spawn().map_err(|_| evidence_unavailable())?);
    let pipe = PipeReader::new(child.0.stdout.take().expect("piped Git stdout"))
        .map_err(|_| evidence_unavailable())?;
    let mut capture = collect(pipe);
    let started = Instant::now();
    let mut status = None;
    loop {
        if started.elapsed() >= Duration::from_secs(15) {
            return Err(evidence_unavailable());
        }
        if status.is_none() {
            status = child.0.try_wait().map_err(|_| evidence_unavailable())?;
        }
        let state = capture.state.lock().map_err(|_| evidence_unavailable())?;
        if state.failed {
            return Err(evidence_unavailable());
        }
        if status.is_some() && state.finished {
            break;
        }
        drop(state);
        thread::sleep(Duration::from_millis(5));
    }
    capture.stop();
    let state = capture.state.lock().map_err(|_| evidence_unavailable())?;
    if !status.is_some_and(|s| s.success()) || state.truncated || !state.finished || state.failed {
        return Err(evidence_unavailable());
    }
    // `cat-file commit` can peel an annotated tag, and Git does not always
    // rehash loose objects on read. Bind the exact raw COMMIT bytes to oid.
    let mut hash = ring::digest::Context::new(if oid.len() == 40 {
        &ring::digest::SHA1_FOR_LEGACY_USE_ONLY
    } else {
        &ring::digest::SHA256
    });
    hash.update(format!("commit {}\0", state.bytes.len()).as_bytes());
    hash.update(&state.bytes);
    let actual = hash
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if actual != oid {
        return Err(evidence_unavailable());
    }
    parse_commit_object(&state.bytes).map_err(|_| evidence_unavailable())
}

fn evidence_unavailable() -> ServiceError {
    ServiceError::new(
        "COMMIT_EVIDENCE_UNAVAILABLE",
        "Git 提交对象未能完整核验；不得据此宣布成功或自动重试写入",
    )
}

// The binary is selected by trusted Host environment configuration, never by
// model tool arguments. Git versions lacking --no-lazy-fetch fail closed.
fn object_command(program: &Path, root: &Path, oid: &str) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .args([
            "--no-optional-locks",
            "--no-replace-objects",
            "--no-lazy-fetch",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=",
            "-c",
            "maintenance.auto=false",
            "-c",
            "gc.auto=0",
            "-c",
            "protocol.allow=never",
            "cat-file",
            "commit",
            oid,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{git_commit_intent_digest, RepositoryVersion};
    use wex_core::operation::{Action, OperationIntent};

    fn fixture() -> (tempfile::TempDir, PreparedCommit, Dispatch) {
        let dir = tempfile::tempdir().unwrap();
        assert!(Command::new("git")
            .args(["init", "--quiet", "--object-format=sha1"])
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        let p = PreparedCommit::new(
            "message\n".into(),
            vec!["a".into()],
            RepositoryVersion {
                identity: "host_repository_identity".into(),
                branch: "refs/heads/work".into(),
                head: None,
                index_sha256: "a".repeat(64),
                selected_sha256: "b".repeat(64),
            },
        )
        .unwrap();
        let d = Dispatch {
            intent: OperationIntent {
                id: "op".into(),
                actor_id: "agent".into(),
                conversation_id: "chat".into(),
                context_id: None,
                connection_id: "local".into(),
                capability_id: "git_commit".into(),
                resource_id: "repo".into(),
                action: Action::Write,
                expected_version: None,
                intent_digest: git_commit_intent_digest("repo", dir.path(), &p).unwrap(),
            },
            attempt_id: "attempt".into(),
        };
        (dir, p, d)
    }
    fn git_program() -> std::path::PathBuf {
        std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join(if cfg!(windows) { "git.exe" } else { "git" }))
            .find(|p| p.is_absolute() && p.is_file())
            .expect("Git required for adapter tests")
    }

    fn commit_object(root: &Path, message: &str) -> String {
        use std::io::Write;
        let tree = Command::new(git_program())
            .current_dir(root)
            .args(["mktree"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(tree.status.success());
        let tree = String::from_utf8(tree.stdout).unwrap().trim().to_string();
        let mut child = Command::new(git_program())
            .current_dir(root)
            .args([
                "-c",
                "user.name=WEX Test",
                "-c",
                "user.email=wex@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "commit-tree",
                &tree,
                "-F",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(message.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn replacement_refs_cannot_substitute_commit_evidence() {
        let (dir, p, d) = fixture();
        let original = commit_object(dir.path(), "original\n");
        let replacement = commit_object(dir.path(), "replacement\n");
        assert!(Command::new(git_program())
            .current_dir(dir.path())
            .args(["replace", &original, &replacement])
            .status()
            .unwrap()
            .success());
        let object =
            read_approved_git_commit_object(&d, "repo", dir.path(), &p, &git_program(), &original)
                .unwrap();
        assert_eq!(object.message, "original\n");
    }

    #[test]
    fn oversized_object_is_drained_but_never_accepted() {
        let (dir, p, d) = fixture();
        let oid = commit_object(dir.path(), &"x".repeat(96 * 1024));
        assert_eq!(
            read_approved_git_commit_object(&d, "repo", dir.path(), &p, &git_program(), &oid)
                .unwrap_err()
                .code,
            "COMMIT_EVIDENCE_UNAVAILABLE"
        );
    }

    #[test]
    fn annotated_tag_oid_cannot_stand_in_for_a_commit_oid() {
        let (dir, p, d) = fixture();
        let oid = commit_object(dir.path(), "message\n");
        assert!(Command::new(git_program())
            .current_dir(dir.path())
            .args([
                "-c",
                "user.name=WEX Test",
                "-c",
                "user.email=wex@example.invalid",
                "tag",
                "-a",
                "reviewed",
                "-m",
                "tag",
                &oid
            ])
            .status()
            .unwrap()
            .success());
        let output = Command::new(git_program())
            .current_dir(dir.path())
            .args(["rev-parse", "refs/tags/reviewed"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let tag = String::from_utf8(output.stdout).unwrap().trim().to_string();
        assert_eq!(
            read_approved_git_commit_object(&d, "repo", dir.path(), &p, &git_program(), &tag)
                .unwrap_err()
                .code,
            "COMMIT_EVIDENCE_UNAVAILABLE"
        );
    }

    #[test]
    fn corrupted_loose_object_content_cannot_match_requested_oid() {
        let (dir, p, d) = fixture();
        let oid = commit_object(dir.path(), "original\n");
        let other = commit_object(dir.path(), "tampered\n");
        let object_path = |id: &str| {
            dir.path()
                .join(".git/objects")
                .join(&id[..2])
                .join(&id[2..])
        };
        let target = object_path(&oid);
        #[cfg(windows)]
        {
            let mut permissions = std::fs::metadata(&target).unwrap().permissions();
            permissions.set_readonly(false);
            std::fs::set_permissions(&target, permissions).unwrap();
        }
        std::fs::remove_file(&target).unwrap();
        std::fs::copy(object_path(&other), &target).unwrap();
        assert_eq!(
            read_approved_git_commit_object(&d, "repo", dir.path(), &p, &git_program(), &oid)
                .unwrap_err()
                .code,
            "COMMIT_EVIDENCE_UNAVAILABLE"
        );
    }
    #[test]
    fn reads_real_object_without_requiring_old_head_to_remain_current() {
        let (dir, p, d) = fixture();
        let oid = commit_object(dir.path(), "message\n");
        assert!(Command::new(git_program())
            .current_dir(dir.path())
            .args(["symbolic-ref", "HEAD", "refs/heads/work"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new(git_program())
            .current_dir(dir.path())
            .args(["update-ref", "refs/heads/work", &oid])
            .status()
            .unwrap()
            .success());
        let actual =
            read_approved_git_commit_object(&d, "repo", dir.path(), &p, &git_program(), &oid)
                .unwrap();
        assert_eq!(actual.message, "message\n");
        assert!(actual.parents.is_empty());
        assert_eq!(actual.tree, "4b825dc642cb6eb9a060e54bf8d69288fbee4904");
    }
    #[test]
    fn refuses_wrong_dispatch_before_starting_any_program() {
        let (dir, p, d) = fixture();
        assert_eq!(
            read_approved_git_commit_object(
                &d,
                "other",
                dir.path(),
                &p,
                Path::new("/missing"),
                &"a".repeat(40)
            )
            .unwrap_err()
            .code,
            "DISPATCH_REJECTED"
        );
    }
    #[test]
    fn refuses_argument_like_or_invalid_object_ids() {
        let (dir, p, d) = fixture();
        for oid in ["--filters", "HEAD", "../object", ""] {
            assert_eq!(
                read_approved_git_commit_object(&d, "repo", dir.path(), &p, &git_program(), oid)
                    .unwrap_err()
                    .code,
                "INVALID_INPUT"
            );
        }
    }
    #[test]
    fn missing_object_is_not_success_evidence() {
        let (dir, p, d) = fixture();
        assert_eq!(
            read_approved_git_commit_object(
                &d,
                "repo",
                dir.path(),
                &p,
                &git_program(),
                &"0".repeat(40)
            )
            .unwrap_err()
            .code,
            "COMMIT_EVIDENCE_UNAVAILABLE"
        );
    }

    #[test]
    fn promisor_repository_does_not_fetch_missing_commit_during_evidence_read() {
        let (source, _, _) = fixture();
        let oid = commit_object(source.path(), "remote-only\n");
        assert!(Command::new(git_program())
            .current_dir(source.path())
            .args(["update-ref", "refs/heads/work", &oid])
            .status()
            .unwrap()
            .success());
        let configure = |root: &Path| {
            for (key, value) in [
                ("core.repositoryformatversion", "1"),
                ("extensions.partialClone", "origin"),
                ("remote.origin.promisor", "true"),
                ("remote.origin.url", source.path().to_str().unwrap()),
            ] {
                assert!(Command::new(git_program())
                    .current_dir(root)
                    .args(["config", key, value])
                    .status()
                    .unwrap()
                    .success());
            }
        };
        // Positive control: this local-only promisor fixture really can fetch.
        let (control, _, _) = fixture();
        configure(control.path());
        assert!(Command::new(git_program())
            .current_dir(control.path())
            .args([
                "-c",
                "protocol.file.allow=always",
                "cat-file",
                "commit",
                &oid
            ])
            .output()
            .unwrap()
            .status
            .success());
        let (dir, p, d) = fixture();
        configure(dir.path());
        assert_eq!(
            read_approved_git_commit_object(&d, "repo", dir.path(), &p, &git_program(), &oid)
                .unwrap_err()
                .code,
            "COMMIT_EVIDENCE_UNAVAILABLE"
        );
        assert!(!dir.path().join(".git/FETCH_HEAD").exists());
        assert!(std::fs::read_dir(dir.path().join(".git/objects/pack"))
            .unwrap()
            .next()
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn timeout_closes_reader_and_terminates_idle_child() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, p, d) = fixture();
        let pid_file = dir.path().join("reader-child-pid");
        let program = dir.path().join("idle-git");
        let quoted = pid_file.to_str().unwrap().replace('\'', "'\\''");
        std::fs::write(
            &program,
            format!("#!/bin/sh\nprintf '%s' \"$$\" > '{quoted}'\nexec /bin/sleep 60\n"),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let started = Instant::now();
        assert_eq!(
            read_approved_git_commit_object(&d, "repo", dir.path(), &p, &program, &"a".repeat(40))
                .unwrap_err()
                .code,
            "COMMIT_EVIDENCE_UNAVAILABLE"
        );
        assert!(started.elapsed() < Duration::from_secs(20));
        let pid: i32 = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}
