//! Trusted local organization administration ABI (not the AI/offline runtime).
//! The host supplies a fixed app-private root out-of-band. Organization names
//! and positions never confer file, Connection, AI or multi-user permissions.
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::{ptr, slice};
use wex_workspace_store::{Change, OrganizationError, OrganizationStore, OrganizationView};

const INPUT_MAX: u32 = 16_384;
const ROOT_MAX: u32 = 4_096;
const OUTPUT_CAP: u32 = 524_288;

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
    PreviewMigration {},
    MigrateV1 {
        expected_revision: String,
    },
    CreateSpace {
        expected_revision: String,
        name: String,
    },
    RenameSpace {
        expected_revision: String,
        id: String,
        name: String,
    },
    DeleteEmptySpace {
        expected_revision: String,
        id: String,
    },
    CreateGroup {
        expected_revision: String,
        space_id: String,
        name: String,
    },
    RenameGroup {
        expected_revision: String,
        id: String,
        name: String,
    },
    DeleteEmptyGroup {
        expected_revision: String,
        id: String,
    },
    CreateContext {
        expected_revision: String,
        space_id: String,
        group_id: Option<String>,
        name: String,
    },
    RenameContext {
        expected_revision: String,
        id: String,
        name: String,
    },
    MoveContext {
        expected_revision: String,
        id: String,
        space_id: String,
        group_id: Option<String>,
    },
}

enum Action {
    List,
    PreviewMigration,
    MigrateV1(u64),
    Apply(u64, Change),
}

fn request_id_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn revision(value: &str) -> Option<u64> {
    if value.is_empty()
        || value.len() > 20
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    value.parse::<u64>().ok()
}

fn parse_action(command: Command) -> Option<Action> {
    match command {
        Command::List {} => Some(Action::List),
        Command::PreviewMigration {} => Some(Action::PreviewMigration),
        Command::MigrateV1 { expected_revision } => {
            revision(&expected_revision).map(Action::MigrateV1)
        }
        Command::CreateSpace {
            expected_revision,
            name,
        } => {
            revision(&expected_revision).map(|rev| Action::Apply(rev, Change::CreateSpace { name }))
        }
        Command::RenameSpace {
            expected_revision,
            id,
            name,
        } => revision(&expected_revision)
            .map(|rev| Action::Apply(rev, Change::RenameSpace { id, name })),
        Command::DeleteEmptySpace {
            expected_revision,
            id,
        } => revision(&expected_revision)
            .map(|rev| Action::Apply(rev, Change::DeleteEmptySpace { id })),
        Command::CreateGroup {
            expected_revision,
            space_id,
            name,
        } => revision(&expected_revision)
            .map(|rev| Action::Apply(rev, Change::CreateGroup { space_id, name })),
        Command::RenameGroup {
            expected_revision,
            id,
            name,
        } => revision(&expected_revision)
            .map(|rev| Action::Apply(rev, Change::RenameGroup { id, name })),
        Command::DeleteEmptyGroup {
            expected_revision,
            id,
        } => revision(&expected_revision)
            .map(|rev| Action::Apply(rev, Change::DeleteEmptyGroup { id })),
        Command::CreateContext {
            expected_revision,
            space_id,
            group_id,
            name,
        } => revision(&expected_revision).map(|rev| {
            Action::Apply(
                rev,
                Change::CreateContext {
                    space_id,
                    group_id,
                    name,
                },
            )
        }),
        Command::RenameContext {
            expected_revision,
            id,
            name,
        } => revision(&expected_revision)
            .map(|rev| Action::Apply(rev, Change::RenameContext { id, name })),
        Command::MoveContext {
            expected_revision,
            id,
            space_id,
            group_id,
        } => revision(&expected_revision).map(|rev| {
            Action::Apply(
                rev,
                Change::MoveContext {
                    id,
                    space_id,
                    group_id,
                },
            )
        }),
    }
}

fn error(id: Option<&str>, code: &str, retryable: bool) -> Value {
    json!({
        "protocol_version": 1, "request_id": id, "status": "error",
        "data": null, "error": {"code": code, "retryable": retryable}
    })
}

