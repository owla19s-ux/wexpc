//! Core Dispatch boundary for a prepared local Git commit. No Git is executed.
use super::git_commit_evidence::{PreparedCommit, RepositoryVersion};
use crate::{Result, ServiceError};
use std::path::Path;
use wex_core::operation::{Action, Dispatch};

pub fn git_commit_intent_digest(
    resource_id: &str,
    root: &Path,
    prepared: &PreparedCommit,
) -> Result<String> {
    if !root.is_absolute() || !root.is_dir() {
        return Err(ServiceError::new(
            "RESOURCE_UNAVAILABLE",
            "Git 仓库目录不可用",
        ));
    }
    let root = root
        .to_str()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "Git 仓库目录编码无效"))?;
    let mut parts = prepared.binding_parts();
    parts.insert(0, root.as_bytes());
    Ok(super::intent_binding::digest_arguments(
        "git_commit",
        resource_id,
        &parts,
    ))
}

pub fn verify_git_commit_dispatch(
    dispatch: &Dispatch,
    resource_id: &str,
    root: &Path,
    prepared: &PreparedCommit,
    current: &RepositoryVersion,
) -> Result<()> {
    verify_git_commit_approval(dispatch, resource_id, root, prepared)?;
    prepared
        .recheck(current)
        .map_err(|_| ServiceError::new("VERSION_CONFLICT", "Git 仓库、索引或选中文件版本已变化"))
}

// Shared by the pre-write version gate and post-write evidence reader. The
// latter must not require the old HEAD to still be current after a commit.
pub(super) fn verify_git_commit_approval(
    dispatch: &Dispatch,
    resource_id: &str,
    root: &Path,
    prepared: &PreparedCommit,
) -> Result<()> {
    if dispatch.intent.action != Action::Write
        || dispatch.intent.capability_id != "git_commit"
        || dispatch.intent.resource_id != resource_id
        || dispatch.intent.expected_version.as_deref() != prepared.expected_head()
    {
        return Err(ServiceError::new(
            "DISPATCH_REJECTED",
            "Git 提交不符合已批准范围",
        ));
    }
    if git_commit_intent_digest(resource_id, root, prepared)? != dispatch.intent.intent_digest {
        return Err(ServiceError::new(
            "INTENT_MISMATCH",
            "Git 提交参数或仓库与批准时不一致",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wex_core::operation::{ExecutionChecks, OperationIntent, OperationLedger};
    fn version() -> RepositoryVersion {
        RepositoryVersion {
            identity: "host_worktree_gitdir".into(),
            branch: "refs/heads/work".into(),
            head: Some("a".repeat(40)),
            index_sha256: "b".repeat(64),
            selected_sha256: "c".repeat(64),
        }
    }
    fn prepared(message: &str, paths: &[&str], v: RepositoryVersion) -> PreparedCommit {
        PreparedCommit::new(
            message.into(),
            paths.iter().map(|p| (*p).into()).collect(),
            v,
        )
        .unwrap()
    }
    fn approved(root: &Path, p: &PreparedCommit, head: Option<String>) -> Dispatch {
        let intent = OperationIntent {
            id: "operation".into(),
            actor_id: "agent".into(),
            conversation_id: "conversation".into(),
            context_id: None,
            connection_id: "local".into(),
            capability_id: "git_commit".into(),
            resource_id: "repository".into(),
            action: Action::Write,
            expected_version: head,
            intent_digest: git_commit_intent_digest("repository", root, p).unwrap(),
        };
        let mut ledger = OperationLedger::default();
        ledger.propose(intent.clone()).unwrap();
        ledger.approve("operation", &intent, 7).unwrap();
        ledger
            .begin(
                "operation",
                "attempt",
                ExecutionChecks {
                    authority_revision: 7,
                    connection_enabled: true,
                    authenticated: true,
                    platform_available: true,
                    resource_allowed: true,
                },
            )
            .unwrap()
    }
    #[test]
    fn consumed_core_dispatch_accepts_exact_prepared_commit() {
        let dir = tempfile::tempdir().unwrap();
        let v = version();
        let p = prepared("message", &["src/a"], v.clone());
        let d = approved(dir.path(), &p, v.head.clone());
        assert!(verify_git_commit_dispatch(&d, "repository", dir.path(), &p, &v).is_ok());
    }
    #[test]
    fn changing_message_paths_or_root_breaks_the_approval() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let v = version();
        let p = prepared("message", &["src/a"], v.clone());
        let d = approved(dir.path(), &p, v.head.clone());
        for changed in [
            prepared("other", &["src/a"], v.clone()),
            prepared("message", &["src/b"], v.clone()),
        ] {
            assert_eq!(
                verify_git_commit_dispatch(&d, "repository", dir.path(), &changed, &v)
                    .unwrap_err()
                    .code,
                "INTENT_MISMATCH"
            );
        }
        assert_eq!(
            verify_git_commit_dispatch(&d, "repository", other.path(), &p, &v)
                .unwrap_err()
                .code,
            "INTENT_MISMATCH"
        );
    }
    #[test]
    fn wrong_dispatch_scope_or_expected_head_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let v = version();
        let p = prepared("message", &["a"], v.clone());
        for field in 0..4 {
            let mut d = approved(dir.path(), &p, v.head.clone());
            match field {
                0 => d.intent.action = Action::Read,
                1 => d.intent.capability_id = "git_status".into(),
                2 => d.intent.resource_id = "other".into(),
                _ => d.intent.expected_version = None,
            }
            assert_eq!(
                verify_git_commit_dispatch(&d, "repository", dir.path(), &p, &v)
                    .unwrap_err()
                    .code,
                "DISPATCH_REJECTED"
            );
        }
    }
    #[test]
    fn every_changed_live_version_is_rejected_after_approval() {
        let dir = tempfile::tempdir().unwrap();
        let v = version();
        let p = prepared("message", &["a"], v.clone());
        let d = approved(dir.path(), &p, v.head.clone());
        for field in 0..5 {
            let mut current = v.clone();
            match field {
                0 => current.identity.push('x'),
                1 => current.head = None,
                2 => current.branch.push('x'),
                3 => current.index_sha256 = "0".repeat(64),
                _ => current.selected_sha256 = "0".repeat(64),
            }
            assert_eq!(
                verify_git_commit_dispatch(&d, "repository", dir.path(), &p, &current)
                    .unwrap_err()
                    .code,
                "VERSION_CONFLICT"
            );
        }
    }
    #[test]
    fn unborn_branch_binds_absent_head() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = version();
        v.head = None;
        let p = prepared("message", &["a"], v.clone());
        let d = approved(dir.path(), &p, None);
        assert!(verify_git_commit_dispatch(&d, "repository", dir.path(), &p, &v).is_ok());
    }
    #[test]
    fn relative_or_unavailable_root_is_rejected() {
        let p = prepared("message", &["a"], version());
        assert_eq!(
            git_commit_intent_digest("repository", Path::new("relative"), &p)
                .unwrap_err()
                .code,
            "RESOURCE_UNAVAILABLE"
        );
    }
}
