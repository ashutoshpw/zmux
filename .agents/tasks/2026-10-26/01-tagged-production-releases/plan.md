# Tag-triggered production releases

**Date:** 2026-10-26
**Task:** 01-tagged-production-releases

## Objective
Plan a GitHub Actions production release triggered by a valid `vYYYY-MM-DD` tag,
publishing verified macOS, Windows and Linux downloads to the matching GitHub Release.

## Context
- macOS has universal development packaging; Windows cannot currently compile and
  neither Windows nor Linux has a desktop UI/package. All three desktop releases
  therefore require platform development as well as release automation.
- This task authorizes planning only; preserve unrelated changes and do not publish.

## Approach
Add a separate `Production release` workflow: validate the tag and source, run checks,
build/sign/test each required platform, assemble an exact asset manifest, then upload
to a draft GitHub Release and publish only when every required asset is verified.
Reuse existing build owners; do not promote development ZIPs into production packages.
Default scope is desktop applications on all three OSes, with CLI bundled alongside.

## Steps
- [x] Inspect current task evidence, workflows, build scripts and platform constraints.
- [x] Specify the release pipeline, platform prerequisites and implementation slices.
- [x] Review the plan against source and record remaining external prerequisites.
- [ ] Implement shared release metadata/tag validation and reusable packaging entrypoints.
- [ ] Establish Windows runtime/IPC support and Windows/Linux desktop presentation.
- [ ] Add signed macOS/Windows packages and a supported Linux package; native smoke each.
- [ ] Add tag-triggered validation/build/assembly/publication jobs with bounded retries.
- [ ] Exercise a nonpublishing dry run, then verify a separately authorized first release.

## Acceptance criteria
- [x] Exact date-tag policy and publication/retry behavior are specified.
- [x] Each OS has an explicit deliverable, architecture and validation requirement.
- [x] Current capability gaps and credentials are separated from workflow work.
- [x] The plan is saved in this task folder; no release workflow is activated.
- [ ] After implementation, pushing an eligible date tag publishes the complete asset set;
  bad tags, missing platforms, failed signing/tests and checksum mismatches publish nothing.

## References
- [references/release-design.md](references/release-design.md) — source findings and proposed release contract.
