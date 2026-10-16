# A02b: first Claude Code hook adapter

2026-10-16. Approved implementation boundary; owner prerequisites and adapter work proceed in
`feat/claude-message-hooks` from main `69ff15c`. No provider turn or user
configuration mutation is authorized. Original MIT design.

## Decision and useful result

Support Claude Code 2.1.301 first, for synchronous root-session
`UserPromptSubmit` context submission through a session-only local plugin. The
adapter never types, pastes, presses Enter, edits a human draft, launches a turn
from idle, or derives permission from terminal text, elapsed silence or a live PID.
`Stop` cannot establish acceptance or acknowledge messages. Only a future
independent provider protocol receipt could authorize Ack; a correlated Stop could
then mark exactly those Ack-confirmed IDs read. This slice never calls Ack or
ReadTurn, claims from Stop, or requests a continuation. Claude
`asyncRewake`, background polling, Codex, Feed, hibernation, remote sessions,
provider accounts and general launch/resume recovery stay out of scope.

The useful first result is an explicit, bounded attempt to attach one queued
message to an already occurring supported provider request, with durable status
showing whether submission was attempted and whether its outcome is unknown.
The existing CLI inbox remains authoritative. Coordinate native queued-inbox and
attempt detail with its separate owner; do not add another message store or hold
provider design on current native UI absence. No full A02/CMD-025..029 parity claim.

## Verified inputs and limits of the evidence

- Main inspected at `69ff15c266f768e7f37fe3817bc389d6ef56f3e8`; A02a owner delivery
  at `983d2c2c15502b136b88d4f00758ca1a1ff865c7`. Read the original
  `a02-agent-messages.md`, implementation evidence, `closure-work-packages.md`
  A02, and the agents/remote audit. A02a implements Register/Poll/Claim/Ack/
  ReadTurn, eight pending credits, eight-message leases and original ten-second
  deadlines. It does not authenticate a provider or implement a native inbox.
- Installed read-only probes: Claude Code `2.1.301`; Codex CLI `0.162.1` with
  `hooks` listed stable/enabled. `--help` and version/feature output establish
  available configuration interfaces, not actual hook execution/context delivery.
  No model invocation, authentication probe, transcript read or paid call occurred.
- Official Claude hooks reference, fetched 2026-10-16:
  <https://code.claude.com/docs/en/hooks> and plugin reference:
  <https://code.claude.com/docs/en/plugins-reference>. The installed CLI advertises
  session-only `--plugin-dir`, additional `--settings`, and headless `--print`.
  Hook exec form uses a fixed executable plus argument array. Root input includes
  session ID and current prompt ID; strict support must verify those fields in
  the actual installed version, rather than invent missing correlation.
- `UserPromptSubmit` is a supported context point, but also fires for scheduled
  tasks, background-subagent reports and cross-session messages. Do not describe
  every invocation as a human typing or use it to infer idle state. The adapter
  adds context only to an already occurring provider request.
- Official docs describe stdout/additionalContext becoming a system reminder.
  They do not expose a synchronous, stable acceptance callback to an interactive
  command hook. They recommend debug-log inspection for confirmation. Transcript
  writes may lag; injected prompt context has no visible transcript entry. Debug
  or transcript scraping cannot be a production acknowledgement protocol.
- Claude's hook lifecycle output stream is advertised for `stream-json`/headless
  operation. It is not proof of an interactive receipt channel. Headless is
  deliberately excluded here; no undocumented interactive callback is assumed.
- Additional context above 10,000 characters is replaced by a file path and a
  preview of at most 2,000 characters. Existing Claim can return eight 4 KiB
  bodies, so rendering the entire batch would not prove complete inline context.
- Frozen reference read through Git objects only: cmux
  `3ba653b9e5afb11d0bc578031142950d6136697e`, `docs/agent-messages.md` and
  `docs/cli-contract.md`. Its observable contracts distinguish hook delivery from
  terminal typing, held prompts, headless no-op, Codex no idle wake and independent
  install/uninstall. No cmux implementation, tests, algorithms, renderer text or
  assets were read/copied for this adapter design.
