//! Trusted Host-side pre-dispatch gate for task-bound operations.
//! No Connector is invoked here. A committed dispatch must be consumed once.
//! Ordinary conversation operations remain supported by OperationStore directly.
use crate::operations::{CommittedDispatch, OperationStore, OperationStoreError};
use crate::tasks::{TaskStore, TaskStoreError, TaskStoreView};
use wex_core::operation::{ExecutionChecks, OperationLedger, OperationState, Outcome};
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

/// Pure preflight used by the trusted Host before committing InFlight.
/// This does not replace a cross-store lock or the final authority checks.
pub fn validate_linked_dispatch(
    task_view: &TaskStoreView,
    ledger: &OperationLedger,
    operation_id: &str,
) -> Result<(), AgentDispatchError> {
    TaskStore::validate_links(task_view, ledger)?;
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
    if task.status != TaskStatus::Running
        || task.assigned_agent.as_deref() != Some(link.agent_id.as_str())
    {
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
    Ok(())
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
    // Keep the task lock until InFlight is durably committed. Concurrent
    // task transitions through this store cannot invalidate preflight.
    // The trusted Host must use the same lock ordering for all cross-store
    // operations: task first, then operation.
    tasks.with_locked_view(|task_view| {
        if task_view.revision != expected_task_revision {
            return Err(AgentDispatchError::Task(TaskStoreError::Storage(
                crate::OrganizationError::Conflict,
            )));
        }
        let operation_view = operations.load()?;
        if operation_view.revision != expected_operation_revision {
            return Err(AgentDispatchError::Operation(OperationStoreError::Storage(
                crate::OrganizationError::Conflict,
            )));
        }
        let ledger = OperationLedger::from_trusted_view(operation_view.ledger)
            .map_err(|_| AgentDispatchError::InvalidLedger)?;
        validate_linked_dispatch(&task_view, &ledger, operation_id)?;
        // OperationStore::begin rechecks revision, approval and authority
        // and persists InFlight before releasing the dispatch.
        operations
            .begin(expected_operation_revision, operation_id, attempt_id, checks)
            .map_err(Into::into)
    })
}

/// Persist a trusted receipt first, then reconcile its linked task.
/// If the task write fails, the operation receipt remains durable and the Host
/// must retry reconciliation only (never repeat the external side effect).
/// This is a sequential two-store protocol, not an atomic transaction.
pub fn finish_linked_operation(
    tasks: &TaskStore,
    operations: &OperationStore,
    expected_operation_revision: u64,
    operation_id: &str,
    attempt_id: &str,
    outcome: Outcome,
) -> Result<TaskStoreView, AgentDispatchError> {
    let operation_view = operations.apply(
        expected_operation_revision,
        crate::operations::OperationChange::Finish {
            id: operation_id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            outcome,
        },
    )?;
    let ledger = OperationLedger::from_trusted_view(operation_view.ledger)
        .map_err(|_| AgentDispatchError::InvalidLedger)?;
    // A concurrent task edit may cause a revision conflict. That must not
    // roll back or erase the operation receipt.
    let task_view = tasks.load()?;
    tasks
        .reconcile_operation(task_view.revision, operation_id, &ledger)
        .map_err(Into::into)
}

/// Repair a task projection from an already persisted operation receipt.
/// This path does not issue another Connector call or mutate the ledger.
pub fn reconcile_linked_receipt(
    tasks: &TaskStore,
    operations: &OperationStore,
    operation_id: &str,
) -> Result<TaskStoreView, AgentDispatchError> {
    let operation_view = operations.load()?;
    let ledger = OperationLedger::from_trusted_view(operation_view.ledger)
        .map_err(|_| AgentDispatchError::InvalidLedger)?;
    let task_view = tasks.load()?;
    tasks
        .reconcile_operation(task_view.revision, operation_id, &ledger)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wex_core::operation::{Action, OperationIntent};
    use wex_core::task::Task;
    use wex_core::task_operation::TaskOperationLink;

    fn fixture(status: TaskStatus) -> (TaskStoreView, OperationLedger, OperationIntent) {
        let intent = OperationIntent {
            id: "op_1".into(),
            actor_id: "agent_1".into(),
            conversation_id: "chat_1".into(),
            context_id: None,
            connection_id: "local".into(),
            capability_id: "write".into(),
            resource_id: "file_1".into(),
            action: Action::Write,
            expected_version: None,
            intent_digest: "a".repeat(64),
        };
        let mut ledger = OperationLedger::default();
        ledger.propose(intent.clone()).unwrap();
        let view = TaskStoreView {
            revision: 1,
            tasks: vec![Task {
                id: "task_1".into(),
                project_id: "project_1".into(),
                objective: "Implement".into(),
                assigned_agent: Some("agent_1".into()),
                status,
                evidence_refs: vec![],
            }],
            links: vec![TaskOperationLink {
                task_id: "task_1".into(),
                operation_id: "op_1".into(),
                agent_id: "agent_1".into(),
            }],
        };
        (view, ledger, intent)
    }

    #[test]
    fn successful_receipt_adds_evidence_without_completing_task() {
        let (view, mut ledger, intent) = fixture(TaskStatus::Running);
        ledger.approve("op_1", &intent, 1).unwrap();
        ledger
            .begin(
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
        ledger
            .finish(
                "op_1",
                "attempt_1",
                Outcome::Succeeded {
                    evidence_ref: "receipt_1".into(),
                },
            )
            .unwrap();
        let recovered = TaskStore::recover_with_ledger(view, &ledger).unwrap();
        assert_eq!(recovered.tasks[0].status, TaskStatus::Blocked);
        assert_eq!(recovered.tasks[0].evidence_refs, vec!["receipt_1"]);
    }

    #[test]
    fn unknown_receipt_blocks_running_task_without_evidence() {
        let (view, mut ledger, intent) = fixture(TaskStatus::Running);
        ledger.approve("op_1", &intent, 1).unwrap();
        ledger
            .begin(
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
        ledger.finish("op_1", "attempt_1", Outcome::Unknown).unwrap();
        let recovered = TaskStore::recover_with_ledger(view, &ledger).unwrap();
        assert_eq!(recovered.tasks[0].status, TaskStatus::Blocked);
        assert!(recovered.tasks[0].evidence_refs.is_empty());
    }

    #[test]
    fn approved_running_task_passes_preflight() {
        let (view, mut ledger, intent) = fixture(TaskStatus::Running);
        ledger.approve("op_1", &intent, 1).unwrap();
        assert_eq!(validate_linked_dispatch(&view, &ledger, "op_1"), Ok(()));
    }

    #[test]
    fn pending_approval_never_dispatches() {
        let (view, ledger, _) = fixture(TaskStatus::Running);
        assert_eq!(
            validate_linked_dispatch(&view, &ledger, "op_1"),
            Err(AgentDispatchError::OperationNotApproved)
        );
    }

    #[test]
    fn blocked_task_never_dispatches_even_when_approved() {
        let (view, mut ledger, intent) = fixture(TaskStatus::Blocked);
        ledger.approve("op_1", &intent, 1).unwrap();
        assert_eq!(
            validate_linked_dispatch(&view, &ledger, "op_1"),
            Err(AgentDispatchError::TaskNotRunning)
        );
    }

    #[test]
    fn wrong_agent_cannot_use_another_agents_link() {
        let (mut view, mut ledger, intent) = fixture(TaskStatus::Running);
        ledger.approve("op_1", &intent, 1).unwrap();
        view.links[0].agent_id = "agent_2".into();
        assert!(matches!(
            validate_linked_dispatch(&view, &ledger, "op_1"),
            Err(AgentDispatchError::Task(TaskStoreError::Link(_)))
        ));
    }

    #[test]
    fn already_dispatched_operation_cannot_be_dispatched_again() {
        let (view, mut ledger, intent) = fixture(TaskStatus::Running);
        ledger.approve("op_1", &intent, 1).unwrap();
        ledger
            .begin(
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
        assert_eq!(
            validate_linked_dispatch(&view, &ledger, "op_1"),
            Err(AgentDispatchError::OperationNotApproved)
        );
    }

    #[test]
    fn missing_task_link_never_dispatches() {
        let (mut view, mut ledger, intent) = fixture(TaskStatus::Running);
        ledger.approve("op_1", &intent, 1).unwrap();
        view.links.clear();
        assert_eq!(
            validate_linked_dispatch(&view, &ledger, "op_1"),
            Err(AgentDispatchError::MissingLink)
        );
    }
}
