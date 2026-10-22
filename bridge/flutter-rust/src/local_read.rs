//! Trusted composition and bounded read ABI. Handles are not permissions.
use serde::Deserialize;
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::{Mutex, TryLockError};
use std::{ptr, slice};
use wex_access_store::{ReadSession, RegisteredReadError, StoreError};
use wex_core::access::Denial;
use wex_local_resources::{FileReadError, ResourceError};

struct Table {
    next: u64,
    sessions: BTreeMap<u64, ReadSession>,
}
static TABLE: Mutex<Table> = Mutex::new(Table {
    next: 1,
    sessions: BTreeMap::new(),
});

fn guarded(action: impl FnOnce() -> i32) -> i32 {
    catch_unwind(AssertUnwindSafe(action)).unwrap_or(-3)
}
fn access(action: impl FnOnce(&mut Table) -> i32) -> i32 {
    access_table(&TABLE, action)
}
fn access_table(table: &Mutex<Table>, action: impl FnOnce(&mut Table) -> i32) -> i32 {
    guarded(|| match table.try_lock() {
        Ok(mut table) => action(&mut table),
        Err(TryLockError::WouldBlock) => -6,
        Err(TryLockError::Poisoned(_)) => -5,
    })
}
fn store_error(reason: StoreError) -> i32 {
    if reason == StoreError::Busy {
        -6
    } else {
        -5
    }
}
fn read_error(reason: RegisteredReadError) -> i32 {
    match reason {
        RegisteredReadError::Store(reason) => store_error(reason),
        RegisteredReadError::File(reason) => match reason {
            FileReadError::Denied(reason) => match reason {
                Denial::InvalidTarget => -1,
                Denial::NoGrant => -10,
                Denial::PersonalNotAuthorized => -11,
                Denial::SystemUnavailable => -12,
                Denial::OperationNotAuthorized => -13,
            },
            FileReadError::TooLarge => -18,
            FileReadError::Changed => -19,
            FileReadError::Resource(ResourceError::InvalidPath) => -1,
            FileReadError::Resource(ResourceError::UnknownDirectory) => -17,
            FileReadError::Resource(_) => -14,
        },
    }
}
fn absolute(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 4096
        && !path.contains('\0')
        && !path
            .split('/')
            .skip(1)
            .any(|p| p.is_empty() || p == "." || p == "..")
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Layout {
    version: u32,
    private_root: String,
    internal_roots: Vec<String>,
}
impl Layout {
    fn valid(&self) -> bool {
        self.version == 1
            && (1..=2).contains(&self.internal_roots.len())
            && self.internal_roots.iter().all(|p| absolute(p))
            && !(self.internal_roots.len() == 2 && self.internal_roots[0] == self.internal_roots[1])
            && absolute(&self.private_root)
            && self.private_root == format!("{}/files/access-grants", self.internal_roots[0])
    }
}
#[no_mangle]
pub extern "C" fn wex_local_read_abi_version() -> u32 {
    1
}

/// Open a trusted Host layout. Never supply config from AI/Space input.
/// # Safety
/// Buffers must be live, stable and disjoint; handle_out must be aligned and
/// writable for one u64. Caller owns memory. Invalid pointers cause UB.
#[no_mangle]
pub unsafe extern "C" fn wex_local_read_open(
    config: *const u8,
    config_len: u32,
    handle_out: *mut u64,
) -> i32 {
    if handle_out.is_null() {
        return -1;
    }
    // SAFETY: caller guarantees one live aligned writable u64.
    unsafe { handle_out.write(0) };
    if config.is_null() || config_len == 0 || config_len > 65536 {
        return -1;
    }
    // SAFETY: caller guarantees readable stable input of this bounded length.
    let bytes = unsafe { slice::from_raw_parts(config, config_len as usize) };
    guarded(|| {
        let Ok(layout) = serde_json::from_slice::<Layout>(bytes) else {
            return -4;
        };
        if !layout.valid() {
            return -4;
        }
        access(|table| {
            if table.sessions.len() >= 16 || table.next > i64::MAX as u64 {
                return -7;
            }
            let roots: Vec<_> = layout.internal_roots.iter().map(Path::new).collect();
            let session = match ReadSession::open(&layout.private_root, &roots) {
                Ok(session) => session,
                Err(reason) => return store_error(reason),
            };
            let handle = table.next;
            table.next += 1;
            table.sessions.insert(handle, session);
            // SAFETY: same caller-owned handle_out, disjoint from all input.
            unsafe { handle_out.write(handle) };
            0
        })
    })
}

/// Trusted explicit directory selection only. Does not grant access.
/// # Safety
/// Path/output/length allocations must be live, stable and disjoint for the
/// declared lengths; output_len is aligned and writable for one u32.
#[no_mangle]
pub unsafe extern "C" fn wex_local_read_register(
    handle: u64,
    path: *const u8,
    path_len: u32,
    output: *mut u8,
    output_capacity: u32,
    output_len: *mut u32,
) -> i32 {
    if output_len.is_null() {
        return -1;
    }
    // SAFETY: caller guarantees one writable aligned u32.
    unsafe { output_len.write(0) };
    if path.is_null() || output.is_null() || path_len == 0 || path_len > 4096 {
        return -1;
    }
    if output_capacity < 64 {
        return -2;
    }
    // SAFETY: caller guarantees stable readable path_len bytes.
    let bytes = unsafe { slice::from_raw_parts(path, path_len as usize) };
    let Ok(path) = std::str::from_utf8(bytes) else {
        return -1;
    };
    if !absolute(path) {
        return -1;
    }
    access(|table| {
        let Some(session) = table.sessions.get_mut(&handle) else {
            return -8;
        };
        let id = match session.register(path) {
            Ok(id) => id,
            Err(_) => return -9,
        };
        if id.len() != 64 {
            return -5;
        }
        // SAFETY: ID is exactly 64 bytes, checked capacity, disjoint output.
        unsafe {
            ptr::copy_nonoverlapping(id.as_ptr(), output, 64);
            output_len.write(64);
        }
        0
    })
}

/// Read a direct file using live grants. Host supplies current system_access.
/// # Safety
/// All pointer allocations must be live, stable and non-overlapping for their
/// declared sizes; output_len is aligned and writable for one u32. Run off UI.
#[no_mangle]
#[allow(clippy::too_many_arguments)] // Fixed C ABI with explicit buffer lengths.
pub unsafe extern "C" fn wex_local_read_bytes(
    handle: u64,
    directory_id: *const u8,
    directory_id_len: u32,
    leaf: *const u8,
    leaf_len: u32,
    system_access: u8,
    output: *mut u8,
    output_capacity: u32,
    output_len: *mut u32,
) -> i32 {
    if output_len.is_null() {
        return -1;
    }
    // SAFETY: caller guarantees one writable aligned u32.
    unsafe { output_len.write(0) };
    if directory_id.is_null()
        || leaf.is_null()
        || output.is_null()
        || directory_id_len != 64
        || leaf_len == 0
        || leaf_len > 4096
        || system_access > 1
    {
        return -1;
    }
    if output_capacity < 65536 {
        return -2;
    }
    // SAFETY: both caller-owned allocations are readable and bounded above.
    let id_bytes = unsafe { slice::from_raw_parts(directory_id, 64) };
    // SAFETY: leaf_len is bounded and caller guarantees readable bytes.
    let leaf_bytes = unsafe { slice::from_raw_parts(leaf, leaf_len as usize) };
    if !id_bytes
        .iter()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        return -1;
    }
    let Ok(id) = std::str::from_utf8(id_bytes) else {
        return -1;
    };
    let Ok(leaf) = std::str::from_utf8(leaf_bytes) else {
        return -1;
    };
    if leaf.contains('/') || leaf.contains('\0') || leaf == "." || leaf == ".." {
        return -1;
    }
    access(|table| {
        let Some(session) = table.sessions.get(&handle) else {
            return -8;
        };
        let bytes = match session.read(id, leaf, system_access == 1) {
            Ok(bytes) => bytes,
            Err(reason) => return read_error(reason),
        };
        if bytes.len() > 65536 {
            return -5;
        }
        // SAFETY: capacity at least 65536; output and length disjoint from inputs.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
            output_len.write(bytes.len() as u32);
        }
        0
    })
}