- Official OpenAI documentation, fetched independently:
  <https://developers.openai.com/codex/hooks>. Codex supports UserPromptSubmit
  additionalContext and Stop continuation; Stop creates a continuation prompt.
  Changed hooks require explicit trust. Installed Codex also supports a shared
  daemon and `--no-daemon`; a later adapter must verify session/environment routing
  rather than assuming hooks execute under a directly owned TUI process.

Existing user hook configurations were inspected for event names/counts only,
without exposing commands, prompts, credentials or bearer values. Both contain
many unrelated hooks, so replacing a complete event map is unacceptable.

## Delivery truth: submission is not provider acceptance

Do not call Ack because a hook wrote stdout, flushed, exited zero, emitted valid
JSON, received a later generic lifecycle event, or prompted the model to call a
receipt tool. A model-generated callback alone is not independently verified
protocol acceptance and no such callback workaround is proposed.

Until a documented, executed interactive provider receipt exists, the supported
outcome after any possible context exposure is **delivery_unknown**. The public
message stays queued, with a separate durable attempt record; it is not labelled
delivered or read. Exact transport submission can be reported as "Sent to hook;
provider acceptance unconfirmed". This is useful evidence for an explicit retry
choice, not a successful provider-consumption claim.

A future receipt can enable Ack only if an independent provider-owned channel
binds the actual acceptance to provider runtime/session/prompt, adapter generation,
consumer, exact lease/message IDs and complete payload digest before the original
lease deadline. Capturing a versioned acceptance event in an owned provider
transport would be a separate reviewed capability. No refreshed lease, guessed
receipt, debug-log parser or arbitrary model action may substitute for it.
ReadTurn remains unused for unknown attempts. If receipts later become available,
Stop applies it only to the exact Ack-confirmed IDs for that exact completed
prompt/generation; a newer arrival cannot be marked read by a broad cursor.

## Small owner prerequisites

1. Add optional bounded Claim size (`max_messages`, initially request **1**, default
   existing behavior for existing callers). Preserve one exact lease set; never
   truncate a returned batch or acknowledge IDs whose full bodies were omitted.
   Enforce a complete rendered-context budget below the provider's 10,000-character
   threshold, including our original headers. Reject oversize before any write.
2. Add exact-capability consumer retirement. Retire names surface, consumer and
   current bearer capability, checked under the authoritative installation lock.
   It removes only that current generation and its own reservations. If superseded,
   return that fact and leave the newer registration/leases untouched. A held Poll
   is not retirement; Register(interactive=false) is unsafe as a retirement shortcut
   because it can overwrite a newer consumer.
3. Add durable exposure-attempt metadata to the same inbox owner/journal: attempt
   UUID, immutable message IDs, target namespace/surface/SessionId, consumer/lease,
   provider/runtime/session/prompt correlation, payload digest, original expiry,
   and preparation/submission/unknown outcome. Store no duplicate body or plaintext
   consumer token. Preserve the four existing public message states.
4. Prepare/suppress the exact attempt durably **before** returning its body for
   possible stdout exposure. A crash after preparation is conservatively unknown,
   including a crash before the actual write. A post-write status failure must not
   erase that fence. Unknown attempts are excluded from future automatic Claims,
   including after provider/helper/Desktop restart; they must not block unrelated
   queued messages or be evicted to admit new work.
5. Provide an explicit full-access retry operation against the original immutable
   message IDs and current exact target. It creates a new attempt, not a new Send
   UUID/message. Explain that an unconfirmed prior submission may already have
   arrived and retry can duplicate it. No timer, lease expiry, new registration,
   restart, hook replay or next prompt silently authorizes that retry. A provable
   zero-byte pre-submission failure can be recorded separately, with no claim that
   uncertain partial JSON could not have been treated as plaintext by the provider.

These are proposed bounded changes, not existing A02a capabilities. All journal
I/O, sync, locking and unknown-attempt reconciliation remain on its worker.
Executor and hook paths use bounded queues/nonblocking policy checks, never I/O
under the shared live-view lock. Keep parent-approved fsynced `handed=true`
linearization: an already authorized in-flight response may arrive after foreign
supersession inside the original TTL; fresh handoff/status/Ack reject the old
consumer. Do not claim instantaneous global revocation or exactly-once delivery.

