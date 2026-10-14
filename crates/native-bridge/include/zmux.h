#ifndef ZMUX_H
#define ZMUX_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

enum {
    ZMUX_OK = 0,
    ZMUX_INVALID_HANDLE = -1,
    ZMUX_INVALID_ARGUMENT = -2,
    ZMUX_BUFFER_TOO_SMALL = -3,
    ZMUX_BUSY = -4,
    ZMUX_CAPACITY = -5,
    ZMUX_INTERNAL = -6
};

/* Tokens are process-local, monotonically allocated, and never pointer addresses. */
typedef struct { int32_t status; uint64_t value; } ZmuxResult;
uint32_t zmux_abi_version(void);
ZmuxResult zmux_core_create(void);
int32_t zmux_core_destroy(uint64_t core);
/* request must remain readable for length bytes. NULL is allowed only for zero length.
   The returned token owns a UTF-8 JSON response, not a NUL-terminated string.
   Workspace effects are returned for the future coordinator; no terminal is spawned. */
ZmuxResult zmux_core_dispatch(uint64_t core, const uint8_t *request, size_t length);
ZmuxResult zmux_buffer_length(uint64_t buffer);
/* destination must be writable for capacity bytes. Failure never writes partial output. */
int32_t zmux_buffer_copy(uint64_t buffer, uint8_t *destination, size_t capacity);
int32_t zmux_buffer_release(uint64_t buffer);

/* Native desktop operations: actual Rust-owned shells, actions, and terminal frames.
   Dispatch acknowledges queue acceptance. Later snapshots report command outcomes.
   Snapshot OK/value=0 means unchanged; nonzero value is an owned buffer token.
   Destroy waits for cleanup and MUST run off the native UI thread. */
ZmuxResult zmux_desktop_create(void);
ZmuxResult zmux_desktop_dispatch(uint64_t desktop, const uint8_t *request, size_t length);
ZmuxResult zmux_desktop_snapshot(uint64_t desktop, uint64_t revision);
int32_t zmux_desktop_destroy(uint64_t desktop);

#ifdef __cplusplus
}
#endif
#endif
