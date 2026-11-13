# Managed SSH pre-Hello EOF evidence recheck

**Date:** 2026-11-13
**Base:** `2efb76d9bcbb9e0a00015602b8bcaf81c00920de`
**Scope:** Read-only review of the uncommitted `remote_agent_lifecycle.rs` change against the pre-Hello diagnosis and evidence. No tests, builds, app actions, or implementation edits were made. This file is the only change made for this recheck.

## Verdict

The bounded stderr capture, closed classification, and retained-child status observation are source-consistent with the diagnostic scope. Do not accept this slice yet: the disabled-stderr descriptor path can fail on macOS when another poll iteration is needed after stderr EOF. The current real SSH fixtures close stdout and stderr together and do not exercise that interleaving.

## Finding

### P2 — macOS poll rejects the disabled stderr fd

In `crates/desktop-runtime/src/remote_agent_lifecycle.rs:801-805`, stderr EOF changes its `pollfd.fd` to `-1`, but `descriptor_count` still includes it at line 822. The pinned `filedescriptor = 0.8.3` macOS implementation uses `select`, adds every entry to its exception set, and rejects negative descriptors (`check_fd` returns `IllegalFdValue`). Linux `poll(2)` ignores a negative descriptor, but this macOS shim does not.

If stderr reaches EOF while stdout remains open or has more than the current four-read quantum pending, the next loop iteration passes `-1` to the macOS shim and returns a poll error before continuing stdout processing. This can prematurely lose a managed SSH lifecycle stream on the primary macOS target. The source review establishes the invalid descriptor path; it does not establish that this ordering caused the earlier hosted failure. Exclude the closed fd from the actual slice passed to `poll`, keeping stdout and conditional stdin indices correct.

## Accepted source behavior

- Both child output pipes are nonblocking. Stderr retains no more than 4 KiB before valid Hello; each ready pipe has a four-read, 4 KiB-buffer quantum, excess stderr is drained and discarded, and the existing stdout quantum remains four reads. When both are ready, service order alternates.
- Capture is cleared when valid Hello is accepted and later stderr is drained with retention disabled. Only pre-Hello stdout EOF reaches the additional class/status diagnostic, after cancellation and handshake-deadline checks. Cancellation and deadline outcomes keep their prior messages; post-Hello EOF keeps the prior `hello`/`active` message.
- Status is observed for this retained child's PID through `observe_child(child.id(), false)`. The helper uses `waitid` with `WNOWAIT`; this path does not reap or mark the owner reaped. Teardown still owns the child and uses the existing process-group cleanup, bounded synchronous reap, and deferred-reaper path. The existing guard against signaling after reap is unchanged.
- The fixed classifier emits only one of the five closed labels. The formatted diagnostic contains no raw stderr, command arguments, host/path, nonce, token, or provider payload.

## Test and evidence review

The existing evidence note records that the regression first failed on the old generic result (`managed SSH lifecycle stream closed (hello=false, active=false)`) at the missing-class assertion, and that the owner module later passed 10 tests with formatting and strict Clippy. I did not rerun those commands, as requested.

The real loopback SSH fixture writes a canary and a known authentication marker, then 1 MiB of additional stderr and exits remotely with 37. It asserts the class, absence of canary/raw marker, completion before eight seconds, and an allowed numeric-or-unknown **local SSH child** status. It correctly does not claim that the remote exit code must be observable as the local child's status. A separate owned-child test asserts exit 23 and SIGTERM 15 while `reaped` remains false; the existing test still verifies that group signaling is refused after reap.

The Hello-then-close peer writes a pre-Hello canary, sleeps 500 ms, then sends Hello and Active, writes another canary, and closes. The delay encourages the owner to drain stderr but is not a synchronization proof that the pre-Hello canary entered the retained buffer. The observable post-Hello contract is still exercised: the real owner accepts Active and returns its existing post-Hello EOF message without either canary or diagnostic labels. The source explicitly clears the buffer at Hello; proving the internal clear separately would require observing private state and is not warranted by a test-only production seam or predicate test.

`import time` at `PROVIDER_SEQUENCE_GAP_PEER` (line 1243) is unused by that fixture; its body has no `time` call. It is an unnecessary test-fixture import with no runtime effect (P3 cleanup only). The 500 ms delay is in the separate `HELLO_THEN_CLOSE_PEER`, where `time` is used.

The previously reviewed CLI fixture repair remains present: Codex uses `. <setup> && command codex ...` and OpenCode uses `. <setup> && opencode ...` in `apps/cli/tests/ssh.rs`. That separate P2 was not broadly re-reviewed here.

## Proof limits

This recheck does not run tests or builds and does not identify the cause of the macOS 15 hosted EOF. The recorded 1 MiB case is synthetic drain/privacy evidence, not a reproduction of that hosted failure. A fresh hosted experiment remains the evidence needed to determine whether the original issue recurs and under which conditions.