## Provider registration and held safety

Use a narrow opt-in interactive binding wrapper plus an owned per-launch helper;
this is adapter bootstrap, not A03 general agent launch/resume parity. Record the
canonical surface UUID, exact current SessionId, installation/namespace, wrapper
and retained provider Child lifetime/full image, supported provider version, local
adapter generation and root provider session ID. Validate current route through
the shared Desktop owner. Ambient ZMUX IDs, provider labels and a PID alone are
insufficient. Never retarget to focus after a move, close, restore or reconnect.

The wrapper preserves provider arguments and uses only its installed session-local
plugin. It refuses headless/print/streaming/background/cloud/desktop attachment,
unsupported/unknown invocation modes and missing interactive routing before
registration. Parse the supported provider grammar, including literal `--`, rather
than treating a prompt containing a flag as a mode switch. Missing/disabled hooks,
`--bare`/safe-mode, missing surface, unavailable durable state, readonly socket or
unsupported version produce no registration/exposure and an honest local status.

SessionStart registers once only after the exact root provider session binding is
validated. Resume, compaction, duplicate SessionStart and subagent events must not
silently supersede the root generation. A new explicit launch/resume has a new
adapter generation and registration; no old token is restored from disk. The helper
retains the neutral bearer in memory and keeps it out of logs, hook context and
provider model context. SessionEnd, normal Child exit, wrapper loss and uninstall
request exact retirement with a fixed cleanup budget; failed retirement is reported
and never retries by overwriting a new generation. Specify a safe stale-registration
projection if helper loss makes exact retirement unavailable.

Only synchronous UserPromptSubmit can attempt new context in this slice. Strictly
bind root session and nonmissing prompt ID, reject malformed/oversized payloads,
ignore subagents, and never log the human prompt/transcript. Unknown event fields
are bounded/ignored as schema policy permits; absent required correlation is a no-op.
Use typed PermissionRequest/Elicitation/question-tool events to set held state.
Clear it only on a positively correlated provider completion/resolution event that
this version actually exposes. Unsupported or unpaired held events remain held
until explicit local resume/new registration; never infer resolution from output,
permission_mode alone, a timer or another unrelated tool. Plan mode is conservatively
held until supported resolution is established. Permission/question/plan controls
and an unrelated human draft must remain untouched.

Render an original, bounded data envelope identifying the sender as a chosen label,
plus message/thread/reply IDs and exact body. Treat it as third-party data beneath
existing operator/developer instructions; do not copy the reference renderer or
promote sender body into operator instructions. Emit one valid JSON hook response
with the exact provider event name and additionalContext. Fixed executable/argv,
JSON serialization and no shell evaluation of body/label are required. No fallback
PTY write or hidden continuation is permitted.

Each hook has a fixed total budget (propose 3 seconds; never beyond its remaining
original lease), one bounded IPC request and no background loop. On failure it
returns the provider's nonblocking no-op (`{}`/exit0), while preserving the exact
attempt/unknown status in Zmux. Do not block the human's prompt with exit2 or a
blocking decision merely because messaging is unavailable.

## Install, update, uninstall and rollback

Proposed commands need explicit implementation approval; none exist yet:
`zmux hooks claude install|status|uninstall` and an explicit interactive run wrapper.
Install first supports an owned local plugin artifact under Zmux's private state,
with `.claude-plugin/plugin.json` and `hooks/hooks.json`; invoke it through the
provider's supported `--plugin-dir`. No `.mcp.json`, receipt tool or model callback.
There is no global Claude settings edit in this first mode. Existing user/project/
managed hooks remain loaded and may veto or supersede model processing; that is
another reason our output alone is not proof of delivery.

Install/update validate the exact supported executable/version, owned paths/modes/
no-follow files and complete generated manifests before atomic publication. Record
owned file hashes and format/version; retain a recoverable previous owned version.
Idempotent repeat changes nothing. Do not trust/enable Codex hooks, install a remote
plugin, change provider permissions, edit project instructions or replace unrelated
user files. A future global merge installer is a separate contract and must preserve
unknown settings/event groups, compare source version and remove only exact owned
entries; this plan does not assume that implementation.

