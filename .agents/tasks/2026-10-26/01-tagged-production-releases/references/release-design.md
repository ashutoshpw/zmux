# Production release design

## Verified source baseline

Inspected on 2026-10-26 at local `310eae8` on `main`. This is source evidence;
no hosted runs, repository settings, signing accounts or production services were verified.

- `.github/workflows/macos-development.yml` builds on main/manual dispatch. It builds
  Linux x86_64/aarch64 scheduler helpers first, embeds same-source helpers in a
  universal macOS app, exercises native checks, and uploads expiring Actions artifacts.
  There is no tag-triggered GitHub Release publisher in the inspected workflows.
- `scripts/build-macos-dev:15` exposes host/universal and optional dependency inputs.
  It compiles Rust and Swift, creates `zmux.app`, bundles CLI/runtime/notices and uses
  `ditto` to preserve bundle metadata. Reuse this build logic through shared functions.
- `scripts/build-macos-dev:180` hardcodes app version `0.2.0`; `Cargo.toml:11`
  sets workspace version `0.1.0`. The build ad-hoc signs; metadata says not notarized.
  `config/deployment.json` contains development bundle/client/callback identities,
  and the script explicitly enforces the development OAuth identity.
- `crates/session-runtime/src/lib.rs:4` rejects non-Unix compilation. Session I/O,
  process groups and shutdown use Unix APIs. `apps/cli/src/main.rs:9` and
  `crates/desktop-runtime/src/control.rs:14` use Unix sockets; local authorization
  relies on Unix filesystem permissions and peer identity.
- `apps/macos` is the only current desktop presentation implementation.
  `docs/architecture.md:18` defers Windows/Linux UI work. Existing Linux scheduler
  helpers and a headless prototype are not Linux desktop releases. Files named
  `windows.rs` and `check-native-windows` manage application windows, not Windows OS.
- `.github/workflows/check.yml` runs foundation checks on Linux/macOS only.
  `scripts/check` owns fmt, workspace tests, Clippy, licenses and foreign C ABI checks.
- Related scope: `.agents/tasks/2026-10-14/01-build-zmux/plan.md`,
  `.agents/tasks/2026-10-14/03-macos-development-build/plan.md`, and
  `.agents/tasks/2026-10-18/01-cloud-origin-build/plan.md`. Preserve their history.

## Release trigger and identity

Create `.github/workflows/release.yml`, named `Production release`, separate from
development builds. Its automatic trigger is:

```yaml
on:
  push:
    tags:
      - 'v[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
permissions:
  contents: read
concurrency:
  group: production-release-${{ github.ref }}
  cancel-in-progress: false
```

GitHub uses a glob here. Preflight must also apply anchored regex
`^v[0-9]{4}-[0-9]{2}-[0-9]{2}$` and parse an actual Gregorian date, rejecting
examples such as `v2026-02-30`. The date need not equal the runner's current date;
allow delayed builds and retries. One immutable release per date; no suffixes.

Fetch full history, resolve annotated/lightweight tags with `refs/tags/<tag>^{commit}`,
and require that commit to be reachable from freshly fetched `origin/main`.
Use the peeled commit as `source_sha` everywhere, including scheduler inputs; tag
objects have a different SHA. Checkout that commit and verify a clean source tree.
Before publishing, refetch the tag and reject any changed target. Document tag rules
that restrict creation to maintainers/release automation and prevent updates/deletion.

Use the exact date tag for release title, filenames and a shared embedded release
manifest. Keep a single checked-in product SemVer for Cargo/app metadata and normalize
existing versions; date tags are not SemVer. Generate platform build numbers from a
documented monotonically increasing mapping within each platform's numeric limits.
Do not put a raw hyphenated date into Apple/Windows numeric version fields. The
manifest and CLI version output must expose both product version and exact release tag/SHA.

## Required downloads

Assume the user means installable desktop apps, with bundled CLI, across all three
OSes. Initial architecture scope is universal macOS and x86_64 Windows/Linux.
Windows/Linux ARM64 can follow when native execution gates exist; do not advertise it yet.

- macOS 14+, arm64 + x86_64: `zmux-<tag>-macos-universal.dmg`, containing the signed,
  notarized, stapled app and its CLI/runtime/helpers. Build with a pinned macOS/Xcode
  image. Retain Linux musl scheduler builds on native Ubuntu x86_64/ARM64 runners,
  and require the full same-source scheduler manifest as development packaging does.
