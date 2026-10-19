# Status

- **State:** in progress
- **Updated:** 2026-10-19

## Current delivery gate
- Source commit `53bac2cdaee02bee2b59851fd8bf110d855d11b5` is pushed to `origin/main`; `git
  ls-remote` confirmed remote parity. Unrelated older status and profile files were preserved.
- Contract, implementation, focused regressions, and source-independent review are complete.
  Review cleared after fixes for balanced URL trimming, far-away mouse-up handling, and the
  unregistered/wrong-nonce burst P2; the later `Box<SessionError>` cleanup handoff also cleared
  targeted review.
- Hosted Swift checks on both architectures passed, including native persistence, cloud
  credentials, device, relay, callback, browser-download, and file-panel checks. This does not
  establish physical-Mac behavior or a delivered browser effect.
- Focused green results: terminal adapter 18 tests; session runtime 25 tests; managed SSH 7 tests;
  core URL policy 1 test and browser integration 2 tests. The desktop integration asserts an actual
  browser surface and URL in the intended pane, including same-size viewport generation.
- Full workspace test phase: 619 passed across 84 passing suites, with 2 existing tests ignored.
  Workspace Clippy, format, license inventory, diff checks, C ABI, and native profile-lease ABI
  passed. Portable native-input checks (341), Ruby/YAML checks, whitespace checks, and the
  `TerminalSelectionSmoke` audit table with three browser-kind classifications passed; the CU14
  receipt is green.
- Final `RUST_TEST_THREADS=2 scripts/check` passed with exit 0 at
  `/tmp/zmux-terminal-browser-check-clean-20261019.log`. Dependency review covered 112 resolved
  dependencies, 76 notice texts, and 3 unbundled license packages; the latter are unchanged
  Windows/UEFI-specific baseline items, not new dependencies.