Uninstall first disables new adapter admissions, retires only exact active adapter
registrations, and removes only verified owned artifacts. A changed/unknown file is
retained with a diagnostic; no recursive deletion of an unverified replacement.
Do not terminate the user's provider or terminal. A missing plugin/helper after
partial uninstall must no-op, not damage a human prompt. Report partial retirement,
publication or removal and the exact safe rollback path. Rollback must not revive
old capabilities, unconfirmed attempts or auto-resends.

## Companion native projection

Coordinate with `feat/native-agent-inbox`; no duplicate UI owner. Keep queued
messages available through the existing shared owner and project attempt detail:
"Submission unconfirmed", exact provider/version, session/prompt and attempt ID,
immutable target/message IDs, timestamps and a digest when useful. Never show
consumer secrets or imply a claimed/submitted body was read. Explicit Retry must
use the same original message and disclose possible duplicate exposure. Existing
operator Mark Read is an explicit human action, not a provider receipt. No native
reply path may reuse notification PTY typing.

## Implementation sequence and acceptance gates

1. Owner extensions first: exact retirement, Claim limit1 and durable attempt/
   suppression/retry contract. Add one primary actual-worker regression per distinct
   race: old retire cannot remove newer generation; prepared-then-crash suppresses
   replay across restart; partial/unknown submission cannot auto-claim again; explicit
   retry preserves immutable message ID; unrelated new messages remain claimable.
2. Pure adapter contract with fake stdin/stdout only proves parsing/rendering/bounds.
   Keep that evidence labelled synthetic. Assert no body reaches a terminal using
   the existing actual Desktop/CLI/socket and real PTY counter boundary. Exercise
   duplicate/reordered hooks, wrong session/prompt, held/draft/plan, missing surface,
   headless/disabled provider, full queue and stopped helper before one byte emits.
3. Installation tests use isolated HOME/plugin directories and sentinel unrelated
   settings/hooks. Verify idempotence, owned-only uninstall/update, symlink/inode
   substitution, partial atomic failure and rollback. Never install into the user's
   active config to run these tests.
4. Independently review before one explicitly authorized actual Claude 2.1.301
   interactive run in an isolated terminal/test configuration. Observe actual
   additionalContext acceptance through supported provider diagnostics as execution
   evidence only; do not turn an unstable debug format into the production receipt.
   Verify prompt/draft bytes, held dialogs, headless no-op, no idle wake, one bounded
   inline message and unknown outcome/UI. No real external message is sent during
   this planning task.
5. Record exact sources, provider executable/version/hash, generated plugin hashes,
   event/correlation IDs and red/green results. Keep body/prompt/token out of durable
   diagnostics. Run scoped owner/CLI tests and `scripts/check` sequentially with
   stable executable paths. Native acceptance, provider receipt capability and
   Codex/Claude wake parity remain open even if these gates pass.

## Retained failed-r6 fixture cleanup limitation

This is separate runtime validation evidence, not provider-adapter cleanup.
Failed owner `63922dff-53c6-4aad-886f-5625def0dd2c` remains in
`/tmp/r03-7527a5398e3044b4a06e9c0c0df9b04c/profile/registry.json`.
The foreground CLI Session was physically joined. Concurrent Cargo feature/default
builds had replaced its original supervisor executable; `/proc` still identifies
that exact retained image as `zmux (deleted)`, while its actual shell waits on input.
The full-image guard therefore correctly refuses normal original-owner cleanup.
No global/adopted process kill, original workload/server/supervisor signal, terminal
command replay or weakening of identity checks was attempted.

Rechecked 2026-10-16 04:51:46 UTC against pinned start ticks and parent lineage:
server2094032/start5672400, supervisor2094192/start5672412, workload2094223/
start5672415 were still alive. The workload image is `/usr/bin/zsh`; the server
image remains the pinned readiness dependency. PID numbers alone are not cleanup
authority. Preserve the original binding/boot/start evidence before any future
owner-authorized recovery decision. Stable sequential validation later passed;
that does not make this retained owner cleaned up.

Evidence: `/tmp/zmux-input-readiness-r6-live-process-diagnosis.json`, SHA256
`ca7e5db50a46d4e685ef6a8053fb005c59e4cda9dd7de862dd6c21571f435639`;
latest read-only observation is `retained-r6-cleanup-status.json` alongside this
plan. Avoid future executable replacement by using separate target directories or
strictly sequential Cargo commands during actual child-lifetime tests.

