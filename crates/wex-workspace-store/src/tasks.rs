//! Private, revision-checked task snapshot storage. No dispatch authority.
//! Separate operation persistence is NOT an atomic cross-store transaction.
use super::{OrganizationError, OrganizationStore, SNAPSHOT_LIMIT};
use serde::{Deserialize, Serialize};
use std::path::Path;
use wex_core::task::{Task, TaskBoard, TaskError, TaskStatus};
use wex_core::task_operation::{LinkError, TaskOperationIndex, TaskOperationLink};
use wex_core::operation::OperationLedger;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStoreError {
    Storage(OrganizationError),
    Domain(TaskError),
    Link(LinkError),
    InvalidSnapshot,
    UnsupportedVersion,
}
impl From<OrganizationError> for TaskStoreError {
    fn from(e: OrganizationError) -> Self { Self::Storage(e) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStoreView {
    pub revision: u64,
    pub tasks: Vec<Task>,
    pub links: Vec<TaskOperationLink>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status { Queued, Running, Blocked, Completed, Failed }

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
            id: t.id, project_id: t.project_id, objective: t.objective,
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
            id: t.id, project_id: t.project_id, objective: t.objective,
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
        Self { task_id: l.task_id, operation_id: l.operation_id, agent_id: l.agent_id }
    }
}
impl From<LinkWire> for TaskOperationLink {
    fn from(l: LinkWire) -> Self {
        Self { task_id: l.task_id, operation_id: l.operation_id, agent_id: l.agent_id }
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
struct Version { schema_version: u64 }

#[derive(Debug)]
pub struct TaskStore { io: OrganizationStore }
impl TaskStore {
    /// Root must be chosen by a trusted Host, never supplied by an AI.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, TaskStoreError> {
        Ok(Self { io: OrganizationStore::open_named(root, "agent-tasks")? })
    }

    fn read_locked(&self) -> Result<TaskStoreView, TaskStoreError> {
        let Some(bytes) = self.io.read_bytes_locked()? else {
            return Ok(TaskStoreView { revision: 0, tasks: vec![], links: vec![] });
        };
        let version: Version = serde_json::from_slice(&bytes)
            .map_err(|_| TaskStoreError::InvalidSnapshot)?;
        if version.schema_version != 1 { return Err(TaskStoreError::UnsupportedVersion); }
        let snapshot: Snapshot = serde_json::from_slice(&bytes)
            .map_err(|_| TaskStoreError::InvalidSnapshot)?;
        let tasks: Vec<Task> = snapshot.tasks.into_iter().map(Into::into).collect();
        // Validate all identities and task state, but preserve live state until
        // the Host explicitly requests cold recovery.
        let board = TaskBoard::restore(tasks.clone()).map_err(|_| TaskStoreError::InvalidSnapshot)?;
        let _ = board;
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
        let next = current.revision.checked_add(1)
            .ok_or(OrganizationError::RevisionExhausted)?;
        let view = TaskStoreView { revision: next, tasks, links };
        Self::validate_links(&view, ledger)?;
        let bytes = serde_json::to_vec(&Snapshot {
            schema_version: 1,
            revision: next,
            tasks: view.tasks.clone().into_iter().map(Into::into).collect(),
            links: view.links.clone().into_iter().map(Into::into).collect(),
        }).map_err(|_| OrganizationError::Unavailable)?;
        if bytes.len() as u64 > SNAPSHOT_LIMIT {
            return Err(OrganizationError::LimitExceeded.into());
        }
        if let Err(error) = self.io.save_bytes_locked(&bytes) {
            lock.release = false;
            return Err(error.into());
        }
        Ok(view)
    }

    /// Cold recovery must not silently resume a previously running task.
    pub fn recover(view: TaskStoreView) -> Result<TaskStoreView, TaskStoreError> {
        let board = TaskBoard::restore(view.tasks).map_err(TaskStoreError::Domain)?;
        Ok(TaskStoreView { revision: view.revision, tasks: board.snapshot(), links: view.links })
    }
}
