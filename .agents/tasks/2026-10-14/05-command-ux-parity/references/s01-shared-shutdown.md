# Shared desktop shutdown

## Contract

The native app begins one Rust shutdown ticket with a 10,000ms budget and a
registered native cleanup barrier. Repeated observation or begin calls do not
renew the deadline. Runtime admission stops while trusted download completion
callbacks remain accepted. Cancellation begins across independent owners; the
final topology document is retained for persistence before sessions are removed.

Logical terminal closure is not physical cleanup. Sessions, launchers, transfers,
connection cleanup, profile/persistence workers, downloads and control listener
must finish their physical ownership before release. Native downloads preserve
an already-reserved final save, and their receipts remain charged until cleanup
is acknowledged. SSH cleanup barriers follow transfer worker release.
Connection cleanup receipts distinguish drained workers from successful deletion:
queue rejection and filesystem removal failures must be reported as cleanup issues,
not silently accepted as successful removal. Launcher completion also requires its
thread join, not merely a posted completion message.

Swift stops action/polling admission, closes browser presentation and starts
native cancellation. Image preparation stops independently. Its temporary files
remain until Rust runtime cleanup completes so transfers cannot lose input files.
Only completed native download and image cleanup acknowledge the native barrier.
The app releases its handle and terminates after physical completion.

Deadline expiry keeps owners retained. A nonmodal status window reports pending
cleanup while bounded polling continues; expiry never manufactures success or
restarts a stopped desktop. Cleanup issues are bounded and surfaced on completion.

## Verification state

Independent source review and isolated `scripts/check` pass (285 Rust tests,
strict Clippy, notices and C ABI). Owner tests exercise the shared deadline,
retained resources, native barrier, final-save race and cleanup error retention.
The shared-deadline, reservation and cleanup-ledger regressions have observed
pre-fix failures and post-fix passes. Native Swift compilation and actual
final-window close/save/reopen remain required.
No macOS lifecycle acceptance or full parity is claimed by this source change.
