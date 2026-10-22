#![cfg(unix)]
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use wex_runtime::{
    wex_organization_admin_abi_version, wex_organization_admin_call, wex_runtime_call,
};

const CAP: usize = 524_288;
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "wex-organization-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn root(&self) -> PathBuf {
        self.0.join("private-organization")
    }
    fn raw(&self, input: &[u8]) -> Value {
        let root_path = self.root();
        let root = root_path.to_str().unwrap().as_bytes();
        let mut output = vec![0xA5; CAP + 8];
        let mut length = 99_u32;
        let status = unsafe {
            wex_organization_admin_call(
                root.as_ptr(),
                root.len() as u32,
                input.as_ptr(),
                input.len() as u32,
                output.as_mut_ptr(),
                CAP as u32,
                &mut length,
            )
        };
        assert_eq!(status, 0);
        assert!((1..=CAP as u32).contains(&length));
        assert!(output[length as usize..].iter().all(|b| *b == 0xA5));
        serde_json::from_slice(&output[..length as usize]).unwrap()
    }
    fn call(&self, command: Value) -> Value {
        self.raw(
            &serde_json::to_vec(&json!({
                "protocol_version": 1, "request_id": "org-test", "command": command,
            }))
            .unwrap(),
        )
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn change(operation: &str, rev: &str, rest: Value) -> Value {
    let mut command = rest;
    command["operation"] = json!(operation);
    command["expected_revision"] = json!(rev);
    command
}

#[test]
fn distinct_organization_abi_and_store_are_unavailable_to_offline_runtime() {
    assert_eq!(wex_organization_admin_abi_version(), 1);
    let command =
        br#"{"protocol_version":1,"request_id":"a","operation":"create_context","payload":{}}"#;
    let mut output = vec![0_u8; 65536];
    let mut length = 0;
    assert_eq!(
        unsafe {
            wex_runtime_call(
                command.as_ptr(),
                command.len() as u32,
                output.as_mut_ptr(),
                65536,
                &mut length,
            )
        },
        0
    );
    let reply: Value = serde_json::from_slice(&output[..length as usize]).unwrap();
    assert_eq!(reply["error"]["code"], "UNKNOWN_OPERATION");
}

#[test]
fn ungrouped_context_and_cross_container_move_are_persistent_and_authority_free() {
    let s = Sandbox::new();
    assert_eq!(
        s.call(json!({"operation":"list"}))["data"],
        json!({"revision":"0","spaces":[],"groups":[],"contexts":[]})
    );
    let personal = s.call(change("create_space", "0", json!({"name":"个人"})));
    assert_eq!(personal["status"], "ok");
    let personal_id = personal["data"]["spaces"][0]["id"].as_str().unwrap();
    let work = s.call(change("create_space", "1", json!({"name":"工作"})));
    let work_id = work["data"]["spaces"][1]["id"].as_str().unwrap();
    let work_group = s.call(change(
        "create_group",
        "2",
        json!({"space_id":work_id,"name":"开发"}),
    ));
    let group_id = work_group["data"]["groups"][0]["id"].as_str().unwrap();
    let created = s.call(change(
        "create_context",
        "3",
        json!({"space_id":personal_id,"group_id":null,"name":"隐私资料"}),
    ));
    assert_eq!(created["status"], "ok");
    let ctx_id = created["data"]["contexts"][0]["id"].as_str().unwrap();
    assert_eq!(created["data"]["contexts"][0]["group_id"], Value::Null);
    assert_eq!(created["data"]["contexts"][0]["space_id"], personal_id);

    let moved = s.call(change(
        "move_context",
        "4",
        json!({"id":ctx_id,"space_id":work_id,"group_id":group_id}),
    ));
    assert_eq!(moved["status"], "ok");
    assert_eq!(moved["data"]["contexts"][0]["id"], ctx_id);
    assert_eq!(moved["data"]["contexts"][0]["group_id"], group_id);
    let ungrouped = s.call(change(
        "move_context",
        "5",
        json!({"id":ctx_id,"space_id":work_id,"group_id":null}),
    ));
    assert_eq!(ungrouped["data"]["contexts"][0]["id"], ctx_id);
    assert!(ungrouped["data"]["contexts"][0]["group_id"].is_null());
    let deleted = s.call(change("delete_empty_group", "6", json!({"id":group_id})));
    assert_eq!(deleted["status"], "ok");
    let again = s.call(json!({"operation":"list"}));
    assert_eq!(again["data"], deleted["data"]);
    let contents = fs::read_to_string(s.root().join("workspace.json")).unwrap();
    assert!(!contents.contains("grant"));
    assert!(!contents.contains("shared"));
    assert_eq!(
        fs::metadata(s.root()).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn group_mismatch_and_stale_revision_never_mutate_state() {
    let s = Sandbox::new();
    let a = s.call(change("create_space", "0", json!({"name":"A"})));
    let a_id = a["data"]["spaces"][0]["id"].as_str().unwrap();
    let b = s.call(change("create_space", "1", json!({"name":"B"})));
    let b_id = b["data"]["spaces"][1]["id"].as_str().unwrap();
    let group = s.call(change(
        "create_group",
        "2",
        json!({"space_id":a_id,"name":"G"}),
    ));
    let g_id = group["data"]["groups"][0]["id"].as_str().unwrap();
    let bad = s.call(change(
        "create_context",
        "3",
        json!({"space_id":b_id,"group_id":g_id,"name":"X"}),
    ));
    assert_eq!(bad["error"]["code"], "INVALID_INPUT");
    let stale = s.call(change(
        "create_context",
        "2",
        json!({"space_id":b_id,"group_id":null,"name":"X"}),
    ));
    assert_eq!(stale["error"]["code"], "REVISION_CONFLICT");
    assert_eq!(s.call(json!({"operation":"list"}))["data"], group["data"]);
}

#[test]
fn public_is_a_local_label_and_not_a_remote_share_permission() {
    let s = Sandbox::new();
    let public = s.call(change("create_space", "0", json!({"name":"公共"})));
    let space_id = public["data"]["spaces"][0]["id"].as_str().unwrap();
    let context = s.call(change(
        "create_context",
        "1",
        json!({"space_id":space_id,"name":"公开类别但未共享"}),
    ));
    assert_eq!(context["status"], "ok");
    assert_eq!(context["data"]["contexts"][0]["group_id"], Value::Null);
    let raw = fs::read_to_string(s.root().join("workspace.json")).unwrap();
    for word in ["shared", "credential", "principal", "permission", "grant"] {
        assert!(!raw.contains(word));
    }
}

#[test]
fn explicit_legacy_migration_never_infers_default_categories() {
    let s = Sandbox::new();
    assert_eq!(
        s.call(json!({"operation":"preview_migration"}))["data"]["required"],
        false
    );
    // Initialize a private store, then replace its durable state with V1.
    s.call(json!({"operation":"list"}));
    let v1 = serde_json::to_vec(&json!({
        "schema_version":1,"revision":7,"next_id":4,
        "spaces":[{"id":"0000000000000001","name":"旧容器"}],
        "groups":[{"id":"0000000000000002","space_id":"0000000000000001","name":"旧分组"}],
        "contexts":[{"id":"0000000000000003","group_id":"0000000000000002","name":"旧任务"}]
    }))
    .unwrap();
    let path = s.root().join("workspace.json");
    fs::write(&path, &v1).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let preview = s.call(json!({"operation":"preview_migration"}));
    assert_eq!(
        preview["data"],
        json!({
            "required":true,"revision":"7","spaces":1,"groups":1,"contexts":1
        })
    );
    assert_eq!(
        s.call(json!({"operation":"list"}))["error"]["code"],
        "MIGRATION_REQUIRED"
    );
    assert_eq!(
        s.call(change("migrate_v1", "6", json!({})))["error"]["code"],
        "REVISION_CONFLICT"
    );
    assert_eq!(fs::read(s.root().join("workspace.json")).unwrap(), v1);
    assert!(!s.root().join("workspace.v1.bak").exists());
    let updated = s.call(change("migrate_v1", "7", json!({})));
    assert_eq!(updated["status"], "ok");
    assert_eq!(updated["data"]["revision"], "8");
    assert_eq!(updated["data"]["contexts"][0]["id"], "0000000000000003");
    assert_eq!(
        updated["data"]["contexts"][0]["space_id"],
        "0000000000000001"
    );
    assert_eq!(
        updated["data"]["contexts"][0]["group_id"],
        "0000000000000002"
    );
    assert_eq!(fs::read(s.root().join("workspace.v1.bak")).unwrap(), v1);
    assert_eq!(
        s.call(json!({"operation":"preview_migration"}))["data"]["required"],
        false
    );
}

#[test]
fn malformed_commands_are_rejected_before_storage_or_side_effects() {
    let s = Sandbox::new();
    for input in [
        "{",
        r#"{"protocol_version":1,"request_id":"a","command":{"operation":"list","root":"/tmp"}}"#,
        r#"{"protocol_version":1,"request_id":"a","command":{"operation":"list"},"root":"/tmp"}"#,
        r#"{"protocol_version":1,"request_id":"a","request_id":"b","command":{"operation":"list"}}"#,
        r#"{"protocol_version":1,"request_id":"a","command":{"operation":"list","operation":"list"}}"#,
        r#"{"protocol_version":1,"request_id":"a","command":{"operation":"create_space","expected_revision":"01","name":"X"}}"#,
        r#"{"protocol_version":1,"request_id":"a","command":{"operation":"create_space","expected_revision":0,"name":"X"}}"#,
        r#"{"protocol_version":1,"request_id":"a","command":{"operation":"unknown"}}"#,
    ] {
        assert_eq!(s.raw(input.as_bytes())["error"]["code"], "INVALID_REQUEST");
        assert!(!s.root().exists());
    }
}

#[test]
fn ffi_short_buffers_and_invalid_roots_do_not_write_or_create_data() {
    let s = Sandbox::new();
    let input = br#"{"protocol_version":1,"request_id":"test","command":{"operation":"list"}}"#;
    let root_string = s.root().to_string_lossy().into_owned();
    let root = root_string.as_bytes();
    let mut output = vec![0xA5; CAP + 8];
    for (root_buf, root_len, request_buf, req_len, cap, expected) in [
        (
            root,
            root.len() as u32,
            &input[..],
            input.len() as u32,
            (CAP - 1) as u32,
            -2,
        ),
        (root, 0, &input[..], input.len() as u32, CAP as u32, -1),
        (
            &b"relative"[..],
            8,
            &input[..],
            input.len() as u32,
            CAP as u32,
            -1,
        ),
        (
            &b"/bad\0root"[..],
            9,
            &input[..],
            input.len() as u32,
            CAP as u32,
            -1,
        ),
    ] {
        let mut length = 123;
        assert_eq!(
            unsafe {
                wex_organization_admin_call(
                    root_buf.as_ptr(),
                    root_len,
                    request_buf.as_ptr(),
                    req_len,
                    output.as_mut_ptr(),
                    cap,
                    &mut length,
                )
            },
            expected
        );
        assert_eq!(length, 0);
        assert!(output.iter().all(|byte| *byte == 0xA5));
        assert!(!s.root().exists());
    }
}

#[test]
fn large_valid_view_is_not_capped_by_old_64k_runtime_buffer() {
    let s = Sandbox::new();
    s.call(json!({"operation":"list"}));
    let spaces = vec![json!({"id":"0000000000000001","name":"大空间"})];
    let contexts: Vec<_> = (2..=900_u64)
        .map(|id| {
            json!({
                "id":format!("{id:016x}"),
                "name":format!("文档{id}-{}", "Z".repeat(90)),
                "space_id":"0000000000000001",
                "group_id":null
            })
        })
        .collect();
    let fixture = json!({
        "schema_version":2, "revision":1, "next_id":901,
        "spaces":spaces, "groups":[], "contexts":contexts,
    });
    let serialized = serde_json::to_vec(&fixture).unwrap();
    assert!(serialized.len() > 65_536);
    assert!(serialized.len() < 262_144);
    let path = s.root().join("workspace.json");
    fs::write(&path, serialized).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let response = s.call(json!({"operation":"list"}));
    assert_eq!(response["status"], "ok");
    assert_eq!(response["data"]["contexts"].as_array().unwrap().len(), 899);
}
