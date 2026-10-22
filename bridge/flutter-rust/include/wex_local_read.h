#ifndef WEX_LOCAL_READ_H
#define WEX_LOCAL_READ_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Trusted Host composition only; handles are library-instance-local, not grants.
 * Buffers are caller-owned, live, stable, aligned and non-overlapping.
 * config/path/ID/leaf are UTF-8 bytes, without terminators.
 * open errors set handle_out=0; other output errors set output_len=0 and do
 * not write content. Allocation lengths must match actual memory.
 * config <=65536 bytes; path/leaf <=4096; ID=64 lowercase hex bytes.
 * register output_capacity >=64; bytes output_capacity >=65536.
 * read output is raw binary, including successful empty files (length zero).
 * system_access is exactly 0/1, queried by Host, never from AI input.
 * All disk calls run off UI. No cancellation or deadline is guaranteed.
 */
enum wex_local_read_status {
    WEX_READ_OK=0, WEX_READ_INVALID_ARGUMENT=-1, WEX_READ_SMALL_BUFFER=-2,
    WEX_READ_PANIC=-3, WEX_READ_INVALID_LAYOUT=-4, WEX_READ_UNAVAILABLE=-5,
    WEX_READ_BUSY=-6, WEX_READ_SESSION_LIMIT=-7, WEX_READ_UNKNOWN_SESSION=-8,
    WEX_READ_REGISTER_FAILED=-9, WEX_READ_NO_GRANT=-10,
    WEX_READ_PERSONAL_REQUIRED=-11, WEX_READ_SYSTEM_UNAVAILABLE=-12,
    WEX_READ_OPERATION_DENIED=-13, WEX_READ_FAILED=-14,
    WEX_READ_UNKNOWN_DIRECTORY=-17, WEX_READ_TOO_LARGE=-18, WEX_READ_CHANGED=-19
};
uint32_t wex_local_read_abi_version(void);
int32_t wex_local_read_open(const uint8_t *config, uint32_t config_len, uint64_t *handle_out);
int32_t wex_local_read_register(uint64_t handle, const uint8_t *path, uint32_t path_len,
    uint8_t *output, uint32_t output_capacity, uint32_t *output_len);
int32_t wex_local_read_bytes(uint64_t handle,
    const uint8_t *directory_id, uint32_t directory_id_len,
    const uint8_t *leaf, uint32_t leaf_len, uint8_t system_access,
    uint8_t *output, uint32_t output_capacity, uint32_t *output_len);
/* close Busy means NOT closed: retain the handle. Panic/open/register results
 * may be unknown; do not automatically repeat or fall back to an older session.
 * Poisoned service fails closed. Closed handles are never reused until unload.
 */
int32_t wex_local_read_close(uint64_t handle);
#ifdef __cplusplus
}
#endif
#endif
