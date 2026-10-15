# R01c authenticated regular-file transfer

Date: 2026-10-15. Original MIT implementation. Extends R01b (`ec9422e`), which passed
Linux/macOS Foundation checks (`38711025578`) and universal/native smoke (`38711037026`).
These Actions receipts are diagnostic; V01 remains false and no build link is shared.

## Implemented subset

- Each SSH terminal owns a mode-0700 private, short OpenSSH control path. The foreground
  master cannot persist/fork into an unmanaged background connection. A session resource
  keeps the channel alive through child/PTY cleanup; bounded background cleanup avoids
  filesystem waits on the core executor, with a shutdown drain and explicit errors.
- Auxiliary operations use that socket, ignore configuration and disable local commands,
  fresh authentication and network fallback. Terminal route/config/identity selects the
  authenticated channel; auxiliary operations cannot choose a different route.
- A worker opens bounded regular sources without following symlinks, streams through SSH
  with nonblocking input/output, polls cancellation/deadline and captures bounded errors.
  The remote script creates private storage, validates exact file length and cleans an
  incomplete current file. Acknowledged completed files remain reported after later failure;
  unconfirmed remote outcomes are explicitly flagged, including lost acknowledgements.
- Shared executor resolves/captures origin surface/session before launch, bounds active
  jobs/history, exposes progress/cancel, and inserts quoted paths into that exact terminal
  without submitting. Switching focus cannot change the destination; close/cancel suppress
  insertion. Local file paths use the same entrypoint without remote upload.
- Public `paste-files`, `list-uploads`, `cancel-upload`; native Finder file drop/file paste,
  destination highlight, byte progress/cancel icon and SSH endpoint labels.
- Native ABI v1, local-control v2 and model v3 retained; additive native snapshot fields.

## Verification and test ownership

Local `scripts/check` passes: 78 Rust tests, fmt, Clippy with warnings denied, notices
and C ABI. First and second reviews returned `fix-first`; the final retention-before-ACK repair passed a new fresh review (`ship`, no material findings).
Exact-SHA native compilation/smoke are pending.

The existing real CLI/socket/OpenSSH journey is extended rather than duplicated. It
removes the fixture's authorized key after login, then verifies successful shared-channel
upload, exact 100003-byte binary content and shell-decoded paths with Unicode, apostrophe
and `$(touch injected)` in the filename. An independent shell interpretation confirms the
literal path and that no injected file exists. Focus switches to a local terminal before
upload; the originating remote receives the path. Stalled upload cancellation and origin
closure suppress insertion while the core status command remains responsive. OpenSSH
`-G` tests independently enforce closed auxiliary configuration and private ownership.
Read-only public CLI checks deny new mutations before target/filesystem work.

Authoring gate: these tests protect authenticated routing, actual delivered bytes, literal
shell paths, lifecycle cancellation and authorization. Credible regressions include
reauthenticating with unavailable credentials, quoting failure, focus-based insertion or
blocking input. Existing tests cover terminal startup only. No test-only production seam
or duplicated SSH server is introduced. Session resource retention has a real production
connection owner; config resolution tests validate OpenSSH's interpretation.

## Review repairs and regression evidence

Fresh reviews found cancellation/acknowledgement ordering and cleanup defects. Three
real-sshd regressions have observed red/green results at the public CLI boundary:

- Delay outer SSH exit after the production command ACKs. Before repair, cancellation
  lost the actual completed file path. The repaired worker retains the received path
  before process EOF and suppresses insertion on cancellation.
- Close ACK output while ignoring SIGPIPE. Before repair, one remote directory remained.
  The repaired script explicitly cleans the known file/empty directory on ACK write failure.
- Send HUP immediately after printf writes the ACK. Before repair, the app reported a
  completed insertion with uncertainty false while its cleanup trap had deleted the file.
  The script now commits retention by disarming cleanup before emitting the ACK.

Cancellation stops source writes and drains acknowledgements for up to 250 ms. Started
operations without a received acknowledgement expose `remote_result_uncertain` and a
specific diagnostic; cleanup across transport loss is not promised. Acknowledged completed
paths remain reported after later failure/cancellation.

The catalog finding is repaired: `paste-files` uses the shared targeted-command route.
Task next actions are updated. A final fresh read-only review returned `ship` with no
material findings. Exact-SHA native validation remains pending.

## Remaining gates

Exact-SHA Linux/macOS tests and native compile/smoke; physical
Finder/clipboard/drag feedback and UI state review. Raw clipboard image preparation and
cleanup, directory/symlink contracts, custom uploads, password and host-key/jump routes,
real two-host transfer/disconnect/performance and all broader parity remain open.
Regular-file filesystem calls can stall inside the kernel despite O_NONBLOCK; cancellation
is checked between calls, work stays off UI/core, and shutdown reports a missed deadline. R01
and V01 stay incomplete. No artifact is accepted for user delivery from this slice.
