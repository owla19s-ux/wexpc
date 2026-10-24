#![cfg(unix)]
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use wex_core::operation::{Action, ExecutionChecks, OperationIntent, OperationLedger, Outcome};
use wex_core::task::{Task, TaskStatus};
use wex_core::task_operation::TaskOperationLink;
use wex_workspace_store::operations::{OperationChange, OperationStore};
use wex_workspace_store::tasks::{TaskChange, TaskStore, TaskStoreError};
use wex_workspace_store::OrganizationError;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "wex-task-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn write(&self, data: &[u8]) {
        let path = self.0.join("agent-tasks.json");
        fs::write(&path, data).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(self.0.join("agent-tasks.json")).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn task() -> Task {
    Task {
        id: "task_1".into(),
        project_id: "project_1".into(),
        objective: "Build".into(),
        assigned_agent: Some("agent_1".into()),
        assignment_epoch: 0,
        status: TaskStatus::Running,
        evidence_refs: vec![],
    }
}
fn persisted_ledger(root: &Root) -> OperationLedger {
    let store = OperationStore::open(&root.0).unwrap();
    let intent = OperationIntent {
        id: "op_1".into(),
        actor_id: "agent_1".into(),
        conversation_id: "chat_1".into(),
        context_id: None,
        connection_id: "local".into(),
        capability_id: "build".into(),
        resource_id: "project_1".into(),
        action: Action::Write,
        expected_version: None,
        intent_digest: "a".repeat(64),
    };
    store
        .apply(1, OperationChange::Propose(intent.clone()))
        .unwrap();
    store
        .apply(
            2,
            OperationChange::Approve {
                id: "op_1".into(),
                exact_intent: intent,
                authority_revision: 1,
            },
        )
        .unwrap();
    store
        .begin(
            3,
            "op_1",
            "attempt_1",
            ExecutionChecks {
                authority_revision: 1,
                connection_enabled: true,
                authenticated: true,
                platform_available: true,
                resource_allowed: true,
            },
        )
        .unwrap();
    store
        .apply(
            4,
            OperationChange::Finish {
                id: "op_1".into(),
                attempt_id: "attempt_1".into(),
                outcome: Outcome::Succeeded {
                    evidence_ref: "receipt_1".into(),
                },
            },
        )
        .unwrap();
    OperationLedger::from_view(
        OperationStore::open(&root.0)
            .unwrap()
            .load()
            .unwrap()
            .ledger,
    )
    .unwrap()
}

#[test]
fn cold_recovery_commits_blocked_state_and_is_idempotent() {
    let root = Root::new();
    let store = TaskStore::open(&root.0).unwrap();
    store
        .apply(0, TaskChange::Insert(task()), &OperationLedger::default())
        .unwrap();
    let reopened = TaskStore::open(&root.0).unwrap();
    assert_eq!(
        reopened.load().unwrap().tasks[0].status,
        TaskStatus::Running
    );
    let result = reopened
        .recover_and_commit(1, &OperationLedger::default())
        .unwrap();
    assert_eq!(result.revision, 2);
    assert_eq!(result.tasks[0].status, TaskStatus::Blocked);
    assert_eq!(
        TaskStore::open(&root.0).unwrap().load().unwrap().tasks,
        result.tasks
    );
    assert_eq!(
        reopened
            .recover_and_commit(2, &OperationLedger::default())
            .unwrap()
            .revision,
        2
    );
}

#[test]
fn stale_revision_cannot_overwrite_task_or_recovery() {
    let root = Root::new();
    let store = TaskStore::open(&root.0).unwrap();
    store
        .apply(0, TaskChange::Insert(task()), &OperationLedger::default())
        .unwrap();
    let before = root.bytes();
    assert_eq!(
        store
            .apply(
                0,
                TaskChange::RecordEvidence {
                    task_id: "task_1".into(),
                    evidence_ref: "wrong".into()
                },
                &OperationLedger::default()
            )
            .unwrap_err(),
        TaskStoreError::Storage(OrganizationError::Conflict)
    );
    assert_eq!(
        store
            .recover_and_commit(0, &OperationLedger::default())
            .unwrap_err(),
        TaskStoreError::Storage(OrganizationError::Conflict)
    );
    assert_eq!(root.bytes(), before);
}

#[test]
fn malformed_or_future_snapshot_fails_closed_without_reset() {
    let root = Root::new();
    let store = TaskStore::open(&root.0).unwrap();
    store
        .apply(0, TaskChange::Insert(task()), &OperationLedger::default())
        .unwrap();
    for bad in [
        b"{".as_slice(),
        br#"{"schema_version":2,"revision":1,"tasks":[],"links":[]}"#.as_slice(),
    ] {
        root.write(bad);
        let error = TaskStore::open(&root.0).unwrap().load().unwrap_err();
        assert!(matches!(
            error,
            TaskStoreError::InvalidSnapshot | TaskStoreError::UnsupportedVersion
        ));
        assert_eq!(root.bytes(), bad);
    }
}

