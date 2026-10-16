# R03 first exact native readiness diagnostic: reviewed failure

2026-10-16. [Run38861027393](https://github.com/ashutoshpw/zmux/actions/runs/38861027393)
executed exact reviewed source0fca8c888b603667e79d64d38dc44772cb172066 on
macOS15.7.9 ARM64. Original receipts/logs remain unchanged. The finite diagnostic
artifact12029716713 expires2026-10-23T06:37:21Z,65405428 ZIP bytes, SHA256
`9996f9c67a499a6997415107bf4b7d67a564ae254e4d01dd8bee50072a5b124f`.

## Passed boundary

- Exact approved readiness package download/admission, reused without rebuild.
- Swift6 strict native compilation, host-only bundle/signing and final unchanged
  bundled dependency signature/version.
- All59 actual owner tests, zero ignored, against final bundled dependency.
  The actual CLI in those suites remains Cargo-built; this is not bundled CLI
  or AppKit first-line proof.
- Copied bundle with spaces in its relocated path, exact byte inventory, removed
  original build path, relocated bundle/dependency signatures and actual
  relocated readiness capability.

## Failure boundary

The actual owned AppKit process16508 reached the configured bridge constructor,
which refused the diagnostic control socket directly under shared `/tmp`:

```text
zmux local control startup failed: control socket directory is not private and owned by this user: /tmp
zmux startup failed: BridgeFailure(message: "Native operation failed (-6)")
```

The common startup-error shutdown path kept waiting after the bridge returned
no handle. The owned process-group runner enforced its150s deadline and reaped
its child; failure-profile cleanup passed with no local-Zellij owners. No native
readiness report or local-Zellij start/attach/input/completion acceptance exists
from this run. No main activation or user-facing artifact handoff occurred.

The separate production issue is recorded for repair after the next scoped
run starts: `ApplicationCoordinator.performShutdown()` always requests a Rust
ticket and retries every observation error, even when `NativeBridge.start`
failed before installing any handle. A repair must distinguish never-created
from known-live or outcome-unknown owners; it must not manufacture shutdown
success if an owned handle existed. Its exact semantics and behavioral regression
will be reviewed separately from the fixture repair.

## Independent retained evidence review

Local root `/tmp/zmux-readiness-native-darwin-38861027393/` contains original
artifact ZIP, safe extracted diagnostic, reproducible `audit-failure.py`, and
`failure-review.json` SHA256 `f0ccec2542ad90481f77e11d42780789cc19bfd243912073e41999652a25a34b`.
The unchanged review is retained in repository
`tools/local-zellij-feasibility/evidence/readiness-native-38861027393-failure-review.json`.

All324 uploaded compiler inputs match the reviewed v3 freeze/current source.
All phase log SHA256 and exact byte accounting match the original receipt
SHA256 `3543682547c1974a2a980fc7f5f666edda3363cce8bb0e750ce91295f05eb67b`.
The retained dependency ZIP matches originala4d93dd90c1a26aeddcaa203ac140e1a424c69aa68dd0980c7016a2512b61b0a;
the strict public owner verifier independently confirms59 passed with no ignored
coverage. The failed AppKit phase is preserved as exit-9/timed_out=true.

## Narrow fixture repair

Create a fresh owned0700 short directory under canonical `/tmp`, put the control
socket at its `c` child, and remove the empty private parent after the normal
absent-socket gate. No production/runtime/default/admission source changes.
Do not weaken the existing private control-parent admission policy or accept a
timeout as native success. Source review precedes the next authorized native run.

Portable verification for the fixture repair: both variant input checks pass;
required `scripts/check` passed396 tests across72 groups,3 explicit subprocess
fixture ignores, strict Clippy/notices and both ABI gates. Log
`/tmp/zmux-readiness-native-socket-repair-scripts-check-20261016.log`, SHA256
`57e0a339ae58e30bb8b85fe5f05e2122da90605aa461d76b2d0ebef05a4647ba`. The exact Darwin run remains the red
behavioral result; the repaired same native command is still pending.
