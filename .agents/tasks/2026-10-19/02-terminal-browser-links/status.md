# Status

- **State:** complete
- **Updated:** 2026-10-19

## Delivery

Source `8098580de9f8eb38ce6d0ce1e499c1700b604ec0` is pushed to `origin/main` with
remote parity confirmed. The implementation began in `53bac2c`; the final source includes the
native frame repair and removes all temporary diagnostic tracing. This receipt update changes
documentation only. Unrelated task and profile-lifecycle work remains untouched.

[Download the verified universal development artifact](https://github.com/ashutoshpw/zmux/actions/runs/39277527877/artifacts/12199759438).
Extract the outer artifact, then `zmux-dev-universal-8098580.zip`. The app requires macOS 14+;
app, CLI and native bridge each contain arm64 and x86_64 slices. Signing is ad-hoc, not notarized.

Archive SHA256:
`608bfdff13d457ca79f50571f5c2c66f38946937e8e1fd59e00217e190ef03b5`.

The outer and embedded build metadata agree with the source SHA. The archive checksum matches,
and both slices of each executable carry ad-hoc CodeDirectory flags. Hosted strict/deep codesign
verification passed. All 340 retained source files match both their recorded hashes and the
committed source bytes. See [artifact-verification.json](references/artifact-verification.json).

## Verification

- Final `RUST_TEST_THREADS=2 scripts/check` exited 0 at
  `/tmp/zmux-browser-native-frame-repair-check.log`: 620 passing tests across 84 passing suite
  summaries, 2 existing ignored tests, format, workspace Clippy with warnings denied, dependency
  notice inventory, C ABI, and native profile-lease ABI checks. License inventory covers 112
  packages and 76 distinct notice texts. Three missing upstream license-text warnings are unchanged
  Windows/UEFI baseline items, not newly introduced dependencies.
- The documentation delivery receipt passed independent evidence review. Its required pre-commit
  `RUST_TEST_THREADS=2 scripts/check` also exited 0 at
  `/tmp/zmux-browser-delivery-receipt-check.log`; source remained unchanged from the verified build.
- Focused hosted run [39277504464](https://github.com/ashutoshpw/zmux/actions/runs/39277504464),
  artifact `12197779210`: receipt and native result complete, 41 cases with zero mismatches.
  Source compilation, decoder compilation/contract and mounted native execution exited 0 without
  timeout. Plain HTTP and OSC 8 Command-clicks loaded the exact fixture page/title in the original
  pane; both HTTP requests were independently captured. Drag and release-only movement preserved
  selection without opening a browser. This was automated arm64-host execution.
- Full hosted run [39277527877](https://github.com/ashutoshpw/zmux/actions/runs/39277527877),
  artifact `12199759438`: universal build, native selection/link smoke against the existing package,
  broader packaged app smoke, cloud-terminal lifecycle and native sidebar/status-menu checks passed.
  The universal selection receipt is complete with all 41 cases matching and both fixture requests
  recorded. The broader smoke retains its existing exclusions: native browser downloads are false
  and save-dialog accessibility is `skipped_user_deferred`; those are not link acceptance claims.
- GitHub run/job status endpoints were stale and intermittently returned 502, even after successful
  uploads. Acceptance uses the downloaded source-bound receipts and completed job log, which records
  successful artifact finalization at 15:52:00 UTC, rather than those stale progress fields.
- Adapter tests: 18 passed. Session runtime: 25 passed. Managed SSH connection suite: 7 passed,
  including a real key-authenticated isolated loopback sshd with pinned host key. Desktop browser
  integration: 2 passed, including a helper-driven browser surface in the original pane and strict
  stale/moved/closed owner rejection. The Command-click test passed 10 consecutive runs after its
  quiet-fixture repair. URL policy and Swift request decoding checks passed.
- Independent implementation and final repair reviews found no remaining issues. No real GitHub
  login/provider action, physical user-Mac interaction, VoiceOver or Intel-host execution was done.

## Hosted findings and repairs

1. Full run `39268201123` caught a test race: interactive shell output after resize could stale the
   captured generation before the valid click. The fixture now prints its marker then executes a
   silent `cat`; production generation equality remains strict. Focused run `39268249169` built and
   signed the host app but exposed missing real FileHostClient/FileLoadGate inputs in the decoder
   probe. The input list now includes both, with retained source hashes.
2. Focused run `39271066715` compiled the probe, then failed the far mouse-up clipboard assertion.
   Full run `39271019599` was canceled to avoid redundant validation of that known failure.
   Diagnostic run `39273979536` proved event delivery and ordinary selection routing, but a zero
   viewport generation and no pending link. The automatically triggered diagnostic full run was
   intentionally canceled. All temporary event tracing was subsequently removed.
3. The actual product defect was the native poll-frame projection: `WireViewport` omitted link spans
   and viewport generation, although full core snapshots contained them. It now borrows the bounded
   links and forwards the generation on full and changed native packets; existing Swift delta merge
   retains the metadata. A real ABI regression failed before the fix at the absent-generation
   assertion, then passed exact plain/OSC 8 spans, same-size resize with no changed rows, link
   replacement and clearing. The two ABI tests use test-local setup serialization to avoid racing
   the real registry try_lock; command failures are never retried or ignored. Native smoke now
   requires exact link spans, nonzero generation and the installed callback before gestures.

Earlier portable regressions also caught wrong/missing-nonce bursts overflowing the opener queue,
balanced URL punctuation truncation, and historical short-row resize/scroll indexing. These were
repaired at their owners with red/green evidence. Nonce filtering occurs before decoding/queue
accounting; the private nonce and opener events remain absent from published snapshots.

## Scope and limits

The accepted wire, ownership and bounded resource contract is in
[browser-open-contract.md](references/browser-open-contract.md). Local and managed SSH shell
integration installs private HTTP(S) opener helpers while preserving explicit BROWSER/GH_BROWSER
and startup overrides. Desktop runtime validates URLs, live owners and rate limits, then shares the
existing browser action with deliberate Command-clicks. Plain URL output and paste never request
an automatic open. Visible plain HTTP(S) and OSC 8 spans use fresh pane/session/generation checks.

Automatic opener routing through nested tmux/screen, integration-disabled or unsupported shells,
explicit SSH commands and configured RemoteCommand is unsupported. SSH loopback/local callback URLs
are rejected because no applicable-forward proof API exists; generic OAuth callback forwarding is
not claimed. The optional Ghostty backend supports plain visible links only, not OSC 8 or automatic
opener metadata. Cloud viewers retain their existing nil-callback behavior. Physical Mac/provider
login testing remains separate from the passing fixture and hosted evidence.

Original Zmux implementation remains MIT. The minimal vt100 0.16.2 patch retains its upstream MIT
license and archive provenance in `vendor/vt100/UPSTREAM.md`; no cmux implementation was imported.
