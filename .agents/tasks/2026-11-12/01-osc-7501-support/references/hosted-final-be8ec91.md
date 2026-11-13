# Hosted acceptance check for `be8ec91`

**Date:** 2026-11-13 UTC
**Source:** `be8ec912a3b745c328f66fd196d19ddc59920845` on `main`
**Verdict:** The fresh Ghostty linkage gate passed its requested ARM64 and dual-target compile checks. Foundation macOS failed in an unrelated SSH upload integration test. The universal app workflow was still running when the failure was collected; no final app artifact is accepted here.

## Source identity and retained evidence

- Local `HEAD`, `origin/main`, and the three new workflow heads resolve to `be8ec912a3b745c328f66fd196d19ddc59920845`.
- Initial workspace state had unrelated modified task-status files and untracked profile-lifecycle work. They were preserved. This check did not edit source, the index, those files, or workflow state; it did not rerun `scripts/check`, retry/dispatch a workflow, launch an app, commit, or push.
- The full failed Foundation macOS job log is retained at `/tmp/zmux-osc7501-final-be8ec91/foundation-macos-job-131637055309.log` (51,877 bytes; SHA-256 `32538db570c1b92ec4dc686c7f315cdee36d7d472a2f23a13dea45d2e6b0599b`). GitHub's workflow-run artifact list was empty, so no separate diagnostic artifact was uploaded by that run.
- The successful linkage workflow artifact is retained at `/tmp/zmux-osc7501-final-be8ec91/linkage.zip` and safely extracted at `/tmp/zmux-osc7501-final-be8ec91/linkage-artifact/`.

## Fresh workflow results

| Gate | Run | Observed result | Evidence |
|---|---:|---|---|
| Foundation checks | [43349565551](https://github.com/ashutoshpw/zmux/actions/runs/43349565551) | **Failure** | Linux job `131637054993` passed. macOS job `131637055309` failed step 6, `Check existing Rust modules and foreign C ABI` (`scripts/check`). |
| Universal macOS development build | [43349565567](https://github.com/ashutoshpw/zmux/actions/runs/43349565567) | **Incomplete at stop** | Swift type-check and both Linux scheduler jobs passed. The development job was still in `Validate Rust core and foreign ABI`; no final app result or artifact had been checked. |
| Versioned Ghostty linkage | [43349584656](https://github.com/ashutoshpw/zmux/actions/runs/43349584656) | **Success** | ARM64 linked engine, Program Status, Session, and linked real-PTY Session executions passed; ARM64 and x86_64 fixture compile/link phases passed. `native_host_requested=false`. |

### Foundation macOS failure

The first reported failed test was:

```text
ssh_cli_opens_an_actual_remote_session_and_reopen_never_replays_its_command
```

It panicked at `apps/cli/tests/ssh.rs:194:9`. The observed upload state was `failed` after 16,384 of 8,388,608 bytes: delivery `cancelled`, `delivery_error=originating terminal closed`, and `write upload input: Broken pipe (os error 32); remote result is uncertain`. The log also records remote lifecycle cancellation `EPERM` and a lifecycle startup retry after `managed SSH lifecycle stream closed (hello=false, active=false)`. The test binary summary was **2 passed, 1 failed, 0 ignored**, exit 101.

The log establishes the failed test and those emitted errors. It does not establish why the originating terminal closed or prove which lifecycle message caused the upload failure. No source diagnosis or fix is inferred. The current macOS log did pass `automatic_local_opencode_tracks_exact_root_and_preserves_user_configuration` within `provider_hooks` (**15 passed, 0 failed, 2 ignored**), so the prior OpenCode setup failure is not reused as current evidence.

### Ghostty linkage artifact verification

- GitHub artifact `14003586498`, `terminal-engine-linkage-be8ec912a3b745c328f66fd196d19ddc59920845`, reports **22,998,506 bytes**, SHA-256 `216fa1e3a07f3e6fab045f3589fa25077f1c0de037b1f1dd6819774eb4502b70`; the API digest, downloaded bytes, and computed digest match. ZIP CRC passed; bounded extraction accepted 368 regular entries totaling 70,707,544 uncompressed bytes, with no unsafe path or link entries.
- The linkage receipt is `complete`, binds source SHA `be8ec912a3b745c328f66fd196d19ddc59920845`, and has SHA-256 `99331b30a93d32b94abd64d8cb5bbf0445c2093de99644b452565cc00cd3b41d`. All **229** receipt source-hash entries match Git objects at that commit (228 unique normalized paths; the one alias is `scripts/process_group.py`).
- Executed ARM64 logs show engine **4/4 passed**, linked 7501 engine **1/1 passed** (6 filtered), Session **4/4 passed**, and linked real-PTY Session **1/1 passed** (6 filtered). The receipt claims all four lanes executed. All recorded compile/link phases for ARM64 and x86_64 finished with exit code 0.
- The selected candidate is `ghostty-7501-v1`: `source.json` SHA-256 `d19f74d9138b065014b17fb9deef9d9a2e653e6b0fd8c022eef3d1f7868d9cd8`; `linkage.json` SHA-256 `7ddc18b2c12a524358378e46326868c1d83ed88c683bf0f622d6485feaf2e077`.
- The unchanged accepted producer is run `43315098805`, source `7bf0864f9155384684f8fe92be8e9c547fdd9add`, artifact `13988885464`, **9,322,902 bytes**, SHA-256 `aa232650b06d2c9184e94cfd41f38d80d91f61901726a703aed25b7ade69b855`. Fresh run/artifact API metadata matches the new linkage receipt. The nested producer receipt SHA-256 is `e68fd9637da96fd89c710cd525f4185fc3d0718438b99c12c41a11c3f0a2cfcc`; its canonical recipe SHA-256 is `6c80d3798197d5c014f8962ab2a79306a938aba399343ad32df1c6b9cec99cc8`.
- The accepted producer library bytes remain ARM64 `37e4296467733d9ee13e0005d555969ecc6c9500183ce568381fbecb177f3ad5` and x86_64 `ce663b3259e064bd2d3de8bccaf895bae5202753b0ac8123306cde5429467464`; each hash matches the bytes embedded as accepted linkage input. No Ghostty producer rebuild occurred.

## Proof limits and stop point

- The linkage workflow requested `session_runtime=true` and `native_host=false`. Its Intel fixtures were compiled/linked but not executed; it does not prove AppKit linkage, Intel runtime, minimum-OS runtime, physical hardware, or packaged app acceptance.
- No final universal app artifact, packaged native sidebar receipt, or screenshot was verified in this check. The app workflow's native/sidebar stages and artifact were left unobserved after the Foundation failure, as requested; no delivery link is available from these results.
- The successful local full-check and Desktop review figures supplied by the task owner were not rerun or used to override this exact-head hosted failure.
