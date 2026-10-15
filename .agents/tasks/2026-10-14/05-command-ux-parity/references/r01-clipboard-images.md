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
Native/hosted SDK checks remain pending after push.
No macOS SDK is available on the Linux workstation. Physical clipboard/drag, remote image delivery on a
second machine, cache/resource stress, directory/custom transfers and full parity remain open.
