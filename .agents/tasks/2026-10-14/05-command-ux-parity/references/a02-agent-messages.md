# A02a — Durable agent message and inbox contract

Original scope/design, 2026-10-16. The authorized Rust/CLI slice is now integrated;
see [implementation evidence](a02-agent-messages-implementation.md) for exact scope
and remaining provider/native gates.
Current source inspected at zmux `1bb05b179427a49477851dee0be7c2c92bad4dfc`.
The existing A02 work package remains open. Parent owns the plan link and status;
this task adds only this reference, preserving concurrent main work.

## Recommended first slice

Build one original Rust owner for durable text messages, replies, inbox inspection,
message settings and generation-bound consumer claim/ack. Add thin local CLI/socket
adapters. This is useful without a running provider: a send is durably queued to
one exact terminal surface and the inbox truthfully reports that no consumer is
registered. Do not implement delivery by sending bytes to that terminal.

Defer provider hook installation, automatic wake, native Agent Inbox/reply UI,
hibernation, Feed, agent launch/resume and remote relay to later slices. Their
entrypoints must consume this same owner. In particular a generic registration
or a live child PID is not evidence of a supported agent or an idle agent that
can safely be woken. Completing A02a closes a bounded subset of CMD-025..029, not
all `agent.messages`, `agent.inbox-wakeup`, or A02.

## Evidence boundary

The frozen cmux reference is `3ba653b9e5afb11d0bc578031142950d6136697e`, read
through Git objects. Only externally observable behavior/contracts inform this
note; no GPL/BUSL code, algorithms, test bodies, renderer prose or assets are to
be imported or translated.

- Existing audit: `audit-agents-remote.md`, Agent ownership and delivery;
  `audit-agents-remote.json` entries `agent.messages` and `agent.inbox-wakeup`;
  `closure-work-packages.md:168`; frozen command coverage CMD-025..029 remains
  unresolved in `apps/cli/src/registry/frozen-coverage.json:181`.
- Reference behavior: `docs/agent-messages.md:1` describes durable target/reply,
  four public states, separate chosen sender name/surface attribution, opt-outs,
  provider-specific handoff and no terminal typing. CLI parser/adapters are in
  `CLI/CMUXCLI+AgentMessages.swift:82,138,184,236,285,372,397` and settings in
  `CLI/CMUXCLI+AgentMessageSettings.swift:9,75`. These are reference observations,
  not current Zmux capabilities or native execution evidence.
- Reference target resolution is explicit IDs/refs or exact/unique-prefix workspace
  title; workspace resolution selects a current agent terminal or terminal fallback
  (`Sources/TerminalController+AgentMessages.swift:393`). Its hook methods separate
  poll, deferred claim, lease acknowledgement and mark-read (`:245,283,327`). Native
  reply field focus and duplicate submission have their own owner
  (`Sources/AgentInbox/AgentInboxView.swift:304,341`).
- Zmux source has no agent namespace, durable message journal, provider registration,
  poller or wake owner after targeted dispatcher/domain searches. Its notification
  inbox is a separate human attention lifecycle (`attention.rs:213,607`), and its
  native notification reply deliberately writes to a PTY (`replies.rs:18,49,75`).
  Neither can stand in for durable agent delivery.
- Reuse stable namespace-derived UUIDs and explicit scope validation
  (`workspace-core/src/targets.rs:200,308,316,399`). Caller environment is local
  attribution input, not provider authentication (`apps/cli/src/targets.rs:611`).
  That helper deliberately drops ambient caller when an explicit recipient scope
  exists: message sender resolution must be independent of recipient resolution.
- Reuse socket policy and executor revalidation (`control_policy.rs:179,233`),
  private worker I/O, locking and atomic commit machinery
  (`persistence.rs:456,536,736,809`), and durable-operation projection patterns
  (`profiles.rs:125,160,205`). Do not reuse notification focus/read semantics.

All abbreviated Rust paths above are under `crates/desktop-runtime/src/` except
where `workspace-core` is named. Source inspection proves available boundaries,
not a working message path. No provider, native UI or live messaging test was run.

## Authoritative model and targeting

One message has immutable message UUID, request UUID, thread UUID, optional parent
message UUID, exact sender attribution snapshot, exact recipient surface UUID and
admission workspace UUID, UTF-8 body, sequence/timestamps, and delivery history.
Public state is queued, delivered, read or failed; claims/attempts are separate
receipts, never a fifth implicit successful state. Body and chosen sender label
are data, never shell commands or instructions from the human operator.

