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
- Complete option/default/output/error/focus/policy schemas and remaining audit drift enforcement; the bounded local schema below is partial.
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

## Deadline fixture repair after hosted528be92

Foundation38764217074 failed the external550ms wall bound after reporting the correct
300ms request timeout with no retry; measured whole-process duration was571ms. This
bound also includes startup, scheduling and exit collection outside socket ownership.
Removing that bound retains all fragmented response, deadline error, no-output and
no-replay assertions. An isolated400ms pre-main-work delay reproduces the old failure
(703ms) and passes the revised test. A separate deliberate body/acceptance deadline
reset still fails: split-stage response succeeds at363ms. Neither injected change
is part of production code. Actual cause of the hosted scheduling delay is unknown.

## F01b bounded executable contracts and frozen coverage

2026-10-15. The existing `commands` catalog retains schema version 1 and its fields;
additive `contract_schema_version: 1` exposes 48 partial local leaf contracts.
They describe typed arguments, selected argument relationships, successful output
and error formats, focus, availability requirements and limitations. Namespace
dispatcher availability never implies availability of arbitrary child commands.
Dispatchers without a bounded leaf description explicitly report unresolved
argument contracts. No family, parity or artifact-delivery gate is completed.

`commands --check` is offline. Both it and catalog generation validate dispatcher/
alias uniqueness, parse each leaf's safe sample argv through the real CLI parser,
check its request route/discriminator and validate frozen coverage references.
The samples build requests only: they never connect, execute scripts, submit input,
create sessions or query a live app. Explicit sample targets suppress inherited
caller context. Focus and operation/deadline defaults are read from actual parsed
requests; download bounds reuse runtime constants. Global timeout bounds are shared
with the real parser. These probes establish parser agreement, not runtime behavior
or exhaustive option/output/error coverage. All leaf contracts remain partial.

An original `apps/cli/src/registry/frozen-coverage.json` maps all 428 immutable
historical command inventory rows by ID, spelling and review owner. Thirty-seven
rows name a local counterpart with compatibility explicitly unverified; 391 remain
unresolved. A counterpart with a different spelling does not add the historical
entrypoint. Dynamic forms remain historical entries requiring further expansion.
The compiled checker compares the mapping against the original frozen inventory
and reconciliation artifacts and live parser-probed contracts. Missing/extra or
duplicate IDs, changed spellings/owners, stale contract references and overstated
compatibility states fail discovery with a nonzero exit and the ordinary textual
CLI error. The frozen audit artifacts are build-time inputs; packaged discovery
requires neither the checkout nor an app at runtime. The original audit statuses
remain untouched.

The 217 source candidates, 171 UI actions and 571 RPC identifiers remain separate
unresolved reconciliation gates. Successful checks and a mapping count are coverage
disclosure, never a behavioral parity percentage. Existing parser/real socket tests
remain the behavior owners. The offline discovery test now also checks the new
contract surface, malformed ambient caller independence, unimplemented browser/window
leaf disclosure and invalid check syntax; no test-only production seam was added.

### F01b verification and remaining boundaries

A fresh Linux snapshot of the committed baseline plus the five owned CLI inputs
passed `scripts/check`: 237 Rust tests, formatting, strict Clippy, dependency notices
and the C ABI harness. Receipt: `/tmp/zmux-f01-final-check-receipt.json`; complete
log: `/tmp/zmux-f01-final-check.log`. The snapshot used a private Cargo target and
all five CLI source hashes matched the working files at freeze. This excludes
concurrent uncommitted T03/F04/native work and is not an integrated or macOS result.

Three source mutations in a separate private snapshot demonstrated that the actual
`commands --check` executable rejects drift with exit 1: removing CMD-428 produced
`unmapped frozen command CMD-428`; mapping CMD-064 to the absent `new-window`
contract produced `refers to stale local contract new-window`; removing `snapshot`
from the live browser parser's accepted verbs produced `browser snapshot: unknown
browser subcommand`. The unchanged control passed. Receipt:
`/tmp/zmux-f01-drift-controls-results/receipt.json`. No control mutation was applied
to production sources. These are checker sensitivity controls, not command behavior
or compatibility tests.

The remaining F01 gates above still apply. The 48 bounded descriptions do not
establish exhaustive options, defaults, output variants, stable error taxonomy,
frozen alias equivalence or native execution. Integrating further commands, target
contexts or protocol revisions requires rechecking this catalog against that final
source and preserving honest unresolved states.

Parent independent full `scripts/check` passed frozen snapshot
`/tmp/zmux-f01-parent-verify-ww8nn32n` with a fresh target; log
`/tmp/zmux-f01-parent-check.log`. Independent reviewer passed offline discovery and
four actual drift controls, including false-complete mapping rejection; receipt
`/tmp/zmux-f01-independent-review-20261015/receipt.json`, SHA256
`61e5ec7065ca1255831a13aa066805ed2d8d9f5cfe3ec8a362f6f21e6bfb2c56`.
No command execution/socket occurs in safe parser probes. Remaining ownership work:
move compiled immutable baselines out of completed-task cleanup lifecycle and
ensure CI checks baseline-only changes. All48 contracts remain partial;37 local
counterparts remain unverified and391 inventory rows unresolved.

### Durable baseline ownership

The CLI now embeds `apps/cli/contracts/baseline/command-inventory.json` and
`audit-reconciliation.json`. Both were migrated byte-for-byte; their initial
SHA-256 values and archival relationship are recorded in the baseline README.
Historical task copies remain evidence, not build inputs. Existing Foundation
`apps/cli/**` filters cover baseline-only changes for push and pull requests.

Fresh isolated `scripts/check` passed at `/tmp/zmux-baseline-check-nqja9g_7`
(log `/tmp/zmux-baseline-check.log`). Removing both historical task copies from
that isolated checkout still allowed `cargo run -p zmux -- commands --check`
to pass: 48 partial contracts, 428 inventory entries, parity explicitly incomplete.
No new counterpart or compatibility claim is introduced by this ownership repair.
