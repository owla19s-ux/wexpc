use super::task_gate::{TaskError, TaskGate, TaskPhase};
use super::ReservationError;

#[test]
fn task_lifecycle_releases_scope_on_completion() {
    let gate = TaskGate::default();
    let a = gate.start("task-a", &["C:/repo/src".into()]).unwrap();
    assert_eq!(gate.is_running("task-a"), Ok(true));
    assert!(matches!(gate.start("task-b", &["C:/repo/src/main.rs".into()]), Err(TaskError::Scope(ReservationError::Conflict))));
    a.finish(TaskPhase::Completed).unwrap();
    assert_eq!(gate.is_running("task-a"), Ok(false));
    let _b = gate.start("task-b", &["C:/repo/src/main.rs".into()]).unwrap();
}

#[test]
fn task_lifecycle_releases_on_unwind_or_cancellation() {
    let gate = TaskGate::default();
    {
        let _abandoned = gate.start("task-a", &["C:/repo".into()]).unwrap();
        assert_eq!(gate.is_running("task-a"), Ok(true));
    }
    assert_eq!(gate.is_running("task-a"), Ok(false));
    let b = gate.start("task-b", &["C:/repo".into()]).unwrap();
    b.finish(TaskPhase::Cancelled).unwrap();
    assert_eq!(gate.is_running("task-b"), Ok(false));
}

#[test]
fn task_lifecycle_rejects_duplicate_task_and_allows_disjoint_agents() {
    let gate = TaskGate::default();
    let a = gate.start("same-id", &["C:/repo/src".into()]).unwrap();
    assert!(matches!(gate.start("same-id", &["D:/other".into()]), Err(TaskError::AlreadyRunning)));
    let b = gate.start("other-id", &["C:/repo/tests".into()]).unwrap();
    assert_eq!(gate.is_running("other-id"), Ok(true));
    a.finish(TaskPhase::Failed).unwrap();
    assert_eq!(gate.is_running("other-id"), Ok(true));
    drop(b);
    assert_eq!(gate.is_running("other-id"), Ok(false));
}

#[test]
fn failed_admission_does_not_leave_ghost_task() {
    let gate = TaskGate::default();
    let held = gate.start("one", &["C:/repo".into()]).unwrap();
    assert!(matches!(gate.start("two", &["C:/repo/sub".into()]), Err(TaskError::Scope(ReservationError::Conflict))));
    assert_eq!(gate.is_running("two"), Ok(false));
    assert!(matches!(gate.start("two", &["relative".into()]), Err(TaskError::Scope(ReservationError::Invalid(_)))));
    drop(held);
    assert!(gate.start("two", &["C:/repo/sub".into()]).is_ok());
}

#[test]
fn scoped_execution_holds_reservation_through_callback() {
    let gate = TaskGate::default();
    let observer = gate.clone();
    let result = gate.execute("job", &["C:/repo/src".into()], || {
        assert_eq!(observer.is_running("job"), Ok(true));
        assert!(matches!(observer.start("conflict", &["C:/repo/src/a.rs".into()]), Err(TaskError::Scope(ReservationError::Conflict))));
        Ok::<_, &'static str>(42)
    });
    assert_eq!(result, Ok(Ok(42)));
    assert_eq!(gate.is_running("job"), Ok(false));
}

#[test]
fn scoped_execution_returns_tool_error_and_releases_lease() {
    let gate = TaskGate::default();
    let result = gate.execute("job", &["C:/repo/src".into()], || Err::<(), _>("tool failed"));
    assert_eq!(result, Ok(Err("tool failed")));
    assert_eq!(gate.is_running("job"), Ok(false));
    assert!(gate.start("retry", &["C:/repo/src".into()]).is_ok());
}
