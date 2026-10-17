# Use the supplied app logo everywhere

**Date:** 2026-10-17
**Task:** 02-app-logo

## Objective
Use the user's supplied transparent logo throughout the repository, validate the
affected surfaces, and commit and push the cohesive change to `origin/main`.

## Context
- The user authorized asset conversion and pushing to `origin/main`.
- Native packaging already generates every macOS ICNS size from `AppIcon.png`.
- The status menu and cloud page still draw unrelated terminal prompt marks.
- Preserve unrelated untracked profile-lifecycle work.

## Approach
Keep the supplied PNG unchanged as the canonical app asset. Reuse the bundled ICNS
for the native menu, preserving unread attention. Derive smaller web PNG/ICO assets,
serve them explicitly, include them in the container, and display the logo in README.
Correct the existing asset provenance without inferring an upstream license.

## Steps
- [x] Inventory image consumers, packaging, notices, and existing validation.
- [x] Replace the canonical asset and wire native, cloud, and README branding.
- [x] Validate assets, cloud behavior, and `scripts/check`; record native limitations.
- [ ] Commit only this task's files, push `main`, and verify remote SHA parity.

## Acceptance criteria
- [x] Native packaging retains transparency and generates its existing icon sizes.
- [x] Menu, cloud header/favicon/touch icon, and README use the supplied artwork.
- [x] Required available checks pass; unavailable macOS gates are explicit.
- [ ] The validated commit is published to `origin/main`.

## Validation notes
- `RUST_TEST_THREADS=2 scripts/check` passes: formatting, workspace tests, Clippy,
  license inventory, native bridge and profile-lease C ABI checks.
- Existing native status smoke checks the rendered status button and captures a
  screenshot. Its template-only assertion must accept the new full-color logo;
  retain the visibility and menu interaction checks. No new test seam is needed.
- Canonical PNG matches the attachment byte for byte. PNG dimensions/alpha and all
  four ICO representations pass asset inspection. Native ICNS generation continues
  to use the existing `sips`/`iconutil` pipeline; execution needs macOS.
- Cloud TypeScript typecheck passes. HTTP checks confirm exact asset bytes, MIME
  types, page references, and unknown-path 404 behavior. The production container
  builds, and its real app handler serves all three packaged assets with networking
  disabled and database access forbidden by the temporary smoke harness.
- Browser inspection at 1280 and 375 CSS pixels confirms the actual logo loads at
  48 pixels without horizontal overflow. The temporary public-page harness returns
  an unauthenticated session; it does not establish authenticated cloud behavior.
- Database integration tests cannot start without a disposable `DATABASE_URL`;
  the attempted transaction suite reports this prerequisite. No database was used.
- Local host is Linux with no Apple SDK. Native source compilation, ICNS packaging,
  and the status-menu screenshot/interaction suite remain hosted macOS gates.
