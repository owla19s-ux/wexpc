//! Trusted Host admission for local Git writes. No Git command is issued.
//! The shared resource lease and durable unresolved-write fence are separate.
use super::git_commit_evidence::PreparedCommit;
use super::git_commit_snapshot::GitCommitSnapshotSource;
use crate::{Result, ServiceError};
use wex_core::{
    dispatch_leases::{ResourceLease, ResourceLeases},
    operation::ExecutionChecks,
};
use wex_workspace_store::operations::{CommittedDispatch, OperationStore};

pub struct GitCommitBegin<'a> {
    pub expected_store_revision: u64,
    pub operation_id: &'a str,
    pub attempt_id: &'a str,
    pub checks: ExecutionChecks,
    pub source: &'a GitCommitSnapshotSource<'a>,
    pub prepared: PreparedCommit,
}

#[derive(Debug)]
pub struct GitCommitAdmission {
    committed: CommittedDispatch,
    prepared: PreparedCommit,
    _lease: ResourceLease,
}
impl GitCommitAdmission {
    pub fn committed(&self) -> &CommittedDispatch {
        &self.committed
    }
    pub fn prepared(&self) -> &PreparedCommit {
        &self.prepared
    }
}

pub fn git_commit_resource_id(source: &GitCommitSnapshotSource<'_>) -> Result<String> {
    // Registration/grant IDs vary across conversations; physical identity must
    // not. Retained registry handles prevent inode reuse during this session.
    let binding = source
        .registry
        .export_binding(source.directory_id)
        .map_err(|_| ServiceError::new("GIT_SNAPSHOT_UNAVAILABLE", "仓库绑定身份不可用"))?;
    let mut stamp = Vec::new();
    for value in binding.stamp {
        stamp.extend_from_slice(&value.to_be_bytes());
    }
    Ok(super::intent_binding::digest_arguments(
        "git_repository_write_scope",
        "physical",
        &[std::env::consts::OS.as_bytes(), &stamp],
    ))
}

/// Trusted Host contract:
/// - Hold live authority ownership from this call through actual dispatch.
/// - Share one ResourceLeases registry and OperationStore session across workers.
/// - Reconciliation and state-changing workers must coordinate on the same
///   repository lease; a final observation cannot lock out future mutations.
/// - Keep the returned admission alive until the external result is settled.
///
/// This only coordinates this admission protocol for the same physical
/// worktree. It is not an OS lock, a universal file/terminal lock, or a writer.
pub fn begin_git_commit(
    operations: &OperationStore,
    leases: &ResourceLeases,
    request: GitCommitBegin<'_>,
) -> Result<GitCommitAdmission> {
    use wex_core::operation::{Action, OperationState, Outcome};
    use wex_workspace_store::operations::OperationChange;
    super::git_commit_snapshot::verify_current_git_write_access(request.source, &request.prepared)?;
    let resource = git_commit_resource_id(request.source)?;
    let lease = leases
        .acquire(request.attempt_id, &[format!("repository:{resource}")])
        .map_err(|e| match e {
            wex_core::dispatch_leases::LeaseError::Busy => {
                ServiceError::new("RESOURCE_BUSY", "同一仓库已有写入资源占用")
            }
            _ => ServiceError::new("RESOURCE_LOCK_ERROR", "无法取得仓库写入资源占用"),
        })?;
    let view = operations.load().map_err(store_error)?;
    if view.revision != request.expected_store_revision {
        return Err(ServiceError::new(
            "OPERATION_CONFLICT",
            "操作账本版本已变化",
        ));
    }
    if view.ledger.records.iter().any(|record| {
        record.intent.resource_id == resource
            && record.intent.action == Action::Write
            && matches!(
                record.state,
                OperationState::InFlight { .. } | OperationState::OutcomeUnknown { .. }
            )
    }) {
        return Err(ServiceError::new(
            "GIT_WRITE_UNRESOLVED",
            "该物理仓库存在尚未核实的写入，不允许重新派发",
        ));
    }
    let record = view
        .ledger
        .records
        .iter()
        .find(|r| r.intent.id == request.operation_id)
        .ok_or_else(|| ServiceError::new("DISPATCH_REJECTED", "Git 提交操作不存在"))?;
    let root = request
        .source
        .registry
        .export_binding(request.source.directory_id)
        .map_err(|_| ServiceError::new("GIT_SNAPSHOT_UNAVAILABLE", "仓库绑定已失效"))?
        .path;
    if record.intent.action != Action::Write
        || record.intent.capability_id != "git_commit"
        || record.intent.resource_id != resource
        || record.intent.expected_version.as_deref() != request.prepared.expected_head()
    {
        return Err(ServiceError::new(
            "DISPATCH_REJECTED",
            "Git 提交未绑定当前物理仓库或父版本",
        ));
    }
    if record.intent.intent_digest
        != super::git_commit_binding::git_commit_intent_digest(&resource, &root, &request.prepared)?
    {
        return Err(ServiceError::new(
            "INTENT_MISMATCH",
            "Git 提交消息、范围或版本与批准记录不符",
        ));
    }
    let current =
        super::git_commit_snapshot::observe_prepared_git_commit(request.source, &request.prepared)?;
    request
        .prepared
        .recheck(&current)
        .map_err(|_| ServiceError::new("VERSION_CONFLICT", "已批准的仓库或文件版本已变化"))?;
    // OperationStore atomically rechecks its revision/approval/authority and
    // releases a dispatch ONLY after durable InFlight publication succeeds.
    let committed = operations
        .begin(
            view.revision,
            request.operation_id,
            request.attempt_id,
            request.checks,
        )
        .map_err(store_error)?;
    if let Err(error) = super::git_commit_snapshot::verify_live_git_commit_dispatch(
        &committed.dispatch,
        &resource,
        request.source,
        &request.prepared,
    ) {
        // No Connector is invoked on this path. A failed receipt write must
        // leave InFlight fenced; never manufacture rollback or retry authority.
        let _ = operations.apply(
            committed.revision,
            OperationChange::Finish {
                id: request.operation_id.into(),
                attempt_id: request.attempt_id.into(),
                outcome: Outcome::Unknown,
            },
        );
        return Err(ServiceError::new(
            "GIT_ADMISSION_UNCERTAIN",
            format!(
                "持久准入后的复核失败（{}），未返回执行许可；须核查后处理",
                error.code
            ),
        ));
    }
    verify_durable_admission(operations, &committed)?;
    Ok(GitCommitAdmission {
        committed,
        prepared: request.prepared,
        _lease: lease,
    })
}

