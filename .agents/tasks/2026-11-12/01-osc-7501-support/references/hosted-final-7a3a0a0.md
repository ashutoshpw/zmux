# Hosted final acceptance check for `7a3a0a0`

**Date:** 2026-11-13 UTC
**Source:** `7a3a0a06f8c330b20db916a8f0e45d5481bf98d8` on `main`
**Verdict:** The Foundation workflow failed on its macOS `scripts/check` step. Its Ubuntu matrix job passed. The universal macOS workflow was still running when the Foundation failure was collected, so it was not awaited; no final app artifact or native Programs receipt is accepted here.

## Source identity and preservation

- Local `HEAD` and `origin/main` both resolved to `7a3a0a06f8c330b20db916a8f0e45d5481bf98d8`.
- The checkout had pre-existing changes in `.agents/tasks/2026-10-18/01-cloud-origin-build/status.md`, this OSC task's `status.md`, `.github/workflows/profile-lifecycle-probe.yml`, and `tools/native-profile-lifecycle/`. They were preserved. The index was clean. This check did not edit source, the index, or those existing files. It saved full logs under `/tmp/zmux-osc7501-final-7a3a0a0/` and added only this evidence record.
- The task status records the latest local `scripts/check` as **763 passed, 0 failed, 7 ignored in 262.13 seconds**. That result and the accepted [transfer-retirement final review](transfer-retirement-final-recheck.md) were treated as prior evidence; neither was rerun here.
- No workflow was retried or dispatched, and no app was launched.

## Fresh hosted workflow results

| Gate | Run | Result | Evidence |
|---|---:|---|---|
| Foundation checks | [43358543419](https://github.com/ashutoshpw/zmux/actions/runs/43358543419) | **Failure** | macOS job `131666291293` failed `Check existing Rust modules and foreign C ABI` (`scripts/check`). Ubuntu job `131666291766` passed. |
| Universal macOS development build | [43358543431](https://github.com/ashutoshpw/zmux/actions/runs/43358543431) | **Incomplete at stop** | At the Foundation failure, the development job was still in `Validate Rust core and foreign ABI`; Swift type-check and both Linux scheduler jobs had passed. The run was not awaited after the primary failure. |

### macOS Foundation failure

The full job log is retained at `/tmp/zmux-osc7501-final-7a3a0a0/foundation-macos-job-131666291293.log` (**122,774 bytes**, SHA-256 `ba7b599758455f7ba70529d1f3d4e256bbf6424b2b32296ddec786e9df09200a`). The failed test binary reported **1 passed, 2 failed, 0 ignored**. Across all test-result summaries emitted before Cargo stopped: **592 passed, 2 failed, 5 ignored** in 59 binaries.

The failing tests were:

```text
checkpoint_tests::checkpoint_during_final_drain_keeps_pre_retirement_state
checkpoint_tests::checkpoint_waits_for_preexisting_prompt_bytes_on_real_pty
```

Both panicked at `crates/session-runtime/src/lib.rs:2403:57` with:

```text
bind child readiness socket: Error { kind: InvalidInput, message: "path must be shorter than SUN_LEN" }
```

The step exited 101. This records the hosted diagnostic only; no cause beyond the emitted error is inferred. The failed Foundation run's artifact list was empty (`total_count: 0`), so there was no separate artifact to download.

### Ubuntu Foundation pass

The full job log is retained at `/tmp/zmux-osc7501-final-7a3a0a0/foundation-ubuntu-job-131666291766.log` (**162,984 bytes**, SHA-256 `90cca3b81c9f443349fd131dec2df0365cc25bc812d93a346a89447cdc8ef61c`). `scripts/check` passed: **763 passed, 0 failed, 7 ignored** across 95 test binaries. The job also reports the C ABI and native profile lease C ABI checks passing, followed by **148 DOM browser cases passed**.

## Ghostty linkage closure recheck

The previously accepted [Ghostty linkage run 43349584656](https://github.com/ashutoshpw/zmux/actions/runs/43349584656), artifact `14003586498`, remains closure-compatible with this source:

- Its receipt lists 229 source inputs. Hashing those inputs at `7a3a0a06f8c330b20db916a8f0e45d5481bf98d8` produced **229 matches, 0 mismatches, 0 missing**. The receipt's recorded `tools/terminal-feasibility/../../scripts/process_group.py` path was normalized to `scripts/process_group.py` for the Git lookup.
- The retained ZIP at `/tmp/zmux-osc7501-final-be8ec91/linkage.zip` is **22,998,506 bytes**, SHA-256 `216fa1e3a07f3e6fab045f3589fa25077f1c0de037b1f1dd6819774eb4502b70`; this matches the GitHub artifact API digest and size, and `unzip -t` passed. The receipt SHA-256 is `99331b30a93d32b94abd64d8cb5bbf0445c2093de99644b452565cc00cd3b41d`.
- The accepted run's recorded ARM64 results were engine **4/4**, linked Program Status **1/1**, Session **4/4**, linked real-PTY Session **1/1**; ARM64 and x86_64 compile/link phases succeeded. No Ghostty or engine producer was rerun. This does not establish Intel runtime or packaged-app acceptance.

## App artifact and proof limits

No final `zmux-macos-dev-universal-7a3a0a06f8c330b20db916a8f0e45d5481bf98d8` artifact was downloaded or accepted. At the stop point the app workflow had not advanced beyond Rust/foreign-ABI validation, so no `native-sidebar-programs.json` or `zmux-sidebar-programs.png` receipt was available for this acceptance. The app workflow's terminal result was intentionally left unobserved after Foundation failed. There is no app download link, package checksum, signing/notarization result, or visual inspection to report from these runs.