Send requires exactly one explicit target or parent reply UUID. Resolve against
one executor model publication and pin namespace plus object UUIDs before enqueue.
A surface target must be a live terminal; browsers, completed saved-history surfaces,
wrong-kind, stale and cross-scope selectors fail. No implicit foreground recipient.
Workspace title lookup uses exact match, then unique prefix, and rejects ambiguity.
Existing `TargetRef` does not parse titles; add a message-specific resolver that
normalizes a successful lookup to the existing stable target owner rather than
changing every command's target grammar.

For a workspace target, prefer its focused registered interactive agent terminal;
otherwise require a unique registered interactive agent terminal. With no registered
agent choose that workspace's focused live terminal and return
`recipient_has_agent: false`; if no such terminal exists, fail. Multiple unfocused
registered consumers require an explicit surface. This deterministic ambiguity
failure is a deliberate first-slice difference from the reference's tie-breaker.
The resolved recipient is immutable even if focus, title or current agent changes.

Resolve optional sender caller independently, including managed tmux binding and
surface/workspace consistency. Capture its UUIDs, workspace title and attribution
kind (`local_surface_claim` or `external_local_user`). A supplied `--from` only
changes the bounded display label; it cannot forge provider identity or permission.
Unknown external callers may send but have no replyable surface unless they supply
valid, consistent local attribution. Same-user/full-socket authorization is the
actual authority; this is not multi-user or hostile same-UID process isolation.

Reply resolves the stored parent's exact sender surface, not its former title or
current focus. Require a live matching namespace/object UUID, preserve parent ID,
and inherit its thread. Missing/closed sender and conflicting explicit thread or
target fail with no replacement destination. Messages without a sender surface
remain inspectable but cannot be replied to through this address mechanism.

A live surface move preserves its surface UUID and messages; retain the original
workspace snapshot for attribution and workspace opt-out matching. Explicit close
fails queued messages for that exact removed surface; undo-created new identities
never inherit them. Ordinary saved-session restart may restore the same durable
surface UUID with a fresh session ID: queued history survives, but delivery requires
new live registration. No process/consumer/lease is recreated from saved bytes.
Do not interpret startup-before-restore, orderly Desktop shutdown, an in-flight
restore or another Desktop's namespace as canonical recipient closure. Failure
requires an exact committed removal in the owning model, not generic absence
from an incomplete or unrelated snapshot.

## Durability, bounds and settings

Keep an installation-state journal separate from portable `SessionDocument`.
Import/export of a workspace must not clone an inbox, consumer capability or pending
wake. Use the configured private state directory; transient desktops explicitly
report messaging unavailable. Coordinate file names with S01 reserved-path checks
so generic session save/open cannot overwrite/import the message journal or lock.

All read/validate/lock/serialize/sync/rename work stays on a bounded worker, not the
workspace executor, socket listener or MainActor. Reuse existing secure descriptor
walking and atomic-write primitives with a narrow internal API; do not duplicate a
second path-safety implementation. Serialize journal transactions across Desktop
instances with one private file lock and revision compare; re-read the authoritative
journal under the lock before each read-modify-write. No stale cached writeback.

Return attributable admission first, with request UUID and pending operation receipt;
the CLI reports queued success only after the worker confirms durable commit.
A failed or uncertain sync must not claim Saved or trigger consumer handoff.
Before returning claimed text after worker completion, revalidate the exact live
surface/session/consumer and effective policy; a close or disable while I/O was
pending must not expose a stale candidate. Reconcile/cancel only its exact lease,
without retargeting or releasing another consumer's work. Expose
operation status by the original request UUID; uncertain responses never cause an
automatic fresh send. Repeated retained request UUID plus identical canonical input
returns the same message; conflicting input fails. Retain request evidence for every
unsettled message plus the newest 256 settled request receipts, inside the same byte
budget. This is a retained-request horizon, not permanent exactly-once admission.
After pruning, status must say unknown-or-expired and the CLI must not automatically
retry the send. Publish that limit with the operation receipt.

Proposed initial bounds: 4 KiB body, 64 UTF-8-byte single-line sender label,
128 messages, 256 explicit setting entries, eight pending worker completions,
32 inbox rows per page, eight messages per claim, at most 128 current consumer
registrations and 128 active leases, and 512 KiB full serialized journal.
Reject unsafe body controls except newline/tab; preserve intentional whitespace.
Validate full escaped candidate bytes before admission and commit. Never evict queued,
leased or delivered-unread work to accept a new send; capacity fails explicitly.
Only settled read/failed history may be pruned under a documented policy, retaining
parent addressing or returning a truthful expired-parent error.

