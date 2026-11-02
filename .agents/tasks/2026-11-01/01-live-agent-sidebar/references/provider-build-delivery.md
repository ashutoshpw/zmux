# Provider build delivery evidence

**Updated:** 2026-11-02

**Source:** `c787b1b155cb24fb817164e4a0dd66b1316dfa88`

## Latest exact-source result

**BLOCKED: no accepted universal macOS app artifact exists for this source.** Foundation Ubuntu passed, but Foundation macOS failed in the synthetic Codex foreign-thread notification check after its exact local terminal had attached and published a connected Idle baseline. The independent full macOS development workflow passed `scripts/check`, universal compilation, terminal-selection/browser-link acceptance and packaged native rendering, then failed the sidebar smoke before the provider-row selection loop. Only the candidate and failure-diagnostics artifacts were uploaded; final app upload was skipped. No same-source retry was started.

- [Foundation checks run 41460747674](https://github.com/ashutoshpw/zmux/actions/runs/41460747674), source SHA `c787b1b155cb24fb817164e4a0dd66b1316dfa88`.
  - Ubuntu job [125328831708](https://github.com/ashutoshpw/zmux/actions/runs/41460747674/job/125328831708) passed.
  - macOS job [125328831202](https://github.com/ashutoshpw/zmux/actions/runs/41460747674/job/125328831202) failed **Check existing Rust modules and foreign C ABI**. Runner image was `macos-26-arm64` on macOS 26.6.2. Full downloaded log: `/tmp/zmux-provider-foundation-mac-c787b1b.raw`.
- [macOS development build run 41460747813](https://github.com/ashutoshpw/zmux/actions/runs/41460747813), same source SHA.
  - Both Linux scheduler jobs and `swift-typecheck` passed. The development runner image was `macos-15-arm64`.
  - Development job [125330554189](https://github.com/ashutoshpw/zmux/actions/runs/41460747813/job/125330554189) passed `scripts/check`, universal app compilation, terminal-selection/browser-link acceptance and packaged app/native rendering. `Exercise native sidebar and system status menu` failed. Full downloaded log: `/tmp/zmux-provider-development-mac-c787b1b.log`.
  - Candidate artifact [13149858349](https://github.com/ashutoshpw/zmux/actions/runs/41460747813/artifacts/13149858349) and diagnostics artifact [13150379365](https://github.com/ashutoshpw/zmux/actions/runs/41460747813/artifacts/13150379365) are not accepted delivery. The diagnostics archive was downloaded to `/tmp/zmux-provider-diagnostics-c787b1b`. The required `zmux-macos-dev-universal-c787b1b155cb24fb817164e4a0dd66b1316dfa88` final artifact was not uploaded.

## Failure diagnosis

There were two distinct hosted failures after the ingress repair; neither reports the prior `receive_timeout_setup/InvalidInput` class.

### Foundation macOS Codex fixture

`automatic_local_codex_uses_its_exact_real_desktop_terminal_route` failed at `apps/cli/tests/provider_hooks.rs:1930` with `synthetic daemon did not send the foreign thread notification`. The parent had already observed `CODEX_FIXTURE_READY`, attached the SessionStart identity to the exact existing terminal and asserted a connected Idle baseline before writing `foreign-probe` at line 1927. It then waited three seconds for `foreign-ack`; that file was not created. The full job log contains the child panic and a later local-control `Broken pipe`, but no bounded fixture-terminal dump, ingress classification or watcher-attach error. Those are absent from this timeout branch, so this is not evidence of the old ingress failure or a failed initial attach.

The exact producer is `provider-synthetic-codex.py:188-206`: its app-server daemon watches `foreign-probe`, sends a foreign `turn/started` notification, and writes `foreign-ack` only after `send_json` returns. Its outer handler silently returns for `EOFError`, `OSError`, `ValueError`, `KeyError` and JSON decode errors. The present log cannot distinguish a daemon that stopped polling from a closed WebSocket/send failure. Actionable next diagnostic: at the existing timeout, include at most 4 KiB of `terminal_output_for`, bounded daemon PID/socket liveness, and a static producer phase (`probe_seen`, `notification_sent`, or sanitized send/fixture error). Keep the foreign-thread isolation assertion and state/ownership expectations unchanged; do not infer a timeout-equality cause or widen production deadlines. The subsequent `provider_hooks.rs:216` mutex-poison failures are cascades.

The same exact Codex owner-boundary test passed in the development Mac `scripts/check` run (`/tmp/zmux-provider-development-mac-c787b1b.log`, test line 696). That makes the Foundation failure intermittent in the synthetic follow-up producer, but does not turn its failed required Foundation Mac job into a pass.

### Native sidebar/status fixture

The downloaded diagnostics report all three same-owner synthetic observations accepted: Claude Working, Codex Waiting, and OpenCode Idle, with the expected provider labels/groups visible in `zmux-sidebar-agents.png`. This is fixture/UI evidence only; it invokes no provider process or model turn.

The sidebar smoke failed before testing any provider-row selection. At `apps/macos/Sources/App.swift:975-976`, it clicked the other workspace row and waited for focus to move to that workspace; after 20 seconds the snapshot still showed `focused=6` (`model=20`, two terminal panes, session 8, PID 53906, running). The failure screenshot is `zmux-sidebar-failure.png`. The agent screenshot and failure screenshot were inspected from `/tmp/zmux-provider-diagnostics-c787b1b/`. No selection result or before/after complete surface/session/PID inventory was produced, and `native-status-menu.json` is absent, so neither provider selection nor system status menu acceptance is established. The app process and owned socket were cleanly removed after failure. The actionable next step is to add bounded diagnostics for the exact WorkspaceRowView click target/action and resulting workspace-selection request/snapshot at this assertion; retain the later exact-provider identity and complete inventory checks.

`native-smoke.json` confirms generic packaged smoke success, and the separate terminal selection/browser link and package rendering jobs passed. The build candidate metadata reports arm64/x86_64, minimum macOS 14, ad-hoc signing and `notarized: false`; it is candidate evidence, not an accepted final artifact. The development failure prevented the native sidebar/status receipt and final artifact upload.

## Verification and limits

- `RUST_TEST_THREADS=2 scripts/check r10` passed locally before this source was pushed: `/tmp/zmux-provider-support-check-r10.log`.
- Exact-source Foundation Ubuntu passed and Foundation macOS failed the synthetic foreign-thread fixture as described above. In the development workflow, both scheduler builds, Swift typecheck, the Mac `scripts/check`, universal compilation, candidate retention, terminal-selection/browser links and packaged native rendering passed; sidebar/status failed before provider-row selection. No final accepted app artifact or status-menu receipt exists.
- The downloaded candidate `build-info.json` records source SHA `c787b1b155cb24fb817164e4a0dd66b1316dfa88`, arm64/x86_64, minimum macOS 14.0, ad-hoc signing and `notarized: false`. These values do not establish acceptance because final artifact upload was skipped.
- The synthetic fixture does not prove authenticated provider turns, live model behavior, physical Mac/audio behavior, or Intel runtime behavior.
- Unrelated cloud-origin/profile-lifecycle changes were preserved.

## Previous failed-source history

- Source `de857fc8e36f09fd3744b6b157f285d2265b1b0d`: Foundation run [41453473961](https://github.com/ashutoshpw/zmux/actions/runs/41453473961) and development run [41453473964](https://github.com/ashutoshpw/zmux/actions/runs/41453473964) both reached `CODEX_FIXTURE_READY`, then lost the provider event route with an empty baseline. The later `983f426` source added the operation classification and surfaced `receive_timeout_setup/InvalidInput`; `de857fc` itself established ingress loss without identifying that operation.
- Source `8523529549da19776947d8124b03eda27aaf3a21`: Foundation run [41445123588](https://github.com/ashutoshpw/zmux/actions/runs/41445123588) failed when a Darwin Unix-socket fixture path exceeded `SUN_LEN`; development run [41445123728](https://github.com/ashutoshpw/zmux/actions/runs/41445123728) produced no accepted universal app. The short private `/tmp` fixture path was added afterward.
- Source `b1e302b47004a3d39596e426650f9248aa1e3115`: Ubuntu Foundation passed; macOS Foundation [41447789115](https://github.com/ashutoshpw/zmux/actions/runs/41447789115) and development [41447789186](https://github.com/ashutoshpw/zmux/actions/runs/41447789186) failed while the synthetic daemon cold-start consumed the bounded status-probe interval. The fixture was changed to start the daemon explicitly before adapter invocation; production status deadlines were unchanged.
- Source `5559f09b2c4b3ee12baa345dcd5ffc126ed74b3c`: Foundation [41450440329](https://github.com/ashutoshpw/zmux/actions/runs/41450440329) and development [41450440458](https://github.com/ashutoshpw/zmux/actions/runs/41450440458) reached `CODEX_FIXTURE_READY`, then failed the empty local Codex baseline assertion. No cause was visible in that source's diagnostics, so no speculative timeout or ownership change was made.
