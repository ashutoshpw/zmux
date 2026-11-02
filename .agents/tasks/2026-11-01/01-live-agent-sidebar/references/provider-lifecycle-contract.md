# Generic provider lifecycle contract

This contract is the shared admission boundary for Codex and OpenCode local
normalizers and managed SSH producers. It does not install provider hooks,
change provider trust settings, or authenticate a provider process. A remote
frame is a sanitized claim from the account reached through the exact managed
SSH terminal binding.

## Rust API

The normalized, provider-neutral types live in
`zmux_remote_runtime::agent_lifecycle` so the remote wire crate does not depend
on desktop runtime:

- `ProviderKind::{Codex, OpenCode}`
- `ProviderLifecycleObservation { provider, scope, session_id, turn_id, state, transition, marker }`
- `ProviderScope::{Root, Subagent}`
- `ProviderState::{Working, Waiting, Idle}`
- `ProviderTransition::{Snapshot, StateChanged, TurnCompleted, TurnFailed, TurnInterrupted}`
- `ProviderMarker::{Live, Baseline, Recovery}`
- `ProviderObservationInput { target, runtime_id, generation, sequence, observation }`
- `ProviderSessionEnd { target, provider, session_id, runtime_id, generation, sequence }`

The desktop owner exposes these stable entry points:

- `Runtime::apply_local_provider_observation(model, ProviderObservationInput { target, runtime_id, generation, sequence, observation }, now)`
- `Runtime::apply_remote_provider_observation(model, ProviderObservationInput { target, runtime_id, generation, sequence, observation }, now)`
- `Runtime::apply_local_provider_evidence_lost(model, ProviderSessionEnd { target, provider, session_id, runtime_id, generation, sequence }, now)`
- `Runtime::apply_local_provider_session_ended(model, ProviderSessionEnd { target, provider, session_id, runtime_id, generation, sequence }, now)`
- local control `Action::ProviderObservation { target, runtime_id, generation, sequence, observation }`
- local control `Action::ProviderEvidenceLost { target, provider, session_id, runtime_id, generation, sequence }`
- local control `Action::ProviderSessionEnded { target, provider, session_id, runtime_id, generation, sequence }`

Both APIs validate the exact local or remote terminal owner and then use the
same lifecycle state machine. Local sequence numbers start at 1 and advance by
one for each observation for that provider session. A local duplicate or gap
fences activity and connection to `Unknown`. A remote observation uses the
managed stream's channel sequence; heartbeat frames may occur between provider
observations.

The existing `Action::Register` / capability based event path remains Claude
specific. Codex and OpenCode cannot register a generic capability and then
change its provider through that older path.

`Request` serializes its `Action` as an internally tagged object: the action
name is the `action` property and enum fields are siblings of that property.
This is the exact local producer control shape (UUID and target values below
are placeholders):

```json
{"request_id":"<request UUID>","action":{"action":"provider_observation","target":{"identity_namespace":"<UUID>","surface_id":12,"surface_uuid":"<UUID>","session_id":13},"runtime_id":"<runtime UUID>","generation":"<generation UUID>","sequence":1,"observation":{"provider":"codex","scope":"root","session_id":"thread_7","turn_id":"turn_8","state":"working","transition":"state_changed","marker":"live"}}}
```

The adapter reports a daemon/plugin failure with the exact local end action:

```json
{"request_id":"<request UUID>","action":{"action":"provider_evidence_lost","target":{"identity_namespace":"<UUID>","surface_id":12,"surface_uuid":"<UUID>","session_id":13},"provider":"codex","session_id":"thread_7","runtime_id":"<runtime UUID>","generation":"<generation UUID>","sequence":2}}
```

When the provider session exits normally, send this action with the next local
sequence:

```json
{"request_id":"<request UUID>","action":{"action":"provider_session_ended","target":{"identity_namespace":"<UUID>","surface_id":12,"surface_uuid":"<UUID>","session_id":13},"provider":"opencode","session_id":"sess_root_7","runtime_id":"<runtime UUID>","generation":"<generation UUID>","sequence":2}}
```

Both end actions use the next per-session local sequence, must match the exact
provider and current runtime/generation, and fence that record so further
observations require a new runtime or generation. Evidence loss publishes
`Unknown` activity and connection; provider exit publishes `Unknown` activity
with `Disconnected` connection. They create no Claude capability.

The control adapter accepts generic provider actions only for a running exact
local terminal. It derives `codex_local` or `opencode_local` from the
provider-tagged observation, validates the owner and current session, and
returns the ordinary lifecycle receipt with no capability. The managed SSH
reader routes `provider_state` frames through the remote API after validating
the current nonce/generation/terminal binding; local actions cannot claim a
remote provider.
`Action::Register` and `Action::Event` remain the Claude capability path; they
reject generic providers. Generic provider actions never register or expose a
capability.

## Opaque IDs and snapshot projection

`session_id` and `turn_id` are opaque, nonempty UTF-8 values of at most 256
bytes, with no control characters. They are never parsed as UUIDs, paths, or
working directories. The provider event's root session identity is authoritative;
cwd equality is not a correlation rule.

