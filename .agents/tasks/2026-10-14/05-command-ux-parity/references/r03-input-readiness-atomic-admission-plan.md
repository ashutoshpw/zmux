# Atomic opt-in readiness package and native consumer plan

2026-10-16. Source-backed design only; current run 38852664400 is still building.
Do not activate the frozen observer4 runtime on main without the matching
admitted dependency and native bundle proof. Ordinary startup/default acquisition
and the existing retained legacy package stay unchanged.

## Existing production owners

- `package-patched.py` and shared `readiness_package.py` bind actual producer,
  silent readiness/public owner, variant, corresponding source and archive bytes.
  Readiness presently requires `--prepare-only`; its manifest is not promotable.
- `package_admission.py` has one exact reviewed tuple in
  `retained-native-package.json`. It checks `promotion_ready=true`, safe bounded
  archives/files, complete file inventory, reviewed producer/owner/source/policy,
  actual ARM64 Mach-O identity and exact download metadata. It does no build or
  execution. Reuse these generic safety owners, not a second extractor.
- `scripts/build-macos-dev` calls admission before deleting/replacing generated
  bundle output, extracts to Resources/Dependencies/local-zellij, preserves the
  producer executable signature/bytes, then verifies payload after app signing.
- `native-package.py` downloads only that exact immutable artifact, builds the
  app, checks the bundled binary and runs actual Rust Desktop/CLI owners against
  that path. It does not launch AppKit and declares native start/attach false.
- `crates/native-bridge/src/lib.rs` DesktopConfiguration rejects unknown fields
  and has no local_zellij field; configured desktops use default for that option.
  `apps/macos/Sources/NativeBridge.swift` likewise exposes storage/engine only.
  Rust actual-owner fixtures use DesktopOptions.local_zellij explicitly.

## Ordered delivery and promotion

1. Finish current source/evidence-only run, audit actual receipts and all retained
   compiler/source/log hashes. Treat success as package-chain and Rust consumer
   evidence only. No archive/executable from this run is retained, so no exact
   future download tuple can be derived from its evidence artifact.
2. Review the explicit readiness promotion/retention design and source before
   another build. Permit a named `input-readiness-v1` output only after the
   existing full producer/plugin/notice/source gates plus actual readiness and
   named observer4 semantics pass. Preserve legacy package output/admission.
   Retain the promoted package plus sidecar in a separately named finite-life
   artifact only under this reviewed scope, with all corresponding legal/source
   data included; no latest/fallback selection and no default/native activation.
   New compilation is necessary because current runner bytes are ephemeral.
3. Independently inspect those exact archive bytes, manifest, embedded plugins,
   compiler units, source reconstruction, notice policy and all owner receipts.
   Record immutable source/run/archive/manifest/binary/variant/inventory/schema/
   policy/download IDs, byte bounds/digests and expiry in a new readiness tuple
   and independent-review evidence; do not rewrite the old retained tuple.
4. Build ONE atomic opt-in integration branch from current main: include the
   reviewed observer4 consumer, new readiness tuple/admission selection, native
   configuration and final-bundle owner. Do not land observer4 alone. Reconcile
   equivalent existing formatter/Darwin repairs without duplicating them.
5. Run exact retained-download admission and build a host-only Darwin ARM64 app
   using the new explicit variant. Recheck signed bundle payload and capability
   and execute full public owner against its final bundled dependency path with
   dedicated sequential Cargo target. Then launch the owned relocated dev bundle
   for actual AppKit start/attach/input/completion tests. Retain exact bundle,
   source and semantic receipts only under the separately reviewed retention
   scope. No user-running cmux modification, no ordinary startup change.
6. Review the final atomic branch and receipts before landing. Claims remain
   Darwin ARM64 and the actual tested OS only; Intel/universal, minimum-OS runtime,
   public distribution/notarization and default selection remain separate.

## Atomic implementation footprint

- Extend the existing admission owner with explicit variant selection (legacy
  default preserved) and a distinct `retained-readiness-native-package.json`.
  Generic archive/download safety stays shared. Readiness selection additionally
  verifies pinned variant/inventory/protocol and the exact new independent
  review. No arbitrary manifest or dynamically generated pin is accepted.
