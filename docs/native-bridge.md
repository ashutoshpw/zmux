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
There are at most four desktop contexts and 32 terminals per desktop, including pending or
retiring processes. The pure `zmux_core_*` API retains its model-only behavior.

`zmux_desktop_dispatch_acknowledged` executes one `action` or `windows` command and replies with
the executor's own result: `{ok:true,revision}` once the command ran, `{ok:false,error,
outcome_unknown}` when the executor refused it (for example closing a file tab whose model is
dirty). Plain dispatch acknowledges only queue acceptance, so a caller that must not act before
the command really happened (the native file panel dropping unsaved text on Don't Save) uses
this export. Any other command kind is refused before it is queued, `ZMUX_BUSY` means the queue
was full and nothing ran (retry), and `outcome_unknown: true` (two second timeout or a closing
desktop) must not be replayed. It blocks, so call it off the UI thread.

## Trusted native file channel

`zmux_desktop_file` is the one export that carries file text and previews. It is trusted
native-only: excluded from the local socket and from generic JSON dispatch, and the file
bytes never travel in snapshots, control replies or logs. Follow the `zmux_desktop_download`
pattern: call it off the UI thread, expect a reserved reply buffer (`ZMUX_CAPACITY` means
nothing ran), and never replay after a nonzero status, a timeout or `outcome_unknown: true`.
The single exception is `observe`, an idempotent absolute-state write of the view's dirty bit
that the view retries (bounded backoff) until the snapshot's `file.dirty` agrees.

Frame, in both directions: `u32 big-endian header_len | header JSON (UTF-8) | body bytes`.
The body is raw: no JSON escaping and no base64.

| Limit | Value |
| --- | --- |
| Request header | at most 16384 bytes (`ZMUX_FILE_MAX_HEADER`) |
| Request frame, every op except `save` | at most 16384 bytes, empty body |
| `save` request frame | header plus a body of at most 2097152 bytes (`ZMUX_FILE_MAX_SAVE_BODY`); frame at most 2113536 (`ZMUX_FILE_MAX_SAVE_FRAME`) |
| Reply | at most the 8 MiB response cap; the largest file reply is 4 + 16384 + 7340032 |
| Text / preview read cap | 2097152 / 7340032 bytes |
| Waits | 2 s for attach, reload, observe and detach; 5 s for save; about 3 s worst case for open (1 s probe of all operands plus the 2 s native command) |
| Worker | 8 queued jobs plus one running; one operation in flight per tab; stalled after 5 s |

A malformed, truncated, unknown-op, unknown-field, non-`save` body or oversized frame is
`ZMUX_INVALID_ARGUMENT` before any work. Every other failure is a reply with status 0 whose header
is `{"ok":false,"op_id":N|null,"outcome_unknown":bool,"error":{"code","message"}}`, plus
`"conflict":{"kind":"modified"|"deleted","disk_sha256","disk_size"}` for a save conflict, and
`"generation":N` for a save that failed with `sync_failed` after the rename landed (the written
text is on disk and is now the view's baseline at that generation; a repeat save of the same
bytes on it is the recovery, offered by the caller and never automatic). The bridge statuses
`ZMUX_BUSY` and `ZMUX_CAPACITY` mean the request did not run and may be retried with back-off;
the reply header may be as large as the 8 MiB reply cap, which a client must accept.

Ops (request headers; `owner` is `{epoch, identity_namespace, surface_id, host_id}` as returned
by attach and fenced on every call):

```json
{"op":"attach","epoch":"<uuid>","surface_id":9,"host_id":"<uuid>"}
{"op":"reload","owner":{},"discard":false}
{"op":"save","owner":{},"base_generation":3,"force":false}
{"op":"observe","owner":{},"dirty":true}
{"op":"detach","owner":{}}
{"op":"open","items":[{"kind":"path","value":"/abs/x.md"}],"focus":true,"force_mode":null}
```

`attach` and `reload` reply with the file bytes as the body (a UTF-8 BOM is removed and reported
as `bom`) and `{ok,op_id,owner,mode,size,generation,sha256,mtime_ns,writable,bom}`. `save`
replies `{ok,op_id,generation,sha256,size,mtime_ns}`; its body must be valid UTF-8 without NUL
bytes. Conflict detection compares content hashes, so a touched or identically rewritten file is
not a conflict; `force: true` overwrites a modified file or recreates a deleted one. The save is
an atomic same-directory replace that preserves the permission bits, refuses hard-linked files
and has no in-place fallback. `observe`/`detach` reply `{ok,revision}`. `open` takes up to 16
absolute paths (URLs are routed by the CLI, not this channel) and replies
`{ok,result:{ok,items:[...]}}` with a per-item result; `force_mode:"markdown"` requires text.

Error codes: `invalid_request`, `invalid_operand`, `not_found`, `permission_denied`,
`is_directory`, `not_regular_file`, `too_large`, `binary_file`, `invalid_utf8`,
`content_mismatch`, `path_not_utf8`, `io_error`, `probe_timeout`, `file_worker_unavailable`,
`busy`, `file_capacity`, `tab_capacity`, `unknown_surface`, `stale_owner`, `host_conflict`,
`timeout`, `dirty`, `stale_view`, `conflict`, `hard_linked`, `directory_not_writable`,
`path_changed`, `sync_failed`, `closed`, `rejected`.

The desktop snapshot gains `files`: `{epoch, worker:{state:"ok"|"stalled",stalled_ms},
entries:[{surface_id,state,attached,generation,disk_generation,size,mtime_ns,error_code,
last_op:{op_id,kind,state,code}}]}` with at most 16 entries and one bounded receipt each.
`state` is `unattached|in_sync|changed|deleted|unreadable`; `last_op.state` is
`pending|completed|failed|outcome_unknown`. A caller whose save timed out learns the outcome
from `last_op` for the returned `op_id`, then reloads; the save is never repeated for it. Live
reload is bounded polling (1 s per attached tab) on the file worker; a session restore rotates
`epoch` so every old owner is stale.

The macOS app consumes this channel from `FileHostClient.swift` (frame codec, `NativeBridge.file`,
decode of the shapes above) and presents it in `FilePanelView.swift`: an editable monospaced text
view, rendered Markdown (`AttributedString(markdown:)`, links limited to http, https and mailto),
an image view and a PDF view, with explicit error, empty, too-large and binary states. The view
holds the unsaved text, so its own dirty state is the truth and the model's `file.dirty` is a
mirror that the view re-sends (`observe`, bounded backoff) until a snapshot agrees. Cmd-S saves
through the channel; a `conflict` offers Cancel, Reload from Disk or Overwrite; a clean view
reloads itself on `changed`, a dirty view shows a banner; closing a dirty tab, pane, workspace,
window or quitting asks Save, Don't Save or Cancel first. `crates/native-bridge/tests/
swift_file_contract.rs` checks on Linux that the Swift `Decodable` shapes and frame limits match
what Rust actually emits; compiling and running the Swift is left to macOS CI.

## Workspace model version 7

The JSON model carries `model_version: 7`; the function ABI/request envelope remains version 1.
Each pane owns ordered `surfaces` and `selected_surface`; a terminal surface owns `session_id`,
`title` and optional `cwd`. Browser surfaces have `kind: browser`, no session, a URL and a
navigation generation. File surfaces (version 7) have `kind: file`, no session or URL, and a
`file` object `{path, mode, dirty}`: an absolute canonical path, a presentation mode (`text`,
`markdown`, `image`, `pdf`) and a dirty mirror that only the native document owner may set. File
content never travels in the snapshot. Every session this build saves is session format
version 10, whether or not it has a file surface. A build that predates version 10 refuses all of
those documents, not only the ones with file tabs, so rolling back to such a build cannot restore
any session this build saved. Documents written by earlier builds (versions 1 to 9) restore unchanged.
Native observations update metadata without issuing another navigation. The old pane-level `session_id` field is replaced by this model.
Native UI and Rust are packaged together; older hosts must reject an unfamiliar model version.
Legacy socket pane inspection still projects the selected session for existing CLI callers.

Desktop `frames` projects only selected tabs in the active workspace. Hidden sessions stay alive;
local-control inspection sees all sessions. Moving a surface retains its IDs and process. Close
history contains descriptors, never input or process handles, and uses new IDs on restoration.
The coordinator consumes a history record only when all launches succeed; failure cleans up the
provisional group and retains history. Pure core users applying `reopen_closed` must call Rust
`finish_restore` on launch completion; the model-only C API does not orchestrate restoration.
