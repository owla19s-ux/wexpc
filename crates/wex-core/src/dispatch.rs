//! Pure dispatch admission rules. This module never invokes tools or grants authority.
//! The Host must separately validate current permissions, credentials and resource locks.
use crate::task::{TaskBoard, TaskStatus};
use crate::dispatch_leases::{ResourceLease, ResourceLeases, LeaseError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchRefusal {
    MissingTask,
    WrongProject,
    NotReady,
    Unassigned,
    WrongAgent,
    StaleAssignment,
    ResourceBusy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchTicket {
    pub task_id: String,
    pub project_id: String,
    pub agent_id: String,
    pub assignment_epoch: u64,
}

/// Checks that a task is eligible to *request* execution. It is not an approval.
/// A caller must acquire a resource lease and recheck this ticket against the
/// authoritative task snapshot immediately before starting an operation.
pub fn admit(
    board: &TaskBoard,
    task_id: &str,
    project_id: &str,
    agent_id: &str,
    expected_epoch: u64,
    resources_available: bool,
) -> Result<DispatchTicket, DispatchRefusal> {
    let task = board.get(task_id).ok_or(DispatchRefusal::MissingTask)?;
    if task.project_id != project_id {
        return Err(DispatchRefusal::WrongProject);
    }
    if !matches!(task.status, TaskStatus::Queued | TaskStatus::Blocked) {
        return Err(DispatchRefusal::NotReady);
    }
    let assigned = task.assigned_agent.as_deref().ok_or(DispatchRefusal::Unassigned)?;
    if assigned != agent_id {
        return Err(DispatchRefusal::WrongAgent);
    }
    if task.assignment_epoch != expected_epoch {
        return Err(DispatchRefusal::StaleAssignment);
    }
    if !resources_available {
        return Err(DispatchRefusal::ResourceBusy);
    }
    Ok(DispatchTicket {
        task_id: task.id.clone(),
        project_id: task.project_id.clone(),
        agent_id: assigned.to_owned(),
        assignment_epoch: task.assignment_epoch,
    })
}

/// Recheck a previously issued ticket before an operation begins. A task that
/// has already entered Running may continue only under the same assignment.
pub fn validate_ticket(board: &TaskBoard, ticket: &DispatchTicket) -> Result<(), DispatchRefusal> {
    let task = board.get(&ticket.task_id).ok_or(DispatchRefusal::MissingTask)?;
    if task.project_id != ticket.project_id {
        return Err(DispatchRefusal::WrongProject);
    }
    if task.assigned_agent.as_deref() != Some(ticket.agent_id.as_str()) {
        return Err(DispatchRefusal::WrongAgent);
    }
    if task.assignment_epoch != ticket.assignment_epoch {
        return Err(DispatchRefusal::StaleAssignment);
    }
    if !matches!(task.status, TaskStatus::Queued | TaskStatus::Blocked | TaskStatus::Running) {
        return Err(DispatchRefusal::NotReady);
    }
    Ok(())
}


#[derive(Debug)]
pub struct DispatchGuard {
    pub ticket: DispatchTicket,
    pub lease: ResourceLease,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchStartError {
    Refused(DispatchRefusal),
    Lease(LeaseError),
}

/// Reserve scopes atomically and recheck assignment under the resource lease.
/// The Host must still persist Running and recheck tool authority before I/O.
pub fn reserve(
    board: &TaskBoard,
    leases: &ResourceLeases,
    task_id: &str,
    project_id: &str,
    agent_id: &str,
    epoch: u64,
    scopes: &[String],
) -> Result<DispatchGuard, DispatchStartError> {
    let ticket = admit(board, task_id, project_id, agent_id, epoch, true)
        .map_err(DispatchStartError::Refused)?;
    let lease = leases.acquire(agent_id, scopes).map_err(DispatchStartError::Lease)?;
    validate_ticket(board, &ticket).map_err(DispatchStartError::Refused)?;
    Ok(DispatchGuard { ticket, lease })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::Task;

    fn board() -> TaskBoard {
        let mut board = TaskBoard::default();
        board.insert(Task {
            id: "task_1".into(),
            project_id: "pc".into(),
            objective: "Implement dispatcher".into(),
            assigned_agent: Some("agent_a".into()),
            assignment_epoch: 2,
            status: TaskStatus::Queued,
            evidence_refs: vec![],
        }).unwrap();
        board
    }

    #[test]
    fn admits_only_current_project_agent_and_epoch() {
        let board = board();
        let ticket = admit(&board, "task_1", "pc", "agent_a", 2, true).unwrap();
        assert_eq!(validate_ticket(&board, &ticket), Ok(()));
        assert_eq!(admit(&board, "task_1", "other", "agent_a", 2, true), Err(DispatchRefusal::WrongProject));
        assert_eq!(admit(&board, "task_1", "pc", "agent_b", 2, true), Err(DispatchRefusal::WrongAgent));
        assert_eq!(admit(&board, "task_1", "pc", "agent_a", 1, true), Err(DispatchRefusal::StaleAssignment));
        assert_eq!(admit(&board, "task_1", "pc", "agent_a", 2, false), Err(DispatchRefusal::ResourceBusy));
    }

    #[test]
    fn old_ticket_cannot_dispatch_after_reassignment() {
        let mut board = board();
        let ticket = admit(&board, "task_1", "pc", "agent_a", 2, true).unwrap();
        board.assign("task_1", "agent_b".into()).unwrap();
        assert_eq!(validate_ticket(&board, &ticket), Err(DispatchRefusal::WrongAgent));
        assert_eq!(admit(&board, "task_1", "pc", "agent_a", 2, true), Err(DispatchRefusal::WrongAgent));
    }

    #[test]
    fn completed_and_failed_tasks_are_not_dispatchable() {
        let mut board = board();
        board.transition("task_1", TaskStatus::Running, None).unwrap();
        assert_eq!(admit(&board, "task_1", "pc", "agent_a", 2, true), Err(DispatchRefusal::NotReady));
        board.transition("task_1", TaskStatus::Completed, None).unwrap();
        assert_eq!(admit(&board, "task_1", "pc", "agent_a", 2, true), Err(DispatchRefusal::NotReady));
    }

    #[test]
    fn running_ticket_can_be_revalidated_but_not_readmitted() {
        let mut board = board();
        let ticket = admit(&board, "task_1", "pc", "agent_a", 2, true).unwrap();
        board.transition("task_1", TaskStatus::Running, None).unwrap();
        assert_eq!(validate_ticket(&board, &ticket), Ok(()));
        assert_eq!(admit(&board, "task_1", "pc", "agent_a", 2, true), Err(DispatchRefusal::NotReady));
    }
    #[test]
    fn reserve_rejects_conflicts_and_releases_on_drop() {
        let board = board();
        let leases = ResourceLeases::default();
        let scopes = vec!["src/main.rs".to_owned()];
        let first = reserve(&board, &leases, "task_1", "pc", "agent_a", 2, &scopes).unwrap();
        assert!(matches!(
            reserve(&board, &leases, "task_1", "pc", "agent_a", 2, &scopes),
            Err(DispatchStartError::Lease(LeaseError::Busy))
        ));
        drop(first);
        assert!(reserve(&board, &leases, "task_1", "pc", "agent_a", 2, &scopes).is_ok());
    }

    #[test]
    fn reserve_blocks_nested_write_scope_without_blocking_siblings() {
        let board = board();
        let leases = ResourceLeases::default();
        let parent = vec!["src".to_owned()];
        let child = vec!["src/main.rs".to_owned()];
        let sibling = vec!["docs/readme.md".to_owned()];
        let first = reserve(&board, &leases, "task_1", "pc", "agent_a", 2, &parent).unwrap();
        assert!(matches!(
            reserve(&board, &leases, "task_1", "pc", "agent_a", 2, &child),
            Err(DispatchStartError::Lease(LeaseError::Busy))
        ));
        let independent = reserve(&board, &leases, "task_1", "pc", "agent_a", 2, &sibling).unwrap();
        drop(first);
        assert!(reserve(&board, &leases, "task_1", "pc", "agent_a", 2, &child).is_ok());
        drop(independent);
    }

    #[test]
    fn reserve_does_not_lock_scopes_for_rejected_assignment() {
        let board = board();
        let leases = ResourceLeases::default();
        let scopes = vec!["src/main.rs".to_owned()];
        assert!(matches!(
            reserve(&board, &leases, "task_1", "pc", "agent_b", 2, &scopes),
            Err(DispatchStartError::Refused(DispatchRefusal::WrongAgent))
        ));
        assert!(!leases.is_busy("src/main.rs").unwrap());
    }

}
