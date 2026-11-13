# Managed SSH fixture repairs recheck

**Date:** 2026-11-13 UTC
**Baseline:** `6230bd4fb6842d15901f1c02daab7fa91dfb9dea`
**Scope:** Read-only review of `apps/cli/tests/ssh.rs`,
`apps/cli/tests/fixtures/provider-synthetic-codex.py`, and
`crates/remote-runtime/tests/connection.rs` against the baseline, using
`managed-ssh-codex-readiness-repair.md`,
`managed-ssh-codex-admission-diagnosis.md`,
`hook-event-receipt-deadline-repair.md`, and
`hook-event-receipt-timeout-diagnosis.md`.

## Verdict

No concrete defect found in the scoped fixture changes. The readiness probe
observes the real nonce-scoped lifecycle generation file before launching the
synthetic daemon or provider. The process-entry and tracked-hook markers remain
separate conditions. The receipt negative control and exact positive receipt
assertions are preserved under the corrected timing budget. No production
source changed in the reviewed diff.

## Readiness probe

- `ssh.rs:195-254` checks the lowercase 32-character nonce and exact
  `/tmp/zmux-shell-<nonce>` root, then the directory owner and `0700` mode. It
  opens `agent-lifecycle-generation` with `O_NOFOLLOW`, checks regular-file,
  owner, `0600`, single-link and 64-byte bounds, decodes ASCII, and rejects a
  nil UUID. These checks match the relevant `root_context()` and
  `generation_value()` behavior in `provider-launch.py:48-62,238-265`.
- A missing marker is polled for at most eight seconds at up to 50 ms
  intervals. The existing `screen()` wait remains ten seconds. The lifecycle
  producer atomically publishes the validated activation generation before
  sending `Active` (`claude-lifecycle-stream.py:48-105,112-142`), so the probe
  reads the producer-owned marker rather than a test-fabricated readiness
  value.
- The success output is formed by concatenated fragments and matched as an
  exact terminal line. Echoing the probe source cannot produce that line. The
  fixture call-count and app-server PID files are asserted absent before and
  after the barrier; the daemon/provider command is sent only after the success
  assertion.
- The probe is a close validation clone, not a semantic weakening: its UUID
  parsing, file constraints and missing-file behavior match the production
  reader. No mismatch with observable readiness was found.

The existing fixture does not withhold activation, so this is positive evidence
that the real producer published a valid generation before fixture launch. It
does not independently exercise the failure marker against a forced missing
generation. This remains a proof limit, not a defect requiring a fabricated
marker or new activation-control seam.

## Provider-entry and tracked-start markers

`provider-synthetic-codex.py:482-520` emits
`CODEX_PROVIDER_PROCESS_ENTERED` only inside `provider()`, before SessionStart
hook execution. A failed hook emits `CODEX_PROVIDER_TRACKING_UNAVAILABLE` and
returns; `CODEX_FIXTURE_READY` is emitted only after successful hook handling.
The SSH test waits for a terminal outcome, then separately requires both the
exact process-entry line and exact tracked-ready line. Entry alone and the
untracked fallback cannot satisfy tracked startup. Help, version, app-server,
and daemon dispatch remain outside `provider()`; the existing argument report,
call count, identity, completion, Stop-hook and fallback assertions remain.

## Hook receipt deadline

`connection.rs:463-590` scopes the 250 ms socket timeout to the wrong-generation
no-delivery assertion. The synthetic writer supplies a 1.8-second monotonic
receipt deadline, matching the production hook budget. The local `Instant`
deadline starts immediately before SSH writer execution; the valid send and
socket read each use only its remaining duration. Remote and local monotonic
origins are not compared numerically. This is the documented conservative
duration bound, subject to comparable elapsed-time rates across hosts, rather
than a synchronized cross-host deadline claim.

All exact nonce, generation, event ID, sequence, disposition, and datagram
assertions remain. The positive read retains the remaining-deadline socket
timeout. There is no post-receive wall-clock assertion that could misclassify
preemption after a valid receive. The preexisting lifecycle-frame receive is
bounded by its 15-second deadline (`connection.rs:19-45`).

## Validation and limits

- Parent-reported focused Codex/SSH coverage (three SSH cases), the one remote
  connection receipt case, the dependent local provider-hook case, formatting,
  and scoped Clippy passed. This review did not rerun them.
- The parent `scripts/check` aggregate was active during this review; it was not
  run here to avoid overlap. Hosted behavior and native app acceptance are
  outside this scoped fixture review.
- `HEAD` was the baseline commit. The reviewed diff contains only the three
  requested test/fixture files; the relevant production lifecycle files have
  no diff. No source, index, unrelated file, or validation state was changed.