## Owner implementation progress (2026-10-16)

The first owner slice adds optional `max_messages` (1..8, existing default8) and
optional `exposure` to Claim. Exposure requires limit1, strictly bounded provider/
version/runtime/generation/session/prompt correlation, and a complete original
JSON data envelope below9,000 characters. The worker prepares a `delivery_unknown`
attempt before returning any body. Replaying the same provider prompt cannot
prepare another message, and replaying an already handed exposure cannot return
its body again. Ack refuses these leases because no provider receipt is available.

Message rows add default-empty `delivery_attempts` and default-false
`retry_authorized`. Attempts retain immutable target namespace/surface/SessionId,
consumer/lease, provider correlation, context SHA256 and preparation/original-expiry
metadata. `ReportSubmission` records a transport report without changing unknown,
queued, delivered or read truth. No body or capability is copied into attempt
metadata. History is capped at8 attempts per message. Unknown evidence cannot be
evicted for a new Send even after operator Mark Read, policy failure or closure;
capacity exhaustion refuses admission instead.

Full-access `Retry` requires exact queued unconfirmed IDs at the same live surface
and prior SessionId plus explicit `accept_duplicate=true`. The CLI spelling is
`agent retry --surface UUID --accept-duplicate MESSAGE_ID...`. It retains message
identity, cancels the prior exact reservations and authorizes one future exposure;
no automatic timer/restart/register path grants that authority.

`Retire` removes only the exact current capability and its reservations. A stale
consumer returns retired=false without affecting the newer owner. Successful
local Retire/Retry increments the live generation only after fsync and before
completion, fencing cached unpublished local handoffs. A retired=false operation
has no such effect. Already authorized foreign in-flight delivery retains the
previously accepted fsynced-handoff/original-TTL linearization.

Primary regression evidence is at the actual journal worker, with a distinct public
CLI/socket/real-PTY test for explicit retry parsing and no terminal input. Intended
red results were observed for unsupported Claim limit/Retire, duplicate-prompt
exposure, current retirement leaking cached body, and unknown-evidence eviction.
Logs: `/tmp/zmux-a02-{retire,exposure,prompt-replay,cached-retire,retention}-red.log`.
The initial limit fixture failed on unknown `max_messages` at the public wire parser.
The reviewed final validation receipt is recorded after the sequential check ends.

Final owner validation: `scripts/check` passed sequentially at this slice, including
25 actual journal-worker message tests, four actual CLI/socket/PTY message tests,
workspace tests, strict Clippy, notices and C ABI checks. The final log is
`/tmp/zmux-a02-owner-scripts-check-final.log`. Retained Retire/Retry request replay
was independently red for both mutations (`retained-mutation-red.log`); the final
worker carries the fresh/retained distinction so receipt retrieval never invalidates
a newer handoff. Register now returns additive namespace/SessionId fields for exact
adapter route comparison, without authenticating a provider.

## Session-local adapter implementation and evidence boundary

`hooks claude install --directory DIR --provider ABSOLUTE_PATH`, `status`,
`uninstall`, and `run --directory DIR --surface UUID -- [PROVIDER_ARGS]` implement
the explicit local adapter. DIR is a dedicated private owned plugin directory,
not provider settings. No user/project/managed provider configuration is edited.
Installation runs only `--version`, requires the exact `2.1.301 (Claude Code)`
version string, and pins executable path/SHA256/device/inode/size/mtime. Files above
512 MiB and group/other-writable executable files are rejected. Run preserves the
supported interactive argument subset, respects literal `--`, and rejects headless,
resume, arbitrary settings/plugins and unknown modes before starting the provider.

The wrapper must itself descend from the full live process identity of the exact
Desktop-published local terminal child, not merely name an ambient or explicit
surface UUID. It pins namespace/surface/SessionId, uses a fresh explicit provider
session UUID, retains the provider Child, and binds exec-form hook peers by kernel
UID/PID, full image and exact provider parent identity. It compares Register's
namespace/SessionId result and rechecks current route before exposure. Consumer
bearer stays in wrapper memory; all ambient ZMUX variables, including any control
password, are removed from the provider environment except the private event
socket route. Sender/message bodies are serialized as data; no shell, PTY input,
model callback or continuation carries them.

