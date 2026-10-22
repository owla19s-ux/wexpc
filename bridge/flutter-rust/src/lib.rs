//! Separate offline runtime and trusted user-settings C ABI entrypoints.
mod access_admin;
mod local_read;
mod organization_admin;
pub use access_admin::{wex_access_admin_abi_version, wex_access_admin_call};
pub use local_read::{
    wex_local_read_abi_version, wex_local_read_bytes, wex_local_read_close, wex_local_read_open,
    wex_local_read_register,
};
pub use organization_admin::{wex_organization_admin_abi_version, wex_organization_admin_call};
use std::panic::{catch_unwind, UnwindSafe};
use std::{ptr, slice};

const REQUEST_LIMIT: usize = 16384;
const RESPONSE_CAPACITY: usize = 65536;

#[no_mangle]
pub extern "C" fn wex_runtime_abi_version() -> u32 {
    1
}

fn guarded_reply(
    input: &[u8],
    processor: impl FnOnce(&[u8]) -> Vec<u8> + UnwindSafe,
) -> Result<Vec<u8>, i32> {
    let reply = catch_unwind(|| processor(input)).map_err(|_| -3)?;
    if reply.is_empty() || reply.len() > RESPONSE_CAPACITY {
        return Err(-4);
    }
    Ok(reply)
}

/// Process one bounded request into caller-owned output memory.
///
/// # Safety
/// Non-null pointers must reference live, correctly aligned, non-overlapping
/// allocations of the declared lengths. `output_len` must be writable for one
/// u32. No pointer may be invalidated or mutated concurrently during this call.
/// This API cannot detect dangling pointers or recover from undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn wex_runtime_call(
    request: *const u8,
    request_len: u32,
    output: *mut u8,
    output_capacity: u32,
    output_len: *mut u32,
) -> i32 {
    if output_len.is_null() {
        return -1;
    }
    // SAFETY: the caller guarantees a live, aligned output_len allocation.
    unsafe { output_len.write(0) };
    if request.is_null()
        || output.is_null()
        || request_len == 0
        || request_len as usize > REQUEST_LIMIT
    {
        return -1;
    }
    if (output_capacity as usize) < RESPONSE_CAPACITY {
        return -2;
    }
    // SAFETY: the caller provides request_len readable bytes; checked bounded.
    let input = unsafe { slice::from_raw_parts(request, request_len as usize) };
    let reply = match guarded_reply(input, wex_protocol::process_json) {
        Ok(reply) => reply,
        Err(code) => return code,
    };
    // SAFETY: the caller provides at least RESPONSE_CAPACITY writable bytes,
    // disjoint from request and output_len. The temporary reply is Rust-owned.
    unsafe {
        ptr::copy_nonoverlapping(reply.as_ptr(), output, reply.len());
        output_len.write(reply.len() as u32);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::{guarded_reply, RESPONSE_CAPACITY};

    #[test]
    fn processor_panic_maps_to_native_panic() {
        assert_eq!(guarded_reply(b"request", |_| panic!("test panic")), Err(-3));
    }

    #[test]
    fn empty_and_oversized_encoded_outputs_are_internal_errors() {
        assert_eq!(guarded_reply(b"", |_| Vec::new()), Err(-4));
        assert_eq!(
            guarded_reply(b"", |_| vec![0; RESPONSE_CAPACITY + 1]),
            Err(-4)
        );
    }

    #[test]
    fn exact_capacity_is_allowed_without_adding_a_terminator() {
        assert_eq!(
            guarded_reply(b"", |_| vec![1; RESPONSE_CAPACITY])
                .unwrap()
                .len(),
            RESPONSE_CAPACITY
        );
    }
}
