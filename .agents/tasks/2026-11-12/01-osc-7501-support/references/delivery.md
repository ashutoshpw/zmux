# Final delivery receipt

**Accepted:** 2026-11-13 UTC
**Source:** `2c0cf2b40c4f0ee04048209272e3172a1ac30ce1` on `main`

## Download and package

[Download the final universal macOS development build](https://github.com/ashutoshpw/zmux/actions/runs/43455598200/artifacts/14057620449)
from successful [workflow run 43455598200](https://github.com/ashutoshpw/zmux/actions/runs/43455598200).
The artifact expires at **2026-11-27T16:21:25Z**. GitHub sign-in may be required.

- Downloaded Actions artifact: **30,775,347 bytes**; SHA-256
  `3c0a077e41f71fd56465f7c4e2fe8c2a47f32d33582e473f4a2bee44ad40c1c4`.
- Inner app ZIP: **26,916,243 bytes**; SHA-256
  `3da3a1a57243550c791e2146d721b3cf674ebbc256bb24ff5b0aa0e90fd805eb`.
- Exact packaged source metadata and `Info.plist` were verified. App, CLI, native
  bridge dylib and universal scheduler contain **arm64 and x86_64** slices. Minimum
  macOS is **14.0**. The package is ad-hoc signed and not notarized.
- The app's default terminal engine is `vt100`; optional Ghostty is not linked in
  this artifact.

## Validation and packaged acceptance

- Final local `scripts/check`: **767 passed, 0 failed, 7 ignored**, 262.30 seconds;
  source remained unchanged and unrelated work was preserved.
- Exact-source [Foundation run 43455639671](https://github.com/ashutoshpw/zmux/actions/runs/43455639671)
  passed Ubuntu **767/0/7** and macOS **765/0/7** across 95 result groups per platform.
- The universal run's integrated Rust/foreign-ABI check passed **765/0/7** across
  95 groups. Build, terminal selection, packaged native smoke, sidebar, system status
  menu and final artifact upload passed on the exact source.
- Packaged Programs acceptance used literal OSC 7501 shell `printf` output through
  the app PTY and production VT parser. The exact owner/selection/epoch and record
  inventory assertions passed for an idle `Root` row and a working `Tests` row at
  70% progress. Provider count and owner surface stayed unchanged. The native
  process, sidebar, status-menu and terminal-selection receipts all match the exact
  source; all hosted acceptance assertions passed.
- Parent reviewed the actual downloaded `zmux-sidebar-programs.png` and
  `zmux-sidebar-agents.png`. The compact Programs image uses a scrolled viewport.
  The expanded Agents image uses the measured fixture viewport and does not establish
  that all rows fit at the normal 28% sidebar height allocation. The idle/working fixture asked
  for no alert sound, so audio playback was not tested.

Full hosted receipts: [hosted-final-2c0cf2b.md](hosted-final-2c0cf2b.md). The final
artifact, package verifier and hosted acceptance JSON were retained during acceptance
under `/tmp/zmux-osc7501-final-2c0cf2b/`.

## Retained Ghostty linkage evidence

Optional Ghostty acceptance is a separate retained artifact, not part of the delivered
app. [Run 43407437492 / artifact 14027538458](https://github.com/ashutoshpw/zmux/actions/runs/43407437492/artifacts/14027538458)
records source `2efb76d9bcbb9e0a00015602b8bcaf81c00920de`. Its engine was not rebuilt;
**229/229** source inputs matched the delivered commit's pins. Both Apple targets
compiled and linked. Retained tests passed: ARM engine **4/4**, linked OSC 7501 engine
**1/1**, Session **4/4**, and real-PTY OSC 7501 Session **1/1**. No Intel runtime or
native-host/AppKit execution is claimed for this linkage evidence.

## Proof limits

Hosted native acceptance ran on Apple Silicon. The universal package's Intel slice
was architecture-verified, but Intel runtime behavior was not tested. Physical-user
and production acceptance are not claimed. The artifact is ad-hoc signed, not
notarized, and does not contain Ghostty.
