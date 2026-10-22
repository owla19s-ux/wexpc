//! Trusted user-settings entrypoint, separate from offline/AI runtime commands.
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::{ptr, slice};
use wex_access_store::{Grant, Store, StoreError};
use wex_core::access::Access;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol_version: u32,
    request_id: String,
    command: Command,
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    List {},
    Set {
        expected_revision: String,
        directory_id: String,
        access: Mode,
        explicit_personal: bool,
    },
    Revoke {
        expected_revision: String,
        directory_id: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Read,
    Write,
    ReadWrite,
}

enum Action {
    List,
    Set(u64, Grant),
    Revoke(u64, String),
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn revision(text: &str) -> Option<u64> {
    if text.is_empty()
        || text.len() > 20
        || (text.len() > 1 && text.starts_with('0'))
        || !text.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    text.parse().ok()
}

fn error(id: Option<&str>, code: &str, retryable: bool) -> Value {
    json!({
        "protocol_version":1, "request_id":id, "status":"error", "data":null,
        "error":{"code":code,"retryable":retryable},
    })
}

fn store_error(id: &str, reason: StoreError) -> Value {
    let code = match reason {
        StoreError::Unavailable => "STORE_UNAVAILABLE",
        StoreError::Busy => "STORE_BUSY",
        StoreError::InvalidSnapshot => "INVALID_SNAPSHOT",
        StoreError::InvalidGrant => "INVALID_GRANT",
        StoreError::Conflict => "REVISION_CONFLICT",
        StoreError::LimitExceeded => "GRANT_LIMIT",
        StoreError::RevisionExhausted => "REVISION_EXHAUSTED",
        StoreError::CommitUnknown => "COMMIT_UNKNOWN",
    };
    error(Some(id), code, reason == StoreError::Busy)
}

fn response(root: &str, input: &[u8]) -> Value {
    let Ok(request) = serde_json::from_slice::<Request>(input) else {
        return error(None, "INVALID_REQUEST", false);
    };
    let id = valid_id(&request.request_id).then_some(request.request_id.as_str());
    if request.protocol_version != 1 {
        return error(id, "UNSUPPORTED_VERSION", false);
    }
    let Some(id) = id else {
        return error(None, "INVALID_REQUEST", false);
    };
    // Validate the entire command before opening or creating a private store.
    let action = match request.command {
        Command::List {} => Action::List,
        Command::Set {
            expected_revision,
            directory_id,
            access,
            explicit_personal,
        } => {
            let Some(revision) = revision(&expected_revision) else {
                return error(Some(id), "INVALID_REQUEST", false);
            };
            if !valid_id(&directory_id) {
                return error(Some(id), "INVALID_GRANT", false);
            }
            Action::Set(
                revision,
                Grant {
                    directory_id,
                    access: match access {
                        Mode::Read => Access::Read,
                        Mode::Write => Access::Write,
                        Mode::ReadWrite => Access::ReadWrite,
                    },
                    explicit_personal,
                },
            )
        }
        Command::Revoke {
            expected_revision,
            directory_id,
        } => {
            let Some(revision) = revision(&expected_revision) else {
                return error(Some(id), "INVALID_REQUEST", false);
            };
            if !valid_id(&directory_id) {
                return error(Some(id), "INVALID_GRANT", false);
            }
            Action::Revoke(revision, directory_id)
        }
    };
    let store = match Store::open(root) {
        Ok(store) => store,
        Err(reason) => return store_error(id, reason),
    };
    let data = match action {
        Action::List => store.load().map(|snapshot| {
            let grants: Vec<_> = snapshot
                .grants
                .into_iter()
                .map(|grant| {
                    json!({
                        "directory_id":grant.directory_id,
                        "access": match grant.access {
                            Access::Read => "read",
                            Access::Write => "write",
                            Access::ReadWrite => "read_write",
                        },
                        "explicit_personal":grant.explicit_personal,
                    })
                })
                .collect();
            json!({"revision":snapshot.revision.to_string(),"grants":grants})
        }),
        Action::Set(revision, grant) => store
            .set(revision, grant)
            .map(|revision| json!({"revision":revision.to_string()})),
        Action::Revoke(revision, directory_id) => store
            .revoke(revision, &directory_id)
            .map(|revision| json!({"revision":revision.to_string()})),
    };
    match data {
        Ok(data) => json!({
            "protocol_version":1, "request_id":id, "status":"ok", "data":data, "error":null,
        }),
        Err(reason) => store_error(id, reason),
    }
}

#[no_mangle]
pub extern "C" fn wex_access_admin_abi_version() -> u32 {
    1
}

/// Bounded trusted-admin request using an out-of-band private store location.
///
/// # Safety
/// All non-null pointers must refer to live, aligned, non-overlapping buffers
/// of the specified sizes; output_len must reference one writable u32. No
/// allocation may be mutated or invalidated concurrently. Only a trusted Host
/// may supply the private root; it must never come from AI/Space input.
/// Caller retains ownership of all buffers. Run disk I/O off the UI thread.
#[no_mangle]
pub unsafe extern "C" fn wex_access_admin_call(
    root: *const u8,
    root_len: u32,
    request: *const u8,
    request_len: u32,
    output: *mut u8,
    output_capacity: u32,
    output_len: *mut u32,
) -> i32 {
    if output_len.is_null() {
        return -1;
    }
    // SAFETY: caller guarantees a live, aligned output_len allocation.
    unsafe { output_len.write(0) };
    if root.is_null()
        || request.is_null()
        || output.is_null()
        || root_len == 0
        || root_len > 4096
        || request_len == 0
        || request_len > 16384
    {
        return -1;
    }
    if output_capacity < 65536 {
        return -2;
    }
    // SAFETY: caller guarantees readable, bounded, stable input allocations.
    let root_bytes = unsafe { slice::from_raw_parts(root, root_len as usize) };
    let Ok(root) = std::str::from_utf8(root_bytes) else {
        return -1;
    };
    if root.contains('\0') || !Path::new(root).is_absolute() {
        return -1;
    }
    // SAFETY: request_len is bounded and caller guarantees readable bytes.
    let input = unsafe { slice::from_raw_parts(request, request_len as usize) };
    let reply = match super::guarded_reply(input, |bytes| {
        serde_json::to_vec(&response(root, bytes)).expect("serialize bounded admin response")
    }) {
        Ok(reply) => reply,
        Err(code) => return code,
    };
    // SAFETY: output capacity is at least 65536, checked by guarded_reply;
    // caller guarantees disjoint, writable buffers and output_len.
    unsafe {
        ptr::copy_nonoverlapping(reply.as_ptr(), output, reply.len());
        output_len.write(reply.len() as u32);
    }
    0
}
