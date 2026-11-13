# Session resize contention fixture repair

**Date:** 2026-11-13
**Scope:** Fixture-only repair in `crates/session-runtime/tests/lifecycle.rs` for
the existing real-PTY resize/input test and the two active-session geometry
observations in the malformed-request test. No production code or other tests
were changed.

## Observed failure and source contract

The parent-reported Round 30 `scripts/check` stopped at 614 passed, 1 failed,
and 5 ignored. The failure was the `session.resize(...).unwrap()` at line 804
in `input_and_resize_reach_the_real_terminal`, which received
`SessionError::Busy`; it was not one of the snapshot unwraps.

`Session::resize` validates the size, checks that the session is running, then
uses `resize.try_lock()` to install a coalesced value before calling `wake()`.
Either the resize lock or the wake lock can return `Busy`. Therefore `Busy`
does not promise that the resize value was not installed: a wake-lock failure
can happen after installation. The Desktop owner retains and retries its
pending coalesced size on `Busy` (`crates/desktop-runtime/src/lib.rs`).
Repeating this fixture's identical size is safe because resize state is
coalesced; this reasoning does not apply to replaying raw input bytes.

`Session::snapshot` also uses nonblocking `try_lock`. The existing `wait_for`
helper already retries snapshot `Busy` within its five-second deadline,
returns a successful snapshot, fails on other errors, and reports a failed
session. The two snapshots in the malformed-request test occurred while its
session was running, so they use that existing observation path now. Joined,
post-shutdown snapshots elsewhere were left alone.

## Repair

`input_and_resize_reach_the_real_terminal` now retries only `Busy` for the
same `Size { rows: 12, cols: 45 }`, with a five-second deadline and 5 ms
between attempts. Other errors fail immediately with context. The test then
retains its geometry wait, real `stty size` assertion, exact accepted input
assertion, process-exit wait, and shutdown assertion.

The malformed-request test now obtains its initial geometry and confirms its
unchanged geometry with `wait_for`, preserving both invalid resize and invalid
input assertions. No generic retry helper, new test, production API, queue-full
retry, input replay, or forced-lock seam was added.

## Test-authoring gate

1. **Observable behavior:** The existing production-owner test proves that a
   valid resize reaches the PTY and that the exact accepted input reaches the
   real terminal and process.
2. **Credible regression:** Transient contention on either nonblocking resize
   lock can make the fixture fail before those PTY and input assertions run.
3. **Coverage boundary:** This existing real-PTY lifecycle test is the
   strongest owner-boundary proof for resize plus input. Its runtime assertions
   are preserved; no duplicate test or artificial lock interleaving is needed.
   Successful snapshot observation is already covered by the shared
   Busy-aware `wait_for` helper.
4. **Production seam:** None. The fixture uses the existing public Session
   operations and the actual PTY.

## Focused validation

- `cargo test --locked --offline -p zmux-session-runtime --test lifecycle input_and_resize_reach_the_real_terminal -- --exact` — passed (1/1).
- `cargo test --locked --offline -p zmux-session-runtime --test lifecycle malformed_requests_do_not_change_session_geometry_or_reach_input -- --exact` — passed (1/1).
- `cargo test --locked --offline -p zmux-session-runtime --test lifecycle` — passed (17/17).
- `cargo fmt --package zmux-session-runtime -- --check` — passed.
- `cargo clippy --locked --offline -p zmux-session-runtime --all-targets -- -D warnings` — passed.
- `git diff --check -- crates/session-runtime/tests/lifecycle.rs` — passed.

The parent-owned aggregate was not rerun. No commit, push, workflow, app, or
build action was performed here.

## Proof limits

These focused Linux/default-engine lifecycle checks establish the fixture
behavior at the Session real-PTY boundary. They do not establish the parent
aggregate, linked Ghostty, macOS native, hosted, or app acceptance.

Parent review retains an explicit after-read `assert_eq!` for malformed-request
geometry. Both snapshots use the existing Busy-aware successful-running read,
so snapshot contention cannot become an assertion failure and expected geometry
is compared directly instead of used as a wait predicate. The first combined
aggregate passed 763/0/7 in 257.26 seconds; final-source validation follows
this small test-quality correction.
