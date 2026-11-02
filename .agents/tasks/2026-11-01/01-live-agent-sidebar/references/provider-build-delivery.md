# Provider build delivery evidence

**Updated:** 2026-11-02

**Source:** `983f426da15223dcb0da4912e6aee53a18616251`

## Latest exact-source result

**BLOCKED: no accepted universal macOS app artifact exists for this source.** Both exact-source workflows failed the same macOS Codex provider-hook fixture with the newly surfaced `receive_timeout_setup/InvalidInput` class. Foundation Ubuntu, both Linux scheduler builds, and standalone Swift typechecking passed. Development packaging and packaged acceptance were skipped after the Rust-core gate failed. This is a source-level deadline-boundary failure, not an infrastructure failure; no same-source retry was started.

- [Foundation checks run 41456641683](https://github.com/ashutoshpw/zmux/actions/runs/41456641683), source SHA `983f426da15223dcb0da4912e6aee53a18616251`.
  - Ubuntu job [125314882092](https://github.com/ashutoshpw/zmux/actions/runs/41456641683/job/125314882092) passed.
  - macOS job [125314881697](https://github.com/ashutoshpw/zmux/actions/runs/41456641683/job/125314881697) failed **Check existing Rust modules and foreign C ABI**. Full downloaded job log: `/tmp/zmux-provider-foundation-mac-983f426.log`.
- [macOS development build run 41456641596](https://github.com/ashutoshpw/zmux/actions/runs/41456641596), same source SHA.
  - Both Linux scheduler jobs and `swift-typecheck` passed.
  - Development job [125316307081](https://github.com/ashutoshpw/zmux/actions/runs/41456641596/job/125316307081) failed **Validate Rust core and foreign ABI** on the same fixture. Full downloaded job log: `/tmp/zmux-provider-development-983f426.log`.
  - Universal compilation, candidate retention, packaged terminal/native/sidebar checks, and downloadable development build upload were skipped. The artifact inventory contains only scheduler helpers and queue-layout evidence; no `zmux-macos-dev-universal-983f426da15223dcb0da4912e6aee53a18616251` artifact exists.

## Failure diagnosis

The primary failure is `automatic_local_codex_uses_its_exact_real_desktop_terminal_route`, whose child times out at `apps/cli/tests/provider_hooks.rs:139` with an empty provider snapshot. In both full macOS job logs, the bounded fixture terminal shows the synthetic daemon running and `CODEX_FIXTURE_READY`, followed by:

```text
zmux: provider ingress failure: receive_timeout_setup/InvalidInput
zmux: provider event route was lost; activity remains Unknown
zmux: Codex SessionStart tracking is not active; review the Zmux hook in /hooks. No lifecycle state is being inferred.
```

This class identifies receive-timeout setup in the local event reader as the
failed operation. The code passed each remaining absolute budget to
UnixStream::set_read_timeout. Rust rejects a zero timeout; equality at the
deadline is a source-level route to InvalidInput. The hosted diagnostics did
not record the supplied duration, so they do not independently prove that
exact equality was the operating-system failure's cause. The current repair
uses explicit nonblocking reads and poll under the same 200 ms deadline,
including zero/expired-budget rejection, and removes per-byte receive-timeout
configuration entirely. Native execution remains necessary to accept it.

In `provider-synthetic-codex.py`, the fixture prints `CODEX_FIXTURE_READY` only after its SessionStart hook subprocess returns exit code zero (`apps/cli/tests/fixtures/provider-synthetic-codex.py:295-311, 339-350`). The hook's `send_local_event` connects and writes one bounded JSON line followed by a newline (`apps/cli/src/hooks/provider.rs:195-208`). The successful fixture marker therefore confirms the producer completed its write path; it does not establish listener acceptance or decoding. The later route-lost message follows `listener_lost`, and no watcher attach error appears (`apps/cli/src/hooks/provider.rs:600-635, 691-698`), so the Codex daemon snapshot and Desktop admission are downstream and not implicated by this failure.

The repair preserves byte/queue bounds and ownership/generation assertions.
Existing real socket/trickle and Desktop provider tests cover those observable
contracts. No test-only clock seam or predicate-only equality test was added.
The failed native owner-boundary test must pass before artifact acceptance.

Poisoned-mutex failures after the primary fixture panic are cascades, not separate defects.

## Verification and limits

- `RUST_TEST_THREADS=2 scripts/check r9` passed locally before this source was pushed: `/tmp/zmux-provider-support-check-r9.log`.
- Exact-source Foundation Ubuntu, both development scheduler builds, and standalone Swift typechecking passed. The Foundation macOS job and macOS development Rust-core gate failed the same fixture with the same ingress class; app packaging and packaged smoke did not execute.
- The development artifact inventory contains only scheduler helpers and queue-layout evidence. No accepted app ZIP, app checksum, packaged source/build metadata, architecture/minimum-OS receipt, signing/notarization receipt, native sidebar/status/selection evidence, or sidebar screenshot exists for this source.
- The synthetic fixture does not prove authenticated provider turns, live model behavior, physical Mac/audio behavior, or Intel runtime behavior.
- Unrelated cloud-origin/profile-lifecycle changes were preserved.

## Previous failed-source history

- Source `de857fc8e36f09fd3744b6b157f285d2265b1b0d`: Foundation run [41453473961](https://github.com/ashutoshpw/zmux/actions/runs/41453473961) and development run [41453473964](https://github.com/ashutoshpw/zmux/actions/runs/41453473964) both reached `CODEX_FIXTURE_READY`, then lost the provider event route with an empty baseline. This source surfaced `receive_timeout_setup/InvalidInput`, locating the failure at receive-timeout setup as described above.
- Source `8523529549da19776947d8124b03eda27aaf3a21`: Foundation run [41445123588](https://github.com/ashutoshpw/zmux/actions/runs/41445123588) failed when a Darwin Unix-socket fixture path exceeded `SUN_LEN`; development run [41445123728](https://github.com/ashutoshpw/zmux/actions/runs/41445123728) produced no accepted universal app. The short private `/tmp` fixture path was added afterward.
- Source `b1e302b47004a3d39596e426650f9248aa1e3115`: Ubuntu Foundation passed; macOS Foundation [41447789115](https://github.com/ashutoshpw/zmux/actions/runs/41447789115) and development [41447789186](https://github.com/ashutoshpw/zmux/actions/runs/41447789186) failed while the synthetic daemon cold-start consumed the bounded status-probe interval. The fixture was changed to start the daemon explicitly before adapter invocation; production status deadlines were unchanged.
- Source `5559f09b2c4b3ee12baa345dcd5ffc126ed74b3c`: Foundation [41450440329](https://github.com/ashutoshpw/zmux/actions/runs/41450440329) and development [41450440458](https://github.com/ashutoshpw/zmux/actions/runs/41450440458) reached `CODEX_FIXTURE_READY`, then failed the empty local Codex baseline assertion. No cause was visible in that source's diagnostics, so no speculative timeout or ownership change was made.
