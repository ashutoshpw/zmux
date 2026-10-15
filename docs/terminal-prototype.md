# Headless terminal feasibility prototype

The `zmux prototype` command runs a local Unix command through portable-pty 0.9.0 and
an independently written adapter around vt100 0.16.2. It emits only the final visible
screen and process status as JSON. This is a development tool, not a native terminal UI,
remote client, complete terminal protocol implementation, or production engine selection.

## Runtime behavior

A dedicated session worker owns PTY I/O, parsing, resize and child reaping. The caller
uses nonblocking input enqueueing, coalesced resize requests and cheap immutable snapshots.
Ordinary input acknowledgment means accepted into the queue, not executed by the child.
The optional write-receipt path used for notification replies reports local PTY acceptance,
zero-byte failure, partial write or uncertainty under an absolute deadline. It never proves
child or remote consumption. Queue-full errors are explicit; automatic retries of already
accepted or uncertain input are forbidden.

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
  a deadline. Joining is prohibited on the AppKit UI thread and workspace executor.

The prototype's explicit close policy forcibly terminates the owned Unix process group
and reaps its leader. A daemonized descendant that creates a separate process group is
outside this guarantee. If the leader exits while inherited slave descriptors remain,
output draining fails visibly after two seconds rather than waiting forever. Native lifecycle and recovery behavior is tracked separately from this prototype command.

## Verification boundaries

Portable integration tests exercise real PTYs, input/resize, output floods, queue
saturation, write receipts, exit status and cleanup. GitHub Actions also runs them on
macOS. The adapter now exports styled cell runs, cursor and mode state, bounded
scrollback, title/cwd metadata and attention signals to an AppKit renderer. Packaged
macOS smoke checks exercise selected native journeys; they do not establish complete
protocol, physical IME/VoiceOver, graphics or remote reconnect parity.

Measured CPU/memory/input-latency budgets and the production engine decision remain
open under F04 in the command/UX parity task. The current timing constants and queue
limits are resource bounds, not measured performance guarantees.
