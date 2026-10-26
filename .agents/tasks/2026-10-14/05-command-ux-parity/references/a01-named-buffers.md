# A01 named text buffers

Implement original MIT CLI storage and shared target delivery for `set-buffer`,
`list-buffers` and `paste-buffer`. Reference behavior was inspected at pinned cmux
`3ba653b9e5afb11d0bc578031142950d6136697e`, including command help, text/stdin
preservation and ordinary versus bracketed delivery; no implementation was copied.

The CLI owns `~/.config/zmux/buffers`: a private directory, stable exclusive lock,
bounded validated JSON, private temporary file, file sync, atomic rename and directory
sync. It preserves existing malformed/unsafe state and serializes concurrent writers.
It releases the lock before socket work. Set/list work without a running app;
stdin is read before connection. `--` preserves option-looking literal text.

Paste uses shared caller/target resolution on the desktop executor and the existing
Session input/paste owners. Read-only policy denies both delivery variants. A missing
buffer or stale target cannot send input, and an uncertain response never retries it.
Terminal paste mode controls brackets, matching native paste behavior.

## Verification

The shipped-CLI integration test fails on baseline because set-buffer is absent.
The implemented slice passes two production-boundary journeys: separate CLI processes
preserve exact Unicode/whitespace and concurrent names, reject invalid/oversized input,
retain corrupt state and refuse symlink/hardlink files; real PTYs receive exact raw or
bracketed bytes after stale-target and read-only denials. No substitute store or terminal
is used. Existing tests do not cover this previously missing command/storage path.

The isolated base `3fc2364` plus eight source/test files passed the full workspace test
suite and formatting. Strict Clippy identified a large parsed invocation; its target is
now boxed with no protocol change, and the focused journeys and strict workspace Clippy
pass again. The fixture uses a process-local counter to avoid same-clock collisions.
Offline command discovery also passes with inherited managed-terminal environment.
Notices and C ABI checks complete the required `scripts/check` stages separately.
Independent source review found no blocking findings; retained receipts live under
`target/agent-validation/buffers-*` and `/tmp/zmux-buffers-independent-source-review-20261016.json`.

## Remaining parity gates

This slice deliberately reports partial compatibility: 4096 UTF-8 bytes per buffer,
128 names and 512 KiB serialized storage are bounded local limits. Listing reports byte
counts and JSON, while the reference has grapheme-count/plain-text output. Complete
historical capacity/output compatibility, native macOS execution and wider tmux
compatibility remain open. These commands do not establish A01 or full product parity.
