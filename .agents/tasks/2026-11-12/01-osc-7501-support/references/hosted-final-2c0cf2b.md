# Hosted acceptance at `2c0cf2b40c4f0ee04048209272e3172a1ac30ce1`

**Observed:** 2026-11-13 UTC
**Source:** `2c0cf2b40c4f0ee04048209272e3172a1ac30ce1` (`main`)
**Scope:** Existing Foundation and universal macOS workflow runs only. No retry, dispatch, source edit, local test, commit, push, or app launch was performed by this test sub-agent.

## Hosted run outcomes

- Foundation checks [run 43455639671](https://github.com/ashutoshpw/zmux/actions/runs/43455639671) was explicitly dispatched once because `apps/macos/Sources/App.swift` is outside the Foundation workflow's path filter. It completed successfully on the exact source. Full job logs contained 95 Rust test-result groups per platform: macOS **765 passed, 0 failed, 7 ignored**; Ubuntu **767 passed, 0 failed, 7 ignored**. Foundation does not upload an app artifact.
- The auto-push universal macOS run [run 43455598200](https://github.com/ashutoshpw/zmux/actions/runs/43455598200) completed successfully at 16:21:35 UTC on the exact source. Swift typecheck, x86_64 and aarch64 Linux scheduler jobs, Rust/foreign ABI `scripts/check`, universal build, terminal selection/browser-link acceptance, packaged-app native rendering, native sidebar/system-status menu, final upload, and build receipt all passed. The `scripts/check` job log contained 95 Rust test-result groups: **765 passed, 0 failed, 7 ignored**. No primary failure or diagnostics artifact occurred.
- The pre-acceptance candidate (artifact `14055009741`) was retained before native checks and was not treated as delivery evidence.

## Final artifact and package verification

Final artifact: [`zmux-macos-dev-universal-2c0cf2b40c4f0ee04048209272e3172a1ac30ce1`](https://github.com/ashutoshpw/zmux/actions/runs/43455598200/artifacts/14057620449)

- Artifact ID `14057620449`; GitHub API size **30,775,347 bytes**; API SHA-256 `3c0a077e41f71fd56465f7c4e2fe8c2a47f32d33582e473f4a2bee44ad40c1c4`. The downloaded ZIP matched both API values.
- Outer ZIP CRC passed. Bounded safe extraction checked 461 entries / 38,336,388 expanded bytes, rejected traversal and symlinks, and stayed below the 512 MiB expansion limit.
- The existing `/tmp/zmux-osc7501-verify-app.py` verifier passed for the exact source. Inner app ZIP SHA-256: `3da3a1a57243550c791e2146d721b3cf674ebbc256bb24ff5b0aa0e90fd805eb`.
- Outer/inner build metadata and `Info.plist` match the full source SHA. The app, native bridge, CLI, and universal scheduler Mach-O binaries each contain arm64 and x86_64 slices. Minimum macOS is 14.0. Embedded ad-hoc code-page signatures verified; metadata says ad-hoc signed and not notarized.
- The app uses the default vt100 engine; optional Ghostty is not linked in this artifact.

Download, extraction, verifier output, and receipt audit are retained under `/tmp/zmux-osc7501-final-2c0cf2b/`, including `final-artifact-14057620449.zip`, `final-extracted/`, `packaged-app-verification.json`, and `hosted-acceptance-verification.json`.

## Packaged native acceptance receipts

The downloaded artifact contains all required receipts: `native-smoke.json`, `native-sidebar.json`, `native-sidebar-programs.json`, plus the source-bearing `native-sidebar-process.json`, `native-sidebar-lifecycle.json`, `native-smoke.json.stages.json`, `native-status-menu.json`, and universal terminal-selection receipt. Their relevant source fields match `2c0cf2b40c4f0ee04048209272e3172a1ac30ce1`; the sidebar process receipt has `success: true` and embeds data equal to both sidebar JSON receipts. The packaged smoke and stage receipts report success / `assertions_complete`; terminal selection reports `complete`.

The actual packaged sidebar Programs fixture reports literal OSC 7501 shell `printf` output through the packaged app PTY and production VT parser. The receipt confirms the expected idle Root and working `build/test` record at 70% progress, available status, alert feed, complete focused records, nonfocused priority summary, unchanged complete inventory, provider coexistence, and exact selection from another workspace. The owner matches the live PTY target and Agent owner on their shared identity fields, and carries `session_epoch: 2` and `command_epoch: 1`. The production acceptance script passed its owner, identity, record, epoch, inventory, provider, status-menu, and screenshot assertions. Its expected nonfocused summary values are one visible record with `nonfocused_records_complete: false`, zero omitted records, and `build/test` as the priority record.

The native Program screenshot is retained for parent visual review at [`zmux-sidebar-programs.png`](/tmp/zmux-osc7501-final-2c0cf2b/final-extracted/zmux-sidebar-programs.png), 81,212 bytes, SHA-256 `9cb74034380efccb35d06908f02abb34f774a20cced00bd445e566e092083f33`. Other native smoke/sidebar screenshots are in the same extracted artifact. This sub-agent verified receipt identity, assertions, archive integrity, and image presence; parent owns the final visual review.

The scoped source change under review is the sidebar smoke fixture viewport for the full populated Agents document, with a temporary minimum viewport constraint removed before compacting. The production 28% sidebar policy remains unchanged. The accepted expanded Agents image uses the fixture viewport; it does not prove that three Agents fit under the normal 28% policy.

## Retained Ghostty linkage evidence

The existing Ghostty linkage run `43407437492` / artifact `14027538458` remains reused from source `2efb76d9bcbb9e0a00015602b8bcaf81c00920de`. The parent-provided reuse receipt `/tmp/zmux-osc7501-final-2c0cf2b/retained-linkage-reuse-verification.json` records **229/229** source inputs matched at the new source and `engine_rebuilt: false`. No producer or linkage run was rerun. This retained linkage evidence does not establish Intel runtime or native-host acceptance.

## Proof limits and ownership

Hosted macOS acceptance and package verification passed. Intel runtime behavior and physical-device/user acceptance remain unverified. This note records test-side evidence only; parent owns final visual acceptance, task status, documentation, and delivery.