- Windows x86_64: `zmux-<tag>-windows-x86_64.msi`, built on `windows-2025` with MSVC.
  Establish a documented minimum OS supported by the runtime/UI (ConPTY requires
  Windows 10 1809 or later; the chosen UI may require newer). Authenticode-sign and
  timestamp embedded executables and installer. This needs a Windows runtime and UI.
- Linux x86_64: `zmux-<tag>-linux-x86_64.AppImage`, built on a declared baseline
  (proposed Ubuntu 22.04 / glibc 2.35). This needs a Linux UI, desktop entry/icon and
  dependency policy. Document FUSE requirements and an extract-and-run fallback.
  Verify launch on the baseline and a newer supported distro, with X11/Wayland coverage
  for the display backends the UI actually supports. `.deb`/`.rpm` may follow later.
- Shared `SHA256SUMS` and `release-manifest.json`: tag, source SHA, product/build
  versions, workflow run URL, OS/CPU/minimum OS, filename, size, SHA-256 and signing
  status for every download. Include MIT/third-party/icon notices inside each package.
  Hash final signed/notarized files, never their unsigned predecessors.

Publish to `https://github.com/ashutoshpw/zmux/releases/tag/<tag>` as a stable
GitHub Release, not a prerelease. Actions artifacts remain intermediate evidence.
The support contract must match the release notes; do not imply full feature parity
before Windows/Linux behavior is implemented. Cloud deployment and auto-updates are
separate tasks, not consequences of publishing desktop installers.

## Job graph and permissions

1. **Preflight:** validate ref/date/main ancestry, product/release metadata, expected
   asset list and existing release state. Use checked-in scripts for policy.
2. **Validate:** run `RUST_TEST_THREADS=2 scripts/check` on Linux/macOS at the release
   commit with the existing SSH/tmux dependencies. Add Windows owner checks once ported;
   adapt checks to real platform owners without deleting Unix coverage. Pin Rust from
   `rust-toolchain.toml`; use locked dependencies and pinned action SHAs.
3. **Build:** native platform jobs, plus the two Linux scheduler jobs required by macOS.
   Run build matrix with `fail-fast: false` and bounded job timeouts. Extract shared
   packaging/build primitives so development and release do not drift. Use current
   default terminal engine; optional Ghostty/Zellij require their own retained-source,
   linkage, licensing and release admission evidence before inclusion.
4. **Sign/package/verify:** keep unsigned build jobs free of signing secrets; grant
   secrets only to tag-gated signing jobs. macOS signs nested Mach-O files inside out
   with Developer ID, hardened runtime and reviewed entitlements, notarizes with
   `notarytool`, staples and checks Gatekeeper acceptance. Review the final DMG too.
   Windows verifies the timestamped Authenticode chain after MSI assembly. Execute
   native smoke against the extracted/installed final package, not just build output.
5. **Assemble:** depend on every required platform validation job succeeding, not just
   completing. Download exact run artifacts, validate receipts/SHA/tag/architecture,
   enforce an allowlist of required assets and reject missing/duplicate/unexpected
   payloads. Produce the aggregate manifest/checksums and release notes from the
   previous stable date tag on first-parent history. Record supported behavior/limits.
6. **Publish:** only this job receives `contents: write` and the repository
   `GITHUB_TOKEN`. Create/reuse a draft for this exact tag, upload all assets, download
   them back and verify names/sizes/checksums, then mark published (`prerelease: false`).
   No PAT is required. Use `gh release ... --verify-tag`/GitHub API without creating or
   moving a tag. Set `latest` only if this date is newer than the latest stable date
   release, serializing publication across tags to avoid a concurrent older run winning.

No environment approval is proposed: after one-time credential/repository setup, a
maintainer's eligible tag push is the release action. External prerequisites should
fail early with descriptive errors. Select exact artifact IDs/names from this run;
never consume a loosely selected "latest successful" development artifact.

## Failure, retries and trust

- A failed check/build/sign/notarization/asset validation prevents publication. Save
  bounded diagnostics with retention limits and a clear Actions summary.
- GitHub Releases is not transactional. Stage in a draft and flip visibility once;
  a failed upload/readback leaves an unpublished draft, never a partial public release.
- Retry using the same tag/source/run's verified artifacts where retained. A publisher
  retry reuses a draft only after matching its stored source/release identity; reconcile
  only its expected assets. Reject an unrelated draft or mismatched tag/SHA.
