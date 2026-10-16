#ifndef ZMUX_H
#define ZMUX_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

enum {
    ZMUX_OK = 0,
    /* Browser event only: rejected obsolete receipt; payload and downstream effects not applied. */
    ZMUX_BROWSER_EVENT_OBSOLETE = 1,
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
/* Explicit JSON host configuration (1..8192 bytes), denying unknown fields:
   {state_directory?:absolute-path,terminal_engine?:{engine:"vt100"|"ghostty",...}}.
   Ghostty requires explicit limits/reply lifetime and a matching optional build;
   unavailable/invalid selection fails before launch, with no backend fallback. */
ZmuxResult zmux_desktop_create_configured(const uint8_t *request, size_t length);
/* Ordinary envelopes <=16384 bytes. Only strict paste envelopes may exceed that
   up to99328 bytes, with parsed text <=16384 UTF-8 bytes, admitted once as one unit.
   Queue acceptance never authorizes automatic retry after partial/unknown delivery. */
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
/* Strict profile command JSON <=2048 bytes: add{name}, rename{profile,name}.
   Same bounded receipt, off-main admission and never-replay contract as profile_add. */
ZmuxResult zmux_desktop_profile_command(uint64_t desktop, const uint8_t *request, size_t length);
/* Fixed native datastore namespace; off UI/core. Status and unconstructed-token
   release use desktop=0 and survive logical Desktop shutdown. Never release a
   token after any WebKit datastore construction. */
ZmuxResult zmux_desktop_profile_lease(uint64_t desktop, const uint8_t *request, size_t length);
/* Trusted native download lifecycle JSON, <=16384 bytes. Waits at most two seconds
   off the UI thread. Missing acknowledgement must not replay admission/publication.
   Caller-generated fenced tokens correlate uncertain outcomes with later snapshots. */
ZmuxResult zmux_desktop_download(uint64_t desktop, const uint8_t *request, size_t length);
/* Trusted native window completion/key observations; token-fenced, never replayed. */
ZmuxResult zmux_desktop_window_event(uint64_t desktop, const uint8_t *request, size_t length);
/* Trusted host-only browser channel. Drain reserves an owned buffer before removing
   work, may wait up to one second, and MUST run off the UI thread. Never replay a
   failed drain automatically. Events wait up to two seconds and MUST also run off the UI thread.
   Events are bounded JSON (<=300000 bytes); BUSY means
   the event was not enqueued and may be retried preserving order. */
ZmuxResult zmux_desktop_browser_drain(uint64_t desktop);
/* ZMUX_BROWSER_EVENT_OBSOLETE is an unapplied rejection, not owner
   authentication or permission to retry. All other nonzero statuses are errors. */
int32_t zmux_desktop_browser_event(uint64_t desktop, const uint8_t *request, size_t length);
/* Trusted notification callbacks: events <=16384 bytes, actions <=32768 bytes
   (bounded reply text can expand when JSON escapes it). Both calls wait for
   executor acknowledgement and MUST run off the UI thread. Action returns an owned
   receipt buffer; do not activate a window without its successful open receipt.
   Never replay an action after an error: execution may already have occurred. */
int32_t zmux_desktop_notification_event(uint64_t desktop, const uint8_t *request, size_t length);
ZmuxResult zmux_desktop_notification_action(uint64_t desktop, const uint8_t *request, size_t length);
ZmuxResult zmux_desktop_snapshot(uint64_t desktop, uint64_t revision);
/* One process-local shutdown ticket per desktop. begin installs its deadline once;
   repeated calls never renew it or add barriers. budget_ms/wait_ms <=30000.
   Native cleanup is opt-in only when a real native owner must acknowledge completion.
   begin/status/wait return owned JSON reports, including Pending after deadline expiry.
   runtime_cleanup_complete excludes the native barrier; physical_cleanup_complete also
   requires native downloads, native acknowledgment, control and coordinator final joins.
   Only trusted completion/download-drain callbacks remain admitted after begin.
   wait runs off the UI thread; it is clamped to the existing root deadline.
   native_complete is idempotent and must follow actual physical native cleanup.
   release returns BUSY and retains the handle until physical cleanup is complete. */
ZmuxResult zmux_desktop_shutdown_begin(uint64_t desktop, uint64_t budget_ms, uint8_t native_cleanup_required);
ZmuxResult zmux_desktop_shutdown_status(uint64_t desktop, uint64_t ticket);
ZmuxResult zmux_desktop_shutdown_wait(uint64_t desktop, uint64_t ticket, uint64_t wait_ms);
int32_t zmux_desktop_shutdown_native_complete(uint64_t desktop, uint64_t ticket);
int32_t zmux_desktop_shutdown_release(uint64_t desktop, uint64_t ticket);
/* Compatibility bounded wait. Also refuses release while physical cleanup is pending. */
int32_t zmux_desktop_destroy(uint64_t desktop);

#ifdef __cplusplus
}
#endif
#endif