fn verify_durable_admission(
    operations: &OperationStore,
    committed: &CommittedDispatch,
) -> Result<()> {
    use wex_core::operation::OperationState;
    let current = operations.load().map_err(store_error)?;
    let live = current
        .ledger
        .records
        .iter()
        .find(|r| r.intent.id == committed.dispatch.intent.id);
    if current.generation!=committed.generation || current.revision<committed.revision
        || !live.is_some_and(|r|r.intent==committed.dispatch.intent && matches!(&r.state,OperationState::InFlight{attempt_id} if attempt_id==&committed.dispatch.attempt_id)) {
        return Err(ServiceError::new("GIT_ADMISSION_UNCERTAIN","当前持久尝试已变化或待核查，不返回执行许可"));
    }
    Ok(())
}

fn store_error(_: wex_workspace_store::operations::OperationStoreError) -> ServiceError {
    ServiceError::new(
        "OPERATION_STORE_ERROR",
        "操作账本未能确认本次准入，不得执行或自动重试",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::{git_commit_intent_digest, prepare_git_commit_from_registry};
    use std::{fs, process::Command};
    use wex_core::access::{Access, DirectoryGrant};
    #[cfg(unix)]
    use wex_core::operation::{Action, OperationIntent, OperationState, Outcome};
    use wex_local_resources::{DirectoryRegistry, Protection};
    #[cfg(unix)]
    use wex_workspace_store::operations::OperationChange;
    fn fixture() -> (tempfile::TempDir, DirectoryRegistry, String) {
        let dir = tempfile::tempdir().unwrap();
        assert!(Command::new("git")
            .args(["init", "--quiet", "--object-format=sha1", "-b", "work"])
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        fs::write(dir.path().join("a"), "initial").unwrap();
        assert!(Command::new("git")
            .current_dir(dir.path())
            .args(["add", "--", "a"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(dir.path())
            .args([
                "-c",
                "user.name=WEX",
                "-c",
                "user.email=wex@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "core.hooksPath=",
                "commit",
                "--quiet",
                "-m",
                "initial"
            ])
            .status()
            .unwrap()
            .success());
        let mut registry = DirectoryRegistry::new();
        let id = registry.register(dir.path(), Protection::General).unwrap();
        (dir, registry, id)
    }
    #[cfg(unix)]
    fn checks() -> ExecutionChecks {
        ExecutionChecks {
            authority_revision: 1,
            connection_enabled: true,
            authenticated: true,
            platform_available: true,
            resource_allowed: true,
        }
    }
    #[cfg(unix)]
    fn approve(
        store: &OperationStore,
        source: &GitCommitSnapshotSource<'_>,
        id: &str,
    ) -> (PreparedCommit, u64) {
        let p = prepare_git_commit_from_registry(source, "commit", &["a".into()]).unwrap();
        let resource = git_commit_resource_id(source).unwrap();
        let root = source
            .registry
            .export_binding(source.directory_id)
            .unwrap()
            .path;
        let intent = OperationIntent {
            id: id.into(),
            actor_id: "agent".into(),
            conversation_id: "chat".into(),
            context_id: None,
            connection_id: "local".into(),
            capability_id: "git_commit".into(),
            resource_id: resource.clone(),
            action: Action::Write,
            expected_version: p.expected_head().map(str::to_owned),
            intent_digest: git_commit_intent_digest(&resource, &root, &p).unwrap(),
        };
        let v = store.load().unwrap();
        let v = store
            .apply(v.revision, OperationChange::Propose(intent.clone()))
            .unwrap();
        let v = store
            .apply(
                v.revision,
                OperationChange::Approve {
                    id: id.into(),
                    exact_intent: intent,
                    authority_revision: 1,
                },
            )
            .unwrap();
        (p, v.revision)
    }
    #[test]
    fn physical_repository_scope_is_shared_across_separate_authorization_ids() {
        let (dir, a, id_a) = fixture();
        let mut b = DirectoryRegistry::new();
        let id_b = b.register(dir.path(), Protection::General).unwrap();
        assert_ne!(id_a, id_b);
        let ga = [DirectoryGrant {
            directory_id: &id_a,
            access: Access::ReadWrite,
            explicit_personal: false,
        }];
        let gb = [DirectoryGrant {
            directory_id: &id_b,
            access: Access::ReadWrite,
            explicit_personal: false,
        }];
        let sa = GitCommitSnapshotSource {
            registry: &a,
            directory_id: &id_a,
            system_access: true,
            grants: &ga,
            internal_roots: &[],
        };
        let sb = GitCommitSnapshotSource {
            registry: &b,
            directory_id: &id_b,
            system_access: true,
            grants: &gb,
            internal_roots: &[],
        };
        assert_eq!(
            git_commit_resource_id(&sa).unwrap(),
            git_commit_resource_id(&sb).unwrap()
        );
    }
    #[test]
    #[cfg(unix)]
    fn returns_admission_only_after_durable_inflight_and_keeps_resource_busy() {
        let (_dir, registry, id) = fixture();
        let data = tempfile::tempdir().unwrap();
        let store = OperationStore::open(data.path().join("operations")).unwrap();
        let leases = ResourceLeases::default();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::ReadWrite,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let (p, revision) = approve(&store, &source, "op");
        let admission = begin_git_commit(
            &store,
            &leases,
            GitCommitBegin {
                expected_store_revision: revision,
                operation_id: "op",
                attempt_id: "attempt",
                checks: checks(),
                source: &source,
                prepared: p,
            },
        )
        .unwrap();
        assert!(matches!(
            store.load().unwrap().ledger.records[0].state,
            OperationState::InFlight { .. }
        ));
        assert_eq!(admission.committed().dispatch.attempt_id, "attempt");
        assert_eq!(admission.prepared().paths(), &["a".to_string()]);
        let (p, revision) = approve(&store, &source, "op2");
        assert_eq!(
            begin_git_commit(
                &store,
                &leases,
                GitCommitBegin {
                    expected_store_revision: revision,
                    operation_id: "op2",
                    attempt_id: "attempt2",
                    checks: checks(),
                    source: &source,
                    prepared: p
                }
            )
            .unwrap_err()
            .code,
            "RESOURCE_BUSY"
        );
    }
    #[test]
    #[cfg(unix)]
    fn dropped_admission_does_not_clear_durable_unresolved_write_fence() {
        let (_dir, registry, id) = fixture();
        let data = tempfile::tempdir().unwrap();
        let store = OperationStore::open(data.path().join("operations")).unwrap();
        let leases = ResourceLeases::default();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::ReadWrite,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let (p, revision) = approve(&store, &source, "op");
        drop(
            begin_git_commit(
                &store,
                &leases,
                GitCommitBegin {
                    expected_store_revision: revision,
                    operation_id: "op",
                    attempt_id: "attempt",
                    checks: checks(),
                    source: &source,
                    prepared: p,
                },
            )
            .unwrap(),
        );
        let (p, revision) = approve(&store, &source, "op2");
        assert_eq!(
            begin_git_commit(
                &store,
                &leases,
                GitCommitBegin {
                    expected_store_revision: revision,
                    operation_id: "op2",
                    attempt_id: "attempt2",
                    checks: checks(),
                    source: &source,
                    prepared: p
                }
            )
            .unwrap_err()
            .code,
            "GIT_WRITE_UNRESOLVED"
        );
        // The test knows no external Git write was issued; recording that fact
        // is distinct from retrying an uncertain external operation.
        let revision = store.load().unwrap().revision;
        store
            .apply(
                revision,
                OperationChange::Finish {
                    id: "op".into(),
                    attempt_id: "attempt".into(),
                    outcome: Outcome::Failed {
                        evidence_ref: "test_not_issued".into(),
                    },
                },
            )
            .unwrap();
        let p = prepare_git_commit_from_registry(&source, "commit", &["a".into()]).unwrap();
        let revision = store.load().unwrap().revision;
        assert!(begin_git_commit(
            &store,
            &leases,
            GitCommitBegin {
                expected_store_revision: revision,
                operation_id: "op2",
                attempt_id: "attempt2",
                checks: checks(),
                source: &source,
                prepared: p
            }
        )
        .is_ok());
    }
    #[test]
    #[cfg(unix)]
    fn stale_file_version_does_not_consume_approval_or_persist_inflight() {
        let (dir, registry, id) = fixture();
        let data = tempfile::tempdir().unwrap();
        let store = OperationStore::open(data.path().join("operations")).unwrap();
        let leases = ResourceLeases::default();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::ReadWrite,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let (p, revision) = approve(&store, &source, "op");
        fs::write(dir.path().join("a"), "changed").unwrap();
        assert_eq!(
            begin_git_commit(
                &store,
                &leases,
                GitCommitBegin {
                    expected_store_revision: revision,
                    operation_id: "op",
                    attempt_id: "attempt",
                    checks: checks(),
                    source: &source,
                    prepared: p
                }
            )
            .unwrap_err()
            .code,
            "VERSION_CONFLICT"
        );
        let v = store.load().unwrap();
        assert_eq!(v.revision, revision);
        assert!(matches!(
            v.ledger.records[0].state,
            OperationState::Approved { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn restart_and_new_authorization_id_still_fence_the_same_unknown_repository() {
        let (dir, registry, id) = fixture();
        let data = tempfile::tempdir().unwrap();
        let root = data.path().join("operations");
        let store = OperationStore::open(&root).unwrap();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::ReadWrite,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let (p, revision) = approve(&store, &source, "op");
        drop(
            begin_git_commit(
                &store,
                &ResourceLeases::default(),
                GitCommitBegin {
                    expected_store_revision: revision,
                    operation_id: "op",
                    attempt_id: "attempt",
                    checks: checks(),
                    source: &source,
                    prepared: p,
                },
            )
            .unwrap(),
        );
        let restarted = OperationStore::open(&root).unwrap();
        assert!(matches!(
            restarted.load().unwrap().ledger.records[0].state,
            OperationState::OutcomeUnknown { .. }
        ));
        let mut registry2 = DirectoryRegistry::new();
        let id2 = registry2.register(dir.path(), Protection::General).unwrap();
        assert_ne!(id, id2);
        let grants2 = [DirectoryGrant {
            directory_id: &id2,
            access: Access::ReadWrite,
            explicit_personal: false,
        }];
        let source2 = GitCommitSnapshotSource {
            registry: &registry2,
            directory_id: &id2,
            system_access: true,
            grants: &grants2,
            internal_roots: &[],
        };
        let (p, revision) = approve(&restarted, &source2, "op2");
        assert_eq!(
            begin_git_commit(
                &restarted,
                &ResourceLeases::default(),
                GitCommitBegin {
                    expected_store_revision: revision,
                    operation_id: "op2",
                    attempt_id: "attempt2",
                    checks: checks(),
                    source: &source2,
                    prepared: p
                }
            )
            .unwrap_err()
            .code,
            "GIT_WRITE_UNRESOLVED"
        );
    }

    #[cfg(unix)]
    #[test]
    fn wrong_intent_and_stale_authority_leave_approval_and_lease_unchanged() {
        let (_dir, registry, id) = fixture();
        let data = tempfile::tempdir().unwrap();
        let store = OperationStore::open(data.path().join("operations")).unwrap();
        let leases = ResourceLeases::default();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::ReadWrite,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let (p, revision) = approve(&store, &source, "op");
        let resource = git_commit_resource_id(&source).unwrap();
        let changed =
            prepare_git_commit_from_registry(&source, "different message", &["a".into()]).unwrap();
        assert_eq!(
            begin_git_commit(
                &store,
                &leases,
                GitCommitBegin {
                    expected_store_revision: revision,
                    operation_id: "op",
                    attempt_id: "attempt",
                    checks: checks(),
                    source: &source,
                    prepared: changed
                }
            )
            .unwrap_err()
            .code,
            "INTENT_MISMATCH"
        );
        let mut stale = checks();
        stale.authority_revision = 2;
        assert_eq!(
            begin_git_commit(
                &store,
                &leases,
                GitCommitBegin {
                    expected_store_revision: revision,
                    operation_id: "op",
                    attempt_id: "attempt",
                    checks: stale,
                    source: &source,
                    prepared: p
                }
            )
            .unwrap_err()
            .code,
            "OPERATION_STORE_ERROR"
        );
        assert!(!leases.is_busy(&format!("repository:{resource}")).unwrap());
        assert_eq!(store.load().unwrap().revision, revision);
        assert!(matches!(
            store.load().unwrap().ledger.records[0].state,
            OperationState::Approved { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn refused_store_lock_never_releases_a_dispatch_or_consumes_approval() {
        let (_dir, registry, id) = fixture();
        let data = tempfile::tempdir().unwrap();
        let root = data.path().join("operations");
        let store = OperationStore::open(&root).unwrap();
        let leases = ResourceLeases::default();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::ReadWrite,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let (p, revision) = approve(&store, &source, "op");
        let before = fs::read(root.join("operations.json")).unwrap();
        fs::write(root.join("operations.lock"), "held").unwrap();
        assert_eq!(
            begin_git_commit(
                &store,
                &leases,
                GitCommitBegin {
                    expected_store_revision: revision,
                    operation_id: "op",
                    attempt_id: "attempt",
                    checks: checks(),
                    source: &source,
                    prepared: p
                }
            )
            .unwrap_err()
            .code,
            "OPERATION_STORE_ERROR"
        );
        fs::remove_file(root.join("operations.lock")).unwrap();
        assert_eq!(fs::read(root.join("operations.json")).unwrap(), before);
        assert_eq!(store.load().unwrap().revision, revision);
    }

    #[cfg(unix)]
    #[test]
    fn final_admission_check_rejects_unknown_or_replaced_service_session() {
        let (_dir, registry, id) = fixture();
        let data = tempfile::tempdir().unwrap();
        let root = data.path().join("operations");
        let store = OperationStore::open(&root).unwrap();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::ReadWrite,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let (_p, revision) = approve(&store, &source, "op");
        let committed = store.begin(revision, "op", "attempt", checks()).unwrap();
        assert!(verify_durable_admission(&store, &committed).is_ok());
        // Deterministically place reconciliation at the phase boundary after
        // durable begin and before the admission's final state check.
        store
            .apply(
                committed.revision,
                OperationChange::Finish {
                    id: "op".into(),
                    attempt_id: "attempt".into(),
                    outcome: Outcome::Unknown,
                },
            )
            .unwrap();
        assert_eq!(
            verify_durable_admission(&store, &committed)
                .unwrap_err()
                .code,
            "GIT_ADMISSION_UNCERTAIN"
        );
        let restarted = OperationStore::open(&root).unwrap();
        assert_eq!(
            verify_durable_admission(&restarted, &committed)
                .unwrap_err()
                .code,
            "GIT_ADMISSION_UNCERTAIN"
        );
    }
}
