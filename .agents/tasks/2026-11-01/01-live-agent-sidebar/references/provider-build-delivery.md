# Provider build delivery evidence

**Updated:** 2026-11-02  
**Source:** `8523529549da19776947d8124b03eda27aaf3a21`

## Result

**BLOCKED; no accepted universal macOS app artifact is available for this source.**

The exact-source Foundation workflow failed on its macOS job before completing the
Rust checks. The macOS development workflow passed native Swift typechecking but
was still in Rust core validation when inspected. The app artifact
`zmux-macos-dev-universal-8523529549da19776947d8124b03eda27aaf3a21` was not
uploaded. The run contains only scheduler helper and queue-layout artifacts;
these are not app-build candidates. No app ZIP checksum, packaged metadata,
binary architectures, native screenshot, or packaged signing evidence can be
accepted from this attempt.

## Exact-source workflows

- [macOS development build run 41445123728](https://github.com/ashutoshpw/zmux/actions/runs/41445123728), source SHA `8523529549da19776947d8124b03eda27aaf3a21`.
  - Linux scheduler helper jobs passed.
  - `swift-typecheck` passed.
  - The `development` job reached **Validate Rust core and foreign ABI** and
    remained in progress at the last check; no final app artifact was present.
- [Foundation checks run 41445123588](https://github.com/ashutoshpw/zmux/actions/runs/41445123588), same source SHA.
  - `foundation (macos-latest)` failed at **Check existing Rust modules and
    foreign C ABI**.
  - `foundation (ubuntu-latest)` remained in progress at the last check.

## Failure and diagnostic

The macOS Foundation job failed the test
`hooks::provider::codex::tests::existing_daemon_websocket_is_bounded_and_queues_interleaved_notifications`:

```text
apps/cli/src/hooks/provider/codex.rs:973:57
bind fake app-server socket: Error { kind: InvalidInput,
  message: "path must be shorter than SUN_LEN" }
test result: FAILED. 12 passed; 1 failed; 1 ignored
```

The fixture creates its socket at lines 967–973 from `env::temp_dir()` plus a
unique `zmux-codex-ws-<uuid>/app-server.sock` path. The hosted macOS temporary
directory makes this Unix-domain socket path exceed `SUN_LEN`. This is a test
fixture portability failure; it does not establish a production Codex adapter
failure. The exact downloaded job log is `/tmp/zmux-foundation-41445123588-macos.log`
(SHA-256 `7fc0d7bec072494d4db930ae297d63798a60b9f8de8ccf23a8334984606303e`).

This is a source/test failure, so it is not eligible for an infrastructure-only
rerun. A narrowly scoped test fixture correction should use a short unique
directory under `/tmp`, then run the required checks and start fresh workflows
for the resulting source SHA.

## Verification limits

`RUST_TEST_THREADS=2 scripts/check` r4 passed locally on Linux at
`/tmp/zmux-provider-support-check-r4.log`; this does not replace the failed
hosted macOS run or establish AppKit behavior. No artifact from this attempt is
accepted. In particular, the synthetic provider rows do not prove authenticated
Claude, Codex, or OpenCode turns; physical Mac, audible audio, and Intel runtime
behavior remain unverified.

The development workflow documents universal arm64/x86_64 packaging for macOS
14+, ad-hoc signing, and no notarization. Because this run did not complete
packaging, those properties are not verified for an app artifact here.
