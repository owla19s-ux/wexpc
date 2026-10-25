use std::sync::Arc;
use wex_desktop_service::{ProcessRequest, ProcessSupervisor};
fn request() -> ProcessRequest {
    ProcessRequest {
        program: "compiler".into(),
        arguments: vec!["--test".into()],
        timeout_ms: 5000,
    }
}
#[test]
fn cancellation_is_bound_to_exact_conversation_and_attempt() {
    let supervisor = Arc::new(ProcessSupervisor::default());
    let lease = supervisor
        .start("conv_A", "attempt_A", "op_A", &request())
        .unwrap();
    assert_eq!(supervisor.list("conv_A").unwrap().len(), 1);
    assert!(supervisor.list("conv_B").unwrap().is_empty());
    assert!(supervisor.cancel("conv_B", "attempt_A").is_err());
    assert!(!lease.control().is_cancelled());
    supervisor.cancel("conv_A", "attempt_A").unwrap();
    assert!(lease.control().is_cancelled());
    assert!(supervisor.list("conv_A").unwrap()[0].cancellation_requested);
    drop(lease);
    assert!(supervisor.list("conv_A").unwrap().is_empty());
    assert!(supervisor.cancel("conv_A", "attempt_A").is_err());
}
#[test]
fn one_process_cancellation_does_not_stop_another_agent() {
    let supervisor = Arc::new(ProcessSupervisor::default());
    let a = supervisor
        .start("conv_A", "attempt_A", "op_A", &request())
        .unwrap();
    let b = supervisor
        .start("conv_B", "attempt_B", "op_B", &request())
        .unwrap();
    assert!(supervisor
        .start("conv_A", "attempt_A", "op_A", &request())
        .is_err());
    supervisor.cancel("conv_A", "attempt_A").unwrap();
    assert!(a.control().is_cancelled());
    assert!(!b.control().is_cancelled());
    assert_eq!(supervisor.list("conv_B").unwrap()[0].operation_id, "op_B");
}