#[test]
fn cross_store_failure_keeps_operation_receipt_recoverable_without_false_completion() {
    let root = Root::new();
    let store = TaskStore::open(&root.0).unwrap();
    let completed_operation = persisted_ledger(&root);
    store
        .replace(
            0,
            vec![task()],
            vec![TaskOperationLink {
                task_id: "task_1".into(),
                operation_id: "op_1".into(),
                agent_id: "agent_1".into(),
                assignment_epoch: Some(0),
            }],
            &completed_operation,
        )
        .unwrap();
    let before = root.bytes();
    fs::write(root.0.join("agent-tasks.tmp"), b"interrupted write").unwrap();
    assert!(store.recover_and_commit(1, &completed_operation).is_err());
    assert_eq!(root.bytes(), before);
    fs::remove_file(root.0.join("agent-tasks.tmp")).unwrap();
    let recovered = TaskStore::open(&root.0)
        .unwrap()
        .recover_and_commit(1, &completed_operation)
        .unwrap();
    assert_eq!(recovered.tasks[0].status, TaskStatus::Blocked);
    assert_eq!(recovered.tasks[0].evidence_refs, vec!["receipt_1"]);
    assert_eq!(recovered.links.len(), 1);
    assert_eq!(
        TaskStore::open(&root.0).unwrap().load().unwrap().tasks,
        recovered.tasks
    );
}

#[test]
fn reassigned_task_reopens_with_historical_link_without_old_receipt_projection() {
    let root = Root::new();
    let store = TaskStore::open(&root.0).unwrap();
    let ledger = persisted_ledger(&root);
    let linked = store
        .replace(
            0,
            vec![task()],
            vec![TaskOperationLink {
                task_id: "task_1".into(),
                operation_id: "op_1".into(),
                agent_id: "agent_1".into(),
                assignment_epoch: Some(0),
            }],
            &ledger,
        )
        .unwrap();
    let blocked = store
        .apply(
            linked.revision,
            TaskChange::Transition {
                task_id: "task_1".into(),
                status: TaskStatus::Blocked,
                evidence: None,
            },
            &ledger,
        )
        .unwrap();
    let reassigned = store
        .apply(
            blocked.revision,
            TaskChange::Assign {
                task_id: "task_1".into(),
                agent_id: "agent_2".into(),
            },
            &ledger,
        )
        .unwrap();
    let returned = store
        .apply(
            reassigned.revision,
            TaskChange::Assign {
                task_id: "task_1".into(),
                agent_id: "agent_1".into(),
            },
            &ledger,
        )
        .unwrap();
    let reopened = TaskStore::open(&root.0).unwrap();
    let recovered = reopened
        .recover_and_commit(returned.revision, &ledger)
        .unwrap();
    assert_eq!(recovered.revision, returned.revision);
    assert_eq!(
        recovered.tasks[0].assigned_agent.as_deref(),
        Some("agent_1")
    );
    assert_eq!(recovered.tasks[0].assignment_epoch, 2);
    assert_eq!(recovered.tasks[0].status, TaskStatus::Blocked);
    assert!(recovered.tasks[0].evidence_refs.is_empty());
    assert_eq!(recovered.links, returned.links);
    assert_eq!(reopened.load().unwrap(), recovered);
}

#[test]
fn legacy_link_without_epoch_is_retained_but_never_replayed() {
    let root = Root::new();
    let store = TaskStore::open(&root.0).unwrap();
    let ledger = persisted_ledger(&root);
    store
        .replace(
            0,
            vec![task()],
            vec![TaskOperationLink {
                task_id: "task_1".into(),
                operation_id: "op_1".into(),
                agent_id: "agent_1".into(),
                assignment_epoch: Some(0),
            }],
            &ledger,
        )
        .unwrap();
    let mut legacy: serde_json::Value = serde_json::from_slice(&root.bytes()).unwrap();
    legacy["tasks"][0]
        .as_object_mut()
        .unwrap()
        .remove("assignment_epoch");
    legacy["links"][0]
        .as_object_mut()
        .unwrap()
        .remove("assignment_epoch");
    root.write(&serde_json::to_vec(&legacy).unwrap());

    let reopened = TaskStore::open(&root.0).unwrap();
    let recovered = reopened.recover_and_commit(1, &ledger).unwrap();
    assert_eq!(recovered.tasks[0].status, TaskStatus::Blocked);
    assert!(recovered.tasks[0].evidence_refs.is_empty());
    assert_eq!(recovered.links[0].assignment_epoch, None);
    assert_eq!(reopened.load().unwrap(), recovered);
}
