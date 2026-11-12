# Parent repairs from integrated review

## Malformed duplicate ID

The existing P05 protocol test now includes malformed duplicate values that also
exceed total ID length, segment length and depth limits. Each case keeps the
preceding valid `build/test` ID, changes that record's state and preserves root.
The first new case failed on the pre-fix parser with the intended `Applied`
assertion. Moving the malformed-value skip before ID-specific bounds fixed it;
valid over-limit IDs still fail the existing P03 hard-limit/atomicity contract.

- Pre-fix focused P05: 1 failed, 9 filtered, exit 101.
- Post-fix shared protocol suite: 10 passed, exit 0.
- `cargo clippy --locked --offline -p zmux-program-status --all-targets -- -D warnings`: passed.

## Ghostty close after permanent engine abort

Session Kill shutdown aborts the engine before final `terminal_closed` publication.
Ghostty's failed-owner check rejected the latter even though it only updates the
committed Rust ProgramStatusStore. The close method now permits that metadata-only
operation after abort and still rejects an outstanding staged batch. No C engine
access, failed-owner reuse or cancelled batch publication is introduced.

The bootstrap/session test owner is extending the shared explicit-close fixture
under the exact linked real-PTY test selected by hosted linkage acceptance. This
host has no accepted fresh linked library: neither pre-fix linked execution nor
post-fix linked runtime acceptance is claimed here. The hosted run remains required.

## Full producer recipe fixture

The artifact recheck found no implementation defect, but its digest fixture only
modeled `recipe` and headers. It now also models `ghostty`, `zig`, ordered patch
hashes and developer/toolchain identity. The existing negative-control test changes
each sibling component while retaining the previously accepted digest, protecting
against a future validator that hashes only a subset of the producer recipe.

After the final fixture change, `python3 -B -m unittest discover -s
tools/terminal-engine-artifacts -p 'test_*.py' -q` ran 44 tests total, with
43 passed and one expected Apple-linker skip. Each changed recipe leaf retains
its production data shape, and the negative control requires the recipe-digest
rejection message rather than an unrelated input-schema failure.

## Repair review test-quality follow-up

The completed repair recheck found no runtime defect. Parent removed the unit
test's requirement that embedded bootstrap source remain above 128 KiB, retaining
byte-for-byte decoding and the encoded-size budget. The actual exec/SSH regression
already captures the former E2BIG failure. The shared Session scenario now asserts
that final id `last` has a working Report transition followed by its ChildExit
retirement. The explicit-close helper name now claims only committed-status
clearing, since that fixture does not stage a canceled frame.

`RUST_TEST_THREADS=2 cargo test --locked --offline -p zmux-session-runtime --lib
--test program_status` passed 1 unit and 5 real-PTY Program Status tests after these
changes. The linked engine execution remains a hosted gate.

An isolated Cargo harness with the same path dependencies and vendored patches
removed only the final `id=last` OSC from the child fixture. It failed at the
intended `the final PTY write's OSC report was not parsed` assertion; the repository
source was unchanged. Receipt: `/tmp/zmux-osc7501-final-drain-negative.log`.

## Shared protocol linkage source evidence

Parent found the linkage receipt's source walker covered Ghostty, the terminal
adapter and optional Session, but omitted their mandatory shared `program-status`
crate. The existing release-input test was extended to require its implementation
and manifest hashes; it failed before the input-closure repair. The shared crate
is now walked for both linkage routes. The artifact suite again ran 44 tests total,
43 passed and one expected Apple-linker skip. Historical library/source pins were
unchanged; this only expands current Rust consumer evidence.

## Notification metadata argument grouping

The combined Clippy gate exposed `Attention::insert` above the argument-count
limit after adding OSC sound metadata. Parent grouped the existing agent and
sound metadata options into one private `NotificationMetadata` argument. The
source-specific values and serialization remain the same; no lint suppression
or new test-only API was introduced. The complete workspace gate is running.

## Checkpoint cancellation test contract

The new real-PTY checkpoint target passed six tests and strict Clippy. Parent
narrowed one assertion to permit an immediate explicit Cancelled result as well
as a pending handle; neither path permits a successful snapshot before blocked
work resumes. This avoids requiring cancellation notification to be delayed by
an implementation detail. The final aggregate check must cover this small test
edit together with the pending read-order repair.

The timed-out checkpoint handle is now dropped immediately, matching Desktop's
terminal-result handling. The test still requires the explicit timeout and
rejects premature success; it no longer requires a second Cancelled error after
a terminal timeout. The separately cancelled handle remains observed through
retirement, and a fresh checkpoint still must succeed after the PTY resumes.
