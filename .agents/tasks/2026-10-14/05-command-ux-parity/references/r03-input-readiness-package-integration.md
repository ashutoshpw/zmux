# R03 readiness package integration gate

2026-10-16. The parent reviewed package implementation v2 and authorized one
Darwin ARM64 prepare-only run after integration with the frozen consumer and a
successful repository check. This record establishes the pre-dispatch gate;
it does not claim producer, public-owner, archive or negative-control success.

## Exact inputs

- Consumer delivery: `08cdef92a4a867dcf0ae6efe1368390df5e70b50`.
- Consumer freeze manifest SHA-256:
  `983bc4e9ca8bdcaa5e8c76df6119513613fe3035ac0d153286db2966726d8e9d`.
  All 19 frozen source hashes match the combined worktree.
- Package review v2 manifest SHA-256:
  `fe2f5c45de223aaba0cc81f06fda0ef01afce176f9783e7f52ac69ecb391544e`.
  All 11 reviewed source hashes match the combined worktree.
- Reviewed readiness inventory SHA-256:
  `8047ccc7c75929c94c5551ef600081fd27eea501b03ece17785af02f095ffb51`.
- Unchanged notice policy SHA-256:
  `1dba7394d65347f3ec8042b53ddb57341c6b46482f24118e10962e94931e8d79`.
- Unchanged protocol SHA-256:
  `6b82937ff88c4325c0cdcec2b016ab2fd47ef8306948f67bd97a2a360a4332e4`.

The combined branch cherry-picks dependency implementation `d36bc912` and
acceptance documentation `05c46eeb` after the exact consumer delivery. Its sole
integration conflict was the task reference list, resolved by preserving the
existing consumer plan and incoming dependency references. No runtime source
changed during this integration.

## Local verification

`CARGO_TARGET_DIR=/tmp/zmux-input-readiness-package-integrated-check-20261016 scripts/check`
passed against the combined source: 362 tests in 70 result groups, with three
explicit subprocess fixture entries marked ignored; strict workspace Clippy,
notices and both C ABI owners passed. No concurrent Cargo writer touched this
target. Log SHA-256:
`5402b60fd8f0fc5eb00c850b1133b06401579aa6d81e942787189a996a3ddddd`.

Python AST parsing, both workflow YAML parses and `git diff --check` passed.
The seven existing package/public-owner helpers are byte-identical to the
reviewed dependency worktree. The v6 source/binary, retained package pin,
ordinary acquisition, AppKit/default activation and legacy workflow job body
remain unchanged.

## Authorized execution boundary

One named workflow dispatch may build fresh source plugins and the optional
readiness dependency, run the actual silent owner and frozen full public owner
sequentially, prepare the distinct ephemeral `input-readiness-v1` archive, and
exercise its focused positive/negative archive controls. Producer and public
owner Cargo targets are separate. Upload source and evidence only, including
raw compiler-unit and embedded-asset manifests. Do not upload package,
executable or negative-control archive bytes. Record the exact combined commit,
run ID and outcome separately. Package publication and AppKit bundle admission
remain outside this authorization.
