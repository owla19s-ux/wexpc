//! Trusted Host-side pre-dispatch gate for task-bound operations.
//! No Connector is invoked here. A committed dispatch must be consumed once.
//! Ordinary conversation operations remain supported by OperationStore directly.
use crate::operations::{CommittedDispatch, OperationStore, OperationStoreError};
use crate::tasks::{TaskStore, TaskStoreError};
use wex_core::operation::{ExecutionChecks, OperationLedger, OperationState};
use wex_core::task::TaskStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentDispatchError {
    Task(TaskStoreError),
    Operation(OperationStoreError),
    MissingLink,
    TaskNotRunning,
    OperationNotApproved,
    InvalidLedger,
}

impl From<TaskStoreError> for AgentDispatchError {
    fn from(error: TaskStoreError) -> Self {
        Self::Task(error)
    }
}

impl From<OperationStoreError> for AgentDispatchError {
    fn from(error: OperationStoreError) -> Self {
        Self::Operation(error)
    }
}

/// Only the trusted Host can call this entry point. The caller must serialize
/// task mutations and dispatch for the same task; revision checks alone do not
/// create a cross-store transaction. The task snapshot is read again before
/// committing InFlight, and any uncertainty must stop external I/O.
pub fn begin_linked_operation(
    tasks: &TaskStore,
    operations: &OperationStore,
    expected_task_revision: u64,
    expected_operation_revision: u64,
    operation_id: &str,
    attempt_id: &str,
    checks: ExecutionChecks,
) -> Result<CommittedDispatch, AgentDispatchError> {
    let operation_view = operations.load()?;
    if operation_view.revision != expected_operation_revision {
        return Err(AgentDispatchError::Operation(OperationStoreError::Storage(
            crate::OrganizationError::Conflict,
        )));
    }
    let ledger = OperationLedger::from_trusted_view(operation_view.ledger)
        .map_err(|_| AgentDispatchError::InvalidLedger)?;
    let task_view = tasks.load()?;
    if task_view.revision != expected_task_revision {
        return Err(AgentDispatchError::Task(TaskStoreError::Storage(
            crate::OrganizationError::Conflict,
        )));
    }
    TaskStore::validate_links(&task_view, &ledger)?;
    let link = task_view
        .links
        .iter()
        .find(|link| link.operation_id == operation_id)
        .ok_or(AgentDispatchError::MissingLink)?;
    let task = task_view
        .tasks
        .iter()
        .find(|task| task.id == link.task_id)
        .ok_or(AgentDispatchError::MissingLink)?;
    if task.status != TaskStatus::Running || task.assigned_agent.as_deref() != Some(&link.agent_id) {
        return Err(AgentDispatchError::TaskNotRunning);
    }
    let operation = ledger
        .view()
        .records
        .into_iter()
        .find(|record| record.intent.id == operation_id)
        .ok_or(AgentDispatchError::MissingLink)?;
    if !matches!(operation.state, OperationState::Approved { .. }) {
        return Err(AgentDispatchError::OperationNotApproved);
    }
    // The only point at which an external dispatch becomes eligible.
    // OperationStore::begin persists InFlight before returning.
    operations
        .begin(expected_operation_revision, operation_id, attempt_id, checks)
        .map_err(Into::into)
}
