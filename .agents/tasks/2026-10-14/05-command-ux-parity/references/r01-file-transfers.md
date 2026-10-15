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
- The original R01c worker opened bounded regular sources without following symlinks and streamed through SSH
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
Runtime `c9cd45c` passes Linux/macOS Foundation checks (`38716147517`), universal
native compilation and packaged rendering smoke (`38716147327`).

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
material findings. Exact-SHA native compilation/packaged smoke now pass at `c9cd45c`.

## Hosted Darwin repair

At `d7db8fe`, Linux Foundation checks passed but macOS runs `38715393808` and
`38715393792` failed the real upload with `Illegal fd value -1`. The filedescriptor crate
uses select(2) on Darwin and rejects POSIX-poll-style disabled placeholders. The repair
constructs a bounded vector containing only active stdout/stderr/input descriptors.
The same full real SSH journey provides the hosted red/green acceptance; it is not skipped.
Fresh focused repair review returned `ship` with no findings. Repair `c9cd45c` passes
Linux/macOS Foundation checks (`38716147517`), including the same real SSH journey.
Universal native compilation and packaged rendering smoke pass (`38716147327`).

## Remaining gates

Physical Finder/clipboard/drag feedback and UI state review. Raw clipboard image preparation and
cleanup, directory/symlink contracts, custom uploads, password and host-key/jump routes,
real two-host transfer/disconnect/performance and all broader parity remain open.
Regular-file filesystem calls can stall inside the kernel despite O_NONBLOCK; cancellation
is checked between calls, work stays off UI/core, and shutdown reports a missed deadline. R01
and V01 stay incomplete. No artifact is accepted for user delivery from this slice.

## R01e source-baseline link contract (working change)

The frozen cmux upload owners invoke SCP without recursive mode. Directories and
links to directories fail there; recursive directory transfer is not required by this
baseline. A selected symlink to an ordinary file transfers the target bytes under the
selected link name. zmux now follows that contract: the worker opens the selected path
nonblocking, validates the opened descriptor as an ordinary file, and applies the
existing file/total limits. Descriptor ownership prevents later path replacement from
substituting the source. This does not make kernel filesystem calls interruptible.

The real SSH journey verifies alias name/exact bytes, directory/directory-link/dangling
link/FIFO refusal and oversized linked target refusal. Each invalid input follows a
valid input in the selection and must fail before any remote operation. The symlink
assertion was observed failing with ELOOP before the fix and passing afterward.
Parent full verification passes (98 Rust tests plus C ABI) and fresh review found no
transfer-policy defect. Native Actions verification remains pending; this is not R01/V01 closure.

## R01f native file-URL acceptance (authored; macOS execution pending)

The existing `RemoteClipboardImageSmoke.validate` journey now continues from raw PNG
paste into real file-URL selections on the same managed SSH terminal. It reuses the
private loopback fixture, revoked login key and already authenticated connection;
there is no second SSH fixture or production transfer implementation.

A private named pasteboard offers a selected symlink URL plus a text representation.
The genuine `TerminalView.paste` path must choose the URL, preserve the link's selected
name (including space, apostrophe and UTF-8), and upload through its originating SSH
surface/session while the app selects the original local workspace. After a successful
receipt, an explicit Return lets the existing remote read loop independently identify
the inserted path. The text representation must not reach the terminal, and file URLs
must not submit the shell themselves.

The same native entrypoint then selects a directory and a symlink to that directory.
Each must produce an origin-attributed failed upload and a visible error in the real
`MainHeaderView.detailLabel`. Remote upload storage and the previously submitted shell
receipt must remain unchanged. An explicit Return after each refusal must submit an
empty line, independently proving that refusal did not leave a local pathname or text
fallback in the remote input buffer, even when viewport text wraps.

Authoring gate: these checks protect native pasteboard classification, URL/name
preservation, real dispatch/visibility and terminal input behavior. Credible failures
include resolving a selected alias to its target name, wiring file URLs to text/local
paste, routing completion to newly selected focus or hiding asynchronous refusal.
The existing `apps/cli/tests/ssh.rs` remains the primary owner for all-source preflight,
regular-file/special-file classification, transfer bytes/limits and cancellation/ACK
edge cases. These AppKit entrypoint cases cannot be exercised by that CLI test. The
native test adds no production seam and does not replace callbacks or supply receipts.

The added fixture consists of one small ordinary source, one private directory with
one small sentinel file, and regular-file/directory symlinks under the existing owned
fixture root. Filesystem work and process waits stay on detached workers. It uses the
existing 12-second subprocess/20-second observation deadlines and fixture cleanup;
the general pasteboard, user SSH configuration and user credentials are untouched.

Portable verification: `git diff --check` passes. No Swift compiler/macOS SDK is present
on the Linux workstation, so this change has no native compilation or execution
receipt yet. No pre-fix regression is claimed: this adds missing native acceptance for
implemented behavior and has not exposed a reachable product defect locally. The
parent integrates `remote_file_url_symlink` and `remote_file_url_directory_refusal`
receipt requirements into the packaged smoke after the helper source freeze.

Scope remains R01's recorded nonrecursive terminal paste behavior. Frozen public
`vm push/upload` directory/excludes/watch contracts belong to H03; this slice neither
adds recursive terminal upload nor establishes Cloud/VM transfer acceptance. Physical
Finder drag/paste, external-host and resource/stress gates remain open.

Fresh source review found no blocker. The file-URL upload is small and has no ACK
hold, so it can complete before the workspace-selection CLI command: this case does
not establish hidden-terminal completion. The separately gated image journey covers
that shared delivery race. File-URL native execution remains pending.
