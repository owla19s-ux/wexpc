#ifndef WEX_ORGANIZATION_ADMIN_H
#define WEX_ORGANIZATION_ADMIN_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

/* Trusted local organization CRUD. Separate from the AI/offline runtime. */
uint32_t wex_organization_admin_abi_version(void);

/*
 * Only Android/Host composition supplies root; it cannot come from AI,
 * Space labels, Address, remote messages or command JSON.
 * UTF-8 absolute root 1..4096 bytes; request JSON 1..16384 bytes.
 * output must have at least 524288 writable bytes; output_len one u32.
 * 0 returns bounded JSON response, without NUL. Nonzero resets output_len
 * but leaves output bytes untouched: -1 bad arguments, -2 buffer too short,
 * -3 panic, -4 output/internal error. -3/-4 can mean unknown write outcome:
 * never blindly retry a mutation or infer successful persistence.
 * Pointers must reference valid live, nonoverlapping allocations.
 * Caller owns all memory; disk work must run off the UI thread.
 */
int32_t wex_organization_admin_call(
    const uint8_t *root, uint32_t root_len,
    const uint8_t *request, uint32_t request_len,
    uint8_t *output, uint32_t output_capacity, uint32_t *output_len);

#ifdef __cplusplus
}
#endif
#endif
