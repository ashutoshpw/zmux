# OSC 7501 test contracts

## Authoring gate and existing evidence

The initial discovery inspected the real parser, worker, desktop, notification
and native presentation boundaries before authoring tests. The implementation
now has focused protocol, engine, real-PTY, Desktop and transport coverage. This
matrix defines the contracts; current executed results and remaining hosted
gates are recorded in [status](../status.md) and the linked repair evidence.

Relevant retained coverage:

- `crates/terminal-adapter/tests/parsing.rs`: fragmented shell OSC/title/cwd, text
  preservation, control-string bounds, bounded notice sequencing and alternate screen.
- `crates/terminal-adapter/tests/staged_engine.rs`: linked-owner publication,
  commit/abort, metadata and causal reply failure, plus history/alternate screen.
- `crates/session-runtime/tests/engine.rs`, `child.rs`, `shell_integration.rs`:
  real PTY ownership, optional engine lifecycle and shell startup behavior.
- `crates/desktop-runtime/tests/agent_lifecycle.rs`, `native_notifications.rs`,
  `attention_owner.rs`, `attention_scopes.rs`, `terminal_engine.rs`: exact owner
  fences, provider evidence, inbox delivery, visibility and optional-engine routing.
- `crates/native-bridge/tests/native_frames.rs`, `apps/macos/Sources/AgentAlertSmoke.swift`,
  `scripts/check-native-sidebar`: serialized frames, native alert behavior and
  packaged sidebar selection. Existing provider fixtures remain adapter regressions.

Each proposed family below states its independent contract, credible regression,
existing gap and strongest owner. The public API must be called by production;
introducing an export, flag or fake owner only to make a test possible fails the gate.
Use a table-driven case within a family, shared literal wire fixtures and independent
expected values. Do not reproduce the entire protocol matrix at every layer.

## Shared semantic owner

These tests belong to the shared Rust protocol component. Production
callers are the vt100 callback adapter and linked Ghostty normalization owner.
They test public report/record/lifetime operations, not private parsing predicates.

| ID | Observable contract and decisive cases | Credible regression / current coverage gap |
| --- | --- | --- |
| P01 | All five states and clear; optional-query-free reports; kind only on blocked; 0/100 progress and indeterminate missing/invalid progress; inapplicable keys disappear. | A valid state is dropped or invalid progress becomes 0; generic notice tests have no typed program records. |
| P02 | Colon pairs, surrounding whitespace, first equals split, padded/unpadded base64, duplicate keys including later invalid app/progress values, ignored unknown keys and skipped malformed pairs; missing/unknown state is ignored. | Parser treats base64 padding as a separator, retains an overwritten value or treats unknown state as idle; no existing grammar coverage. |
| P03 | Invalid base64, invalid UTF-8, newline/tab/NUL/DEL/C1 decoded text and every hard bound reject atomically; boundary values accepted; oversized duplicate cannot bypass a limit. | Earlier fields partly update the record or validation only sees the last key; existing title sanitization deliberately has different semantics. |
| P04 | Entire-record replacement removes omitted app/title/kind/progress; nested ids inherit nearest ancestor app; absent parents allowed; root/parent updates and eviction alter effective inheritance. | Merge semantics retain old fields or copy stale inherited app; no existing hierarchical program store. |
| P05 | Subtree clear removes `build/test` but not `builder`; root clear removes all; invalid/empty/path-traversal-like ids never address root; segment/depth/total byte bounds. | Prefix matching deletes siblings or bad id overwrites root; existing surface identities do not cover protocol paths. |
| P06 | Exactly 64 legal maximal records fit; update least-recent ordering; the 65th evicts the least recently updated record; churn keeps storage/cursors bounded. | Store grows or evicts the newest record; current 32-notice ring is a different resource contract. |
| P07 | Alternate screen and DECSTR preserve; RIS clears; prompt/attached-child exit remove working/blocked, retain chosen idle policy and done/error; elapsed time alone changes nothing. | Reuses provider TTL or clears all statuses at prompts; current alternate-screen tests only cover terminal cells/history. |
| P08 | Identical reports do not create attention transitions; progress/message updates alone are silent; distinct state transitions remain ordered; journal overflow exposes gap plus valid latest records. | Coalescing loses working -> done or a full ring silently invents complete history; existing notice sequences do not encode this contract. |

Use bounded generated/permutation cases for duplicate keys, chunk-independent
semantic inputs and record churn where useful; no new fuzz infrastructure is
required to establish the initial contract. Keep parser processing and allocation
budgets explicit; do not infer memory safety from a test that merely does not crash.

## Engine and session integration

Use public `Terminal` / `Engine` boundaries and real `Session` PTYs. The semantic
suite above owns field grammar; these families protect framing, ordering and
lifetime transport that a pure decoder cannot observe.

