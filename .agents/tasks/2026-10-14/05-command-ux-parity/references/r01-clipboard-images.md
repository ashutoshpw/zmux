# R01d clipboard image preparation and transfer ownership

Date: 2026-10-15. Original MIT implementation; cmux remains a read-only behavioral
reference. This is a subset of R01, and does not establish R01 or V01 completion.

## Behavior

- File URLs take priority over image representations, then plain text. Image bytes
  from paste or drop are decoded and encoded as a PNG on a dedicated actor, away
  from MainActor and the serial terminal-input command stream.
- Each image paste captures its original surface and session before preparation.
  The executor validates both before insertion or upload creation. Focus changes
  cannot redirect it; closing/reopening a surface cannot deliver it to a new session.
- At most four images are preparing, uploading or being cleaned up at once. The
  native header shows preparation and the existing transfer UI shows upload state.
- Inputs/encoded PNGs are limited to 32 MiB, dimensions to 16,384 and total pixels
  to 16 million. Animated/multiple-frame, corrupt and incomplete inputs fail explicitly.
  ImageIO normalizes source orientation into the PNG pixels without intentionally resizing.
- The private cache uses mode-0700 directories, mode-0600 PNGs, 256 MiB/4,096-entry
  limits and seven-day inactive retention. Ownership/type/symlink checks precede
  cleanup; exact unlink/rmdir operations never recursively delete a directory.
  A nonblocking directory lock serializes cache mutations across development instances.
- Local insertion leaves the file cached for the shell/user. Remote insertion uses
  the existing authenticated SSH upload path and quotes the returned remote path.
  It never submits the terminal command.

## Attributable receipts and cleanup

The additive `zmux_desktop_transfer_files` C function waits on a per-envelope
executor acknowledgement on a detached Swift worker. Ordinary dispatch retains its
queue-only acknowledgement. Image receipts return the exact created upload ID and
snapshot revision; no global `last_request_id` correlation or mutation retries are used.

Native code releases remote prepared sources only after `source_released` confirms
the worker has exited, or after a record has been evicted at/after its acknowledgement
revision (Rust evicts only finished workers). A cancelled delivery alone is insufficient.
Rejected admission releases the file. A missing/unreadable/timed-out receipt retains it
conservatively because its mutation outcome may be unknown. Successful desktop shutdown
drains workers before releasing remaining known remote sources; failed shutdown retains them.

## Verification boundary

The owning regression contracts are native cache/PNG/pasteboard behavior, native
view-to-shell local image insertion, Rust exact-session admission and per-call receipt
attribution, and upload-worker source release after cancellation. These complement the
existing real SSH binary-transfer fixture rather than replaying its transport assertions.

Parent `scripts/check` passes: 79 Rust tests, fmt, Clippy with warnings denied,
dependency notices and C ABI. Fresh read-only review returned `ship` with no findings.
Hosted outcomes and the initial compiler failure are recorded below.
At `c260185`, Foundation run `38719421495` passed on Linux and macOS. Native run
`38719421412` failed Swift compilation before smoke execution: POSIX-wrapper failure
literals inferred `Int`, while Darwin returns `Int32`. The same pattern was found in
preparation cleanup and smoke helpers; all six failure literals now use `Int32(-1)`.
The failed run proves no native runtime behavior; its repair passed the fresh run below.
No macOS SDK is available on the Linux workstation. Physical clipboard/drag, remote image delivery on a
second machine, cache/resource stress, directory/custom transfers and full parity remain open.


## Hosted acceptance receipt

Runtime `ee8952f37dfe3dbe145e7423a66a7a1558da6dd4` passed run `38720024342`.
Inspected executed steps: Rust core/C ABI validation, universal arm64/x86_64 Swift 6
compilation with warnings as errors, and packaged application/native rendering smoke.
Native smoke independently exercised orientation and pixel preservation, cache permissions,
symlink cleanup refusal, exact release, pasteboard precedence and real view-to-shell local
image insertion. All three new image evidence flags are true. The window capture status
remains 1; the inspected PNG is a native view render, not physical screen-capture acceptance.

Downloaded diagnostic ZIP source metadata and smoke SHA both match the runtime commit;
ZIP integrity and SHA-256 were verified:
`019f785ad699804ac7311b1c891860e9cfb25555fcecec0ccaf8c15928ccbed3`.
Foundation run `38719421495` passed on Linux/macOS at `c260185`; the subsequent repair
only changes Swift failure literals and task notes. Parent checks also pass on the repair.
Fresh substantial and compiler-repair reviews both returned `ship`.

This is not direct remote image-coordinator/physical drag/two-host/resource evidence.
R01 and V01 remain incomplete, and no new user-facing build download was shared.
