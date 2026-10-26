# R03 opt-in retained native dependency packaging

2026-10-15. Implementation candidate; independent downloaded-package verification
passed for both hosts of run `38823056442`, source
`986600f2a258a20f896083a4407f08b3778afa2c`. The new explicit feasibility lock pins
only that verified Darwin ARM64 package. Default acquisition remains unchanged.
This consumer still needs independent source review and final native execution;
package/source/policy/capture approval alone does not establish native integration.

## Scope and ownership

This slice owns new package-admission tooling, its behavioral tests, an opt-in
retained package lock/evidence record, `scripts/build-macos-dev` integration and a
manual Darwin ARM64 final-bundle owner gate. It does not edit shared runtime work
in progress, workspace descriptors, CLI routing, native menus or default
acquisition pins. The later native Start/Attach slice remains described in
[r03-native-launch.md](r03-native-launch.md).

Current source has no retained-package consumer. The builder bundles only the
native bridge and CLI, and defaults to a universal application. DesktopOptions
and configured FFI expose persistence and terminal-engine selection; they have no
Zellij installation/CLI path. Surface/restore identities currently model SSH and
local tmux. The foreground LocalZellij owner performs the real reservation,
authenticated supervisor/bootstrap and broker work, but exposes final exit rather
than native reservation/transport observer facts. Replacing SHELL with a wrapper
would bypass the reviewed native descriptor design.

## Admission and build contract

1. Independently verify successful Linux and Darwin package bytes, host receipts,
   current committed source, actual owner semantics, all negative gates, embedded
   rebuilt modules, notices/source containers and ABI evidence. Retain the exact
   run/artifact/target/archive/binary/manifest/policy tuple. This is the prerequisite
   for the new explicit retained lock; existing `pins.json` remains unchanged.
   Hosted artifacts expire after seven days: this is a feasibility input, not
   durable product distribution. An expired/missing exact artifact fails visibly.
   Later default distribution needs immutable owned storage and explicit
   promotion; there is no hidden latest-artifact or refetch fallback.
2. Add a portable admission helper accepting an explicit retained artifact root and
   target. Verify the locked archive digest before parsing, bound compressed and
   uncompressed bytes/member counts, reject aliases/traversal/links/duplicates and
   unexpected types, validate the complete file manifest and required provenance,
   and extract only to an absolute fresh private destination with exclusive files.
   No implicit downloads, rebuilds or executable invocation occur during admission.
3. Add paired `--with-zellij` / `--zellij-package-root` build arguments. Only
   `--architecture host` on Darwin ARM64 has an executed retained candidate.
   Universal/Intel and incomplete flag combinations fail before bundle mutation.
   Ordinary invocations preserve their existing build and dependency choices.
4. Retain the full accepted payload at
   `Contents/Resources/Dependencies/local-zellij/`, including sources, notices and
   provenance. Verify the retained Mach-O signature rather than re-signing that
   dependency; reject an unusable signature. Hash it again after application
   signing and fail on mutation. Build metadata records exact dependency bytes,
   target and package source/run separately from the application source SHA.
5. A manual Darwin host gate builds the opt-in application and reruns existing
   actual production owner conformance against that final bundled binary. It
   proves real workload input/resize, detach/reattach, startup-once and unchanged
   owner identity on the actual final path. This is native execution of the
   bundled dependency through the production CLI owner; it does not claim the
   application's future explicit Start/Attach UI path. Full AppKit lifecycle
   acceptance stays with the later observer/descriptor slice.

## Validation and review

Admission tests must exercise observable refusal of changed archives, missing or
altered legal/source members, unreviewed policy, target mismatch, path aliases,
links, duplicate/oversized entries, reused destinations and partial extraction
failure. Real retained-package admission supplies the positive control; synthetic
fixtures test malformed archive refusal without executing their bytes.

Builder validation checks default flag behavior, explicit host-only refusal,
full payload retention and no dependency signing mutation. The final native gate
uses a fresh private profile and the existing retained process owner; it never
adopts a user's workload or signals an unowned PID. Its output is independently
verified after download without executing downloaded bytes. The optional package
is larger than F04's 64MiB engine-only archive, so this gate needs its own explicit
finite size allowance.