| ID | Owner / observable behavior | Credible regression / gap |
| --- | --- | --- |
| E01 | terminal-adapter: BEL and ESC-backslash termination; split at every byte boundary including ESC/ST; multiple reports in a read; visible text before/after unchanged; sequence length includes actual framing. | Callback drops a fragment, counts reconstructed bytes or renders control payload. Existing fragmented OSC test covers title/cwd only. |
| E02 | terminal-adapter plus selected engine: real RIS, DECSTR, alternate-screen and OSC 133 A bytes reach lifetime operations in wire order, including report/prompt/report in one read. | Backend omits a reset callback or applies a final snapshot out of order. P07 cannot verify VT callback integration. |
| E03 | real Session PTY child emits 7501 query followed by CSI c and reads constant 7501 reply before DA; unsolicited reports work; query cannot return stored text. | Only a capability flag was added, DA is unanswered, or reply ordering is reversed. Current notice tests do not test a program consuming replies. |
| E04 | Session writer: preserve earlier admitted input bytes, queue limits and deadlines; short writes, cancellation and blocked reader cannot interleave, publish a staged candidate early or trigger an uncertain automatic resend. | New reply bypasses the sole writer or commits early. Extend existing causal-write failure fixtures rather than invent a fake PTY that supplies the expected reply. |
| E05 | linked Engine and optional Session: raw 7501 metadata reaches shared records; before commit only old records visible; successful commit publishes once; abort/partial reply retains old publication. Include one maximal report and reset event. | Upstream parser supports the OSC but retained ABI/normalizer drops it. Existing staged title/notice tests cannot establish 7501 capture. Do not duplicate P01-P08 here. |
| E06 | Session real child: emit working/blocked, exit, inspect final retained state; done/error survive exit while terminal remains; close/replacement removes old owner. | Reaping discards final status bytes or preserves stale running status. Core state-machine tests cannot prove final PTY drain ordering. |
| E07 | integrated shells: actual supported bash/zsh/fish startup and command cycles emit OSC 133 A, preserve user prompt hooks, do not duplicate command launches and leave headless/unsupported paths unchanged. | Prompt marker clobbers user startup or fires before status bytes. Reuse existing shell fixtures; availability skips are explicit. |

E03/E04 assert bytes actually written/read at the owning PTY boundary, not calls
to enqueue or output produced by the implementation's own encoder. Use bounded
handshakes/deadlines, no arbitrary sleeps. If deterministic write-failure injection
is needed, reuse existing production I/O abstraction and test support; add no
production-only-for-tests interface. Local acceptance does not prove remote receipt.

## Desktop, remote and alert ownership

Use real desktop requests/snapshots and attention records. Keep the grammar in P01-P08.
Production callers are desktop snapshot publication, attention admission and shared
workspace selection; controlled time may enter an existing owner API, not a test-only export.

| ID | Observable contract | Credible regression / gap |
| --- | --- | --- |
| D01 | Two terminal owners may both report root/app/id names without collision; forged `app=claude-code` never registers a provider or alters its lifecycle; stale/replaced owners cannot update/select another session. | Keying by app/path or numeric surface only crosses owners. Existing provider tests cannot exercise the new OSC route. |
| D02 | Adapter active or pending before first OSC alert wins; OSC-first alert locks its command epoch; late adapter affects lifecycle but does not introduce a second sound source; next prompt reselects priority; missing prompt conservatively retains ownership. | Arrival-order-dependent duplicate sound or OSC downgrades an agent. Exercise both source orders and each handoff; no fixtures preselect the winner for production. |
| D03 | OSC-only blocked, completion and error attention follow the documented transitions; initial done/recovery is silent; repeated/progress updates, clear, eviction and mute cannot replay audio; cooldown caps external delivery while records stay current. | Every snapshot sounds, error uses completion sound, or flood handling loses current state. Existing provider alert IDs cannot establish OSC-local transitions. |
| D04 | New alerts carry an OSC source/event identity; native notification request suppresses OS default sound when app-selected sound is used; denied OS permission preserves in-app attention. | An untagged OSC notice takes both sound paths. Existing generic notices intentionally use the default OS sound. |
| D05 | Journal gap and consumer restart/reconcile baseline current records without retroactive sounds; session replacement rejects old cursors/events. | Missed snapshots replay historical done as new completion. New protocol journal is independent of provider event history. |
| D06 | Managed SSH PTY carries report/query bytes without provider helper; disconnect marks availability lost; reconnect/session replacement fences stale records and old alerts. Test real configured SSH loopback where available and actual lifecycle routing separately. | Reuses local socket, attaches status to wrong remote generation, or calls a stale working record live. Synthetic frames alone do not prove SSH byte transport. |
| D07 | Existing provider lifecycle and generic OSC 9/777 behavior retain their own contracts; no OSC 9;4 mapping overwrites 7501 records; plain text/title/PID never changes program status. | Generic notice ingestion inadvertently gains provider authority. Run existing regression owners; add only a distinct mixed-input case where needed. |
| D08 | Maximum legal per-terminal records across supported terminal owners cannot make ordinary control/native snapshots exceed their existing response caps; aggregate projections expose omissions and permit bounded exact-owner detail access. | Serializing all raw records/journals overflows the 1 MiB control or 8 MiB native response limits. Canonical store bounds alone do not protect the serialized desktop boundary. |

