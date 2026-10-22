use wex_core::{handle, Operation, Reply, Request, Status};

fn call(version: u32, id: &str, operation: Operation) -> Reply {
    handle(Request {
        protocol_version: version,
        request_id: id.into(),
        operation,
    })
}
fn error(reply: Reply, status: Status, code: &str, id: Option<&str>) {
    assert_eq!(reply.protocol_version, 1);
    assert_eq!(reply.request_id.as_deref(), id);
    assert_eq!(reply.status, status);
    assert_eq!(reply.value, None);
    let detail = reply.error.expect("error detail");
    assert_eq!(detail.code, code);
    assert!(!detail.message.is_empty());
    assert!(!detail.retryable);
}
#[test]
fn ping_returns_correlated_pong() {
    let reply = call(1, "req-1", Operation::Ping);
    assert_eq!(reply.protocol_version, 1);
    assert_eq!(reply.request_id.as_deref(), Some("req-1"));
    assert_eq!(reply.status, Status::Ok);
    assert_eq!(reply.value.as_deref(), Some("pong"));
    assert_eq!(reply.error, None);
}
#[test]
fn echo_preserves_empty_unicode_and_embedded_nul() {
    for text in ["", "你好🌙", "a\0b"] {
        let reply = call(1, "r_1", Operation::Echo(text.into()));
        assert_eq!(reply.status, Status::Ok);
        assert_eq!(reply.value.as_deref(), Some(text));
        assert_eq!(reply.error, None);
    }
}
#[test]
fn echo_limit_counts_utf8_bytes() {
    for text in ["a".repeat(4096), "月".repeat(1365) + "a"] {
        assert_eq!(text.len(), 4096);
        assert_eq!(call(1, "r", Operation::Echo(text)).status, Status::Ok);
    }
    for text in ["a".repeat(4097), "月".repeat(1366)] {
        error(
            call(1, "r", Operation::Echo(text)),
            Status::Error,
            "INVALID_REQUEST",
            Some("r"),
        );
    }
}
#[test]
fn demo_failure_is_not_a_transport_failure() {
    error(
        call(1, "r", Operation::DemoFail),
        Status::Error,
        "DEMO_FAILURE",
        Some("r"),
    );
}
#[test]
fn demo_denial_is_a_distinct_status() {
    error(
        call(1, "r", Operation::DemoDeny),
        Status::Denied,
        "DEMO_DENIED",
        Some("r"),
    );
}
#[test]
fn unknown_operation_keeps_request_id() {
    error(
        call(1, "r", Operation::Unknown("future".into())),
        Status::Error,
        "UNKNOWN_OPERATION",
        Some("r"),
    );
}
#[test]
fn invalid_ids_are_not_echoed() {
    for id in ["", " ", "中文", "a.b", "a/b", "a\n", &"a".repeat(65)] {
        error(
            call(1, id, Operation::Ping),
            Status::Error,
            "INVALID_REQUEST",
            None,
        );
    }
}
#[test]
fn id_boundary_and_ascii_alphabet_are_valid() {
    for id in ["A_z-09", &"a".repeat(64)] {
        assert_eq!(call(1, id, Operation::Ping).request_id.as_deref(), Some(id));
    }
}
#[test]
fn unsupported_versions_precede_operation_validation() {
    for version in [0, 2, u32::MAX] {
        error(
            call(version, "r", Operation::Unknown("bad".into())),
            Status::Error,
            "UNSUPPORTED_VERSION",
            Some("r"),
        );
    }
    error(
        call(2, "bad id", Operation::Ping),
        Status::Error,
        "UNSUPPORTED_VERSION",
        None,
    );
}
#[test]
fn repeated_requests_do_not_share_state() {
    for _ in 0..100 {
        error(
            call(1, "deny", Operation::DemoDeny),
            Status::Denied,
            "DEMO_DENIED",
            Some("deny"),
        );
        assert_eq!(
            call(1, "ping", Operation::Ping).value.as_deref(),
            Some("pong")
        );
    }
}