- Full macOS run [39268201123](https://github.com/ashutoshpw/zmux/actions/runs/39268201123)
  failed in the desktop Command-click regression: the valid-click assertion used a viewport
  generation captured before asynchronous shell output could settle after resize. The fixture now
  prints its ready marker and executes a silent `cat`, avoiding interactive prompt redraws.
  Production revalidation remains strict. The repaired Command-click test passed 10 consecutive
  runs; independent review found no issues. `RUST_TEST_THREADS=2 scripts/check` passed with exit 0
  at `/tmp/zmux-terminal-browser-ci-repair-check.log`, including Rust, lint, license and ABI gates.
- Focused terminal-selection run [39268249169](https://github.com/ashutoshpw/zmux/actions/runs/39268249169)
  built and signed the host app, then failed compiling its decoder probe because its input list
  omitted `FileHostClient.swift` and `FileLoadGate.swift`. Both real source files are now included;
  portable input validation passes. Independent review found no issues. Mounted interaction did
  not run on this attempt; hosted replacement runs remain necessary.
- Universal build, native interaction, and artifact verification remain pending. Physical-Mac
  behavior and provider login are unverified. After CI and artifact verification, a final delivery
  receipt commit remains; unsupported integrations remain exclusions, not acceptance results.

## Notes
- The accepted cross-layer contract is recorded in
  [browser-open-contract.md](references/browser-open-contract.md).
- The Swift/AppKit Command-click request now carries the displayed pane ID alongside namespace,
  surface, session, viewport generation, cell, and URL. The native-bridge decode fixture checks
  that pane ownership is required and that `column` uses the expected wire key.
- `git diff --check` passes for the shared checkout. At this initial checkpoint, the focused Rust
  test awaited the matching `desktop-runtime` command; that focused desktop regression now passes
  below. Native macOS compilation and mounted-view smoke remain pending.
- Terminal-adapter slice: explicit OSC open requests use a bounded one-shot queue; visible OSC 8
  and plain HTTP(S) spans carry cell coordinates; viewport generations advance on output, scroll,
  and resize. A focused resize/scroll regression covers shorter historical rows after width growth.
- An earlier `cargo test -p zmux-terminal-adapter --tests` run passed 17 tests, including all five
  focused link/open-request tests; the later 18-test run is recorded below.
- `cargo test -p zmux-terminal-adapter --features ghostty --tests` needs an explicit
  `ZMUX_GHOSTTY_ARTIFACT_ROOT`, unavailable in this environment.
- Added pinned `url = "=2.5.8"` to the desktop-runtime dependency graph and refreshed
  `THIRD_PARTY_NOTICES.md`; `scripts/license-inventory --check` passes.
- At this earlier checkpoint, shell/core regressions, full scoped checks, review, and artifact
  verification were pending. Later focused passes and review clearance are recorded above/below;
  the final full check and delivery gates remain pending.
- Ownership boundary: the session worker checks nonce, strict encoding, UTF-8, and size. Desktop
  runtime owns canonical HTTP(S) URL policy and SSH loopback rejection. URL-only OSC requests bind
  to the session's live pane at core admission; this does not claim the pane at PTY emission time.
  Click requests require `pane_id`, which core revalidates against the live session-to-pane map.
- Bounds: the request queue holds at most 8 URLs of 8 KiB each; published links are capped at 512
  spans and 64 KiB; `vt100` retains at most 64 OSC 8 targets and 256 KiB total (8 KiB per target).
  Vendored dependency provenance is recorded in `vendor/vt100/UPSTREAM.md`.
- Portable native-input checks (341), Ruby/YAML checks, and whitespace checks passed. The helper
  has no program-acknowledgment wire, so OSC emission alone is not proof of opening; the desktop
  regression must assert the actual browser surface and URL in the intended pane.
- At this earlier checkpoint, full Rust checks, native macOS compile and mounted-view smoke, SSH
  integration, review, and artifact verification were pending. Later SSH integration and review
  results are recorded above/below; final full checks and native/artifact gates remain pending.
- Session-runtime PTY regression: with `prepare_with_nonce(launch, &nonce)` and a matching
  `OpenRequestRegistration::new(nonce, Local)`, malformed and wrong-nonce OSC events preceding
  two helper-generated requests do not enter `take_open_urls()`. A valid OSC event is also ignored
  when the session has no registration, even when the caller supplies a reserved nonce variable.
  `cargo test -p zmux-session-runtime --test shell_integration -- --nocapture` passes (5 tests).
- Managed SSH regression uses generated temporary Ed25519 host/client keys, a pinned temporary
  known-hosts file, key-only authentication, and isolated loopback `sshd`. It passes the same nonce
  to `ManagedConnection::terminal_spec(&nonce)` and
  `Session::spawn_with_open_requests(..., Some(OpenRequestRegistration::new(nonce, ManagedSsh)),
  connection)`, then verifies the actual remote shell opener URL arrives through the SSH PTY.
  `cargo test -p zmux-remote-runtime --test connection -- --nocapture` passes (7 tests). Explicit
  SSH command and configured `RemoteCommand` bypass remain covered by the connection/config tests.
- The opener emits to `/dev/tty`; nested tmux/screen can consume the OSC and remains unsupported
  without a tested relay. Initial session-boundary assertions had no captured red; the later
  unregistered/wrong-nonce burst regression did capture the overflow before the fix, with the
  corrected focused green runs recorded below. The host provided `/usr/sbin/sshd`, `ssh`, and
  `ssh-keygen`, so the authenticated fixture ran without an external server or user SSH config.
- Desktop-runtime owner admission is now implemented: the worker drains each live session's
  bounded opener queue independently of frame changes, binds URL-only events to the current
  terminal pane, enforces a four-opens-per-origin rolling ten-second limit, and sends accepted
  URLs through the same `open_terminal_url` path as `zmux open`. Open requests, nonces, and rate
  history remain worker-local and are not published.
- Terminal Command-click admission rechecks the namespace, unique live terminal surface, pane and
  session mapping, running session state, current viewport generation, hit cell, and exact link URL
  against a fresh session snapshot. Same-size resize generations now trigger frame publication.
  Focused coverage also rejects wrong namespace/session/pane, stale generation, moved panes, and
  closed origins before asserting a current-generation click opens in the originating pane.
- Desktop URL admission uses `url::Url` after raw HTTP(S), credential, backslash, whitespace, and
  length checks. Managed SSH loopback/unspecified IPs and localhost names (including trailing-dot
  and IPv4-mapped IPv6 forms) fail closed because the current `ManagedConnection` API exposes no
  applicable-forward proof; local loopback remains allowed and no DNS lookup is made.
- The initial focused desktop regression reached `GH_FIXTURE_READY` and timed out waiting for the
  browser surface, confirming the missing desktop drain/routing behavior. After the core change,
  `cargo test -p zmux-desktop-runtime --test terminal_browser_links -- --nocapture` passes (2 tests),
  and `cargo test -p zmux-desktop-runtime --lib strict_http_validation_rejects_malformed_authorities_and_ssh_loopback_hosts -- --nocapture`
  passes. `cargo check -p zmux-desktop-runtime --tests`, scoped rustfmt check, and `git diff --check`
  pass. At that checkpoint, full workspace checks and native/macOS validation remained with the
  parent task; see Current delivery gate for latest results.
- Session parser overflow regression (2026-10-19): before nonce filtering, the registered and
  unregistered real-PTY burst tests both failed with `terminal open-request parser overflow or
  failure; session recovery required`. The adapter now installs the expected nonce from the
  session registration before PTY creation; missing or mismatched nonces are discarded before URL
  decoding and queue accounting, while session-side nonce matching remains defense in depth. The
  registered test confirms later matching helper requests still drain. After the fix,
  `cargo test -p zmux-terminal-adapter --tests` passes (18 tests) and
  `cargo test -p zmux-session-runtime --tests` passes (25 tests). Scoped rustfmt and
  `git diff --check` pass. The Ghostty-feature artifact limitation above remains unchanged.
- Follow-up workspace lint gate: private local-tmux and local-Zellij launch failures now box the
  retained cleanup session and unbox it at the completion handoff, preserving cleanup ownership
  while keeping the error payload small. Two literal-only `format!` calls were replaced with
  owned strings. `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo fmt --all -- --check`, and `git diff --check` pass. The final `scripts/check` rerun passed;
  see Current delivery gate.
