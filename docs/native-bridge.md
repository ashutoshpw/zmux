# Native bridge feasibility contract

`zmux-native-bridge` builds a C-callable shared/static library around the original Rust
workspace model and desktop runtime. [zmux.h](../crates/native-bridge/include/zmux.h) is
the caller contract. The native Swift/AppKit development app uses the desktop exports;
the original core exports remain available for model-only integrations.

## Ownership and execution

Create one core context and retain its process-local token. Dispatch requests on a serial
background executor; the native Swift actor serializes these calls. Never place model serialization or
session I/O on the AppKit main thread. Each dispatch returns an independently owned buffer
token. Query its byte length, copy into caller-owned writable storage, then release it.
The buffer is UTF-8 JSON without a trailing NUL; the caller supplies any string terminator.

Core/buffer tokens are opaque monotonically allocated integers, not pointers. They are not
credentials and cannot be persisted or carried between processes. They are never reused
within a process. Core destruction rejects subsequent dispatches using that token; requests
that already acquired a context may complete concurrently. Responses remain readable after
core destruction until individually released. Hosts must serialize teardown with their own
request lifecycle; generation-scoped async session callbacks are a later bridge extension.

Non-null request/destination pointers must identify valid caller-owned storage for the
call's duration. Null and length validation cannot establish that arbitrary pointers are
safe. The small unsafe implementation is confined to this bridge; core/runtime crates
forbid unsafe code. Exported operations contain unwinding Rust panics as internal errors;
process aborts or invalid foreign memory are outside that guarantee.

## Protocol version 1

Example snapshot request:

```json
{"version":1,"command":{"method":"snapshot"}}
```

Example action request:

```json
{"version":1,"command":{"method":"apply","params":{"action":"create_workspace","name":"work"}}}
```

Actions use the shared core API: `create_workspace`, `rename_workspace`, `focus_pane`,
`split_pane`, `close_pane`, `create_surface`, `select_surface`, `rename_surface`,
`close_surface`, `reorder_surface`, `move_surface`, `close_other_surfaces`, `close_workspace`
`reopen_closed`, `create_browser`, `navigate_browser` and `observe_browser`, with snake-case field names. Unknown fields and malformed
requests are rejected. Successful replies contain `version`, `ok`, `snapshot` and `effects`.
An unsuccessful protocol reply contains `ok: false` and `error.code` / `error.message`.
Error codes are `invalid_request`, `unsupported_version`, and `action_rejected`.

This model-only bridge returns `start_session`/`stop_session` effects; it does not allocate
PTYs or execute commands. The desktop coordinator applies these effects and publishes
session results through its separate exports. Core-only pane creation does not start a terminal.
This is an in-process integration API, not the final local socket/remote protocol.

## ABI statuses and limits

| Status | Meaning |
| --- | --- |
| `ZMUX_OK` | ABI operation succeeded; decode the protocol reply to learn action success |
| `ZMUX_INVALID_HANDLE` | Missing, destroyed, released, or wrong-kind token |
| `ZMUX_INVALID_ARGUMENT` | Invalid pointer/length combination or oversized request |
| `ZMUX_BUFFER_TOO_SMALL` | No bytes written; allocate the queried length and copy again |
| `ZMUX_BUSY` | Operation rejected before mutation due to contention; schedule a retry |
| `ZMUX_CAPACITY` | Context/response registry capacity reached; no action was applied |
| `ZMUX_INTERNAL` | Internal failure; outcome may be uncertain, never blindly retry a mutation |

Limits: 32 live core contexts, 16 live/reserved response buffers, 16 KiB per request and
8 MiB per response. Reserve response capacity before applying actions. Current bounded
workspace shapes fit within the response ceiling; a later shape/schema change needs to
revisit that bound. Copying an undersized buffer must never partially modify the destination.
Poisoned locks report internal failure, not retryable contention. Released buffers must not
be accessed again; double release returns an invalid-handle error.

## Checks

Run `scripts/check-native-bridge`. It compiles an independent C caller with warnings denied,
links the actual Rust shared library, and decodes its wire replies independently in Python.
It covers malformed/versioned requests, core dispatch, stale/wrong-kind handles, buffer
lifetime after core destruction, insufficient/null destinations, and buffer-capacity rejection
before mutation. It also exits and closes a real desktop terminal while retaining a foreign
host's default SIGPIPE disposition, ensuring cleanup cannot kill that host or change its
signal policy. Full Rust checks run this command through `scripts/check`.

The foundation CI workflow exercises the Rust/PTY/C foundation on Linux and macOS.
The separate [native workflow](development-builds.md) compiles the universal Swift/Rust
app and runs packaged AppKit smoke checks. Neither is a resource-budget measurement.

## Desktop integration

The additional `zmux_desktop_*` exports own a real Rust desktop coordinator and local PTYs.
Dispatch accepts versioned requests with `request_id` and `payload` (action/input/resize).
Acceptance means queued; revisioned snapshots carry model, visible terminal frames and errors.
Snapshots return OK/value=0 when unchanged. Desktop destruction joins cleanup off the UI thread.
There are at most four desktop contexts and eight terminals per desktop, including pending or
retiring processes. The pure `zmux_core_*` API retains its model-only behavior.

## Workspace model version 7

The JSON model carries `model_version: 7`; the function ABI/request envelope remains version 1.
Each pane owns ordered `surfaces` and `selected_surface`; a terminal surface owns `session_id`,
`title` and optional `cwd`. Browser surfaces have `kind: browser`, no session, a URL and a
navigation generation. File surfaces (version 7) have `kind: file`, no session or URL, and a
`file` object `{path, mode, dirty}`: an absolute canonical path, a presentation mode (`text`,
`markdown`, `image`, `pdf`) and a dirty mirror that only the native document owner may set. File
content never travels in the snapshot. Native observations update metadata without issuing another navigation. The old pane-level `session_id` field is replaced by this model.
Native UI and Rust are packaged together; older hosts must reject an unfamiliar model version.
Legacy socket pane inspection still projects the selected session for existing CLI callers.

Desktop `frames` projects only selected tabs in the active workspace. Hidden sessions stay alive;
local-control inspection sees all sessions. Moving a surface retains its IDs and process. Close
history contains descriptors, never input or process handles, and uses new IDs on restoration.
The coordinator consumes a history record only when all launches succeed; failure cleans up the
provisional group and retains history. Pure core users applying `reopen_closed` must call Rust
`finish_restore` on launch completion; the model-only C API does not orchestrate restoration.
