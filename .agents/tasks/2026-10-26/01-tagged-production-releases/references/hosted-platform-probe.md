# Hosted Windows and Ubuntu build probe

## Scope

User requested GitHub Actions verification and prohibited local builds. Add a
manual-only workflow at `.github/workflows/platform-build-probe.yml` to build the
existing CLI and scheduler on native Windows/Ubuntu runners. Preserve compiler
failure as a failed job; never skip incompatible crates or substitute a placeholder.
Run `scripts/check` on Ubuntu in the same workflow. Successful packages include
source/target metadata, notices and a SHA-256 checksum, and expire after 14 days.

This probe does not add a desktop UI, create an installer, push a release tag or
publish a GitHub Release. Source push and workflow dispatch are authorized by the
hosted verification request. No local Cargo/build/smoke commands are permitted.

## Results

- Source/workflow commit: `f25ffa8c33b6e41044dcee2e3101f64127ed4b5c`.
- Run: https://github.com/ashutoshpw/zmux/actions/runs/40389803745
- Dispatch: GitHub connector lacked permission (403); authenticated `gh workflow run`
  successfully dispatched `main`. Verified the run's exact `headSha` matches above.
- Windows job: https://github.com/ashutoshpw/zmux/actions/runs/40389803745/job/121807513081
  Runner `windows-2025`, target `x86_64-pc-windows-msvc`; checkout, Python and Rust
  installation succeeded. The release build failed in `zmux-session-runtime`:
  `crates/session-runtime/src/lib.rs:5` emits "The initial Zmux PTY prototype supports
  Linux and macOS only". Further compiler diagnostics include missing `std::os::unix`
  in `child.rs`/`shell_integration.rs`, missing `rustix::process`, and Unix `nix` APIs.
  No Windows binary/package was generated. This is an application portability failure,
  not an inability of GitHub Actions to provision a Windows build runner.
- Ubuntu job: https://github.com/ashutoshpw/zmux/actions/runs/40389803745/job/121807512784
  Runner `ubuntu-24.04`, target `x86_64-unknown-linux-gnu`; **success**. Full
  `RUST_TEST_THREADS=2 scripts/check` passed, including workspace tests, Clippy,
  license inventory and both foreign/native-profile-lease C ABI checks. Release
  compilation finished successfully; CLI version/help, scheduler protocol/help and a
  real `/bin/sh` PTY command executed on the hosted runner. PTY output contained
  `hosted Ubuntu PTY probe` and state `exited`.
- Artifact: https://github.com/ashutoshpw/zmux/actions/runs/40389803745/artifacts/12682616143
  Name: `zmux-binaries-ubuntu-x86_64-f25ffa8c33b6e41044dcee2e3101f64127ed4b5c`.
  Uploaded artifact size: 2,535,784 bytes. Expires 2026-11-09.
- Downloaded the artifact and verified its inner `.tar.gz` against the companion
  checksum, expected source SHA/target metadata, exact file set (`zmux`,
  `zmux-scheduler`, `build-info.json`, `LICENSE`, `THIRD_PARTY_NOTICES.md`), executable
  permissions and x86_64 ELF headers. No downloaded executable was run locally.

Inner archive SHA-256:

```text
396ce3faa63a8b68b0aba8f76f28981ca8a506e38fa4488499db3790c22e9e0c
```

The workflow's overall conclusion is **failure**, because Windows failed. Ubuntu's
successful job/artifact is independently verified; do not describe the overall run as
passing. The archive is unsigned CLI/scheduler output built on Ubuntu 24.04, without
a desktop UI/installer or older-glibc compatibility proof. There is no Windows artifact.

## Conclusion and next action

GitHub Actions can build and package the existing Ubuntu binaries. It also provisions
the Windows toolchain correctly, but the current application code cannot compile for
Windows. Use native `windows-2025` and `ubuntu-24.04` runners as configured; changing
labels cannot supply the missing Unix process, filesystem, PTY and IPC implementation.
The production release plan still needs the Windows adapter/IPC port, Windows/Linux
desktop presentation and signed platform packaging before all-platform publication.

No local builds/checks or downloaded executables were run during this follow-up.
Local validation was limited to YAML/embedded-Python syntax, documentation links,
whitespace and static inspection of the downloaded archive. All Cargo compilation,
repository checks and executable smoke tests ran inside GitHub Actions.
