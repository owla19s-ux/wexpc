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

/// Confirmed durable terminal record, not authority for another write.
/// The ledger stores an immutable commit reference, not the full readback.
#[derive(Debug, PartialEq, Eq)]
pub struct GitCommitReceipt {
    revision: u64,
    generation: u64,
    operation_id: String,
    attempt_id: String,
    evidence_ref: String,
    commit_oid: String,
}
impl GitCommitReceipt {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn attempt_id(&self) -> &str {
        &self.attempt_id
    }
    pub fn evidence_ref(&self) -> &str {
        &self.evidence_ref
    }
    pub fn commit_oid(&self) -> &str {
        &self.commit_oid
    }
}

/// Settle an admitted attempt using independent trusted Host evidence only.
/// expected_tree must come from the isolated exact-path index before writing;
/// actual must include authenticated object bytes, full changed paths and a
/// current repository/branch/HEAD observation. Never use model-provided claims.
///
/// Retain admission (and its repository lease) through this call. On ANY error,
/// do not issue another Git write: the persisted InFlight/Unknown fence remains,
/// or another trusted worker/session has already changed the record. Recovery
/// must inspect the actual state. This function does not reconcile cold starts.
pub fn settle_verified_git_commit(
    operations: &OperationStore,
    admission: &GitCommitAdmission,
    expected_tree: &str,
    actual: &super::git_commit_evidence::CommitReadback,
) -> Result<GitCommitReceipt> {
    use wex_core::operation::Outcome;
    use wex_workspace_store::operations::OperationChange;
    admission
        .prepared
        .verify_readback(expected_tree, actual)
        .map_err(|_| {
            ServiceError::new(
                "GIT_EVIDENCE_REJECTED",
                "提交回读未能证明批准的结果；不得重派写入",
            )
        })?;
    let current = verify_durable_admission(operations, &admission.committed)?;
    let dispatch = &admission.committed.dispatch;
    let evidence_ref = format!("git_commit_{}", actual.oid);
    // CAS against this fresh view permits unrelated ledger changes, while any
    // concurrent mutation after the check rejects settlement instead of success.
    let settled = operations
        .apply(
            current.revision,
            OperationChange::Finish {
                id: dispatch.intent.id.clone(),
                attempt_id: dispatch.attempt_id.clone(),
                outcome: Outcome::Succeeded {
                    evidence_ref: evidence_ref.clone(),
                },
            },
        )
        .map_err(store_error)?;
    Ok(GitCommitReceipt {
        revision: settled.revision,
        generation: settled.generation,
        operation_id: dispatch.intent.id.clone(),
        attempt_id: dispatch.attempt_id.clone(),
        evidence_ref,
        commit_oid: actual.oid.clone(),
    })
}

fn verify_durable_admission(
    operations: &OperationStore,
    committed: &CommittedDispatch,
) -> Result<wex_workspace_store::operations::OperationStoreView> {
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
    Ok(current)
}