Do not claim exact cross-protocol event correlation: OSC has no shared provider
turn/event identity. D02 tests the deliberate source-exclusion policy. Existing
generic OSC 9/777 duplication remains a documented separate issue.

## Native and package acceptance

| ID | Strongest owner / acceptance | Credible regression / gap |
| --- | --- | --- |
| N01 | Rust -> native snapshot decoding preserves source, exact identity, hierarchy, state and progress; optional absent status decodes older/no-record frames. | Schema update silently drops records or breaks normal terminal snapshots. Extend native_frames only for this wire contract. |
| N02 | Packaged app receives literal reports through real PTY; Programs subgroup and provider Agents coexist; blocked/progress/done/error render distinctly, with terminal provenance, safe bidi/markup text and accessibility labels. | Correct Rust records are mislabelled as authenticated agents or not displayed. Rust tests cannot prove AppKit presentation. |
| N03 | Clicking root/child program row focuses the existing exact workspace/surface/session; stale row is unavailable; complete terminal/session/PID inventory stays unchanged. | Program selection creates a terminal or focuses a similarly named app. Reuse native sidebar selection/inventory harness. |
| N04 | Packaged background OSC fixture exercises notification request plus chosen audio routing and mute; retain structured results/screenshots. | Swift delivery ignores source arbitration or mute even though Rust admission is correct. Physical audibility remains a separate manual check. |

Use real packaged-app screenshots and accessibility/native receipts, not a mock HTML
rendering. Hosted AppKit acceptance does not prove physical Mac audio, VoiceOver,
Intel runtime or authenticated provider turns. Keep those proof limits in status.

## Validation sequence

1. Implement P01-P08 at the shared owner with independent expected results; when
   repairing an identified bug, demonstrate the focused test fails before the fix.
2. Run focused new/existing integration owners, for example:

   ```sh
   cargo test --locked -p zmux-terminal-adapter --test parsing
   cargo test --locked -p zmux-terminal-adapter --test staged_engine
   cargo test --locked -p zmux-session-runtime --test child
   cargo test --locked -p zmux-session-runtime --test shell_integration
   cargo test --locked -p zmux-desktop-runtime --test agent_lifecycle
   cargo test --locked -p zmux-desktop-runtime --test native_notifications
   cargo test --locked -p zmux-native-bridge --test native_frames
   ```

   Add focused `--test program_status` targets at the shared/desktop owners as
   implemented. Default staged_engine does not execute its Ghostty-gated tests.
3. For optional Ghostty, use an accepted `ZMUX_GHOSTTY_ARTIFACT_ROOT` and the
   documented `--features ghostty` staged_engine / Session engine commands. Run
   retained Darwin artifact/linkage workflows if dependency/ABI inputs change.
4. Run `scripts/check` before each implementation commit; preserve hooks, assertions
   and environment-dependent skips. Then execute relevant hosted Foundation/macOS
   and packaged native checks at the exact final source. A filtered pass is not a full pass.
5. Download and verify the final universal artifact and task acceptance receipts;
   record inner ZIP checksum, metadata/source, architectures, minimum OS, signing,
   screenshots and each unavailable/manual gate. Planning-only docs need no app rebuild.

## Provider checkpoint repair contracts

- A real PTY prompt written before checkpoint service must appear in its returned
  snapshot and command epoch. The private Session owner regression controls the
  scheduling interleaving without a production seam; the immediate-publication
  negative control failed at zero bytes read. See
  [drain repair](checkpoint-drain-repair.md).
- A checkpoint behind physically backpressured input must time out or cancel
  without publishing a premature success, preserve exact input/reply ordering,
  and allow a later live checkpoint. See
  [PTY deadline and cancellation evidence](checkpoint-pty-evidence.md).
- A lost or replaced SSH stream must release unconfirmed route reservations and
  their predictive alert source locks while retaining confirmed invocation
  ownership. The scoped repair passed independent review; parent reran the
  framed SSH regression with `--nocapture`, observed the actual sequence-gap
  replacement diagnostic and a pass, with no prerequisite skip message. See
  [retirement repair](remote-route-retirement.md) and
  [independent recheck](remote-route-retirement-recheck.md).
