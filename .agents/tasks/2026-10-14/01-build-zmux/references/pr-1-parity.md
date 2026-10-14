# cmux PR #1 behavior and phased delivery

## Evidence and scope
Source: [ashutoshpw/cmux PR #1](https://github.com/ashutoshpw/cmux/pull/1),
“Adopt Better Auth and fork cloud origin”, inspected 2026-10-14. The reviewed PR
description identifies migration head `97b6967b2963420a18e90a96cca25e9c5d5a4a40`.
This is a requirements summary from PR metadata/description, not an implementation audit
or verification of its reported test results. Reconcile the final PR scope before parity signoff.

User requirement: carry this functionality into Zmux `main` in phased, moderately sized
commits. Implement original equivalents; do not cherry-pick cmux patches into an MIT repo.
Authorship alone does not prove that a patch to GPL/BUSL code is independently reusable.
Any proposed direct reuse needs a separate rights/provenance review; the default is fresh code.

## Required behavior matrix
| Requirement from PR #1 | Zmux phase | Acceptance evidence |
| --- | --- | --- |
| Central validated deployment configuration with generated platform inputs | M0 design; M5/M6 implementation | Invalid/inconsistent origins fail validation; generated native/web/release values agree |
| Owned service authority without operational upstream fallback | M5/M6 | Runtime destinations resolve only to configured Zmux services; missing services fail explicitly |
| Better Auth organizations and native OAuth authorization code with S256 PKCE | M6, M7 mobile | Browser/native sign-in works; bad state, verifier, callback and unregistered clients fail |
| Current session, member and client checks; MFA and localized mail | M6 | Revoked sessions/members cannot access services; MFA and invite/recovery mail journeys verified |
| Protected PostgreSQL migrations | M6 | Migration rehearsal and failure/recovery procedure on disposable database; protected deployment execution |
| Separate macOS/iOS identity and exact channel/tag callback registration | M5 identity; M6/M7 authentication | Distinct bundles, keychain namespaces and callbacks; concurrent/canceled login cannot attach to the wrong flow |
| Session-bound worker/relay tickets | M6 | Tickets bind actor/session/audience, expire, and reject cross-session use and invalid replay |
| Explicit readiness for VM images, relays and artifacts | M6 | Unprovisioned capabilities stay disabled; only verified owned artifacts can be promoted |
| Owned protected deployment and distribution targets | M5/M6 | Repository/environment restrictions and pinned deployment dependencies; inspect actual artifact destinations |
| No legacy API-key/device-login/raw-token handoff paths | M3/M6 | Unsupported flows are absent or explicitly rejected; tokens are not handed off in URLs/logs |
| Administrator deletion/admission and transaction correctness | M6 | Last-admin/concurrent membership scenarios preserve invariants; transactions do not exhaust pools |
| Correct channel/release metadata | M5/M7 | Published manifests, app identities and channel endpoints match the intended Zmux artifact |

Do not reuse `cmux.w3dev.app`, cmux bundle IDs/callbacks, or the PR's Vercel project IDs
as Zmux deployment defaults. They identify another product. Select Zmux-specific owned
values before deployment; do not provision or migrate existing cmux infrastructure implicitly.
Better Auth and plugins must pass the dependency license/version review before adoption.
The fork session save/open feature mentioned in the PR's base history is not a PR #1 change;
Zmux layout persistence is tracked independently in M2.

## Commit and branch policy
- Completed work belongs on Zmux `main`; do not leave final functionality only on a feature branch.
- A commit is a coherent, reviewable behavior or foundation slice, with relevant checks and docs.
  Avoid one commit per trivial file edit and avoid bundling an entire multi-subsystem milestone.
- Example slices: core workspace model plus behavioral checks; persistence plus recovery;
  socket API plus CLI inspection; OAuth service plus security checks; native OAuth client integration.
- Commit after the slice's applicable checks pass. Record unavailable native/hosted checks
  explicitly and do not label a release ready until those gates pass. Never bypass hooks.
- Update milestone status and evidence with each completed slice. Repair failing checks before
  treating the slice as complete; after three repeated failed attempts reassess the approach.
- Initial planning can be committed directly on unborn `main`. If implementation needs a branch,
  create its worktree under `~/.worktrees/ashutoshpw/zmux/<branch-name>/`; for PR work use
  `pr-<number>/`. Use tracking branches when a corresponding remote branch exists, not detached HEAD.
- Keep completed slices integrated into `main`; remote push and hosted deployment are separate
  actions. No remote is configured at planning time.

## Parity signoff
Track each matrix row with implementation paths, original tests, and observed results as work
lands. PR #1's reported tests do not transfer to Zmux. Native builds, authentication journeys,
database migrations, hosted service checks and distribution validation require new evidence.