The smaller body is intentional: current `control.rs:32` caps the complete request
at 16 KiB, whereas the reference accepts a 32 KiB body. Four KiB plus worst-case
JSON escaping leaves room for the bounded envelope/password/identities. Still
validate the complete request; do not silently raise the global socket bound.
Pagination/rendered claim output must also fit the existing 1 MiB response bound.
Reference-sized bodies and retention are explicit remaining compatibility gaps.

Store global, surface and workspace enablement with the journal. Settings reads
report the effective reason and exact owner. No target means a validated caller,
never arbitrary focus. A workspace selector without workspace scope pins its
resolved one terminal; explicit workspace scope controls that exact workspace.
Off refuses new sends without append and atomically fails affected queued,
unhanded messages with a typed reason. On does not resurrect failures. Workspace
off matches the workspace stored at send, even after a later surface move; new
sends use the recipient's new workspace. Do not silently evict an active opt-out
and turn messaging back on when the settings budget fills: fail explicitly.

## Consumer, lease and wake ownership

Registration is a separate local-full mutation for one exact live terminal surface.
Bind a fresh consumer generation to namespace/surface UUID, current SessionId,
Desktop epoch, provider kind and declared interaction capability. Keep a single
current consumer per surface across the locked journal; newer explicit registration
supersedes old pollers. Revalidate current surface/session and socket policy on
claim and ack. Provider PID or ambient surface variables alone cannot establish
idle/busy/held status or permission to execute an agent. A02a may expose a neutral
consumer protocol but must not label its registrations as authenticated providers.

Poll returns bounded counts/held/registration status immediately and claims nothing;
never occupy a socket connection while waiting. Poller registration/supersession
mutates state and therefore requires full access. A status-only read can be read-only.
Claim atomically reserves an exact bounded message-ID set for the current consumer
and lease deadline. Concurrent or superseded consumers cannot receive the same
active lease; no message is marked read merely because it was listed or claimed.
Claim/ack keys need fresh unguessable capabilities; retain only hashes where durable
correlation is required, never plaintext bearer secrets or payloads in logs.

Ack names exact lease, consumer generation and message IDs; wrong owner/session,
expired/unknown lease, replay with different data and old runtime generation cannot
advance state. Repeated valid ack is idempotent for the same retained outcome.
A committed handoff followed by off may still record its exact valid acknowledgement:
settings cannot unsend already handed data. Off before handoff prevents exposure;
lease expiry after off fails its still-queued messages. Ordinary failed messages
cannot be revived by an unrelated stale ack. Turn-completion read acknowledgement
uses explicit delivered IDs/generation, so a new arrival cannot be marked read by
an older turn's broad cursor.

Durability and an external provider cannot be atomically committed together.
A crash/lost ack after context exposure can lead to duplicate delivery under lease
retry. State this at-least-once boundary and require stable message IDs for adapter
reconciliation; do not advertise end-to-end exactly-once delivery. Lease expiry,
restart and supersession must never replay a terminal write, restart a process or
manufacture provider acknowledgement. Defer automatic wake until an adapter has
an explicit tested context-handoff capability and a safe policy for this uncertainty.

Later hook adapters must deliver context through the provider's supported hook,
never PTY paste/Enter. Disabled, missing-surface, headless and unsupported-provider
contexts return the provider's no-op response and claim nothing. Held permission,
question or plan dialogs block handoff. Claude idle wake and Codex prompt-submit/stop
are distinct capabilities: the frozen reference explicitly cannot wake idle Codex
(`docs/agent-messages.md:24`). Preserve that truth rather than inferring readiness
from a running process or copying the reference's polling interval.

## Entrypoints and ownership for implementation approval

Proposed public forms (JSON-first, offline help):

- `agent message TARGET [--from LABEL] [--thread UUID] [--request-id UUID] [--] TEXT|-`
  and alias `agent msg`; `--reply-to UUID` replaces TARGET and inherits its thread.
- `agent inbox [--surface HANDLE] [--state STATE] [--limit 1..32] [--cursor CURSOR]`
  with optional `--mark-read` applying only the returned exact ID set.
- `agent messages on|off|status [TARGET] [--workspace|--global]`; scopes are exclusive,
  and global scope accepts no target. Without target/global, require caller evidence.