/// Busy leaves the handle live; retain it and close after outstanding work drains.
/// Success invalidates it permanently within this loaded library instance.
#[no_mangle]
pub extern "C" fn wex_local_read_close(handle: u64) -> i32 {
    access(|table| match table.sessions.remove(&handle) {
        Some(_) => 0,
        None => -8,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn contention_keeps_a_live_handle_usable_until_successful_close() {
        let base = std::env::temp_dir().join(format!("wex-read-busy-{}", std::process::id()));
        std::fs::create_dir_all(base.join("private/files")).unwrap();
        std::fs::create_dir_all(base.join("public")).unwrap();
        let config = serde_json::to_vec(&serde_json::json!({
            "version":1, "privateRoot":base.join("private/files/access-grants"),
            "internalRoots":[base.join("private")]
        }))
        .unwrap();
        let mut handle = 0;
        assert_eq!(
            unsafe { wex_local_read_open(config.as_ptr(), config.len() as u32, &mut handle) },
            0
        );
        let lock = TABLE.lock().unwrap();
        assert_eq!(wex_local_read_close(handle), -6);
        drop(lock);
        let path = base.join("public").to_str().unwrap().as_bytes().to_vec();
        let mut id = [0; 64];
        let mut len = 0;
        assert_eq!(
            unsafe {
                wex_local_read_register(
                    handle,
                    path.as_ptr(),
                    path.len() as u32,
                    id.as_mut_ptr(),
                    64,
                    &mut len,
                )
            },
            0
        );
        assert_eq!(len, 64);
        assert_eq!(wex_local_read_close(handle), 0);
        assert_eq!(wex_local_read_close(handle), -8);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn poisoned_table_rejects_every_subsequent_operation() {
        let table = Mutex::new(Table {
            next: 1,
            sessions: BTreeMap::new(),
        });
        assert_eq!(access_table(&table, |_| panic!("test table poison")), -3);
        assert_eq!(access_table(&table, |_| panic!("must not execute")), -5);
    }
    #[test]
    fn native_panic_never_unwinds_across_the_abi() {
        assert_eq!(guarded(|| panic!("test read ABI guard")), -3);
    }
}
