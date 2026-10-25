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


#[test]
fn concurrent_disjoint_tasks_hold_independent_leases_and_reject_overlap() {
    use std::sync::mpsc;
    use std::time::Duration;

    let gate = TaskGate::default();
    let (ready_tx, ready_rx) = mpsc::channel();
    let mut release_senders = Vec::new();
    let mut workers = Vec::new();

    for (id, path) in [
        ("agent-a-task", "C:/repo/src"),
        ("agent-b-task", "C:/repo/tests"),
    ] {
        let worker_gate = gate.clone();
        let started = ready_tx.clone();
        let (release_tx, release_rx) = mpsc::channel();
        release_senders.push(release_tx);
        workers.push(std::thread::spawn(move || {
            let lease = worker_gate.start(id, &[path.to_owned()]).unwrap();
            started.send(id).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            lease.finish(TaskPhase::Completed).unwrap();
        }));
    }
    drop(ready_tx);

    let first = ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let second = ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_ne!(first, second);
    assert_eq!(gate.is_running("agent-a-task"), Ok(true));
    assert_eq!(gate.is_running("agent-b-task"), Ok(true));
    assert!(matches!(
        gate.start("agent-c-task", &["C:/repo/src/main.rs".into()]),
        Err(TaskError::Scope(ReservationError::Conflict))
    ));

    for release in release_senders {
        release.send(()).unwrap();
    }
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(gate.is_running("agent-a-task"), Ok(false));
    assert_eq!(gate.is_running("agent-b-task"), Ok(false));
    assert!(gate.start("agent-c-task", &["C:/repo/src/main.rs".into()]).is_ok());
}

#[test]
fn unwinding_task_lease_releases_scope_without_marking_execution_success() {
    let gate = TaskGate::default();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _lease = gate.start("interrupted", &["C:/repo/src".into()]).unwrap();
        assert_eq!(gate.is_running("interrupted"), Ok(true));
        panic!("simulate an interrupted task");
    }));
    assert!(panicked.is_err());
    assert_eq!(gate.is_running("interrupted"), Ok(false));
    assert!(gate.start("next", &["C:/repo/src".into()]).is_ok());
}