This is **local process correlation**, not cryptographic provider authentication.
A same-user provider tool deliberately exec'ing the hook client can imitate local
hook input. The adapter remains opt-in and always reports
`provider_authenticated=false`; no such event, output flush, successful exit or
transport report establishes acceptance, Ack or ReadTurn.

Official source evidence, fetched 2026-10-16:
<https://code.claude.com/docs/en/hooks#exec-form-and-shell-form> says, “A command
hook runs as exec form when `args` is set” and “spawns it directly with `args` as
the argument vector. There is no shell”. The command-hook fields table describes
`command` with `args` as “the executable to spawn directly”. This establishes the
exec/argv/no-shell configuration contract. It does **not** establish the exact
installed provider's OS parent PID topology; the adapter's strict direct-parent
requirement remains an actual-provider compatibility gate.

The same reference's [common input fields](https://code.claude.com/docs/en/hooks#common-input-fields)
describes `prompt_id` as “UUID identifying the user prompt currently being
processed” and “Absent until the first user input”. The adapter requires a nonnil
UUID prompt ID and matching owned root session; missing, malformed, subagent and
wrong-root events no-op. These docs establish the intended schema, not execution
by installed Claude Code 2.1.301. The actual-provider run remains unperformed.

The synchronous hook client has one original 2.7s budget within the plugin's 3s
provider timeout and original 10s message lease. IPC frames and input are bounded
at 64 KiB. Every hook-triggered Claim/Poll/retirement uses the original remaining
budget. Failure before output returns `{}`/exit0. Once stdout may have begun, no
fallback bytes are appended; uncertainty stays durable. A final local transport
report can set `submission_reported_at` without changing `delivery_unknown`.

Stop never claims, continues or calls Ack/ReadTurn. Typed PermissionRequest,
Elicitation, AskUserQuestion and EnterPlanMode set a conservative hold; launch plan
mode also starts held. No generic completion clears it. This slice supports only
a fresh explicit launch to resume; no explicit held-resume command or proof-based
resolution protocol is implemented. Draft terminal bytes and human input remain
outside this path.

Idle wrappers wait in listener poll, with a 250ms cap on child/disable publication
checks (at most 4 per second while idle), not 100Hz file verification. Full owned
artifact, executable metadata and route checks run immediately for hook exposure.
Idle checks never Claim or launch a provider turn. Active uninstall publishes
admission-disabled state before cleanup. A wrapper observes it, retires its exact
capability and retains the provider Child; a shared generation lease retains plugin
files until provider exit. Only verified owned device/inode/content files are
removed. Changed/symlink/inode replacements and unknown staging files remain.
Interrupted uninstall tolerates absent previously owned files while revalidating
every surviving file. No recursive removal or provider termination occurs.

Helper SIGKILL, machine loss or exhausted retirement can leave stale registration
evidence. `recipient_has_agent` means retained registration, not live provider,
liveness, acceptance or automatic-wake evidence. No process is adopted or killed
to clean it up. Unknown attempts survive independently. The native inbox owner
has been notified of this projection boundary.

Installation retains at most 8 owned generations and refuses further updates at
capacity. Publication is atomic after complete generation validation; pre-publication
failure leaves the previous publication and potentially unreferenced staging files.
Tests preserve unknown staging evidence and exercise interrupted unlink recovery;
power-loss/filesystem fault injection was not performed. Safe rollback uses a new
private directory after reviewing retained artifacts and never restores old tokens
or unconfirmed attempts.

The synthetic native C provider prints the version fixture string and drives the
public hook CLI inside the actual Desktop shell/PTY. It is **not Claude Code** and
makes no model call. Tests cover root registration, missing/wrong/subagent input,
UUID prompt replay, complete bounded context, permission/question/plan holds,
Stop no-op, idle queued messages, ambient credential stripping, active uninstall,
provider retention, exact retirement and final artifact cleanup. Linux idle samples
recorded 0 to 1 CPU ticks over 750ms windows; this is a small synthetic sample, not a
native/macOS or production resource benchmark.

