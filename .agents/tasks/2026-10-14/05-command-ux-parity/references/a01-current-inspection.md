# A01b — Bounded published current inspection

Updated: 2026-10-15. Portable slice implemented; full A01 and TOPO-027 remain open.

## Scope and provenance

Implement original MIT `current [--limit 1..200] [--json]` against exactly one
published Desktop snapshot. Public pinned cmux help was read at
`3ba653b9e5afb11d0bc578031142950d6136697e:CLI/CMUXCLI+Current.swift:5–22`
for bounded, read-only, app-wide inspection semantics. No implementation, tests
or assets were imported or translated. Zmux retains its consistent JSON output;
historical text/JSON schema compatibility is unverified.

Report canonical window/workspace/pane/surface order, stable namespace/UUID/ref
identities, publication/model revisions, logical focus and selected tabs. Terminal
facts come from `Snapshot.control_frames`, including hidden tabs. Published child
PID/state is local lifecycle evidence, never an active task, current command,
remote PID, remote health or proof that a process is still alive. Browser activity
and human obligations remain unknown. Workspace metadata and matching unread
notification counts are observations, not inferred work or permission to act.

The limit defaults to 100 and applies to surfaces; total/returned/truncated are
explicit. Wire validation rejects 0 and values above 200. A 512 KiB encoded report
bound rejects oversize output before publication. No transcripts, raw log text,
filesystem/process/network queries, refresh, focus or model mutations occur.
Freshness is the cached publication revision; no observation timestamp is invented.
The local read-only socket permits this method; remote relay remains denied.

## Authoring gate and verification plan

Production owner: the published Desktop snapshot and local control dispatch;
strongest boundary: shipped CLI against a real Desktop/control server, reusing
the existing metadata fixture. Existing metadata tests never inspect hidden
terminal states or an app-wide bounded report. Credible failures include parser
absence, wrong owner/hidden tab omission, treating shell lifecycle as a task,
cross-workspace attention attribution, focus mutation and read-only denial.
No test-only production seam is needed. Raw socket cases independently enforce
the wire limit because the CLI parser cannot establish server-side validation.

- [x] Record actual CLI regression red before implementation.
- [x] Implement bounded owner report, control read classification and CLI/catalog.
- [x] Pass real CLI owner/limit/read-only tests and focused formatting/Clippy.
- [x] Record portable evidence and unresolved native/Cloud/full-inspection gates.

Full Cloud inspection, task/resource knowledge, tree/top/memory/health, event
streams, PR/todo owners, native packaged CLI execution and frozen output parity
are separate gates. This slice must not close TOPO-027 or A01.


## Portable verification receipt

All Cargo commands below used the private target
`target/agent-validation/workspace-current`.

- Red: `cargo test -p zmux --test metadata current_inspects_hidden_tabs --locked`
  failed at the real CLI with `unknown command current`, before owner/parser edits.
- Green: `cargo test -p zmux --test metadata --test cli --locked` passes all 19
  cases (six metadata/current, 13 existing CLI/discovery/transport cases).
  Current covers exact namespace/UUID/ref ownership of a hidden running tab,
  ambient caller independence, unknown active work, read-only access, matching
  surface/workspace attention without foreground leakage, metadata summaries
  without log content, stable order/truncation and unchanged topology/focus.
  Browser activity remains unknown with no terminal session fabricated. Raw
  socket requests establish the server default 100, accept 200 and reject 0/201;
  CLI cases reject malformed, repeated and unsupported arguments.
- `cargo clippy -p zmux -p zmux-desktop-runtime --all-targets --locked -- -D warnings`
  passes. Scoped rustfmt and `git diff --check` pass.
- `cargo test -p zmux-desktop-runtime --lib control_policy --locked` passes
  four existing authorization/revocation/relay cases.
- Shipped binary `commands --check` passes 74 contracts/428 frozen entries;
  compatibility remains false. Offline `help current` displays the limitations.

The response contains only fixed-schema scalar observations and stable identities;
no workspace names, arbitrary metadata/log strings, URLs or terminal content are
copied. All workspace summaries remain present even when the surface limit
truncates items. Only unread notifications whose namespace and still-live owners
match are counted; closed/foreign surface evidence is not reassigned to work.
The 512 KiB encoded guard is defensive; no max-size capacity workload is claimed.
A local tmux surface reports a local tmux client scope; managed SSH reports
only a local SSH transport child. Published PID is explicitly not refreshed.
Standalone CLI Zellij owners have no Desktop surface descriptor and are outside
this report; a shell running such a CLI is still only an observed local child.

No native build, macOS execution, deployed Cloud execution or full frozen
compatibility acceptance is established by this receipt. Parent integration and
independent review remain delivery gates.

Parent independent source review found no remaining blocker in this slice. Combined
isolated `scripts/check` passes289 tests, strict Clippy, notices and C ABI with the
reviewed Session diagnostics included. Native packaged acceptance remains open.
