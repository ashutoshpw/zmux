# Provider epoch arbitration repair

**Date:** 2026-11-12
**Scope:** desktop/session source repair only; no commit, push, workflow, or app action.

## Finding and repair

`Session` mutates terminal parser state on its worker as PTY reads commit, but its
published `Snapshot` is refreshed on the frame cadence. Desktop provider admission
can therefore see the previous command epoch before the next ordinary program-status
observation consumes the prompt transition. The old OSC source lock then suppresses
the provider event even though that event belongs to the next command. Local
pre-spawn registration had the same stale-snapshot risk and could bind to the old
epoch before a later prompt observation pruned the route.

The source policy now resets a retained alert source when a worker-confirmed
`command_epoch` changes. Provider work that can admit an alert or start a route
requests a worker-ordered `ProgramStatusCheckpoint`. Session queues the barrier with
PTY work, refreshes and publishes from its own worker, and returns a nonblocking
handle. Desktop polls that handle, consumes the exact terminal's checkpoint before
resuming provider admission, and validates the same target and launch identity.
Ordinary OSC observation for that target stays held while a provider checkpoint is
pending. Local route acknowledgement waits for the checkpoint and route admission.

Checkpoint admission uses `try_lock`, so it never waits behind a PTY read on the
desktop worker. The handle has a deadline, cancellation flag, and drop cancellation;
Desktop caps pending requests at 64 and uses a two-second checkpoint deadline. A
failed route checkpoint rejects route registration so the wrapper can run untracked.
Provider alert events remain deliverable when their checkpoint fails, avoiding a
lost provider event at the cost of possible duplicate delivery in an ambiguous
failure epoch. The Session worker's deferred input/output ordering remains bounded;
overflow fails the session and requires recovery.

Remote Hook, ProviderState, and route-start frames use the same asynchronous
checkpoint path. Route end/cancel and transport loss retain their existing exact
owner/generation fences and cancellation behavior. OSC cooldown remains limited to
OSC alerts; provider event cadence is unchanged.

## Changed files

- `crates/desktop-runtime/src/program_status.rs` — command-epoch arbitration and
  checkpoint hold policy; removed a public epoch-inspection helper used only by tests.
- `crates/desktop-runtime/src/lib.rs` — bounded nonblocking local/remote checkpoint
  coordination, identity checks, acknowledgement and cancellation handling.
- `crates/session-runtime/src/lib.rs` — worker checkpoint API and ordered publication.
- `crates/desktop-runtime/src/sessions_tests.rs` — real-PTY/control route checkpoint
  coverage.
- `crates/desktop-runtime/src/shared_terminals.rs` — explicit `None` cancellation
  values for existing shared-terminal envelopes after the envelope gained route
  cancellation state.

No `remote_agent_lifecycle.rs`, `shell_integration.rs`, or
`session-runtime/tests/program_status.rs` change was made for this repair.

## Evidence

- Desktop policy owner: `RUST_TEST_THREADS=2 cargo test --locked --offline -p
  zmux-desktop-runtime --lib program_status::tests` — **11 passed**.
- The regression negative control temporarily removed the command-epoch reset and
  failed at `provider_alert_allowed(owner.target, &command_b)` because the old OSC
  lock suppressed the event. The repair was restored; the same test passes.
- Real PTY/control route owner:
  `RUST_TEST_THREADS=2 cargo test --locked --offline -p zmux-desktop-runtime
  --lib provider_route_admission_uses_live_prompt_epoch_and_survives_fast_exit_before_journal_read`
  — **1 passed**.
- Public Desktop Program Status test: **1 passed**. Provider lifecycle integration
  tests: **3 passed**.
- Strict Session Clippy:
  `cargo clippy --locked --offline -p zmux-session-runtime --all-targets -- -D
  warnings` — passed.
- Package formatting checks for `zmux-desktop-runtime` and `zmux-session-runtime`,
  plus `git diff --check` — passed.
- Combined strict Clippy for both packages reaches one scope-external warning at
  `crates/desktop-runtime/src/attention.rs:853` (`clippy::too_many_arguments` in
  `Attention::insert`). Owned-path warnings were fixed; `attention.rs` was not
  changed.

## Limits and stopping point

The policy negative control reproduces the stale-source suppression independently.
The real PTY route test exercises the checkpoint and acknowledgement against a
deliberately stale Desktop cache, but does not deterministically observe the
Session worker's private parser state in the interval before its next published
snapshot. The frame-cadence gap is established by the worker publication path;
there is no test-only seam exposing that mutable state. Checkpoint failure behavior
is explicit as described above, but this focused pass did not fault-inject Busy,
queue-full, timeout, or caller-cancellation outcomes; those paths remain
source-reviewed rather than directly exercised here.

Focused source validation is complete. Full `scripts/check`, the final source
fingerprint, and the fresh separate repair review remain with the parent. No
commit, push, or artifact acceptance is claimed.
