use serde_json::{json, Value};
use std::ptr;
use wex_runtime::{wex_runtime_abi_version, wex_runtime_call};

const CAP: usize = 65536;
fn request(op: &str, text: Option<&str>) -> Vec<u8> {
    let payload = text.map_or_else(|| json!({}), |text| json!({"text":text}));
    serde_json::to_vec(
        &json!({"protocol_version":1,"request_id":"abi-1","operation":op,"payload":payload}),
    )
    .unwrap()
}
fn call(input: &[u8]) -> Value {
    let mut buffer = vec![0xA5; CAP + 8];
    let mut len = 999;
    let result = unsafe {
        wex_runtime_call(
            input.as_ptr(),
            input.len() as u32,
            buffer.as_mut_ptr(),
            CAP as u32,
            &mut len,
        )
    };
    assert_eq!(result, 0);
    assert!((1..=CAP as u32).contains(&len));
    assert!(buffer[len as usize..].iter().all(|b| *b == 0xA5));
    serde_json::from_slice(&buffer[..len as usize]).unwrap()
}
#[test]
fn abi_version_is_one() {
    assert_eq!(wex_runtime_abi_version(), 1);
}
#[test]
fn four_operations_preserve_protocol_and_business_status() {
    for (op, status, code) in [
        ("ping", "ok", None),
        ("echo", "ok", None),
        ("demo_fail", "error", Some("DEMO_FAILURE")),
        ("demo_deny", "denied", Some("DEMO_DENIED")),
    ] {
        let reply = call(&request(op, (op == "echo").then_some("你好🌙\0")));
        assert_eq!(reply["request_id"], "abi-1");
        assert_eq!(reply["protocol_version"], 1);
        assert_eq!(reply["status"], status);
        if let Some(code) = code {
            assert_eq!(reply["error"]["code"], code);
            assert!(reply["data"].is_null());
        } else {
            assert!(reply["error"].is_null());
            assert_eq!(
                reply["data"]["value"],
                if op == "ping" { "pong" } else { "你好🌙\0" }
            );
        }
    }
}
#[test]
fn malformed_bytes_are_protocol_errors_not_abi_errors() {
    for bytes in [vec![255], b"{".to_vec(), b"null".to_vec()] {
        let reply = call(&bytes);
        assert_eq!(reply["error"]["code"], "INVALID_REQUEST");
        assert!(reply["request_id"].is_null());
    }
}
#[test]
fn valid_request_boundaries_and_unknown_operation() {
    assert_eq!(
        call(&request("echo", Some(&"a".repeat(4096))))["status"],
        "ok"
    );
    assert_eq!(
        call(&request("echo", Some(&"a".repeat(4097))))["error"]["code"],
        "INVALID_REQUEST"
    );
    let mut bytes = request("ping", None);
    bytes.resize(16384, b' ');
    assert_eq!(call(&bytes)["status"], "ok");
    assert_eq!(
        call(&request("future", None))["error"]["code"],
        "UNKNOWN_OPERATION"
    );
}
#[test]
fn null_parameters_reset_length_without_writing_output() {
    let input = request("ping", None);
    let mut output = vec![0xA5; CAP];
    let mut len;
    let args = [
        (ptr::null(), input.len() as u32, output.as_mut_ptr()),
        (input.as_ptr(), 0, output.as_mut_ptr()),
        (input.as_ptr(), input.len() as u32, ptr::null_mut()),
    ];
    for (source, size, dest) in args {
        len = 999;
        assert_eq!(
            unsafe { wex_runtime_call(source, size, dest, CAP as u32, &mut len) },
            -1
        );
        assert_eq!(len, 0);
        assert!(output.iter().all(|b| *b == 0xA5));
    }
    assert_eq!(
        unsafe {
            wex_runtime_call(
                input.as_ptr(),
                input.len() as u32,
                output.as_mut_ptr(),
                CAP as u32,
                ptr::null_mut(),
            )
        },
        -1
    );
    assert!(output.iter().all(|b| *b == 0xA5));
}
#[test]
fn insufficient_capacity_and_oversized_request_do_not_write() {
    let input = vec![b' '; 16385];
    let mut output = vec![0xA5; CAP];
    let mut len = 999;
    assert_eq!(
        unsafe {
            wex_runtime_call(
                input.as_ptr(),
                input.len() as u32,
                output.as_mut_ptr(),
                0,
                &mut len,
            )
        },
        -1
    );
    assert_eq!(len, 0);
    let valid = request("ping", None);
    for capacity in [0, 1, 65535] {
        len = 999;
        assert_eq!(
            unsafe {
                wex_runtime_call(
                    valid.as_ptr(),
                    valid.len() as u32,
                    output.as_mut_ptr(),
                    capacity,
                    &mut len,
                )
            },
            -2
        );
        assert_eq!(len, 0);
    }
    assert!(output.iter().all(|b| *b == 0xA5));
}
#[test]
fn repeated_and_parallel_calls_have_no_shared_state() {
    let workers: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..25 {
                    assert_eq!(call(&request("ping", None))["data"]["value"], "pong");
                    assert_eq!(call(&request("demo_deny", None))["status"], "denied");
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
}
