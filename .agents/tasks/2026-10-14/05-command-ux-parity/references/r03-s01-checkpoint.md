# R03 native consumer: S01 reservation checkpoint

2026-10-16. Runtime/checkpoint groundwork only. Native presentation, FFI configuration,
CLI route activation and packaged macOS acceptance remain separate work.

## Persistence owner

The existing session `PersistenceWorker` is the only recovery writer. A validated
`OwnerReserved` event prepares the core attach-only surface against the latest
canonical model, validates browser profiles and enriched todo/session-file bounds,
and allocates the bounded receipt before queue submission. Refused submission
installs nothing. Accepted submission installs the already prepared candidate and
exact launch Slot infallibly on the serialized executor turn; it does not replay a
Start or launch an ordinary placeholder shell.

`IoOperation` retains a typed seal containing the full observer correlation,
numeric session/surface, launch generation, identity, document revision and exact
FIFO operation ID, plus the exact submitted document. Only its actual successful
`Saved` completion can issue the private checkpoint authorization. Authorization
also requires the original namespace, current target/Slot/generation/descriptor,
uncancelled token, unexpired startup deadline and non-shutdown state. Newer todo or
workspace edits do not invalidate the owner or replace the committed baseline with
the older saved document. Accepted write failure latches existing authoritative
persistence protection, cancels the launch, retains the canonical attach-only
failed tile and does not retry or compensate with an older file.

Shutdown drains actual outcomes but never authorizes a reservation. It flushes the
latest safely captured canonical baseline through the same FIFO. Ordinary autosave
submission pauses while the reservation checkpoint is in flight.

## Pending launch capture

Ordinary Save/Export retains its strict pending-launch guard. Inside an active
checkpoint lineage only, canonical pending ordinary shell, SSH, local tmux and
exact known Zellij descriptors may contribute to the fallback recovery document.
Every pending launch must have an exact namespace/surface/session/generation
certificate and live Slot; mutually exclusive descriptor variants are exhaustively
classified. A missing surface requires that exact known launch cancellation token.
Unknown generations, unbound active reservations, full restore and provisional
undo remain excluded. Certificates survive Slot removal until actual launch
completion. Ordinary launch failure keeps its canonical failed tile.

The safe baseline refresh after each canonical action, plus shutdown capture,
prevents a closed Zellij target from reappearing when an ordinary launch remains
pending. Capture compares the whole document, including todo data.

## Session ownership and receipts

Foreground workers keep the original absolute startup deadline. A successful
worker result must pass the same live ownership/cancellation/deadline fences again
when the coordinator consumes it; a revoked result goes to existing retirement.

The existing retired Session owner now retains an optional exact correlation,
numeric session and launch-generation tag. Normal close, failed launch cleanup,
stale completion, restore rollback and root shutdown use the same owner and actual
Session join. Observer monitor join or namespace cleanup never substitutes for
outer Session join. Live observer failure retires only that exact owned Session
and retains a failed canonical tile.

Receipts publish separate `final_frame_received`, `monitor_joined`,
`namespace_cleaned`, `session_joined` and `frontend_exit_code` evidence.
`frontend_joined` requires all cleanup evidence, CLI exit zero, owner preservation,
worker completion and no observer/transport error. Missing final evidence is
`cleanup_unknown`; unsuccessful evidence is `cleanup_failed`. Both propagate an
explicit cleanup issue to shared shutdown reporting.

Setup refusal burns reserved allocator IDs and explicitly releases only its opaque
core ticket. Attach prepares its canonical candidate before launch admission and
installs it only after successful worker spawn. Receipt-capacity refusal cannot
publish an untracked starting tile. Reopen/undo Attach effects participate in the
same retained-terminal capacity preflight as ordinary Start effects.

## Validation boundaries

- `cargo test -p zmux-desktop-runtime`: 127 passed, one intentional subprocess
  fixture ignored, before adding the final retained-capacity regression.
- `cargo test -p zmux-desktop-runtime --lib sessions::tests -- --nocapture`:
  seven passed, including the final capacity regression.
- `cargo clippy -p zmux-desktop-runtime --all-targets --locked -- -D warnings`:
  passed before the final cleanup-reporting clarification; final frozen receipt
  records its rerun.
- Private coordinator tests label synthetic correlation controls explicitly. They
  exercise seven negative authorization fences and do not claim CLI execution.
- Filesystem regressions use the actual S01 FIFO writer: accepted-versus-completed
  save, queue refusal versus accepted failure, newer todos, mixed ordinary pending
  plus target close plus shutdown, and failed ordinary tile retention.
- The real bundled-CLI/pinned-Zellij/Session integration test is owned separately
  by the local-Zellij worker slice. Its actual process/transport/input/cleanup
  receipt is required separately; these S01 tests do not replace it.
- No staged files, commits, pushes, native/FFI UI activation or production claims
  are part of this owner slice.