Run required `scripts/check` on an isolated approved source snapshot, keep other
agents' edits unstaged, freeze this slice and obtain independent source review
before committing. No universal, Intel, older-macOS, signing/notarization,
default-selection or full native Zellij UI claim follows from this slice.


## Local candidate evidence and remaining native gate

The portable package admission, paired host-only builder arguments, full resource
payload retention and manual native final-dependency gate are implemented in the
owned slice. The native helper uses the Cargo-built production CLI with the final
app-bundled Zellij path; it does not claim to execute the future AppKit Start/Attach
entrypoint or persist a new native owner descriptor.

- Nine production-boundary archive/filesystem/download/platform/builder tests passed: `target/agent-validation/r03-native-package-admission-tests-v3-20261015.log`. The actual builder refusal test preserves a pre-existing generated bundle for incomplete flags and unsupported universal selection, plus host selection on Linux, rather than asserting source text or mocking filesystem effects.
- Actual retained ZIP/package positive admission, extraction and full payload verification passed: `target/agent-validation/r03-native-package-proof-v2-20261015/receipt.json`. Six intended negative controls rejected modified legal text, modified executable/signature bytes, missing rebuild instructions, existing output reuse, Intel target and changed archive before output creation; the genuine positive payload was restored and reverified. No downloaded executable was executed locally.
- The first evidence driver appended bytes and expected the later per-file digest guard, but the earlier total-inventory guard correctly rejected first. The final driver uses equal-length byte changes to exercise the intended per-file guard; no product behavior changed for that harness correction.
- Python AST, workflow YAML and scoped whitespace checks passed. Required `scripts/check` is run from a private immutable snapshot of committed source plus only this owned slice, excluding other agents' work in progress. Its receipt is retained under `target/agent-validation/r03-native-package-check-v2-20261015/`.
- The builder verifies the existing dependency signature before use, never signs that dependency, and rechecks every retained file after app signing. Private admission staging permissions become readable resource-directory permissions for the final app; dependency bytes remain exact.

Test authoring gate: the new tests protect archive/filesystem ownership, immutable
artifact identity, target selection and refusal-before-mutation at the real
consumer boundary. Credible regressions include path escape/link extraction,
truncating an existing output, accepting a changed/expired artifact, selecting an
unexecuted Intel/universal target or deleting a prior app before rejecting an
invalid request. Earlier producer gates did not exercise this new consumer. The
functions under test are used by the real admission/build/native drivers; no
production export, flag or wrapper exists solely for tests.

Do not stage, commit, push or dispatch this consumer before independent frozen
source review. The parent is preserving active native run `38825900209`; build
script changes match the native push trigger, so any delivery must wait for parent
coordination. After approval, actual Darwin execution must verify the final
unchanged bundled dependency through production owner input/resize/detach/reattach
and startup-once checks, then independently inspect the retained app/receipts.
No native acceptance is claimed yet.


## Binary download capture review repair

Independent review identified that the initial native driver used the shared
combined diagnostic log for a roughly 62MiB ZIP and decoded that binary into
Unicode. Although the exact ZIP digest failed safely on contamination, a harmless
`gh` stderr warning would break valid retrieval. That draft path is superseded.

The native driver now runs the existing package-local stdout helper with an
explicit exact download-size bound, capped at 128MiB. Metadata retains its 8MiB
default. Raw stdout is an exclusive binary sink; stderr remains a separate bounded
phase log under the unchanged shared process-group owner. The native receipt binds
helper source SHA, actual stdout bytes/hash/bound and diagnostic phase separately.
The committed older package's provenance remains unchanged at source `986600f`;
current capture source is a separately hashed consumer input.

