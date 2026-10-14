# Native bridge feasibility contract

`zmux-native-bridge` builds a C-callable shared/static library around the original Rust
workspace model. [zmux.h](../crates/native-bridge/include/zmux.h) is the caller contract.
This slice proves the foreign C boundary. Swift linkage, terminal drawing handles,
async session events and an AppKit app are not implemented or validated yet.

## Ownership and execution

Create one core context and retain its process-local token. Dispatch requests on a serial
background executor in an eventual Swift wrapper; never place model serialization or
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
`split_pane`, and `close_pane`, with snake-case field names. Unknown fields and malformed
requests are rejected. Successful replies contain `version`, `ok`, `snapshot` and `effects`.
An unsuccessful protocol reply contains `ok: false` and `error.code` / `error.message`.
Error codes are `invalid_request`, `unsupported_version`, and `action_rejected`.

This model-only bridge returns `start_session`/`stop_session` effects; it does not allocate
PTYs or execute commands. The future Rust workspace coordinator must apply effects and
publish session results. Do not treat creation of a pane here as evidence of a live terminal.
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
1 MiB per response. Reserve response capacity before applying actions. Current bounded
workspace shapes fit within the response ceiling; a later shape/schema change needs to
revisit that bound. Copying an undersized buffer must never partially modify the destination.
Poisoned locks report internal failure, not retryable contention. Released buffers must not
be accessed again; double release returns an invalid-handle error.

## Checks

Run `scripts/check-native-bridge`. It compiles an independent C caller with warnings denied,
links the actual Rust shared library, and decodes its wire replies independently in Python.
It covers malformed/versioned requests, core dispatch, stale/wrong-kind handles, buffer
lifetime after core destruction, insufficient/null destinations, and buffer-capacity rejection
before mutation. Full Rust checks also run this command through `scripts/check`.

The CI workflow exercises the existing Rust/PTY/C foundation on Linux and macOS when
published. Its macOS lane is not an AppKit/Swift/UI or resource-budget check. No hosted run
has been executed as part of this local implementation.
