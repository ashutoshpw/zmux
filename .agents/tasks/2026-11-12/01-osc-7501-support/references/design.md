# OSC 7501 design and implementation boundaries

## Evidence and scope

Checked the [canonical specification](https://superlogical.com/rex/docs/build/program-status)
on 2026-11-12: revision 0.4, dated 2026-10-10. Retrieved HTML SHA-256:
`2d47142bb6e81fdf50dfed9a59efef364fe8dab30ba71109632ad46e4b498822`.
This identifies the retrieved page, not a stable normative source artifact; recheck
the revision before implementation and record any changes. The protocol specifies
state and leaves presentation/notifications to the terminal.

The source inspection baseline is `0ac5192`. Planning is documentation-only.
Implement independently from the public specification and retain MIT/dependency
notices. Do not import or translate cmux implementation or assets.

### Existing production boundaries

- `crates/terminal-adapter/src/lib.rs`: `Metadata::unhandled_osc`, `Terminal::process`,
  bounded control-string guard and `Viewport`; currently no 7501 record owner.
- `crates/terminal-adapter/src/engine.rs`: explicit engine selection and staged batches.
- `crates/ghostty-vt/src/metadata.rs`, `src/owner.rs`, `tools/terminal-feasibility/metadata.c`:
  typed callback capture and fixed-storage normalization; current source whitelist
  does not expose 7501 program records. Upstream support alone is insufficient.
- `crates/session-runtime/src/terminal.rs` and `src/lib.rs`: worker-owned terminal,
  real PTY I/O, immutable snapshots, pending input, causal replies and commit/abort.
- `crates/desktop-runtime/src/agent_lifecycle.rs`: exact provider identity, sequence,
  generation, freshness and alert eligibility. Its provider enum is closed and its
  30-second hook freshness policy must not be reused for OSC records.
- `crates/desktop-runtime/src/attention.rs` and `src/lib.rs`: inbox admission and
  projection; OSC 9/777 are generic notices, not provider lifecycle evidence.
- `apps/macos/Sources/AgentSidebarViews.swift`, `App.swift`, `AgentAlertDelivery.swift`,
  `AgentAlertAudio.swift`, `NotificationDelivery.swift`: native rows, exact selection,
  provider event delivery and OS notifications. Generic notifications currently ask
  for OS sound; new OSC alerts must not accidentally enter that untagged sound path.

## Slice 1: resolve parser capabilities before choosing plumbing

Inspect the actual pinned vt100/vte callback API and retained Ghostty metadata ABI:
raw OSC body, exact encoded length/terminator, OSC 133 A, RIS, DECSTR and ordered
primary device-attributes replies are required. Local Cargo registry sources were
not present at the conventional path during planning; those APIs remain unverified.

Prefer existing authoritative parser callbacks. Do not bolt a regex, raw-output
screen scraper or second independent VT parser onto the worker. If callbacks omit
required events, make the smallest reviewed upstream extension or independently
authored dependency supplement, with source/notice provenance and executable proof.
Do not weaken reset/length semantics to avoid a missing hook. A Ghostty supplement
requires new exact-source retained artifacts and linkage receipts; never mutate
frozen artifacts or enable optional Ghostty as the default to obtain this feature.

Place shared typed report decoding/record semantics in a small dependency-neutral
Rust module/crate consumed by both engine owners. A workspace crate such as
`zmux-program-status` avoids a terminal-adapter -> ghostty-vt -> terminal-adapter
dependency cycle. Keep VT framing in the authoritative engine and application
presentation in desktop runtime. Choose the smallest shared API after the hook gate.

## Slice 2: shared protocol contract

Accept `ESC ] 7501 ; pairs ST`, where ST is BEL or ESC-backslash; no query is required
before a report. Split pairs on colon, trim surrounding whitespace, split key/value
at the first equals sign, ignore unknown keys and skip malformed pairs. The last
syntactically well-formed pair for a repeated key wins, then key-specific validation
applies: a later invalid app/progress value must not preserve an earlier value.
Missing/unknown `state` ignores the report.
Keep invalid-field handling distinct from atomic hard-limit rejection:

- States: `idle`, `working`, `done`, `blocked`, `error`; `clear` is a removal operation.
- `kind`: permission/question/auth, only for blocked; unknown values are absent.
- `progress`: integer 0..100, only for working/blocked; invalid/absent is indeterminate.
- `app`: stable self-declared identifier, not provider identity; invalid syntax is absent.
- `id`: absent selects root; a syntactically accepted pair with invalid path grammar ignores the report and never falls back to root; an over-limit path triggers atomic hard-limit rejection. Malformed wire pairs still follow the general skip rule, including preserving a preceding valid ID.
- `msg` and `title`: standard base64, optional padding, strict UTF-8; decoding or
  C0/DEL/C1 controls reject the complete report. Retain plain text; never interpret markup.
- Check every pair and hard bound before changing records, including duplicate keys;
  test over-limit earlier occurrences followed by valid replacements. Unknown fields
  still obey sequence/key limits. Never truncate an invalid report into a valid one.

Hard bounds: 4096 bytes for the full sequence including introducer/terminator;
16-byte keys; `id` <=128 bytes, <=32 per segment, <=8 levels; `app` <=32 bytes;
`msg` <=2732 encoded / 2048 decoded bytes; `title` <=256 encoded / 192 decoded bytes.
Enforce OSC-specific rejection independently of the existing 16 KiB global
control-string failure guard. An oversized 7501 report is discarded without a
partial record update; an unterminated stream beyond the global safety limit may
still fail the terminal explicitly. Preserve that existing recovery contract.

Choose 64 records per terminal, the required minimum; insertion at capacity evicts
the least recently updated record, with deterministic update ordering. Bound raw
scratch and transition storage too, with checked allocations and a documented
aggregate memory budget (current shared-owner target: 512 KiB per terminal,
including retained after-record transition presentation data; verify the ledger
against worst-case legal records and pending events). Account
snapshot clones and aggregate terminal fan-out separately; do not claim an RSS cap.

Each valid report completely replaces its own record. Missing fields disappear.
Resolve app inheritance from the nearest existing ancestor when projecting records;
parent changes/eviction must affect descendants without copying stale inherited app.
Clear removes the addressed path and descendants, respecting segment boundaries;
root clear removes all. An idle parent and blocked child may coexist.

## Slice 3: lifecycle, ordering and transport

Store records at the terminal/session worker, independent of alternate screen and
scrollback. Alternate-screen transitions and DECSTR preserve records; RIS clears
all. OSC 133 A and observed attached-child exit remove working/blocked; choose to
retain idle. Done/error survive prompt and exit while that terminal remains open.
Explicit terminal closure removes its records. Existing auto-close behavior may
close the terminal after exit; do not change that policy to retain a status badge.
Do not infer foreground subprocess exit from output, a PID scan or quiet time.

No heartbeat or elapsed-time expiry is permitted. Transport availability is a
separate projection: an SSH loss cannot keep a program labelled currently running.
Keep any retained result historical/unavailable, fence replacement session epochs,
and baseline recovered snapshots without new sounds. Do not replay reports into a
fresh PTY or persist them as live records across application/session restoration.

The existing shell integration must supply OSC 133 A if it does not already. Add it
to supported integrated prompt boundaries without replacing user hooks or editing
user startup files, and test shell-specific chaining. Without a cooperating prompt
marker, nested-command completion cannot be invented; document that limit.

Keep ordered report/lifetime effects before snapshot coalescing so working -> done
within one read is observable. Use a bounded transition journal with local cursor,
session epoch and explicit dropped-through marker. Overflow retains a valid latest
record snapshot but reconciles consumers without replaying uncertain alerts; it
cannot grow memory or silently claim complete transition delivery.

Reply to `OSC 7501;? ST` with the constant supported states/kinds. Never echo ids,
labels or messages and never invent a record-query API. Reply before a subsequent
CSI c response. Default vt100 also needs primary DA handling if currently absent;
prove this against a real PTY, not a declared capability flag. Use the sole worker
writer after previously admitted bytes, with bounded deadlines/queues. Ghostty
effects and status publication commit only after complete causal writes; partial
writes, cancellation and failure retain the prior publication and require recovery.

SSH/container operation uses the existing PTY byte stream, with no new service,
credential or provider hook. This does not promise passthrough through arbitrary
tmux/screen configurations. Test managed SSH output/generation fences separately;
document tested intermediaries and container/unmanaged SSH limits precisely.

## Slice 4: coexistence and alert policy

Expose a separate `program_status` snapshot with source `osc7501`, exact desktop /
window / workspace / pane / surface / session identity, local epoch/revision, raw
record id, typed state, progress and safe text. Never generate provider UUIDs, turn
IDs, capabilities or `ProviderObservation` from self-declared `app`, `id` or message.
Never let OSC overwrite adapter Working/Waiting/Idle/Unknown or connection evidence.

Use one external-alert source per terminal command epoch, independent of visual
record storage. Prefer the provider adapter whenever its route/launch registration
is pending or active before OSC alert admission. With no adapter, generic OSC may
claim the epoch on its first eligible external alert. Once an external alert is
admitted, retain that source for the epoch; a later adapter attachment updates its
own lifecycle normally but cannot create a second alert source for the same epoch.
At the next observed OSC 133 A choose again, preferring the adapter. Without a
reliable prompt boundary, keep the existing source conservatively until session
replacement. Verify wrapper registration ordering before claiming overlap coverage.

This trades some alerts on late/ambiguous handoffs for avoiding duplicate sounds.
Source arbitration is explicit suppression, not exact cross-protocol event
correlation. OSC and provider reports have no shared event ID, so do not claim
general exactly-once matching. Do not match titles, messages or app names. Existing
OSC 9/777 generic-notice duplication is a separately recorded limitation; this task
does not promise to solve it by guessing which notice belongs to which provider.

For admitted OSC-only sources: entry into blocked creates needs-user attention;
working/blocked -> done creates completion; entry into error creates failure
attention using the existing needs-user sound. Initial done/error baselines do not
produce retrospective completion sounds. Idle/working, progress-only changes,
identical reports, clear/reset/eviction and recovered snapshots are silent. A new
live blocked report can alert without a prior working report. Use local transition
IDs and the existing bounded alert history; never call them provider replay IDs.

Use existing sound preferences and local playback for both local/remote programs.
Muting keeps visual status/inbox available. Tag OSC-origin alerts explicitly so OS
default sound and selected app sound cannot both fire for the new event. Apply a
deterministic per-terminal cooldown (initial proposal: 2 seconds) to external
notifications/sounds without delaying or dropping record updates; count suppressed
events and keep a bounded visual attention summary. No debounce-derived liveness.

## Slice 5: native presentation

Add a Programs subgroup alongside provider Agents, reusing native row/selection
patterns. Show workspace/terminal provenance and a self-reported status source;
use declared app as text/icon hint only. Render working progress, blocked reason,
done and error distinctly. Root and child records remain inspectable; show one
collapsed terminal summary, ordered blocked > error > working > done > idle, with
deterministic ties. This priority is Zmux presentation policy, not a spec mandate.

Keep provider Agents distinct and authoritative; expanded program details may
coexist for the same terminal. Sanitize bidi/invisible formatting outside the grid,
escape text through native labels, and supply accessible state/progress descriptions.
Selection revalidates the exact existing terminal identity and uses shared workspace
actions; stale rows cannot select another terminal or spawn a replacement.

## Slice 6: validation and delivery

Use the [test matrix](test-matrix.md) and existing `scripts/check`, native sidebar
and macOS packaging workflows. If linked ABI inputs change, rebuild/accept retained
Ghostty artifacts and run `terminal-engine-linkage.yml` on the final relevant source;
default workspace tests do not prove optional linked-engine behavior. Keep all
unavailable native, Intel-runtime, physical-audio and real-provider gates explicit.

Delivery follows `docs/development-builds.md`: fresh universal arm64/x86_64 macOS
14+ development app; verify the downloaded inner ZIP checksum, packaged source SHA,
architectures, build metadata, native acceptance and actual screenshots. Report
actual signing/notarization, artifact link and quit-old-Zmux reminder. No production
deployment, release publication, paid resources or replacement of a running app.
