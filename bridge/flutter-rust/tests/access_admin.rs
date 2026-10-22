#![cfg(unix)]

use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use wex_runtime::{wex_access_admin_abi_version, wex_access_admin_call, wex_runtime_call};

const CAP: usize = 65536;
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "wex-admin-月-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn root(&self) -> PathBuf {
        self.0.join("private")
    }
    fn call(&self, command: Value) -> Value {
        self.raw(
            &serde_json::to_vec(&json!({
                "protocol_version":1, "request_id":"admin-1", "command":command,
            }))
            .unwrap(),
        )
    }
    fn raw(&self, request: &[u8]) -> Value {
        let path = self.root();
        let root = path.to_str().unwrap().as_bytes();
        let mut output = vec![0xA5; CAP + 8];
        let mut length = 999;
        let result = unsafe {
            wex_access_admin_call(
                root.as_ptr(),
                root.len() as u32,
                request.as_ptr(),
                request.len() as u32,
                output.as_mut_ptr(),
                CAP as u32,
                &mut length,
            )
        };
        assert_eq!(result, 0);
        assert!((1..=CAP as u32).contains(&length));
        assert!(output[length as usize..].iter().all(|b| *b == 0xA5));
        serde_json::from_slice(&output[..length as usize]).unwrap()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn set(revision: &str, id: &str, mode: &str, personal: bool) -> Value {
    json!({"operation":"set","expected_revision":revision,"directory_id":id,
        "access":mode,"explicit_personal":personal})
}

#[test]
fn distinct_admin_abi_version_is_one() {
    assert_eq!(wex_access_admin_abi_version(), 1);
}

#[test]
fn list_set_reopen_and_revoke_cross_real_abi() {
    let root = Sandbox::new();
    let initial = root.call(json!({"operation":"list"}));
    assert_eq!(initial["data"], json!({"revision":"0","grants":[]}));
    let reply = root.call(set("0", "docs", "read", false));
    assert_eq!(reply["status"], "ok");
    assert_eq!(reply["data"], json!({"revision":"1"}));
    assert_eq!(reply["request_id"], "admin-1");
    assert!(reply["error"].is_null());
    assert!(root.root().join("grants.json").is_file());
    let reopened = root.call(json!({"operation":"list"}));
    assert_eq!(reopened["data"]["revision"], "1");
    assert_eq!(
        reopened["data"]["grants"],
        json!([{
            "directory_id":"docs","access":"read","explicit_personal":false,
        }])
    );
    assert_eq!(
        root.call(json!({
            "operation":"revoke","expected_revision":"1","directory_id":"docs",
        }))["data"]["revision"],
        "2"
    );
    assert_eq!(
        root.call(json!({"operation":"list"}))["data"]["grants"],
        json!([])
    );
}

#[test]
fn stale_edit_is_definite_conflict_without_restoring_old_permissions() {
    let root = Sandbox::new();
    root.call(set("0", "docs", "read_write", true));
    root.call(json!({"operation":"revoke","expected_revision":"1","directory_id":"docs"}));
    let reply = root.call(set("1", "docs", "write", true));
    assert_eq!(reply["error"]["code"], "REVISION_CONFLICT");
    assert_eq!(reply["error"]["retryable"], false);
    assert!(reply["data"].is_null());
    assert_eq!(
        root.call(json!({"operation":"list"}))["data"]["grants"],
        json!([])
    );
}

#[test]
fn read_write_and_personal_consent_survive_exactly() {
    let root = Sandbox::new();
    root.call(set("0", "a", "write", true));
    root.call(set("1", "b", "read_write", false));
    let reply = root.call(json!({"operation":"list"}));
    assert_eq!(
        reply["data"]["grants"],
        json!([
            {"directory_id":"a","access":"write","explicit_personal":true},
            {"directory_id":"b","access":"read_write","explicit_personal":false},
        ])
    );
}

#[test]
fn malformed_unknown_or_duplicate_fields_do_not_create_storage() {
    let root = Sandbox::new();
    for request in [
        "{",
        r#"{"protocol_version":1,"request_id":"a","command":{"operation":"list","root":"/tmp"}}"#,
        r#"{"protocol_version":1,"request_id":"a","command":{"operation":"list"},"root":"/tmp"}"#,
        r#"{"protocol_version":1,"request_id":"a","request_id":"b","command":{"operation":"list"}}"#,
        r#"{"protocol_version":1,"request_id":"a","command":{"operation":"list","operation":"list"}}"#,
        r#"{"protocol_version":1,"request_id":"a","command":{"operation":"future"}}"#,
        r#"{"protocol_version":1,"request_id":"a","command":{"operation":"set","expected_revision":"0","directory_id":"x","access":"execute","explicit_personal":false}}"#,
    ] {
        let reply = root.raw(request.as_bytes());
        assert_eq!(reply["status"], "error");
        assert_eq!(reply["error"]["code"], "INVALID_REQUEST");
        assert!(!root.root().exists());
    }
}

#[test]
fn invalid_identity_version_and_revision_never_touch_store() {
    let root = Sandbox::new();
    for request in [
        json!({"protocol_version":2,"request_id":"a","command":{"operation":"list"}}),
        json!({"protocol_version":1,"request_id":"a/b","command":{"operation":"list"}}),
        json!({"protocol_version":1,"request_id":"a","command":set("01","docs","read",false)}),
        json!({"protocol_version":1,"request_id":"a","command":set("+1","docs","read",false)}),
        json!({"protocol_version":1,"request_id":"a","command":set("18446744073709551616","docs","read",false)}),
        json!({"protocol_version":1,"request_id":"a","command":set("0","../escape","read",false)}),
    ] {
        assert_eq!(
            root.raw(&serde_json::to_vec(&request).unwrap())["status"],
            "error"
        );
        assert!(!root.root().exists());
    }
    let mut command = set("0", "docs", "read", false);
    command["expected_revision"] = json!(0);
    assert_eq!(root.call(command)["error"]["code"], "INVALID_REQUEST");
    assert!(!root.root().exists());
}

#[test]
fn locked_corrupt_and_orphan_states_never_report_success() {
    let root = Sandbox::new();
    root.call(set("0", "docs", "read", false));
    fs::write(root.root().join("grants.lock"), b"").unwrap();
    let busy = root.call(json!({"operation":"list"}));
    assert_eq!(busy["error"], json!({"code":"STORE_BUSY","retryable":true}));
    fs::remove_file(root.root().join("grants.lock")).unwrap();
    fs::write(root.root().join("grants.tmp"), b"interrupted").unwrap();
    assert_eq!(
        root.call(json!({"operation":"list"}))["error"]["code"],
        "STORE_UNAVAILABLE"
    );
    fs::remove_file(root.root().join("grants.tmp")).unwrap();
    fs::write(root.root().join("grants.json"), b"{").unwrap();
    let corrupt = root.call(json!({"operation":"list"}));
    assert_eq!(corrupt["error"]["code"], "INVALID_SNAPSHOT");
    assert!(!serde_json::to_string(&corrupt)
        .unwrap()
        .contains(root.0.to_str().unwrap()));
}

#[test]
fn revisions_above_javascript_integer_precision_are_strings() {
    let root = Sandbox::new();
    root.call(json!({"operation":"list"}));
    fs::write(
        root.root().join("grants.json"),
        br#"{"schema_version":1,"revision":9007199254740993,"grants":[]}"#,
    )
    .unwrap();
    let reply = root.call(set("9007199254740993", "docs", "read", false));
    assert_eq!(reply["data"]["revision"], "9007199254740994");
}

#[test]
fn native_argument_rejections_reset_length_without_side_effects() {
    let root = Sandbox::new();
    let path = root.root();
    let bytes = path.to_str().unwrap().as_bytes();
    let input = br#"{"protocol_version":1,"request_id":"a","command":{"operation":"list"}}"#;
    let mut output = vec![0xA5; CAP];
    let oversized_root = vec![b'/'; 4097];
    let oversized_request = vec![b' '; 16385];
    for (root_ptr, root_len, input_ptr, input_len, cap, code) in [
        (
            ptr::null(),
            bytes.len() as u32,
            input.as_ptr(),
            input.len() as u32,
            CAP as u32,
            -1,
        ),
        (
            bytes.as_ptr(),
            0,
            input.as_ptr(),
            input.len() as u32,
            CAP as u32,
            -1,
        ),
        (
            oversized_root.as_ptr(),
            4097,
            input.as_ptr(),
            input.len() as u32,
            CAP as u32,
            -1,
        ),
        (
            bytes.as_ptr(),
            bytes.len() as u32,
            ptr::null(),
            input.len() as u32,
            CAP as u32,
            -1,
        ),
        (
            bytes.as_ptr(),
            bytes.len() as u32,
            oversized_request.as_ptr(),
            16385,
            CAP as u32,
            -1,
        ),
        (
            bytes.as_ptr(),
            bytes.len() as u32,
            input.as_ptr(),
            input.len() as u32,
            65535,
            -2,
        ),
    ] {
        let mut length = 999;
        assert_eq!(
            unsafe {
                wex_access_admin_call(
                    root_ptr,
                    root_len,
                    input_ptr,
                    input_len,
                    output.as_mut_ptr(),
                    cap,
                    &mut length,
                )
            },
            code
        );
        assert_eq!(length, 0);
        assert!(output.iter().all(|b| *b == 0xA5));
        assert!(!root.root().exists());
    }
}

#[test]
fn invalid_root_encoding_relative_path_and_nul_are_native_errors() {
    let input = b"{}";
    let mut output = vec![0xA5; CAP];
    for bytes in [&b"relative"[..], &b"/tmp/with\0nul"[..], &b"\xff"[..]] {
        let mut length = 99;
        assert_eq!(
            unsafe {
                wex_access_admin_call(
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    input.as_ptr(),
                    input.len() as u32,
                    output.as_mut_ptr(),
                    CAP as u32,
                    &mut length,
                )
            },
            -1
        );
        assert_eq!(length, 0);
        assert!(output.iter().all(|b| *b == 0xA5));
    }
}

#[test]
fn old_offline_runtime_does_not_accept_admin_commands() {
    let request =
        br#"{"protocol_version":1,"request_id":"a","operation":"access_set","payload":{}}"#;
    let mut output = vec![0; CAP];
    let mut length = 0;
    assert_eq!(
        unsafe {
            wex_runtime_call(
                request.as_ptr(),
                request.len() as u32,
                output.as_mut_ptr(),
                CAP as u32,
                &mut length,
            )
        },
        0
    );
    let reply: Value = serde_json::from_slice(&output[..length as usize]).unwrap();
    assert_eq!(reply["error"]["code"], "UNKNOWN_OPERATION");
}

#[test]
fn null_output_pointers_are_rejected_before_store_creation() {
    let root = Sandbox::new();
    let path = root.root();
    let bytes = path.to_str().unwrap().as_bytes();
    let input = b"{}";
    let mut output = vec![0xA5; CAP];
    let mut length = 99;
    assert_eq!(
        unsafe {
            wex_access_admin_call(
                bytes.as_ptr(),
                bytes.len() as u32,
                input.as_ptr(),
                input.len() as u32,
                ptr::null_mut(),
                CAP as u32,
                &mut length,
            )
        },
        -1
    );
    assert_eq!(length, 0);
    assert_eq!(
        unsafe {
            wex_access_admin_call(
                bytes.as_ptr(),
                bytes.len() as u32,
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
    assert!(!root.root().exists());
}

#[test]
fn nested_duplicates_trailing_json_and_invalid_utf8_are_rejected() {
    let root = Sandbox::new();
    for bytes in [
        &br#"{"protocol_version":1,"request_id":"a","command":{"operation":"set","expected_revision":"0","directory_id":"x","access":"read","access":"write","explicit_personal":false}}"#[..],
        &br#"{"protocol_version":1,"request_id":"a","command":{"operation":"revoke","expected_revision":"0","expected_revision":"1","directory_id":"x"}}"#[..],
        &br#"{"protocol_version":1,"request_id":"a","command":{"operation":"list"}}{}"#[..],
        &b"\xff"[..],
    ] {
        assert_eq!(root.raw(bytes)["error"]["code"], "INVALID_REQUEST");
        assert!(!root.root().exists());
    }
}

#[test]
fn valid_request_at_input_limit_is_accepted() {
    let root = Sandbox::new();
    let id = "a".repeat(64);
    let mut input = serde_json::to_vec(&json!({
        "protocol_version":1,"request_id":id,"command":{"operation":"list"},
    }))
    .unwrap();
    input.resize(16384, b' ');
    let reply = root.raw(&input);
    assert_eq!(reply["status"], "ok");
    assert_eq!(reply["request_id"], id);
}

#[test]
fn maximum_grant_list_fits_the_caller_buffer() {
    let root = Sandbox::new();
    root.call(json!({"operation":"list"}));
    let grants: Vec<_> = (0..128)
        .map(|i| {
            json!({
                "directory_id":format!("g{i:03}{}", "x".repeat(60)),
                "access":"read_write","explicit_personal":true,
            })
        })
        .collect();
    fs::write(
        root.root().join("grants.json"),
        serde_json::to_vec(&json!({
            "schema_version":1,"revision":128,"grants":grants,
        }))
        .unwrap(),
    )
    .unwrap();
    let reply = root.call(json!({"operation":"list"}));
    assert_eq!(reply["status"], "ok");
    assert_eq!(reply["data"]["grants"].as_array().unwrap().len(), 128);
    assert_eq!(reply["data"]["revision"], "128");
}

#[test]
fn exact_u64_max_is_valid_but_exhaustion_quarantines_the_store() {
    let root = Sandbox::new();
    root.call(json!({"operation":"list"}));
    fs::write(
        root.root().join("grants.json"),
        serde_json::to_vec(&json!({
            "schema_version":1,"revision":u64::MAX,"grants":[{
                "directory_id":"docs","access":"read_write","explicit_personal":false,
            }],
        }))
        .unwrap(),
    )
    .unwrap();
    let reply = root.call(set("18446744073709551615", "docs", "read", false));
    assert_eq!(
        reply["error"],
        json!({"code":"REVISION_EXHAUSTED","retryable":false})
    );
    assert_eq!(
        root.call(json!({"operation":"list"}))["error"]["code"],
        "STORE_BUSY"
    );
}

#[test]
fn version_errors_correlate_only_valid_request_ids() {
    let root = Sandbox::new();
    let reply =
        root.raw(br#"{"protocol_version":2,"request_id":"a","command":{"operation":"list"}}"#);
    assert_eq!(reply["request_id"], "a");
    assert_eq!(reply["error"]["code"], "UNSUPPORTED_VERSION");
    let invalid =
        root.raw(br#"{"protocol_version":1,"request_id":"bad/id","command":{"operation":"list"}}"#);
    assert!(invalid["request_id"].is_null());
    assert_eq!(invalid["error"]["code"], "INVALID_REQUEST");
    assert!(!root.root().exists());
}
