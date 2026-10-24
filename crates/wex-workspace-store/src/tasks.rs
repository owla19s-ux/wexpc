//! Private, revision-checked task snapshot storage. No dispatch authority.
//! Separate operation persistence is NOT an atomic cross-store transaction.
use super::{OrganizationError, OrganizationStore, SNAPSHOT_LIMIT};
use serde::{Deserialize, Serialize};
use std::path::Path;
use wex_core::operation::OperationLedger;
use wex_core::task::{Task, TaskBoard, TaskError, TaskStatus};
use wex_core::task_operation::{LinkError, TaskOperationIndex, TaskOperationLink};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStoreError {
    Storage(OrganizationError),
    Domain(TaskError),
    Link(LinkError),
    InvalidSnapshot,
    UnsupportedVersion,
}
impl From<OrganizationError> for TaskStoreError {
    fn from(e: OrganizationError) -> Self {
        Self::Storage(e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStoreView {
    pub revision: u64,
    pub tasks: Vec<Task>,
    pub links: Vec<TaskOperationLink>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Queued,
    Running,
    Blocked,
    Completed,
    Failed,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskWire {
    id: String,
    project_id: String,
    objective: String,
    assigned_agent: Option<String>,
    status: Status,
    evidence_refs: Vec<String>,
}
impl From<Task> for TaskWire {
    fn from(t: Task) -> Self {
        Self {
            id: t.id,
            project_id: t.project_id,
            objective: t.objective,
            assigned_agent: t.assigned_agent,
            status: match t.status {
                TaskStatus::Queued => Status::Queued,
                TaskStatus::Running => Status::Running,
                TaskStatus::Blocked => Status::Blocked,
                TaskStatus::Completed => Status::Completed,
                TaskStatus::Failed => Status::Failed,
            },
            evidence_refs: t.evidence_refs,
        }
    }
}
impl From<TaskWire> for Task {
    fn from(t: TaskWire) -> Self {
        Self {
            id: t.id,
            project_id: t.project_id,
            objective: t.objective,
            assigned_agent: t.assigned_agent,
            status: match t.status {
                Status::Queued => TaskStatus::Queued,
                Status::Running => TaskStatus::Running,
                Status::Blocked => TaskStatus::Blocked,
                Status::Completed => TaskStatus::Completed,
                Status::Failed => TaskStatus::Failed,
            },
            evidence_refs: t.evidence_refs,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkWire {
    task_id: String,
    operation_id: String,
    agent_id: String,
}
impl From<TaskOperationLink> for LinkWire {
    fn from(l: TaskOperationLink) -> Self {
        Self {
            task_id: l.task_id,
            operation_id: l.operation_id,
            agent_id: l.agent_id,
        }
    }
}
impl From<LinkWire> for TaskOperationLink {
    fn from(l: LinkWire) -> Self {
        Self {
            task_id: l.task_id,
            operation_id: l.operation_id,
            agent_id: l.agent_id,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: u32,
    revision: u64,
    tasks: Vec<TaskWire>,
    links: Vec<LinkWire>,
}
#[derive(Deserialize)]
struct Version {
    schema_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskChange {
    Insert(Task),
    Assign {
        task_id: String,
        agent_id: String,
    },
    Transition {
        task_id: String,
        status: TaskStatus,
        evidence: Option<String>,
    },
    RecordEvidence {
        task_id: String,
        evidence_ref: String,
    },
}

fn apply_task_change(board: &mut TaskBoard, change: TaskChange) -> Result<(), TaskError> {
    match change {
        TaskChange::Insert(task) => board.insert(task),
        TaskChange::Assign { task_id, agent_id } => board.assign(&task_id, agent_id),
        TaskChange::Transition {
            task_id,
            status,
            evidence,
        } => board.transition(&task_id, status, evidence),
        TaskChange::RecordEvidence {
            task_id,
            evidence_ref,
        } => board.record_evidence(&task_id, evidence_ref),
    }
}

#[derive(Debug)]
pub struct TaskStore {
    io: OrganizationStore,
}
impl TaskStore {
    /// Root must be chosen by a trusted Host, never supplied by an AI.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, TaskStoreError> {
        Ok(Self {
            io: OrganizationStore::open_named(root, "agent-tasks")?,
        })
    }

    fn read_locked(&self) -> Result<TaskStoreView, TaskStoreError> {
        let Some(bytes) = self.io.read_bytes_locked()? else {
            return Ok(TaskStoreView {
                revision: 0,
                tasks: vec![],
                links: vec![],
            });
        };
        let version: Version =
            serde_json::from_slice(&bytes).map_err(|_| TaskStoreError::InvalidSnapshot)?;
        if version.schema_version != 1 {
            return Err(TaskStoreError::UnsupportedVersion);
        }
        let snapshot: Snapshot =
            serde_json::from_slice(&bytes).map_err(|_| TaskStoreError::InvalidSnapshot)?;
        if snapshot.revision == u64::MAX {
            return Err(TaskStoreError::InvalidSnapshot);
        }
        let tasks: Vec<Task> = snapshot.tasks.into_iter().map(Into::into).collect();
        // Validate all identities and task state, but preserve live state until
        // the Host explicitly requests cold recovery.
        TaskBoard::restore(tasks.clone()).map_err(|_| TaskStoreError::InvalidSnapshot)?;
        if snapshot.links.len() > 4096 || tasks.len() > 4096 {
            return Err(TaskStoreError::InvalidSnapshot);
        }
        Ok(TaskStoreView {
            revision: snapshot.revision,
            tasks,
            links: snapshot.links.into_iter().map(Into::into).collect(),
        })
    }

    pub fn load(&self) -> Result<TaskStoreView, TaskStoreError> {
        let _lock = self.io.lock()?;
        self.read_locked()
    }

    /// Validate link ownership against the authoritative operation ledger.
    /// The caller must ensure the ledger is a trusted consistent snapshot.
    pub fn validate_links(
        view: &TaskStoreView,
        ledger: &OperationLedger,
    ) -> Result<(), TaskStoreError> {
        let board = TaskBoard::restore(view.tasks.clone()).map_err(TaskStoreError::Domain)?;
        TaskOperationIndex::restore(view.links.clone(), &board, ledger)
            .map_err(TaskStoreError::Link)?;
        Ok(())
    }

    /// Reconcile a cold task snapshot against a trusted ledger and durably
    /// commit the resulting projection with optimistic revision checking.
    /// This does not make the separate operation store atomic with this store.
    pub fn recover_and_commit(
        &self,
        expected_revision: u64,
        ledger: &OperationLedger,
    ) -> Result<TaskStoreView, TaskStoreError> {
        let mut lock = self.io.lock()?;
        let current = self.read_locked()?;
        if current.revision != expected_revision {
            return Err(OrganizationError::Conflict.into());
        }
        let recovered = Self::recover_with_ledger(current.clone(), ledger)?;
        if recovered.tasks == current.tasks && recovered.links == current.links {
            return Ok(current);
        }
        let next = current
            .revision
            .checked_add(1)
            .ok_or(OrganizationError::RevisionExhausted)?;
        let view = TaskStoreView {
            revision: next,
            ..recovered
        };
        let bytes = serde_json::to_vec(&Snapshot {
            schema_version: 1,
            revision: next,
            tasks: view.tasks.clone().into_iter().map(Into::into).collect(),
            links: view.links.clone().into_iter().map(Into::into).collect(),
        })
        .map_err(|_| OrganizationError::Unavailable)?;
        if bytes.len() as u64 > SNAPSHOT_LIMIT {
            return Err(OrganizationError::LimitExceeded.into());
        }
        if let Err(error) = self.io.save_bytes_locked(&bytes) {
            lock.release = false;
            return Err(error.into());
        }
        Ok(view)
    }

    /// Apply one trusted task mutation against a revisioned snapshot.
    /// The ledger is used only to validate existing links; no tool dispatch occurs.
    pub fn apply(
        &self,
        expected_revision: u64,
        change: TaskChange,
        ledger: &OperationLedger,
    ) -> Result<TaskStoreView, TaskStoreError> {
        let mut lock = self.io.lock()?;
        let current = self.read_locked()?;
        if current.revision != expected_revision {
            return Err(OrganizationError::Conflict.into());
        }
        // Trusted live hydration: retain Running tasks during normal mutations.
        let mut live = TaskBoard::default();
        for task in current.tasks {
            live.insert(task).map_err(TaskStoreError::Domain)?;
        }
        apply_task_change(&mut live, change).map_err(TaskStoreError::Domain)?;
        let next = expected_revision
            .checked_add(1)
            .ok_or(OrganizationError::RevisionExhausted)?;
        let view = TaskStoreView {
            revision: next,
            tasks: live.snapshot(),
            links: current.links,
        };
        Self::validate_links(&view, ledger)?;
        let bytes = serde_json::to_vec(&Snapshot {
            schema_version: 1,
            revision: next,
            tasks: view.tasks.clone().into_iter().map(Into::into).collect(),
            links: view.links.clone().into_iter().map(Into::into).collect(),
        })
        .map_err(|_| OrganizationError::Unavailable)?;
        if bytes.len() as u64 > SNAPSHOT_LIMIT {
            return Err(OrganizationError::LimitExceeded.into());
        }
        if let Err(error) = self.io.save_bytes_locked(&bytes) {
            lock.release = false;
            return Err(error.into());
        }
        Ok(view)
    }

    /// Persist the task-to-operation relationship before allowing dispatch.
    /// The Host must first durably propose the operation, then call this method,
    /// and only after success call OperationStore::begin. A failed/uncertain
    /// link write must never be followed by external dispatch.
    ///
    /// This is not a cross-store transaction: a crash after the operation
    /// proposal but before this link commit leaves an unlinked, unstarted
    /// operation requiring explicit reconciliation.
    pub fn attach_operation(
        &self,
        expected_revision: u64,
        link: TaskOperationLink,
        ledger: &OperationLedger,
    ) -> Result<TaskStoreView, TaskStoreError> {
        let mut lock = self.io.lock()?;
        let current = self.read_locked()?;
        if current.revision != expected_revision {
            return Err(OrganizationError::Conflict.into());
        }
        let mut board = TaskBoard::default();
        for task in &current.tasks {
            board.insert(task.clone()).map_err(TaskStoreError::Domain)?;
        }
        let task = board.get(&link.task_id).ok_or(TaskStoreError::Link(LinkError::MissingTask))?;
        if matches!(task.status, TaskStatus::Completed | TaskStatus::Failed) {
            return Err(TaskStoreError::Domain(TaskError::InvalidTransition));
        }
        let mut index = TaskOperationIndex::restore(current.links, &board, ledger)
            .map_err(TaskStoreError::Link)?;
        index.attach(link, &board, ledger).map_err(TaskStoreError::Link)?;
        let revision = expected_revision
            .checked_add(1)
            .ok_or(OrganizationError::RevisionExhausted)?;
        let view = TaskStoreView {
            revision,
            tasks: board.snapshot(),
            links: index.snapshot(),
        };
        let bytes = serde_json::to_vec(&Snapshot {
            schema_version: 1,
            revision,
            tasks: view.tasks.clone().into_iter().map(Into::into).collect(),
            links: view.links.clone().into_iter().map(Into::into).collect(),
        })
        .map_err(|_| OrganizationError::Unavailable)?;
        if bytes.len() as u64 > SNAPSHOT_LIMIT {
            return Err(OrganizationError::LimitExceeded.into());
        }
        if let Err(error) = self.io.save_bytes_locked(&bytes) {
            lock.release = false;
            return Err(error.into());
        }
        Ok(view)
    }

    /// Compare-and-swap a complete task+link snapshot. This is atomic only
    /// within the task store, NOT with the separate operation store.
    pub fn replace(
        &self,
        expected_revision: u64,
        tasks: Vec<Task>,
        links: Vec<TaskOperationLink>,
        ledger: &OperationLedger,
    ) -> Result<TaskStoreView, TaskStoreError> {
        let mut lock = self.io.lock()?;
        let current = self.read_locked()?;
        if current.revision != expected_revision {
            return Err(OrganizationError::Conflict.into());
        }
        let next = current
            .revision
            .checked_add(1)
            .ok_or(OrganizationError::RevisionExhausted)?;
        let view = TaskStoreView {
            revision: next,
            tasks,
            links,
        };
        Self::validate_links(&view, ledger)?;
        let bytes = serde_json::to_vec(&Snapshot {
            schema_version: 1,
            revision: next,
            tasks: view.tasks.clone().into_iter().map(Into::into).collect(),
            links: view.links.clone().into_iter().map(Into::into).collect(),
        })
        .map_err(|_| OrganizationError::Unavailable)?;
        if bytes.len() as u64 > SNAPSHOT_LIMIT {
            return Err(OrganizationError::LimitExceeded.into());
        }
        if let Err(error) = self.io.save_bytes_locked(&bytes) {
            lock.release = false;
            return Err(error.into());
        }
        Ok(view)
    }

    /// Reconcile trusted operation receipts after cold recovery. A completed
    /// operation only adds evidence; it never completes the enclosing task.
    /// The Host must persist the returned projection before exposing it.
    pub fn recover_with_ledger(
        view: TaskStoreView,
        ledger: &OperationLedger,
    ) -> Result<TaskStoreView, TaskStoreError> {
        let recovered = Self::recover(view)?;
        let mut board = TaskBoard::restore(recovered.tasks).map_err(TaskStoreError::Domain)?;
        let index = TaskOperationIndex::restore(recovered.links.clone(), &board, ledger)
            .map_err(TaskStoreError::Link)?;
        for link in index.snapshot() {
            index
                .reconcile(&link.operation_id, &mut board, ledger)
                .map_err(TaskStoreError::Link)?;
        }
        Ok(TaskStoreView {
            revision: recovered.revision,
            tasks: board.snapshot(),
            links: recovered.links,
        })
    }

    /// Cold recovery must not silently resume a previously running task.
    pub fn recover(view: TaskStoreView) -> Result<TaskStoreView, TaskStoreError> {
        let board = TaskBoard::restore(view.tasks).map_err(TaskStoreError::Domain)?;
        Ok(TaskStoreView {
            revision: view.revision,
            tasks: board.snapshot(),
            links: view.links,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(status: TaskStatus) -> Task {
        Task {
            id: "task_a".into(),
            project_id: "project_a".into(),
            objective: "Build and test".into(),
            assigned_agent: Some("agent_a".into()),
            status,
            evidence_refs: vec!["log_a".into()],
        }
    }

    #[test]
    fn cold_recovery_blocks_interrupted_work_and_preserves_evidence() {
        let view = TaskStoreView {
            revision: 3,
            tasks: vec![task(TaskStatus::Running)],
            links: vec![],
        };
        let recovered = TaskStore::recover(view).unwrap();
        assert_eq!(recovered.revision, 3);
        assert_eq!(recovered.tasks[0].status, TaskStatus::Blocked);
        assert_eq!(recovered.tasks[0].evidence_refs, vec!["log_a"]);
    }

    #[test]
    fn completed_task_remains_completed_on_recovery() {
        let view = TaskStoreView {
            revision: 4,
            tasks: vec![task(TaskStatus::Completed)],
            links: vec![],
        };
        assert_eq!(
            TaskStore::recover(view).unwrap().tasks[0].status,
            TaskStatus::Completed
        );
    }

    #[test]
    fn cold_recovery_with_empty_ledger_preserves_unlinked_tasks() {
        let view = TaskStoreView {
            revision: 7,
            tasks: vec![task(TaskStatus::Running)],
            links: vec![],
        };
        let result = TaskStore::recover_with_ledger(view, &OperationLedger::default()).unwrap();
        assert_eq!(result.tasks[0].status, TaskStatus::Blocked);
        assert_eq!(result.revision, 7);
    }

    #[test]
    fn trusted_task_changes_preserve_other_running_tasks() {
        let mut board = TaskBoard::default();
        board.insert(task(TaskStatus::Running)).unwrap();
        apply_task_change(
            &mut board,
            TaskChange::RecordEvidence {
                task_id: "task_a".into(),
                evidence_ref: "build_2".into(),
            },
        )
        .unwrap();
        assert_eq!(board.get("task_a").unwrap().status, TaskStatus::Running);
        assert_eq!(
            board.get("task_a").unwrap().evidence_refs,
            vec!["log_a", "build_2"]
        );
    }

    #[test]
    fn invalid_task_change_does_not_mutate_board() {
        let mut board = TaskBoard::default();
        board.insert(task(TaskStatus::Running)).unwrap();
        let before = board.snapshot();
        assert_eq!(
            apply_task_change(
                &mut board,
                TaskChange::Assign {
                    task_id: "task_a".into(),
                    agent_id: "agent_b".into(),
                }
            ),
            Err(TaskError::InvalidTransition)
        );
        assert_eq!(board.snapshot(), before);
    }

    #[test]
    fn snapshot_wire_roundtrip_preserves_status_and_fields() {
        let task = task(TaskStatus::Failed);
        let encoded = serde_json::to_vec(&TaskWire::from(task.clone())).unwrap();
        let decoded: TaskWire = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(Task::from(decoded), task);
    }
}
