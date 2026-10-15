# F01a command discovery foundation

Date: 2026-10-15. Parent baseline: `d1393cb`. This is a bounded F01 slice;
full CLI/product parity and artifact eligibility remain false.

## Implemented

One original command registry supplies grouped top-level help, leaf/task-topic offline
help, aliases, parser target routing, the `commands` JSON catalog and `guide`/`--skill`.
`version` works offline. `capabilities` queries the actual running app for protocol/model
versions, target forms, authorization and resource/request limits. Existing commands
retain their handlers; unavailable cmux commands are not successful help-only stubs.
The catalog explicitly declares `parity_status: incomplete`.

## Verification

`scripts/check` passed: 49 Rust tests, formatting, Clippy with warnings denied,
dependency notices and C ABI harness. Public CLI tests exercise offline discovery
with an intentionally invalid socket and real control requests. The new delimiter
regression captures the transmitted input bytes, independently of help rendering.

The first read-only reviewer found two parser violations: literal `--stdin` after
`--` still consumed stdin, and invalid browser verbs/trailing help arguments returned
successful help. Both new regressions failed before repair for the expected reason;
both pass after preserving delimiter state and checking browser help shapes/verbs.
Fresh repair review passed after the final checks.

## Remaining F01 gates

- Expand from the implemented subset to every frozen command leaf, alias and option.
- Executable option/default/output/error/focus/policy schemas and audit drift enforcement.
- Complete option placement, output formatting, path/open startup and docs/settings contracts.
- Native packaged CLI verification on the exact final SHA and full V01 acceptance.
- No next user-facing artifact until all applicable audited contracts/gates pass.

## Review

Initial reviewer `/root/registry_review`: **fix-first**, parser issues repaired above.
Requested `gpt-5.6-sol / high`; realized model/effort unobservable in native metadata.

Fresh reviewer `/root/registry_repair_review`: **ship**, no material findings.
Requested `gpt-5.6-sol / high`; realized model/effort unobservable.
All 35 catalog leaves and aliases were checked offline; native acceptance remains open.

Hosted native evidence: macOS Actions run `38706071891` at `c572484` passed Rust
checks, universal compilation and packaged native smoke. Its archive is diagnostic,
not a full-parity delivery; it was not downloaded/reverified as a final artifact.
