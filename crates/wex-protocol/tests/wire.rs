use serde_json::{json, Value};
use wex_protocol::process_json;

fn request(operation: &str, payload: Value) -> Value {
    json!({"protocol_version":1,"request_id":"r-1","operation":operation,"payload":payload})
}
fn run(value: Value) -> Value {
    serde_json::from_slice(&process_json(&serde_json::to_vec(&value).unwrap())).unwrap()
}
fn code(value: &Value) -> &str {
    value["error"]["code"].as_str().unwrap()
}
#[test]
fn ping_has_exact_wire_shape() {
    assert_eq!(
        run(request("ping", json!({}))),
        json!({
            "protocol_version":1,"request_id":"r-1","status":"ok",
            "data":{"value":"pong"},"error":null
        })
    );
}
#[test]
fn echo_preserves_unicode_nul_and_correlation() {
    for text in ["", "你好🌙", "a\u{0}b"] {
        let reply = run(request("echo", json!({"text":text})));
        assert_eq!(reply["protocol_version"], 1);
        assert_eq!(reply["request_id"], "r-1");
        assert_eq!(reply["data"]["value"], text);
        assert_eq!(reply["status"], "ok");
        assert!(reply["error"].is_null());
    }
}
#[test]
fn demo_statuses_and_unknown_operation() {
    for (op, status, expected) in [
        ("demo_fail", "error", "DEMO_FAILURE"),
        ("demo_deny", "denied", "DEMO_DENIED"),
        ("other", "error", "UNKNOWN_OPERATION"),
    ] {
        let reply = run(request(op, json!({})));
        assert_eq!(reply["status"], status);
        assert_eq!(code(&reply), expected);
        assert_eq!(reply["request_id"], "r-1");
        assert!(reply["data"].is_null());
        assert_eq!(reply["error"]["retryable"], false);
    }
}
#[test]
fn malformed_bytes_and_duplicate_keys_are_rejected() {
    for bytes in [
        vec![255], b"null".to_vec(), b"[]".to_vec(), b"{}{}".to_vec(),
        br#"{"protocol_version":1,"protocol_version":2}"#.to_vec(),
        br#"{"protocol_version":1,"request_id":"r","operation":"echo","payload":{"text":"a","text":"b"}}"#.to_vec(),
        br#"{"protocol_version":1,"request_id":"r","operation":"echo","payload":{"text":"a","\u0074ext":"b"}}"#.to_vec(),
    ] {
        let reply: Value = serde_json::from_slice(&process_json(&bytes)).unwrap();
        assert_eq!(code(&reply), "INVALID_REQUEST");
        assert!(reply["request_id"].is_null());
    }
}
#[test]
fn fields_payloads_and_id_are_strict() {
    let mut extra = request("ping", json!({}));
    extra["unexpected"] = json!(true);
    for value in [
        extra,
        request("ping", json!({"text":"x"})),
        request("echo", json!({})),
        request("echo", json!({"text":42})),
        request("echo", json!({"text":"x","extra":1})),
        request("ping", json!(null)),
    ] {
        let reply = run(value);
        assert_eq!(code(&reply), "INVALID_REQUEST");
        assert_eq!(reply["request_id"], "r-1");
    }
    let mut invalid = request("ping", json!({}));
    invalid["request_id"] = json!("bad id");
    let reply = run(invalid);
    assert_eq!(code(&reply), "INVALID_REQUEST");
    assert!(reply["request_id"].is_null());
}
#[test]
fn version_errors_precede_unknown_operations() {
    for version in [json!(0), json!(2), json!(-1)] {
        let mut value = request("other", json!({}));
        value["protocol_version"] = version;
        assert_eq!(code(&run(value)), "UNSUPPORTED_VERSION");
    }
    for version in [json!(1.0), json!("1"), json!(null)] {
        let mut value = request("ping", json!({}));
        value["protocol_version"] = version;
        assert_eq!(code(&run(value)), "INVALID_REQUEST");
    }
}
#[test]
fn byte_boundaries_and_response_capacity() {
    for text in ["a".repeat(4096), "\u{0}".repeat(2048)] {
        let input = serde_json::to_vec(&request("echo", json!({"text":text}))).unwrap();
        assert!(input.len() <= 16384);
        let output = process_json(&input);
        assert!(output.len() <= 65536);
        assert_eq!(
            serde_json::from_slice::<Value>(&output).unwrap()["status"],
            "ok"
        );
    }
    assert_eq!(
        code(&run(request("echo", json!({"text":"月".repeat(1366)})))),
        "INVALID_REQUEST"
    );
    let mut bytes = serde_json::to_vec(&request("ping", json!({}))).unwrap();
    bytes.resize(16384, b' ');
    assert_eq!(
        serde_json::from_slice::<Value>(&process_json(&bytes)).unwrap()["status"],
        "ok"
    );
    bytes.push(b' ');
    assert_eq!(
        code(&serde_json::from_slice::<Value>(&process_json(&bytes)).unwrap()),
        "INVALID_REQUEST"
    );
}
#[test]
fn deeply_nested_input_is_rejected_without_unbounded_recursion() {
    let nested = format!("{}0{}", "[".repeat(200), "]".repeat(200));
    let bytes = format!(
        r#"{{"protocol_version":1,"request_id":"r","operation":"ping","payload":{{}},"extra":{nested}}}"#
    )
    .into_bytes();
    let reply: Value = serde_json::from_slice(&process_json(&bytes)).unwrap();
    assert_eq!(code(&reply), "INVALID_REQUEST");
    assert!(reply["request_id"].is_null());
}