Use one typed `ControlCommand::AgentMessages { request }` family plus operation-status
queries; follow existing versioned request conventions rather than adding a second
socket protocol. Consumer register/poll/claim/ack/read-turn operations are explicit
internal adapters of this family, not arbitrary send-text aliases. They need schema
and capability discovery, including unsupported provider/wake state. Lease TTL is a
bounded 10 seconds initially; use monotonic time for a live owning runtime, invalidate
old runtime/session capabilities on restart, and never infer renewed authority from
a persisted wall-clock deadline. Multi-Desktop supersession must be serialized under
the journal lock, with old ownership rejected before any further handoff authorization.
Authorization linearizes at the locked, fsynced `handed=true` commit. An already
authorized in-flight response may arrive after foreign supersession, within its
original lease deadline; fresh handoff/status/ack must reject the old generation.
This is not instantaneous revocation or exactly-once provider consumption.

| Owner | Files / responsibility | First deliverable |
| --- | --- | --- |
| Runtime/domain owner | New `crates/desktop-runtime/src/agent_messages.rs` and subordinate storage module; scoped `lib.rs`, `control.rs`, `control_policy.rs`, `shutdown.rs`; minimal worker-safe persistence helper API and reserved names | One schema/state/target/policy owner, locked durable transactions, status receipts, consumer leases, bounded shutdown; real filesystem/domain tests |
| CLI owner | New `apps/cli/src/agent_messages.rs`, scoped `main.rs`, `registry.rs`, `registry/contracts.rs`, frozen coverage and CLI tests | `agent message`/`msg`, reply-to, stdin/`--`, `agent inbox` filters and exact selected-ID mark-read, `agent messages on/off/status`; thin typed requests to runtime |
| Later provider owner | New provider adapter/hook module and tests; shared consumer protocol only | One explicitly selected provider/version, fail-open guards, held/draft safety, claim/ack correlation, actual context handoff; no launcher/hibernation scope |
| Later native owner | Swift bridge/model/inbox/view command entrypoints | Resolved-window inbox and original reply IDs, draft retention and one in-flight reply; no separate store or PTY reply reuse |

Coordinate shared-file edits with current B01/S01 owners; do not overwrite their
pending work. Workers are not alone and must preserve each other's changes. No
new package is justified yet. Local socket methods and domain schemas are the
first operational surface; no new MCP/A2A/remote-relay promise. Read-only sockets
may inspect/list/status; send, settings mutation, mark-read, registration, poller
supersession, claim and ack require full authorization with execution-time revocation.
CLI discovery must remain offline and describe unavailable provider/native gates.
Do not mark the frozen reference rows fully implemented from a smaller JSON-only,
4 KiB, no-native/no-provider subset.

## First behavioral regression and acceptance sequence

1. Authoring gate: use the actual persistent Desktop and real public CLI/socket,
   not stand-in message types or source-string assertions. Extend the existing
   CLI integration fixture in a new `apps/cli/tests/agent_messages.rs`. Start two
   terminal surfaces, place focus elsewhere, send from a known caller to an
   explicit surface with exact Unicode/whitespace stdin, inspect the queued
   message and its immutable IDs/attribution, then close and reopen Desktop on
   the same private state and verify the same queued record. The initial baseline
   should execute this test and fail on missing send/inbox behavior after the
   fixture is ready; compilation/setup/zero tests are not the behavioral red.
   Observe the recipient's real terminal input counter remains untouched. Commit
   that focused failing contract before the repair, then rerun the identical command.
   Proposed first test name:
   `durable_queued_message_survives_restart_without_terminal_input`; focused command:
   `cargo test --locked -p zmux --test agent_messages durable_queued_message_survives_restart_without_terminal_input -- --nocapture`.
2. At the production domain owner, cover wrong/ambiguous/stale target, explicit
   recipient preserving independent sender, reply with changed focus/title and
   closed sender, cross-workspace surface move, fresh session after restore,
   disabled sends/queued failure, off/claim/ack ordering, and no resurrection.
3. With real worker/storage and two independent clients, prove one current poller,
   disjoint active leases, supersession, stale/expired/duplicate ack, read-generation
   fencing, capacity-before-append, exact request-ID conflict/dedup and lease/lost-ack
   semantics. A claim receipt is not provider consumption or physical wake proof.
4. Corrupt/symlink/oversized journal, concurrent Desktop writers, filesystem failure
   and shutdown during an accepted write must preserve authoritative bytes/IDs and
   report uncertainty without replay. Explicit session export/import must not carry
   inbox/credentials. Queue saturation must leave unrelated workspace commands live.
5. Run scoped CLI/runtime/storage tests and required `scripts/check` on the final
   source, with red/green receipts. Separately review before provider/native work;
   actual hook context, held draft/permission behavior, supported idle wake and
   AppKit inbox/reply ownership require their own execution evidence.

The first approval should authorize the Rust/CLI A02a slice only, with the above
bounds and visible parity limits. No provider installation, wake, sending external
messages or changing the user's running applications is implied by this design.
