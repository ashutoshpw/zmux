# R03 finite diagnostic readiness retention

2026-10-16. The parent approved isolated implementation of the
[atomic admission/bundle plan](r03-input-readiness-atomic-admission-plan.md),
including finite scoped diagnostic artifacts. This is implemented source under
review, not an executed promoted package or an AppKit acceptance receipt.

## Source and boundaries

Continuation worktree: `ci/input-readiness-promotion`, based on the integrated
prepare-only package/consumer branch. Current prepare-only run38852664400 stays
immutable at `9a42f8510d84bb8b757e71cba1ff8ca45d134b5e`.

The optional `retain_readiness_package` workflow input is false by default. True
requires explicit readiness and package mode plus both reviewed source-inventory
and notice-policy hashes; an invalid router mode fails descriptively and cannot
run the legacy package job. The legacy job body and existing helpers/pin remain
unchanged. The producer, source-built plugin builder, Wasmtime validator, four
compiled readiness source hashes and all 19 consumer freeze hashes are unchanged.
The source inventory remains
`8047ccc7c75929c94c5551ef600081fd27eea501b03ece17785af02f095ffb51`.

`package-patched.py` retains its successful producer/full public owner/actual
silent owner and complete source/legal/plugin gates. Readiness non-prepare mode
additionally requires `--reviewed-readiness-source-sha256` exactly matching the
executed inventory, while the existing promotion gate still requires the exact
reviewed notice policy. The output has a `DIAGNOSTIC-` prefix, distinct readiness
variant/inventory, `diagnostic_only=true`, and the exact source-review assertion.
No public release/default/native activation claim is added. Legacy packages
reject the readiness assertion.

The shared archive verifier takes an explicit expected promotion state; its
default remains prepare-only. Promoted readiness archives additionally require
diagnostic scope and the exact source-review assertion. The focused gate tool
uses the real new package and both actual owners as positive control, then ten
negative controls, including wrong expected promotion state and the real public
packager's refusal of a different reviewed source inventory. The refusal log and
hash are retained; no synthetic package positive or fabricated red is claimed.

The separate seven-day diagnostic package artifact contains only the named
archive and checksum sidecar, and uploads only after producer, both owners,
packaging, focused gates and source/evidence upload succeed. The source/evidence
artifact stays separate and includes raw compiler units and embedded manifests.
This scope permits an independently reviewed exact download tuple later; it does
not itself modify admission or activate the consumer on main.

## Verification and next gates

Python AST parsing, workflow YAML parse, all workflow shell syntax, whitespace,
unchanged 19 consumer hashes, unchanged readiness inventory and legacy job-body
comparisons passed. The dedicated repository check passed: 362 tests in
70 result groups, with three explicit subprocess fixture entries ignored;
strict workspace Clippy, notices and both C ABI owners passed. Dedicated target:
`/tmp/zmux-input-readiness-promotion-check-20261016`. Log SHA-256:
`377d944ebf740628318af8ee62bfc6ca0cb498108c3bc82ac313c1f6d2722776`. No new producer dispatch has
occurred. Current prepare-only evidence review and parent review of the exact
promotion source freeze must pass before another producer run.