- A published release is immutable: exact matching assets are a no-op; mismatches
  fail without overwrite/deletion. If bytes must change, use a new dated tag. Do not
  depend on bit-identical rebuilds of timestamped signatures.
- Recheck tag identity before final publication. One-time repository tag rules are
  needed to prevent movement during the remaining check/publish window.
- Do not expose signing credentials in logs/artifacts. Use temporary keychains and
  always-run cleanup. Linux checksums provide integrity, not OS publisher signing;
  GitHub artifact attestations are an optional later trust enhancement.

## Prerequisites and implementation slices

1. **Shared release contract:** metadata/version policy, date/source validators, asset
   manifest and dry-run assembly/publisher mode. Add a manual dry-run input that runs
   the candidate through build/verification with no signing secrets or publication;
   report signing/notarization as unverified there. Actual signed acceptance must use
   a credentialed tag run or a separately authorized signed rehearsal.
2. **Platform support:** keep Rust actions/session ownership shared. Introduce a
   Windows ConPTY/process-lifecycle adapter and same-user IPC with Windows ACL/peer
   ownership guarantees; Unix sockets/process-group APIs cannot merely be renamed.
   Choose Windows/Linux presentation after a small native terminal/input/resize probe;
   egui/eframe remains a candidate, not an existing dependency or a settled decision.
   Preserve the Swift/AppKit macOS frontend. Define per-platform launch, input,
   resize, splits, persistence, CLI authorization and teardown acceptance before porting.
3. **Packaging and identities:** parameterize development/production build identities,
   metadata and signing. Decide stable bundle/client IDs, callback scheme, credential
   storage and data migration policy. If production identity changes, register/verify
   its OAuth client/callback before enabling sign-in; do not silently ship dev identity.
   Implement DMG/MSI/AppImage builders and include complete notices.
4. **Signing setup:** repository/environment secrets for Apple Developer ID certificate,
   password and notary API credentials; Windows signing service/certificate and
   timestamp policy. Confirm account/certificate availability before implementation
   relies on them; any new paid provisioning requires separate authorization.
   Never fall back to unsigned/ad-hoc packages while labeling them production.
5. **Workflow and rehearsal:** add the job graph, minimal token scopes, same-source
   handoffs and publication recovery. Update README/development docs with release
   downloads, platform support, checksums and the maintainer tag procedure. Keep
   development workflow available. Commit cohesive validated slices on main.
6. **First release:** after platform/signing gates and a successful dry run, separately
   authorize pushing `vYYYY-MM-DD`. Observe the run and verify public assets/checksums,
   then perform clean-machine install/launch checks for each declared platform.

The tagged commit must already contain `release.yml` and the completed packaging work.
Maintainer procedure after implementation (illustrative; do not execute during planning):

```sh
git switch main
git pull --ff-only origin main
git tag -a v2026-10-26 -m "Zmux v2026-10-26"
git push origin refs/tags/v2026-10-26
```

Choose the intended release date and verify its tag is unused first. A failed workflow
is rerun from Actions; do not delete/recreate the tag as a retry mechanism.

## Validation to implement

Test at policy/package/publication owners: valid leap day vs impossible dates, suffixes,
tag pointing outside main, annotated tag identity, moved tags, wrong-source scheduler
input, missing platform, corrupt checksum, failed signing, and interrupted draft upload.
Verify rerun recovery, published-asset refusal and concurrent older/newer date releases.
Use publisher fixtures for failure recovery, then an authorized hosted rehearsal for
GitHub behavior. Invalid trigger shapes need an event-filter check as well as validator
coverage. No publishing on branch pushes, PRs or manual dry runs.

Native gates must exercise installed artifacts: shell output/input/resize, pane creation,
close/child cleanup, persistence and bundled CLI control. On macOS run existing native
checks and execute both Apple Silicon and Intel slices; architecture inspection alone
is not Intel runtime evidence. On Windows test PowerShell and same-user IPC denial;
on Linux test executable permissions/dependencies and supported display backends.
Record unavailable runner/clean-machine gates explicitly; portable success cannot close them.

The all-platform publication gate remains incomplete until the platform work exists.
A smaller macOS-first release is feasible earlier, but would require explicitly reducing
the requested artifact scope; this plan retains all three platforms as required.