fn store_error(_: wex_workspace_store::operations::OperationStoreError) -> ServiceError {
    ServiceError::new(
        "OPERATION_STORE_ERROR",
        "操作账本未能确认本次状态，不得派发或自动重试写入",
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
    #[cfg(unix)]
    fn with_settlement(
        run: impl FnOnce(
            &OperationStore,
            &std::path::Path,
            &GitCommitAdmission,
            &ResourceLeases,
            super::super::git_commit_evidence::CommitReadback,
            String,
        ),
    ) {
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
        // Synthetic trusted-read fixtures test settlement, not a real Git write.
        let parts = p.binding_parts();
        let tree = "e".repeat(40);
        let actual = super::super::git_commit_evidence::CommitReadback {
            oid: "d".repeat(40),
            parents: vec![p.expected_head().unwrap().into()],
            tree: tree.clone(),
            message: "commit".into(),
            changed_paths: vec!["a".into()],
            repository_identity: String::from_utf8(parts[1].to_vec()).unwrap(),
            branch: String::from_utf8(parts[2].to_vec()).unwrap(),
            head: "d".repeat(40),
        };
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
        run(&store, &root, &admission, &leases, actual, tree);
    }

    #[cfg(unix)]
    #[test]
    fn settlement_rejects_bad_evidence_without_clearing_durable_fence_or_lease() {
        with_settlement(|store, root, admission, leases, actual, tree| {
            let before = fs::read(root.join("operations.json")).unwrap();
            let mut cases = Vec::new();
            let mut bad = actual.clone();
            bad.parents.clear();
            cases.push(bad);
            let mut bad = actual.clone();
            bad.tree = "f".repeat(40);
            cases.push(bad);
            let mut bad = actual.clone();
            bad.message.push('!');
            cases.push(bad);
            let mut bad = actual.clone();
            bad.changed_paths.push("outside".into());
            cases.push(bad);
            let mut bad = actual.clone();
            bad.repository_identity.push('!');
            cases.push(bad);
            let mut bad = actual.clone();
            bad.head = "f".repeat(40);
            cases.push(bad);
            let mut bad = actual.clone();
            bad.oid = "invalid".into();
            cases.push(bad);
            for bad in cases {
                assert_eq!(
                    settle_verified_git_commit(store, admission, &tree, &bad)
                        .unwrap_err()
                        .code,
                    "GIT_EVIDENCE_REJECTED"
                );
            }
            assert_eq!(fs::read(root.join("operations.json")).unwrap(), before);
            assert!(leases
                .is_busy(&format!(
                    "repository:{}",
                    admission.committed.dispatch.intent.resource_id
                ))
                .unwrap());
        });
    }

    #[cfg(unix)]
    #[test]
    fn settlement_returns_receipt_only_after_success_is_saved_and_survives_restart() {
        with_settlement(|store, root, admission, leases, actual, tree| {
            // Unrelated operations may advance the shared revision after begin.
            let mut unrelated = admission.committed.dispatch.intent.clone();
            unrelated.id = "unrelated".into();
            let revision = store.load().unwrap().revision;
            store
                .apply(revision, OperationChange::Propose(unrelated))
                .unwrap();
            let revision = store.load().unwrap().revision;
            let receipt = settle_verified_git_commit(store, admission, &tree, &actual).unwrap();
            assert_eq!(receipt.revision(), revision + 1);
            assert_eq!(receipt.generation(), admission.committed.generation);
            assert_eq!(receipt.operation_id(), "op");
            assert_eq!(receipt.attempt_id(), "attempt");
            assert_eq!(receipt.commit_oid(), actual.oid);
            assert_eq!(receipt.evidence_ref(), format!("git_commit_{}", actual.oid));
            let saved = fs::read(root.join("operations.json")).unwrap();
            assert!(leases
                .is_busy(&format!(
                    "repository:{}",
                    admission.committed.dispatch.intent.resource_id
                ))
                .unwrap());
            // A second settlement cannot publish another terminal revision.
            assert!(settle_verified_git_commit(store, admission, &tree, &actual).is_err());
            assert_eq!(fs::read(root.join("operations.json")).unwrap(), saved);
            let reopened = OperationStore::open(root).unwrap();
            assert!(matches!(&reopened.load().unwrap().ledger.records[0].state,
                OperationState::Succeeded { attempt_id, evidence_ref }
                if attempt_id == "attempt" && evidence_ref == receipt.evidence_ref()));
        });
    }

    #[cfg(unix)]
    #[test]
    fn settlement_store_lock_failure_returns_no_success_and_leaves_inflight() {
        with_settlement(|store, root, admission, _leases, actual, tree| {
            let before = fs::read(root.join("operations.json")).unwrap();
            fs::write(root.join("operations.lock"), "held").unwrap();
            assert_eq!(
                settle_verified_git_commit(store, admission, &tree, &actual)
                    .unwrap_err()
                    .code,
                "OPERATION_STORE_ERROR"
            );
            fs::remove_file(root.join("operations.lock")).unwrap();
            assert_eq!(fs::read(root.join("operations.json")).unwrap(), before);
            assert!(matches!(
                store.load().unwrap().ledger.records[0].state,
                OperationState::InFlight { .. }
            ));
        });
    }

    #[cfg(unix)]
    #[test]
    fn settlement_leftover_temporary_snapshot_returns_no_success_and_restart_is_unknown() {
        with_settlement(|store, root, admission, _leases, actual, tree| {
            let before = fs::read(root.join("operations.json")).unwrap();
            // An interrupted publication marker makes storage unavailable.
            fs::create_dir(root.join("operations.tmp")).unwrap();
            // The storage reader also treats a leftover tmp as unavailable.
            assert!(settle_verified_git_commit(store, admission, &tree, &actual).is_err());
            fs::remove_dir(root.join("operations.tmp")).unwrap();
            assert_eq!(fs::read(root.join("operations.json")).unwrap(), before);
            let reopened = OperationStore::open(root).unwrap();
            assert!(matches!(
                reopened.load().unwrap().ledger.records[0].state,
                OperationState::OutcomeUnknown { .. }
            ));
        });
    }

    #[cfg(unix)]
    #[test]
    fn settlement_rejects_reconciled_unknown_and_replaced_session() {
        with_settlement(|store, _root, admission, _leases, actual, tree| {
            let revision = store.load().unwrap().revision;
            store
                .apply(
                    revision,
                    OperationChange::Finish {
                        id: "op".into(),
                        attempt_id: "attempt".into(),
                        outcome: Outcome::Unknown,
                    },
                )
                .unwrap();
            let before = store.load().unwrap();
            assert_eq!(
                settle_verified_git_commit(store, admission, &tree, &actual)
                    .unwrap_err()
                    .code,
                "GIT_ADMISSION_UNCERTAIN"
            );
            assert_eq!(store.load().unwrap(), before);
        });
        with_settlement(|store, root, admission, _leases, actual, tree| {
            let reopened = OperationStore::open(root).unwrap();
            let before = fs::read(root.join("operations.json")).unwrap();
            assert_eq!(
                settle_verified_git_commit(store, admission, &tree, &actual)
                    .unwrap_err()
                    .code,
                "OPERATION_STORE_ERROR"
            );
            assert_eq!(
                settle_verified_git_commit(&reopened, admission, &tree, &actual)
                    .unwrap_err()
                    .code,
                "GIT_ADMISSION_UNCERTAIN"
            );
            assert_eq!(fs::read(root.join("operations.json")).unwrap(), before);
        });
    }
}