The public snapshot keeps its UUID fields. The desktop maps IDs with UUIDv5 and
the URL namespace using these exact name bytes:

```text
session = "zmux-agent-provider-v1\0" + provider + "\0session\0" + session_id
turn    = "zmux-agent-provider-v1\0" + provider + "\0session\0" + session_id
          + "\0turn\0" + turn_id
```

Provider tags are `codex` and `opencode`. The resulting session UUID is exposed
as `provider_session_id`; the turn UUID is exposed in the existing `prompt_id`
field. Provider runtime and stream generation remain independent Zmux UUIDs and
are never derived from provider IDs. Snapshot provider tags are `codex_local`,
`codex_remote`, `opencode_local`, and `opencode_remote`.

## Activity and alert rules

| Observation | Published activity | Alert behavior |
| --- | --- | --- |
| `Snapshot` with `Baseline` or `Recovery` | Provider reported state | Silent; does not establish a completion eligible run |
| `StateChanged` + `Live` + `Working` | Working | Starts completion eligibility for this exact turn |
| `StateChanged` + `Live` + `Waiting` | Waiting | Needs attention once for this exact turn |
| Ordinary `Idle` state | Idle when it describes the current turn | No completion alert |
| `TurnCompleted` + `Live` + `Idle` | Idle | Completes once only if the same turn had a live Working transition in this runtime and generation |
| `TurnFailed` or `TurnInterrupted` + `Idle` | Idle when current | Never completes; later completion evidence for that turn stays silent |

A completion frame without a matching live Working transition is recorded as
already observed and stays silent. Recovery state cannot synthesize a start to
make an old completion sound. Repeated Waiting, completion, failure, and
interruption observations are deduplicated by the opaque turn's UUIDv5 mapping
within bounded per-session history. A stale sequence, explicit gap, queue
overflow, transport loss, or stale generation leaves activity `Unknown` and
produces no sound.

An unsuperseded `Baseline` or `Recovery` snapshot records a reported Waiting
turn as already observed, without an alert; a later live Waiting transition
for that turn stays silent. Once a live Working or Waiting transition has been
accepted in a generation, a later Baseline or Recovery snapshot in that same
generation may advance its sequence and refresh connection evidence, but cannot
replace the live turn, activity, or completion eligibility. A new generation
after local evidence loss clears that live-evidence guard, so its first
recovery snapshot can establish current activity.

Local replacement after evidence loss or normal provider exit keeps the bounded per-session Waiting,
completed, and non-completable turn histories while clearing completion
eligibility. The owner also retains the most recent 128 retired runtime and
generation pairs per provider session and rejects their later re-admission.
Turn histories and retired identities are bounded to 128 entries each; marks
older than the retained window may be evicted.

Every state that may create an alert requires a turn ID. Outcome transitions
must carry the same root turn ID, report `Idle`, and identify whether the turn
completed, failed, or was interrupted. A mere idle status is not completion
evidence.

## Managed SSH wire frame

Protocol version 3 stays in use. The extension adds `provider_state` alongside
the existing `hook` frame, leaving the Claude Hook and receipt shapes intact.
The frame uses the current stream nonce, generation, and strictly increasing
channel sequence:

```json
{"kind":"provider_state","version":3,"nonce":"<zmux nonce UUID>","generation":"<stream generation UUID>","sequence":12,"observation":{"provider":"opencode","scope":"root","session_id":"sess_root_7","turn_id":"turn:8","state":"idle","transition":"turn_completed","marker":"live"}}
```

The whole newline-delimited JSON frame, including its newline, is limited to
4096 bytes. The reader rejects stale, repeated, or skipped channel sequence
numbers. Producers must emit an explicit `gap` frame when they knowingly drop
or coalesce lifecycle evidence. Existing bounded queues are retained; overflow
ends the stream and the desktop publishes `Unknown`. Generic observations do
not use Claude's hook receipt channel. Existing Claude hook FIFO ordering and
receipts remain unchanged.

`scope` is required. Producers emit `root` only after binding provider events
to the root session/turn IDs. Events from child agents/subagents are either
filtered or labeled `subagent`; desktop admission rejects the latter. OpenCode
producers must ignore unrelated sessions and child sessions. Codex producers
must use the root thread/run identity. Do not guess root ownership from cwd,
terminal title, or output text.

## Producer responsibilities

1. Resolve session and turn IDs from the provider's event API, not the current
   terminal directory.
2. Emit a silent baseline after initial attachment and a silent recovery
   snapshot after reconnect before forwarding live transitions.
3. Report `Working`, `Waiting`, or `Idle` only from provider lifecycle data.
4. Emit `turn_completed` only for confirmed successful completion. Map provider
   failures and user/process interruptions to `turn_failed` or
   `turn_interrupted`; never translate them into successful completion.
5. Preserve one ordered sequence per managed stream, include heartbeat frames
   in that sequence, and send `gap` rather than silently losing an event.
6. Keep provider prompts, tool inputs, transcript text, environment data,
   arbitrary provider payloads, and filesystem paths out of the frame.

The desktop's UUID-shaped snapshot is a compatibility projection, not provider
authentication, provider replay protection, or proof of production behavior.
