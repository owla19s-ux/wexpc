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
fn competing_agents_cannot_both_hold_the_same_scope() {
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

    let gate = TaskGate::default();
    let barrier = Arc::new(Barrier::new(3));
    let (result_tx, result_rx) = mpsc::channel();
    let mut release_senders = Vec::new();
    let mut workers = Vec::new();

    for task in ["agent-a-task", "agent-b-task"] {
        let worker_gate = gate.clone();
        let worker_barrier = barrier.clone();
        let result_tx = result_tx.clone();
        let (release_tx, release_rx) = mpsc::channel();
        release_senders.push(release_tx);
        workers.push(std::thread::spawn(move || {
            worker_barrier.wait();
            let admission = worker_gate.start(task, &["C:/repo/src/main.rs".into()]);
            result_tx.send(admission.is_ok()).unwrap();
            if let Ok(lease) = admission {
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                lease.finish(TaskPhase::Completed).unwrap();
            }
        }));
    }
    drop(result_tx);

    barrier.wait();
    let first = result_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let second = result_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(u8::from(first) + u8::from(second), 1);

    for release in release_senders {
        let _ = release.send(());
    }
    for worker in workers {
        worker.join().unwrap();
    }
    // This proves only process-local admission, not cross-process or crash safety.
}

#[test]
fn scoped_execution_panic_releases_reservation_after_unwind() {
    let gate = TaskGate::default();
    let observer = gate.clone();
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = gate.execute(
            "panicking-task",
            &["C:/repo/src".into()],
            || -> Result<(), ()> {
                assert_eq!(observer.is_running("panicking-task"), Ok(true));
                panic!("simulated tool panic");
            },
        );
    }));
    assert!(unwind.is_err());
    assert_eq!(gate.is_running("panicking-task"), Ok(false));
    assert!(gate.start("replacement", &["C:/repo/src/main.rs".into()]).is_ok());
}

#[test]
fn multi_scope_conflict_rejects_entire_task_without_partial_reservation() {
    let gate = TaskGate::default();
    let held = gate.start("held", &["C:/repo/tests".into()]).unwrap();
    assert!(matches!(
        gate.start(
            "rejected",
            &["C:/repo/src".into(), "C:/repo/tests/sub".into()],
        ),
        Err(TaskError::Scope(ReservationError::Conflict))
    ));
    assert_eq!(gate.is_running("rejected"), Ok(false));
    let disjoint = gate.start("disjoint", &["C:/repo/src".into()]).unwrap();
    assert_eq!(gate.is_running("held"), Ok(true));
    drop(disjoint);
    drop(held);
    assert_eq!(gate.is_running("disjoint"), Ok(false));
    assert_eq!(gate.is_running("held"), Ok(false));
}
