#ifndef WEX_RUNTIME_H
#define WEX_RUNTIME_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ABI version 1; JSON protocol version is a separate field. */
uint32_t wex_runtime_abi_version(void);

/*
 * All non-null pointers must refer to live, non-overlapping allocations:
 * request_len readable bytes; output_capacity writable bytes; one aligned u32
 * for output_len. Caller owns all memory; Rust retains/frees no caller pointer.
 * Request length: 1..16384 bytes. Output capacity: at least 65536 bytes.
 * Return 0: output_len is the UTF-8 JSON byte count, no NUL terminator.
 * Nonzero: output_len is zero if its pointer is non-null and valid; do not read
 * output. Output is not modified on an error return.
 * -1 invalid arguments; -2 insufficient capacity; -3 caught unwind panic;
 * -4 internal encoding/size error. Abort/OOM/invalid memory are not recoverable.
 */
int32_t wex_runtime_call(
    const uint8_t *request, uint32_t request_len,
    uint8_t *output, uint32_t output_capacity, uint32_t *output_len);

#ifdef __cplusplus
}
#endif
#endif