- Nine capture subprocess tests passed, including all seven original metadata/lifecycle cases, arbitrary non-UTF8 binary preservation with an injected warning, explicit overflow and invalid-bound refusal before sink/command creation: `target/agent-validation/r03-native-package-capture-tests-v1-20261015.log`.
- Actual authenticated GitHub ZIP retrieval with a deliberately added stderr warning passed: `target/agent-validation/r03-native-package-binary-capture-v1-20261015/receipt.json`. Stdout was exactly 62,648,250 bytes with accepted ZIP SHA256 `0960c93edcd6c8a0efb1d04398f874043ba32402c0860fad2354140cb92820fc`; the diagnostic log contained only the 22-byte warning. The exact package then passed admission. No downloaded executable was executed.
- The first immutable source check passed but predates this review repair and is superseded for the final freeze. A fresh source snapshot reruns capture tests, admission tests and required `scripts/check` after the helper/driver change.

Shared owner SHA256 remains
`8c2f70b650b72c58764081ac764bd5fea04a5a015ffc2407db73a2b29433ea2f`;
notice policy remains unchanged. Delivery/native dispatch still await parent review
and coordination with active native jobs.


## Final V3 validation boundary

The V2 immutable snapshot at committed base
`de4becd325a6b6fcfc9af8316d96d1feaf522551` passed all nine capture cases, all nine
admission/builder cases and full required `scripts/check` (107.462 seconds), with
its source inventory unchanged. Receipt:
`target/agent-validation/r03-native-package-check-v2-20261015/check-receipt.json`.

The final V3 delta adds the shared runner itself to the native source inventory
and this audit note only. It changes no Rust, builder, capture, admission or
workflow behavior. Focused AST and real input-inventory checks cover that change;
unchanged Rust validation is retained from V2 rather than repeated. The final
freeze, explicit before/after hashes and combined validation receipt are in
`target/agent-validation/r03-native-package-check-v3-20261015/`. Independent source
review and the actual final Darwin bundle/dependency execution remain required.

## Darwin PENDIN investigation and test-only repair

