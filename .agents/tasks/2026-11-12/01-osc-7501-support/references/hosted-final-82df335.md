# Exact-source hosted acceptance: `82df335`

**Source:** `82df3353d2bea3a9b2f6919ba67b48b2df5fb05a` on `main`
**Scope:** read-only observations of existing runs; no workflow dispatches, retries, local checks, source/index/status/plan edits, commits, or pushes.

## Fail-fast result

Foundation run [43366740329](https://github.com/ashutoshpw/zmux/actions/runs/43366740329) checked out the requested SHA. Its `foundation (macos-latest)` job `131692796323` failed in **Validate Rust core and foreign ABI** while running `scripts/check`.

The only failing test was `managed_ssh_bootstrap_fits_a_single_exec_argument` in `crates/session-runtime/tests/shell_integration.rs`. The committed test calls `Command::new("/bin/true").arg(&command).status()`; the call failed at line 83 with `Os { code: 2, kind: NotFound, message: "No such file or directory" }`, before the explicit 64 KiB length assertion. The log establishes `ENOENT` from that spawn, but does not establish why `/bin/true` was unavailable. The test suite reported **12 passed, 1 failed, 2 ignored**. This is not the intentional mutation-child EOF case and there are no poisoned-lock cascades.

The complete failed job log is retained at `/tmp/zmux-osc7501-final-82df335/foundation-macos-job-131692796323-full.log` (128,516 bytes, SHA-256 `817fd590efd895b8d188d15419b9d515d839ed8ab72338ebd2b57bb5aa746cf6`). Across its 64 Rust test-result summaries, totals are **631 passed, 1 failed, 7 ignored**; the other summaries passed.

Foundation `ubuntu-latest` job `131692795049` passed. Its 95 Rust test-result summaries total **763 passed, 0 failed, 7 ignored**. Full log: `/tmp/zmux-osc7501-final-82df335/foundation-ubuntu-job-131692795049-full.log` (162,986 bytes, SHA-256 `645d2d18fffab0687b69489c3ff3aa99e892c57b6f4a28e1f00fc19930ae2027`).

At the same status observation, universal macOS run [43366740304](https://github.com/ashutoshpw/zmux/actions/runs/43366740304) had also completed with failure: `swift-typecheck` and both Linux scheduler jobs passed; `development` failed. Per fail-fast scope, no universal failure log or artifact was investigated. No final app artifact, packaged-app acceptance, or screenshot is claimed.

## Retained Darwin linkage

Run [43366752162](https://github.com/ashutoshpw/zmux/actions/runs/43366752162) succeeded at the requested SHA with `candidate_set=ghostty-7501-v1`, `include_session_runtime=true`, and `include_native_host=false`.

Artifact: [terminal-engine-linkage-82df3353d2bea3a9b2f6919ba67b48b2df5fb05a](https://github.com/ashutoshpw/zmux/actions/runs/43366752162/artifacts/14010980716), artifact ID `14010980716`. Downloaded archive size matched API size (**22,998,513 bytes**); downloaded SHA-256 matched API digest `55866ceb9cf1045b24bd52b665acda50f66d627028c040afea9b66791958e2e2`; ZIP CRC passed (368 entries; 70,707,540 expanded bytes). The archive was safely extracted under `/tmp/zmux-osc7501-final-82df335/linkage-artifact/`.

The exact-source `source.json` and `linkage.json` manifests identify `ghostty-7501-v1`; the linkage manifest pins the source manifest and both Apple library hashes. The linkage receipt’s retained candidate artifact identity and archive hash/size match that manifest. All **229** receipt source pins matched `git show` at the exact source SHA after `posixpath.normpath` and an in-repository path check. The one alias `tools/terminal-feasibility/../../scripts/process_group.py` normalized to `scripts/process_group.py` and matched the same blob.

Both Apple targets passed engine, Program Status, Session, and Session Program Status compile/link phases. ARM64 execution passed: engine **4/4**, linked Program Status **1/1**, Session **4/4**, and linked real-PTY Program Status **1/1**. The real-PTY test verified query-before-DA ordering and retirement after final drain.

The receipt explicitly records no engine rebuild, no default-engine change, no AppKit/native-host link, no Intel execution, and no minimum-OS runtime execution. This is retained-linkage evidence only; it does not establish Intel runtime, native-host, physical-device, packaged-app, or production acceptance.

## Diagnostics and boundaries

- `/tmp/zmux-osc7501-final-82df335/linkage-final-verification.json` records artifact, manifest, compile/link, and ARM execution checks.
- `/tmp/zmux-osc7501-final-82df335/linkage-source-pins-verification.json` records all 229 normalized Git blob checks.
- `/tmp/zmux-osc7501-final-82df335/ubuntu-latest-summary.json` and `macos-latest-summary.json` record hosted test totals.
- The parent-supplied local `scripts/check` result was not rerun. This pass did not alter the pre-existing shared-checkout changes or task status, and did not stage or commit anything.

## Parent universal failure follow-up

Parent retrieved the failed universal development job log at
`/tmp/zmux-osc7501-final-82df335/universal-failed.log` (SHA-256
`69b01922e8891eb10718fe06df8af360cfc870f3fabe074eec6adcb619ba6d2a`).
It failed at the same bootstrap fixture with spawn ENOENT before the length
assertion; the shell suite reported 12 passed, 1 failed, 2 ignored. Universal
app build and native acceptance were skipped. Swift typechecking and both Linux
scheduler jobs passed.