Remaining gate: one separately authorized actual Claude Code 2.1.301 interactive
run must establish real plugin loading, event schema/UUIDs, process ancestry and
complete context observation. Existing hooks may veto processing. No documented
independent stable interactive acceptance receipt exists, so even that observed
context cannot by itself enable Ack/ReadTurn. Native inbox execution, Codex,
headless/resume, wake and full A02 parity remain separate/open.

Event-retirement failure preserves the exact in-memory capability and original
Retire request UUID while disabling context admissions. Standalone shutdown may
retry only that idempotent retirement. One stored 2s cleanup deadline is shared by
explicit cleanup and Drop, so neither renews a failed cleanup budget. The actual
journal-lock regression was red when the event path consumed the capability on
failure; after repair, release during the bounded shutdown reserve retires the
consumer without Register, message replay or a renewed event deadline. The
synthetic fixture exercises permission, question, plan-entry, plan-launch and this
retirement-failure case through the public CLI and actual terminal owner.

Final adapter validation: `scripts/check` passed sequentially with
394 tests in 72 groups, 1 existing ignored subprocess entrypoint, strict workspace
Clippy, notices and C ABI checks. Primary boundaries: 25 message worker tests,
five CLI/socket/PTY message tests (one exercises all five synthetic provider
scenarios), and six plugin/ingress CLI tests. Log:
`/tmp/zmux-a02-adapter-scripts-check-r2.log`.

Red evidence: `/tmp/zmux-a02-plugin-ingress-red.log` and
`/tmp/zmux-a02-adapter-pty-red.log` fail on the missing hook command;
`/tmp/zmux-a02-uninstall-identity-red.log` fails on replacement-inode deletion and
interrupted cleanup; `/tmp/zmux-a02-adapter-retirement-red.log` fails on losing the
capability after actual journal-lock failure. Independent synthetic execution and
idle samples: `/tmp/zmux-a02-adapter-pty-green-final.log`. No actual Claude turn,
provider installation into user configuration, or native UI run occurred.

## Darwin compile follow-up

At main94e8330 Linux Foundation passes, but Darwin Foundation38856977385 and
macOS development38856977404 fail compilation in SocketDirectory cleanup:
rustix Darwin st_dev is signed i32, while MetadataExt::dev returns u64. Normalize
stat device/inode identity to MetadataExt's representation with a macOS-specific
cast before both comparisons; retain both exact replacement checks. No cleanup
authority changes. Full Linux scripts/check passes395 tests/73groups/1existing
ignored plus strict Clippy/notices/both ABI after repair. Hosted Darwin green
remains required. Logs /tmp/zmux-main-94e8330-foundation-darwin-failure.log and
/tmp/zmux-main-darwin-hook-identity-check.log.

## Long temporary-root startup regression (2026-10-16)

Main43508c1 fixes Darwin device-ID compilation, but Foundation38857475918
and native38857475923 fail before the first synthetic-provider barrier.
A per-child long TMPDIR in the existing real Desktop/PTY scenario reproduces
`path must be shorter than SUN_LEN` on Linux (regression56a2125). No global
environment mutation, mock listener or new production test seam is involved.
The existing short-root cases did not exercise the pathname bound.

The repair chooses a canonical socket pathname within Darwin's103-byte bound
before creating the directory, falling back to canonical /tmp when needed.
The full random UUID,0700 directory,0600 socket and exact inode/device cleanup
guards remain. A guard established before bind removes an empty owned directory
on bind failure; an unexpected entry or replaced identity is retained with an
explicit cleanup error. The same five actual-terminal scenarios pass after repair.
Red logs: /tmp/zmux-hook-long-runtime-path-red.log and
/tmp/zmux-hook-long-runtime-path-check-red.log. Focused green:
/tmp/zmux-hook-long-runtime-path-green.log. This is a local runtime startup
repair; actual-Claude and hosted Darwin verification remain separate gates.

Full Linux scripts/check passes395 tests in73groups,1existing ignored, strict
Clippy, dependency notices and both C ABI checks. Log:
/tmp/zmux-hook-long-runtime-path-check-green.log.
