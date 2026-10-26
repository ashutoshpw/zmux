# Status

- **State:** planned
- **Updated:** 2026-10-26

## Notes
- Hosted follow-up complete: run `40389803745` at `f25ffa8` produced a successful
  Ubuntu x86_64 CLI/scheduler package. Windows reached the compiler and failed on the
  Unix-only session runtime and APIs. Overall run conclusion is failure due to Windows.
  See the linked probe evidence for job/artifact URLs, checksum and proof limits.
- No builds, Cargo commands or downloaded executables were run on this machine during
  the hosted follow-up. Local work was limited to editing, static syntax/link checks,
  Git/API operations and archive/checksum inspection. All compilation/checks/smoke
  execution ran on GitHub-hosted `windows-2025` and `ubuntu-24.04` runners.
- Ubuntu artifact downloaded and verified: exact source SHA, SHA-256, expected files,
  ELF x86_64 headers and executable permissions. This is an unsigned binary package,
  not a desktop release. Windows needs a runtime/IPC port; changing runner labels alone
  cannot satisfy its missing APIs. Production release implementation remains pending.
- Follow-up: run no local builds. User authorized checking Windows and Ubuntu with
  GitHub Actions; add/dispatch a manual binary build/package probe and record results.
  Local `scripts/check` is superseded for this slice by the user's hosted-only constraint;
  Ubuntu runs that check in Actions. Existing source support is not changed by the probe.
- Initial planning-turn validation (before the hosted-only instruction): relative
  Markdown links, whitespace and source anchors checked;
  `RUST_TEST_THREADS=2 scripts/check` passed (workspace tests, Clippy, license inventory,
  foreign C ABI and native profile-lease C ABI). No native desktop execution, signing,
  GitHub workflow or release publication was performed by this planning task.
- Planning deliverable complete; implementation and first publication remain pending.
- Source inspected at `310eae8` on main. Defined exact date-tag/source policy, required
  DMG/MSI/AppImage assets, signing/native gates, draft publication and retry semantics.
- Windows Unix-only runtime/IPC and missing Windows/Linux presentation are substantial
  prerequisites. GitHub Actions alone cannot produce those desktop releases today.
- Signing credentials, native runner availability, GitHub tag rules and production app
  identities need setup/verification during implementation; none was changed here.
- Existing dirty cloud-task status, lifecycle workflow and lifecycle tooling preserved.
