# F04 worker-owned terminal startup

2026-10-15. Production preparation approved after isolated resource, metadata and
kernel PTY acceptance. Preserve the current vt100 backend, frame schema, limits,
input/resize/history behavior and application engine selection.

## Scope and invariant

`Session::spawn_with_resource_and_removals` currently creates Terminal and its
initial viewport on its caller, then moves the engine to the session worker.
The accepted candidate C owner is intentionally thread-affine. Make construction,
initial export, all operations and destruction belong to the same session worker
before introducing any candidate linkage; only owned snapshots cross threads.

Move startup into `Worker::start` on the worker. Initialize the terminal and first
frame before spawning the PTY child. A one-item startup channel returns shared
snapshot state and PID, or the actual startup error; it never carries the engine.
Join an ended failed-start worker before returning failure so its owned resource
has been released. A failure to create the OS worker itself necessarily releases
the captured resource on its caller. Existing background launch coordination stays
the caller; this is not a hard deadline or interruption guarantee for native calls.

Terminal's thread affinity is an explicit representation-independent ownership
contract, enforced with a non-Send marker. It prevents a future C backend from
requiring an unsafe Send assertion or exposing thread-bound handles to the desktop.
No backend trait, test-only factory, new parser, dependency or schema is needed.

## Verification plan

- [x] Real public launch regression: missing executable returns startup error and
  releases the caller-supplied resource on the worker; red before repair, green after.
- [x] Existing production adapter/session boundary tests preserve output, queue,
  receipt, resize, history, cancellation and physical process cleanup behavior.
- [x] Focused formatting/clippy and `scripts/check` against a coherent snapshot.
- [x] Record hosted/native gaps separately from portable evidence.

The regression protects actual cleanup/thread ownership on failed construction.
Existing lifecycle tests only cover already-started sessions; no production test
seam is introduced. The accepted feasibility inputs remain unchanged.

## Implementation and local evidence

`Worker::start` now creates the terminal, first frame, PTY child and shared state
on its own worker. `StartupReady` contains only owned/shared snapshot state, PID
and the wake endpoint; the thread-affine terminal never crosses the channel.
Startup panics become explicit launch failures. Failed startup joins the ended
worker before returning, so its resource cleanup is complete; no Running Session
is fabricated. Normal execution/cleanup moved into `Worker::execute` with the
existing I/O loop, input receipts, resize coalescing and history behavior intact.

The exact final regression command is:

```sh
cargo test -p zmux-session-runtime --test lifecycle \
  failed_start_releases_its_owned_resource_on_the_session_worker --locked
```

Against unmodified production source it exits 101 at `startup cleanup ran on the
launching caller`. With the fix it passes and also requires cleanup to have
completed before the error returns. It uses the real public launch constructor
and a nonexistent executable under `/dev/null`, with no injection flag/factory.

The coherent snapshot at `/tmp/zmux-f04-worker-startup-20261015/source` contains
base `32054e16dfbb0068acd839d10c6808a93262672e` plus exactly the three owned code/test
files. Its manifest SHA-256 is
`2b1c233ae8e948f64b995a9b45dcc92ec2a22d65f43808fd59bf72e23b68077c`.
The red/green/full-check receipt is
`/tmp/zmux-f04-worker-startup-20261015/receipt.json`, SHA-256
`8ee11626b6b82428c188a6616c3d7cecf0e1863bdff7b939af4014c3d903b3f9`.
`scripts/check` passes all 250 Rust tests, workspace formatting/clippy, current
dependency-notice inventory and actual C ABI checks. Separate scoped runs pass
all 14 real-session lifecycle and nine adapter tests plus scoped Clippy.

Parent independent review found no blocking issue, and independent session/adapter
tests pass in `/tmp/zmux-worker-startup-parent-ridmfo8e`, with log
`/tmp/zmux-worker-startup-parent.log`. This establishes Linux portable/runtime
behavior, not packaged AppKit, macOS thread/process behavior or engine selection.
The new production slice still needs its hosted/native receipt. No feasibility
engine, patch or contract input changed for this work.

## Remaining engine-selection gates

This slice does not link or select Ghostty. Candidate production integration still
needs verified immutable platform static artifacts and linked-component notices,
complete production Viewport conversion and deliberate style/cursor semantics,
ordered reply/publication integration through the production session FIFO, typed
input/native fidelity and paired named-Mac workloads with numeric budgets. Native
IME/selection/VoiceOver and architecture execution remain independent gates.
