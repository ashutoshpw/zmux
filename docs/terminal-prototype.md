# Headless terminal feasibility prototype

The `zmux prototype` command runs a local Unix command through portable-pty 0.9.0 and
an independently written adapter around vt100 0.16.2. It emits only the final visible
screen and process status as JSON. This is a development tool, not a native terminal UI,
remote client, complete terminal protocol implementation, or production engine selection.

## Runtime behavior

A dedicated session worker owns PTY I/O, parsing, resize and child reaping. The caller
uses nonblocking input enqueueing, coalesced resize requests and cheap immutable snapshots.
Input acknowledgment means accepted into the queue, not executed by the child. Queue-full
errors are explicit; automatic retries of already accepted input are forbidden.

The worker uses readiness polling with a wake descriptor, not continuous busy polling.
Idle polling has a 250 ms maximum wait for process-state observation. Visible snapshots
are coalesced to approximately one per 16 ms while output is active; final exit publication
is immediate. These intervals are initial settings, not measured responsiveness guarantees.
Each read turn processes at most four 4 KiB chunks before checking control/state again.

- Input: 16 queued chunks of at most 4 KiB, plus one in-flight chunk.
- Geometry: at most 120 rows by 240 columns; no zero-sized terminal.
- Scrollback: at most 1,000 lines; the visible viewport alone is exported.
- Control strings: at most 16 KiB before explicit parser failure. Oversized OSC/DCS-style
  payloads are not silently truncated. Long graphics/clipboard protocols are not supported.
- Shutdown: client drop requests cancellation; CLI/worker owners may explicitly join with
  a deadline. Joining is prohibited on an eventual AppKit UI thread.

The prototype's explicit close policy forcibly terminates the owned Unix process group
and reaps its leader. A daemonized descendant that creates a separate process group is
outside this guarantee. If the leader exits while inherited slave descriptors remain,
output draining fails visibly after two seconds rather than waiting forever. Shell choice,
user confirmation and richer close policies belong to the native app phase.

## Verification boundaries

Linux integration tests exercise real PTYs, input/resize, output floods, queue saturation,
exit status and cleanup. They do not prove native GPU rendering, IME, selection, clipboard,
accessibility, macOS process behavior or remote reconnect reliability. The parser emits
plain visible text; color/style cells and a native renderer are not integrated yet.
Numerical CPU/memory/input-latency budgets remain a macOS M0 gate.
