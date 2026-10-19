# Terminal browser links and local opener

- **Date:** 2026-10-19
- **Task:** 02-terminal-browser-links

## Objective
Route the existing `zmux open` flow and deliberate terminal-link clicks into Zmux's local
embedded browser while preserving opener overrides, terminal interaction state, and live owner
validation.

## Context
- CLI flows such as the `gh auth login` fixture may request a browser; Zmux has a local embedded
  browser.
- Original Zmux implementation only. Do not copy or translate cmux code, prose, or assets.
- Do not change the global OS browser default, user startup files, serving database/service,
  public route, tunnel, or the user's running app.
- Exercise fixture launchers only. Never perform a real GitHub login or mutate an account.
- Nested tmux/screen automatic opener requests, integration-disabled shells, explicit SSH commands,
  SSH `RemoteCommand`, and Ghostty OSC 8/opener behavior are outside the supported scope.

## Approach
Implement the accepted Rust/core, shell/CLI, and Swift/AppKit contract in
[the browser-open contract](references/browser-open-contract.md). Only explicit, validated HTTP(S)
requests and deliberate Command-click actions may open a URL. Session workers validate OSC nonce,
encoding, UTF-8, and size; desktop runtime owns canonical HTTP(S) URL and SSH-loopback policy.
Automatic URL-only events bind to the session's live pane at core admission. Keep configured
opener overrides composable, route accepted requests through the shared browser action, and
revalidate the live owner before effects. Reuse the existing `zmux open` flow; do not add a new
CLI subcommand. Unsupported commands and integrations remain out of scope.

## Steps
- [x] Trace the Rust/core, shell/CLI, and Swift/AppKit owners and record their accepted contract
  for OSC requests, owner validation, terminal hyperlinks, and the shared browser action.
- [x] Implement bounded explicit OSC open requests through the existing `zmux open` flow; preserve
  opener overrides and reject malformed, unsupported, stale, or out-of-scope requests.
- [x] Implement serialized visible-link spans and generation-aware Command-click routing through
  authoritative core revalidation; preserve ordinary click, drag, paste, selection, and mouse
  tracking behavior.
- [x] Add focused fixture-backed regressions for opener overrides, OSC requests, link clicks,
  stale owners, and SSH loopback policy; assert the resulting browser surface and URL in the
  intended pane. Helper emission alone does not prove an open effect.
- [x] Complete the final full `RUST_TEST_THREADS=2 scripts/check` run; do not use a real GitHub
  login or account.
- [ ] Complete hosted Swift/macOS compilation and mounted-view validation, plus packaged app and
  universal artifact verification.
- [x] Obtain source-independent review, fix findings, and clear review on the final cleanup-only
  delta.
- [ ] Commit and push cohesive validated slices on `main`.
- [ ] Verify a downloadable existing Zmux universal development build against its source SHA,
  without replacing the user's running app.

## Acceptance criteria
- [ ] The supported fixture browser request opens the local embedded browser; configured opener
  overrides continue to work.
- [ ] In the default `vt100` development artifact, Command-click on a visible plain HTTP(S) or
  OSC 8 link opens only its validated destination. The optional Ghostty backend supports plain
  HTTP(S) spans only; OSC 8 and automatic opener requests are unsupported there. Ordinary click,
  drag, and paste remain terminal interaction; printing or pasting a URL never opens it.
- [ ] Invalid or unsupported URLs and stale or unowned origins cannot cause browser effects.
- [ ] Opening links preserves terminal selection, mouse tracking, and PTY byte ordering.
- [ ] SSH loopback URLs are rejected unless the relevant forwarding is proven.
- [x] Focused regressions, scoped checks, and review pass without a real GitHub login.
- [ ] A verified downloadable existing Zmux universal development artifact is tied to its source
  SHA. Physical-device behavior is reported separately from fixture and hosted evidence.

Unsupported integrations above are exclusions, not acceptance results.

## References
- [Browser-open contract](references/browser-open-contract.md) — accepted wire format, ownership,
  threading, privacy, and interaction rules.
- [Related device-terminal plan](../01-device-terminal-access/plan.md) — Zmux cloud-terminal
  implementation and validation boundary.
