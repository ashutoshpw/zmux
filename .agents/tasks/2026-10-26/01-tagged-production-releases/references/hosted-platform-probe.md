# Hosted Windows and Ubuntu build probe

## Scope

User requested GitHub Actions verification and prohibited local builds. Add a
manual-only workflow at `.github/workflows/platform-build-probe.yml` to build the
existing CLI and scheduler on native Windows/Ubuntu runners. Preserve compiler
failure as a failed job; never skip incompatible crates or substitute a placeholder.
Run `scripts/check` on Ubuntu in the same workflow. Successful packages include
source/target metadata, notices and a SHA-256 checksum, and expire after 14 days.

This probe does not add a desktop UI, create an installer, push a release tag or
publish a GitHub Release. Source push and workflow dispatch are authorized by the
hosted verification request. No local Cargo/build/smoke commands are permitted.

## Results

Pending source publication and exact-SHA hosted run.