- Extend `accept-package.py`, `scripts/build-macos-dev` and `native-package.py`
  with one explicit readiness selection threaded to the same admission owner.
  Reject incompatible options before touching an existing bundle. Bundle
  metadata records exact variant and hashes; verify before copy, after signing,
  after relocation, and after test execution. Never resign dependency bytes.
- Add optional typed `local_zellij` to configured native desktop creation and
  `NativeBridge.start`; absence remains default. The Swift adapter resolves only
  the explicitly admitted final bundle CLI/Zellij paths and owned profile root,
  validates shape off MainActor, and supplies nonnil installation/profile IDs.
  Filesystem/capability/process waits remain on Rust launch workers. Avoid user
  environment/PATH acquisition or blanket enabling merely because bytes exist.
- Add a bounded explicit native start/attach entrypoint that delegates through
  the existing serialized Command::LocalZellij/coordinator action path. No
  alternate mutation owner or frontend input queue/replay. GUI scope starts as
  explicit opt-in, not a new ordinary menu/default choice.
- Add an actual AppKit smoke owner using existing owned launch/activation and
  asynchronous readiness patterns. Bind source SHA, bundle executable identity,
  final dependency digest, installation/profile/owner/attachment identities and
  actual native window/surface/Session/input responder to the runtime receipts.

## Required observable checks

- Admission: old tuple still passes unchanged; unreviewed/wrong variant/schema/
  inventory/expiry/architecture and changed package/binary/source fail before
  replacement or execution. Reuse established archive/path/size tests.
- Native configured owner: absence refuses local start before reservation;
  malformed config/capability mismatch refuses before owner creation; configured
  exact bundled binary follows existing Rust coordinator, cancellation/deadline
  and saved-completion path. Keep behavioral regression-first evidence.
- Actual AppKit: launch owned relocated bundle; start a silent workload, observe
  typed Ready (not transport/output marker), send one first line through actual
  native input route, detach and attach the original owner with a new frontend,
  send one first line, prove each once and startup count one. Confirm EOF and
  joined frontend/cleanup. Then real exits0/7 before Ready must save authentic
  history, mount read-only completion, release/join the frontend, and never admit
  interactive input or spawn a replacement. Correlate UI/session identities with
  runtime evidence rather than declaring success from a screenshot/test count.
- Final receipts distinguish Rust Desktop/CLI runtime proof, final-bundled binary
  execution, actual AppKit interaction and relocation/signature acceptance.

No promotion, retention, admission, native configuration or UI source change is
implemented by this plan. Concrete source review precedes each newly authorized
producer/publication/native gate; current prepared evidence is not an admitted
package or an AppKit receipt.

## Parent review and current status

The parent reviewed this plan and approved isolated implementation on 2026-10-16:
preserve the exact legacy tuple/default; use explicit variant selection and
shared safety ownership; add typed native configuration and the existing action
owner; prove actual relocated AppKit behavior. After successful prepare-only
evidence review, implement readiness promotion and finite diagnostic artifact
retention, then freeze source for parent review before another producer run.
No additional user approval is needed for these scoped diagnostic artifacts;
a user-facing artifact remains gated on the complete parity acceptance.

- [x] Integrate frozen consumer and reviewed prepare-only package tooling.
- [x] Run combined repository check and publish exact combined head.
- [x] Dispatch one authorized prepare-only Darwin run at `9a42f851`.
- [x] Review [run38852664400 source/evidence](r03-input-readiness-package-darwin.md); actual consumer passed, wrapper selection failed, package not run.
- [x] Implement isolated [promotion/finite-retention source](r03-input-readiness-promotion.md).
- [ ] Freeze promotion source and current prepare-only evidence for review.
- [ ] Produce and independently review exact promoted readiness package.
- [ ] Implement one atomic opt-in admission/consumer/native integration branch.
- [ ] Prove final relocated bundle and actual AppKit behaviors before landing.

The prepare-only run failed only at strict owner enumeration of an ignored
reexec fixture; all actual consumer tests passed. The parent approved a narrow
shared selection repair and one fresh combined chain after exact source review.
No later producer dispatch, retained executable, tuple replacement or native
activation has occurred.