Native run [38827122318](https://github.com/ashutoshpw/zmux/actions/runs/38827122318)
at `7b6540e8668b9a4d43feff6024be8fa0531b6289` built and signed the host-only app,
preserved the complete admitted payload, verified the final dependency signature
and version, and passed owner-0/owner-1. Owner-2 failed after the actual CLI stopped:
the restored terminal differed only by local flag `0x20000000`, Darwin `PENDIN`.
No final actual-owner receipt or accepted app ZIP followed that failed gate.

Published Apple source explains this state without establishing the failed
runner's exact kernel revision:

- [XNU tty.c at 43a90889846e00bfb5cf1d255cdc0a701a1e05a4](https://github.com/apple-oss-distributions/xnu/blob/43a90889846e00bfb5cf1d255cdc0a701a1e05a4/bsd/kern/tty.c), tag `xnu-11417.140.69`: lines 1253–1258 copy termios for TIOCGETA; 1358–1379 set PENDIN when enabling ICANON without flushing; 1391 retains kernel PENDIN despite supplied saved flags; 2073–2088 clear it while reprocessing pending raw input. Input/read/readiness paths can consume that state.
- [XNU termios.h at the same commit](https://github.com/apple-oss-distributions/xnu/blob/43a90889846e00bfb5cf1d255cdc0a701a1e05a4/bsd/sys/termios.h), line 259, identifies PENDIN as retype-pending-input state.
- [Libc termios.c at d53736942feb8997a67d27003deb42e6883ccaf3](https://github.com/apple-oss-distributions/Libc/blob/d53736942feb8997a67d27003deb42e6883ccaf3/gen/FreeBSD/termios.c), tag `Libc-1698.140.3`, lines 55–77: tcgetattr uses TIOCGETA and TCSANOW uses TIOCSETA; TCSAFLUSH selects a different ioctl and can discard queued input.

Research copies and hashes are retained under
`target/agent-validation/r03-termios-pendin-20261016/`. The production foreground
owner remains unchanged and still restores with TCSANOW before stop and exit.
Only fresh test snapshots are normalized on Darwin, masking PENDIN in the local
raw libc value and rebuilding the nix wrapper before equality. This handles the
wrapper's private raw snapshot without overlooking any other raw flag, control
character or speed; Linux comparison remains exact.

The same actual-owner test file adds a bounded private-PTY platform probe. It
saves canonical configuration, selects raw mode, queues a known incomplete line,
restores with TCSANOW, and captures termios before any read/readiness query. Darwin
must exhibit exactly the investigated PENDIN delta. The probe then supplies the
newline, reads exactly the original line once, rejects further readable input,
and checks that PENDIN has cleared with every configurable setting preserved.
Its receipt records uname, the actual kernel version, pre/post termios and input
bytes. No user terminal or adopted workload participates. The existing production
CLI input, resize, stop/resume, detach/reattach and startup-once checks remain.

Test-authoring gate: this probe protects queued-input preservation across the
specific platform transition that caused the false terminal-equality failure.
A flushing restore, wrong canonical mode, lost/duplicated bytes or any additional
setting change fails it. The existing workload journey proves resumed forwarding,
but did not isolate input already queued when restoring the outer terminal. No
production seam or new dependency is needed. The hosted owner assertion already
provides the pre-repair red result; Darwin green and exact kernel behavior remain
pending the coordinated native rerun. Linux focused probe and scoped linting are
mechanics/configuration evidence only, not Darwin acceptance. No unrelated broad
Rust suite is repeated for this test-only repair.

## Passed native diagnostic and independent artifact verification

Run [38828839028](https://github.com/ashutoshpw/zmux/actions/runs/38828839028)
passed at diagnostic source `9ec70f7f187394c4ab8f4ef3cc0e7b4fa3513d4a` on
ARM64 macOS 15.7.9. Its private-PTY probe records Darwin 24.6.0,
`xnu-11417.140.69.711.44~1/RELEASE_ARM64_VMAPPLE`: TCSANOW changed local flags
from 1483 to 536872395, exactly PENDIN, and all other raw fields matched. The
queued 22-byte line arrived once, no additional input was readable for 200ms,
and flags returned to 1483 with the original control characters and speeds.
This is actual hosted-kernel evidence; the earlier published source is explanatory.

All 30 owner tests passed (suite counts 2/15/3/4/3/3), including the existing real
input, resize, stop/resume, detach/reattach, startup-once and identity-preservation
journey against the final app-bundled dependency. The dependency signature and
version checks passed. Independent inspection verified all 258 source-input
hashes, phase logs, package identity/legal/source manifests and all 1,237 final
dependency files byte for byte against the retained reviewed package. No downloaded
executable was executed during independent verification.

The accepted app ZIP is 71,975,210 bytes, SHA256
`c7f362738e50cae34a7fdd2aae5ad189b2b7fccdea7435574065f371f7633bde`.
Dependency source remains `986600f2a258a20f896083a4407f08b3778afa2c`; its unchanged
binary SHA256 is `f3168d37e1ecd278e66b28edb0b5755202b6c16ff67ea0cd3d38ebd10d82b98d`.
Outer artifact `12016870967` is 133,342,791 bytes with verified ZIP SHA256
`acdf7213b40320f1637204679e8b340a807afb208ef0167744ecb9f973a8d511`,
expiring `2026-10-23T00:26:52Z`. Extracted artifact members also match that ZIP.
Independent receipts are under
`target/agent-validation/r03-native-package-hosted-38828839028/`:
`verified-receipt.json` (SHA256 `2e6a3675a1af76f1a425a9b2a4450078bc35d38957c8577a0da3de3ad5ad2bb8`)
and `artifact-identity-receipt.json`.

This accepts the opt-in packaged dependency through the Cargo-built production
CLI owner on the executed host. It does not establish AppKit Start/Attach,
Intel/universal support, older-macOS runtime behavior, durable/default distribution
or notarized release acceptance. Production TCSANOW and default acquisition remain
unchanged. The two-file correction retains the prior unchanged-base full
`scripts/check` result plus focused probe/rustfmt/Clippy/whitespace checks; its
separate validation/policy receipts are in
`target/agent-validation/r03-termios-pendin-20261016/repair-v1/`.