fn store_error(id: &str, reason: OrganizationError) -> Value {
    let code = match reason {
        OrganizationError::Unavailable => "STORE_UNAVAILABLE",
        OrganizationError::Busy => "STORE_BUSY",
        OrganizationError::InvalidSnapshot => "INVALID_SNAPSHOT",
        OrganizationError::InvalidInput => "INVALID_INPUT",
        OrganizationError::NotFound => "NOT_FOUND",
        OrganizationError::DuplicateName => "DUPLICATE_NAME",
        OrganizationError::NotEmpty => "NOT_EMPTY",
        OrganizationError::Conflict => "REVISION_CONFLICT",
        OrganizationError::LimitExceeded => "LIMIT_EXCEEDED",
        OrganizationError::RevisionExhausted => "REVISION_EXHAUSTED",
        OrganizationError::CommitUnknown => "COMMIT_UNKNOWN",
        OrganizationError::MigrationRequired => "MIGRATION_REQUIRED",
    };
    error(Some(id), code, reason == OrganizationError::Busy)
}

fn view(view: OrganizationView) -> Value {
    json!({
        "revision": view.revision.to_string(),
        "spaces": view.spaces,
        "groups": view.groups,
        "contexts": view.contexts,
    })
}

fn response(root: &str, input: &[u8]) -> Value {
    let Ok(request) = serde_json::from_slice::<Request>(input) else {
        return error(None, "INVALID_REQUEST", false);
    };
    let id = request_id_valid(&request.request_id).then_some(request.request_id.as_str());
    if request.protocol_version != 1 {
        return error(id, "UNSUPPORTED_VERSION", false);
    }
    let Some(id) = id else {
        return error(None, "INVALID_REQUEST", false);
    };
    // Reject malformed commands before even creating a private directory.
    let Some(action) = parse_action(request.command) else {
        return error(Some(id), "INVALID_REQUEST", false);
    };
    let store = match OrganizationStore::open(root) {
        Ok(store) => store,
        Err(reason) => return store_error(id, reason),
    };
    let result = match action {
        Action::List => store.load().map(view),
        Action::PreviewMigration => store.legacy_summary().map(|legacy| match legacy {
            Some(summary) => json!({
                "required": true, "revision": summary.revision.to_string(),
                "spaces": summary.spaces, "groups": summary.groups,
                "contexts": summary.contexts,
            }),
            None => json!({"required": false}),
        }),
        Action::MigrateV1(rev) => store.migrate_v1(rev).map(view),
        Action::Apply(rev, change) => store.apply(rev, change).map(view),
    };
    match result {
        Ok(data) => json!({
            "protocol_version": 1, "request_id": id, "status": "ok",
            "data": data, "error": null,
        }),
        Err(reason) => store_error(id, reason),
    }
}

#[no_mangle]
pub extern "C" fn wex_organization_admin_abi_version() -> u32 {
    1
}

/// Independent, trusted UI administration entrypoint. Never register as AI tool.
/// ABI output is deliberately larger than the offline ABI: a full organization
/// view can reflect a bounded 256 KiB persisted snapshot.
///
/// # Safety
/// Caller owns every buffer. Non-null pointers must address valid live, stable,
/// non-overlapping memory for the declared lengths; output_len must be aligned
/// and writable for one u32. Invalid pointers are undefined behavior.
/// Only trusted Host composition supplies root. Do disk work off the UI thread.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn wex_organization_admin_call(
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
    // SAFETY: caller guarantees output_len is one live, aligned writable u32.
    unsafe { output_len.write(0) };
    if root.is_null()
        || request.is_null()
        || output.is_null()
        || root_len == 0
        || root_len > ROOT_MAX
        || request_len == 0
        || request_len > INPUT_MAX
    {
        return -1;
    }
    if output_capacity < OUTPUT_CAP {
        return -2;
    }
    // SAFETY: the host guarantees these bounded buffers are readable and stable.
    let root_bytes = unsafe { slice::from_raw_parts(root, root_len as usize) };
    let Ok(root) = std::str::from_utf8(root_bytes) else {
        return -1;
    };
    if root.contains('\0') || !Path::new(root).is_absolute() {
        return -1;
    }
    // SAFETY: request_len has been bounded and caller guarantees valid memory.
    let input = unsafe { slice::from_raw_parts(request, request_len as usize) };
    // No Rust panic is allowed to cross this C ABI. An uncertain write is not retried.
    let encoded = match std::panic::catch_unwind(|| serde_json::to_vec(&response(root, input))) {
        Ok(Ok(encoded)) if !encoded.is_empty() && encoded.len() <= OUTPUT_CAP as usize => encoded,
        Ok(_) => return -4,
        Err(_) => return -3,
    };
    // SAFETY: checked capacity, caller guarantees the destination is disjoint.
    unsafe {
        ptr::copy_nonoverlapping(encoded.as_ptr(), output, encoded.len());
        output_len.write(encoded.len() as u32);
    }
    0
}
