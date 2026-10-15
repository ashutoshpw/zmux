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
/* Opt-in durable state. path is an absolute UTF-8 directory (1..4096 bytes, no NUL).
   The directory is created/validated by the persistence worker; startup snapshots expose
   loading and recovery errors. Existing create() remains nonpersistent and isolated. */
ZmuxResult zmux_desktop_create_persistent(const uint8_t *path, size_t length);
ZmuxResult zmux_desktop_dispatch(uint64_t desktop, const uint8_t *request, size_t length);
/* Executes only ABI-v1 transfer/paste_files requests with expected_session_id. The call
   waits at most two seconds for that envelope's worker acknowledgement and MUST run off
   the native UI thread. A nonzero C status or outcome_unknown=true must not be retried. */
ZmuxResult zmux_desktop_transfer_files(uint64_t desktop, const uint8_t *request, size_t length);
/* Admit one durable browser-profile Add using a UTF-8 name (1..128 bytes).
   Reserves output capacity before admission, waits at most two seconds off the UI
   thread, and returns an exact epoch/operation receipt. Receipt completion is in
   snapshots. Never replay automatically after any failure or uncertain outcome. */
ZmuxResult zmux_desktop_profile_add(uint64_t desktop, const uint8_t *name, size_t length);
/* Trusted native download lifecycle JSON, <=16384 bytes. Waits at most two seconds
   off the UI thread. Missing acknowledgement must not replay admission/publication.
   Caller-generated fenced tokens correlate uncertain outcomes with later snapshots. */
ZmuxResult zmux_desktop_download(uint64_t desktop, const uint8_t *request, size_t length);
/* Trusted host-only browser channel. Drain reserves an owned buffer before removing
   work, may wait up to one second, and MUST run off the UI thread. Never replay a
   failed drain automatically. Events wait up to two seconds and MUST also run off the UI thread.
   Events are bounded JSON (<=300000 bytes); BUSY means
   the event was not enqueued and may be retried preserving order. */
ZmuxResult zmux_desktop_browser_drain(uint64_t desktop);
int32_t zmux_desktop_browser_event(uint64_t desktop, const uint8_t *request, size_t length);
/* Trusted notification callbacks: events <=16384 bytes, actions <=32768 bytes
   (bounded reply text can expand when JSON escapes it). Both calls wait for
   executor acknowledgement and MUST run off the UI thread. Action returns an owned
   receipt buffer; do not activate a window without its successful open receipt.
   Never replay an action after an error: execution may already have occurred. */
int32_t zmux_desktop_notification_event(uint64_t desktop, const uint8_t *request, size_t length);
ZmuxResult zmux_desktop_notification_action(uint64_t desktop, const uint8_t *request, size_t length);
ZmuxResult zmux_desktop_snapshot(uint64_t desktop, uint64_t revision);
int32_t zmux_desktop_destroy(uint64_t desktop);

#ifdef __cplusplus
}
#endif
#endif
