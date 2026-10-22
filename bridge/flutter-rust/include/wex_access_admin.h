#ifndef WEX_ACCESS_ADMIN_H
#define WEX_ACCESS_ADMIN_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

/* Independent ABI v1, only for trusted user settings. Never expose as AI Tool. */
uint32_t wex_access_admin_abi_version(void);

/*
 * Caller owns every allocation; pointers must be live, aligned and disjoint.
 * root: 1..4096 UTF-8 bytes, absolute private Host path without NUL.
 * Root must not come from JSON, Space, Address or an AI response.
 * request: 1..16384 bytes. output capacity: at least 65536 bytes.
 * output_len: one writable u32. No input/pointer is retained by Rust.
 * Return 0: output_len bytes of JSON, no terminator.
 * Nonzero: output_len=0 when non-null/valid; output is not modified.
 * -1 invalid arguments / -2 capacity: rejected before disk I/O.
 * -3 panic / -4 internal error: a mutation may have committed; do not retry.
 * Disk I/O must run on a worker. Timeout is not cancellation.
 * Invalid pointers, abort and OOM cannot be recovered.
 */
int32_t wex_access_admin_call(
    const uint8_t *root, uint32_t root_len,
    const uint8_t *request, uint32_t request_len,
    uint8_t *output, uint32_t output_capacity, uint32_t *output_len);

#ifdef __cplusplus
}
#endif
#endif
