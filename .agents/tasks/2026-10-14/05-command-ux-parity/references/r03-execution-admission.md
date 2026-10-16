# R03 execution admission before user code

Read-only design, 2026-10-16. No implementation is authorized by this document.
The current bound finite-completion cohort remains frozen and separate. Native
input readiness is another gate and is not solved by execution admission.

## Failure and production boundary

The inspected `supervisor.rs`, `files.rs`, `broker.rs` and native `worker.rs`
match the combined finite cohort in
`/tmp/zmux-r03-finite-combined-freeze-20261016/manifest.json`.

`supervisor.rs::run_supervisor` starts the selected shell with `-lc SCRIPT` or
`-i` before the starter has committed a binding. The starter receives its PID,
queries its live identity, waits to observe the exact selected-shell image,
checks `Record::verify_live`, persists the binding and sends it back. The
supervisor then queries that child image again before binding its publisher.

A shell can exit from a builtin command or startup file before any of these
queries. It can also replace its image with `exec`. `process_identity` requires
an executable path, which cannot be recovered reliably from an exited process.
The starter therefore retains an uncertain reservation. The supervisor still
waits for its actual Child, but an uncommitted publisher cannot authenticate its
terminal result. Its local exit status does not repair the missing binding.

The existing finite tests intentionally wait for `finish-once` after binding.
They cover a different ordering and must not be repeated as evidence here.

## Recommended ordering

Keep the original supervisor as the sole owner of the actual Child. Add a small
internal Zmux workload helper that performs a bounded authenticated admission
handshake and then replaces itself with the configured shell through `exec`.
The helper executes no user shell or shell startup file before admission.

1. The supervisor spawns the original Zmux helper as its retained direct Child.
   The helper connects over a private control endpoint and blocks. Its actual
   PID, UID, parent, start identity, boot ID and helper image can be measured
   without racing arbitrary user code.
2. The starter independently authenticates the supervisor/server and helper
   lifetime, validates the intended shell invocation and persists an immutable
   execution binding. The supervisor binds its publisher to that same value and
   acknowledges the exact binding digest.
3. Only after durable binding and publisher agreement does the supervisor send
   a one-use `Run` frame. It names the owner generation, binding digest and
   admission challenge. The helper consumes this authorization once.
4. The helper closes admission descriptors and execs the selected shell with
   exactly the existing `-lc SCRIPT` or `-i` arguments, cwd, terminal descriptors
   and intended environment. There is no extra shell, `eval`, script prefix,
   terminal-input read, terminal ACK or injected output marker.
5. The same actual Child/PID now runs the selected shell. The supervisor retains
   its child handle, observes and reaps its real status, and can publish even if
   shell startup or the supplied command exits immediately. It does not need a
   successful post-exit image lookup or a frontend connection to do so.

The current completion-before-transport observer branch can consume this result
after OwnerBound; it must not manufacture TransportConnected or input readiness.
Binding/publisher agreement precedes Run even if its final bootstrap ACK is lost.
Such loss is uncertainty about the existing admitted owner, never permission to
replay creation.

## Identity contract and arbitrary exec limits

This ordering does not fit the present V1 immutable workload-image assumption.
Recording the helper image as though it were the shell would be false. Globally
dropping `executable` from `ProcessIdentity` checks would weaken unrelated owners.

The parent selected the following V2 product policy: arbitrary user-program exec,
including `exec vim` or another application, is legitimate terminal behavior.
V2 authenticates an admitted direct Child lifetime and its original authorized
invocation. It does not promise continuous workload executable integrity.

Use a separate versioned workload execution contract:

- **Birth identity:** the full kernel-observed identity of the original helper.
- **Stable lifetime:** PID, direct parent, boot ID and process start identity,
  retained by the same supervisor Child handle across the admitted exec. The
  helper's UID belongs to birth provenance and admission authentication; do not
  silently add a continuous workload-UID guarantee to the selected lifetime
  policy. Server/supervisor caller-UID checks remain unchanged.
- **Authorized invocation:** exact configured shell path and a canonical digest
  of its original argv, cwd and applicable environment policy. The gate authorizes
  helper to that shell once; it never accepts a caller-supplied replacement target
  after commit. Keep command plaintext out of new durable registry fields.
- **Live workload policy:** after authenticated Run, a different workload image
  alone does not deny V2 Attach or preservation. Revalidate the admitted lifetime
  and the live original supervisor/server/namespace instead. A sampled image can
  be diagnostic evidence, never a substitute for lifetime ownership or a new
  authority to signal/adopt the process. Keep full-image identity checks for the
  server, supervisor and legacy V1 owners.

V2 continuity sampling needs a narrow, double-sampled PID/parent/start/boot
operation. Do not call today's full `process_identity` and discard its image/UID:
that operation requires executable-path access and can fail precisely because a
legitimate workload changed image or UID. Missing, inconsistent or mixed samples
are Unknown, never proof of continuity or authority to adopt a PID/name. Admission
still requires the helper's full immutable identity, and the supervisor's retained
Child handle and actual wait remain the terminal-result authority.

At bind, the stable lifetime tuple must equal the PID/parent/start/boot projection
of the full independently observed helper birth identity. Parallel fields cannot
name different processes. Before accepting Run, the helper must recompute
`invocation_digest` from the exact argv, cwd and environment policy it will
actually use for exec and compare it to the immutable binding. The descriptor's
declared digest alone is not proof that the admitted invocation matches execution.

An actual terminal Child wait can prove the admitted process lifetime ended with
that status. It cannot prove a continuous executable-image history. Sampling
cannot detect an intermediate `exec` that starts and exits between observations,
and shell code is inherently allowed to create other processes. PID/start/parent
stability is not proof that the process ran only the shell or was never modified
by another same-user process. Existing path-based checks also do not authenticate
the bytes of an executable replaced at the same pathname.
Retained direct-Child completion also says nothing about the lifetime or
containment of descendants the workload may have created.

Consequently V2 terminal evidence must explicitly describe **admitted lifetime
completion**, not continuous exact-shell-image completion. An authentic lifetime
exit can save history regardless of the sampled workload image. The original
authorized shell invocation remains immutable even when the admitted process
subsequently execs another user program. Record whether the configured shell was
observed only as optional diagnostics; lack of a shell sample is allowed for an
immediate terminal result and does not deny authentic completion. It is also not
evidence of successful interactive startup.

Continuous prevention/detection of later arbitrary exec is outside this selected
product contract. It would require a separate OS enforcement/tracing design,
including Darwin portability and shell compatibility. Do not claim it from V2
verification or a successful gate handshake. The explicit V2 discriminator is
essential: treating an existing V1 record this way would silently weaken V1.

## Small versioned wire and migration

Use a new typed execution-binding/terminal-receipt version and domain; retain
the current V1 reader and exact V1 verification behavior. Suggested V2 fields:

```
ExecutionBindingV2
  version, domain, owner_uuid, owner_generation
  namespace root/backend/completion endpoint device+inode identities
  full server identity, full supervisor identity
  full workload birth identity, stable workload lifetime
  authorized shell path, invocation_digest, public admission_id

TerminalReceiptV2
  version, domain, immutable execution binding + binding_digest
  terminal_sequence = 1, actual workload_exit, child_reaped
  admission = run_authorized
  execution_observation = exec_failed | shell_observed | not_observed
  optional bounded exec_failure { stage, errno }
```

`exec_failed` is a correlated private-channel report from the admitted helper
when its configured-shell exec syscall returns an error. It must not be confused
with a user command that legitimately exits 126/127. `not_observed` means the
supervisor has the actual lifetime wait result but did not independently sample
the configured shell image; it is not an implicit successful-exec assertion.
Closing a close-on-exec descriptor alone cannot distinguish successful exec from
helper death before exec. Preserve that uncertainty in the execution observation
while retaining the authentic Child lifetime result. Do not label such a result
as proven shell startup, and do not fabricate an exec failure when no error report
was received. A valid saved history can retain this explicit classification.

Invocation hashing needs a specified canonical field order and byte encoding;
do not concatenate strings or rely on an unordered JSON map. New fields remain
within the existing 4KiB bootstrap/8KiB terminal/16KiB control bounds, or an
explicitly justified bound/version change must accompany the implementation.
The binding's `admission_id` is public durable correlation, not a reusable secret
or authority to Run. Keep the actual one-use admission nonce only in the bounded
private bootstrap exchange/helper state. Run requires that nonce, the matching
binding/admission ID and authenticated original-supervisor peer; a copied saved
binding or public admission ID cannot authorize execution.

The current registry is an array of deny-unknown-field Records. An optional
typed execution descriptor lets new readers accept old records, but old binaries
will reject new records; document that downgrade boundary rather than dropping
the descriptor on rewrite. Never synthesize V2 admission for an existing V1 or
uncertain owner. Explicit Attach cannot upgrade or rerun one.

The observer and completion-control protocols also need explicit version
negotiation for V2 binding/receipt variants. Existing V1 finite completion remains
unchanged. A persisted history containing a V2 receipt needs a new session
document version, with explicit readers for current V7/V1 history. Do not put a
new nested receipt into a V7 document and imply an older reader understands it.
The native model must preserve the versioned evidence/classification it shows.

Use one typed execution-contract discriminator throughout the new registry
descriptor/capability, binding, terminal receipt, observer and completion-control
envelopes, and persisted V8 history. The alternatives are legacy full-image V1
and admitted-lifetime V2, never a combination of both field sets. The absent
descriptor on an actual old record means legacy V1 only; preserve its old wire
shape. Reject V2 metadata paired with a V1 capability, mismatched versions or
domains, duplicate alternatives and unknown fields. The existing observer
protocol's integer version 2 already denotes finite completion; it is not this
new execution-contract version. Negotiate a new observer/control wire revision
for the new typed contract. Negotiation cannot downgrade V2 into V1, synthesize
admission for an old owner, or strip V2 fields during a registry/history rewrite.

## Trust and failure boundaries

The helper is original Zmux code under the pinned production CLI image. Its
private descriptor/socket must have the existing current-user, mode, canonical
namespace, inode, kernel-peer and full-identity checks. Correlate both directions
to the retained supervisor, owner generation and one-use challenge. Admission
descriptors are close-on-exec and must not leak to shell descendants. A terminal
file, stale registry, dead publisher or arbitrary shell ACK cannot authorize Run
or consumption.

Status, Attach and read-side validation of the original owner namespace must only
inspect existing objects, retaining directory descriptors where needed. It must
not call a creating directory helper or recreate a missing original owner path.
This includes failure paths with missing roots, parents, sockets or descriptors.
Explicit Start may create a new owner namespace. After authenticating an existing
owner, authorized Attach may create its own fresh frontend `aUUID`/`zoUUID`
namespace and owned relay resources; those are separate from the original owner.
Repair or cleanup belongs to its explicit mutation operation. Disclose existing
profile/registry-lock initialization separately rather than claiming that an
entire command is filesystem-read-only merely because owner validation is.

Before Run, a disconnected/cancelled gate must not execute the shell. It can end
only the supervisor's still-retained, proven unreleased helper. Once Run may have
been delivered, preserve the existing no-replay/no-adopt policy; an ACK loss does
not prove user code was unexecuted. Completion still requires the live original
supervisor and actual Child wait, followed by the existing durable-consumption
authorization before retirement.

Pre-Run helper death, rejected authorization or disconnection is a typed launch
failure with `run_authorized = false`, not ordinary completed user-command
history. Preserve any actual helper wait status as launch diagnostics. After Run,
the retained Child's raw status is authoritative lifetime evidence, paired with
the execution observation above. A correlated helper exec failure is a distinct
launch-failure classification even if its actual helper exit happens to be 126
or 127; do not turn it into an ordinary user command result. A post-Run
`not_observed` result can retain authentic lifetime history without claiming the
shell was executed or reached interactive readiness.

Distinguish helper failure, configured-shell exec failure and an actual shell
exit. A close-on-exec error channel can carry an exec error without using stdin;
an empty/closed channel alone must not be labeled semantic application readiness.
Do not map every helper failure to an apparently normal user-command exit 127.

## Minimal real-process regression

Use the current public LocalZellij Start/native Desktop owner fixture with the
real retained Zellij dependency and actual production CLI/supervisor. Do not add
a mock publisher, fake PID record or a test-only startup/readiness export.

The primary regression is a small table of commands that append one byte to a
private counter and immediately `exit 0` or `exit 7`. There is no workload marker
wait, input release, sleep or rendering prerequisite. Assert one actual command
execution, the exact real terminal status, a live-authenticated receipt for the
same owner generation, actual saved completed history and joined frontend/
observer ownership. Keep the pre-fix red logs; if a case happens to bind before
exit, it has not independently demonstrated the intended race.

Add the distinct startup-file case using a supported real selected shell and its
private startup-directory option, such as zsh `ZDOTDIR` with a fixture `.zshenv`.
That file appends one byte and exits 7; the command body appends a different byte
if reached. Assert one startup execution, no body execution, exact status and the
same authenticated completion path. Do not change the user's home/startup files
or substitute a fake shell implementing the expected result. Record unavailable
platform/shell coverage rather than treating it as green.

The deterministic admission invariant belongs at the actual helper/supervisor
process protocol: while the real gate is awaiting committed Run, an independently
observed command counter remains absent; a valid one-use Run executes it once;
EOF before Run does not execute it. This is a separate protocol-ordering risk
from public Start/persistence integration. Use actual child exec and kernel peer
checks; do not fabricate the binding result that the starter is responsible for.
Reject duplicate/wrong-generation Run at that boundary. Do not repeat the current
finite-after-bound status table as another alleged fast-exit regression.

## Native input readiness remains separate

`broker.rs` emits TransportConnected after an authenticated relay connection;
the native worker currently turns that into Ready. Neither fact proves the
configured Zellij frontend admits ordinary input. In the pinned Zellij source,
`stdin_handler.rs` discards non-query bytes while its startup parse deadline is
active, and `lib.rs` buffers Render instructions until DoneParsingStdinQuery.

The existing input fixture waits for actual workload rendering before one input
token. That is appropriate fixture admission evidence, not a product contract.
Do not infer readiness from connected sockets, sleeps, arbitrary nonempty output,
the literal loading-screen word `done`, or fabricated query replies/cache data.
A future readiness slice needs an authenticated typed producer event at the real
input-admission transition, correlated to this frontend/launch generation, plus
an immediate one-shot input regression. A pinned-dependency change would require
its own reviewed producer/package/platform evidence. This design neither enables
native Start/Attach nor closes that gate.

## Native saved-history acceptance — 2026-10-16

Run `38847303782` attempt 2 at `a2ac7a5` passed the actual Models decoder
(5 positive variants, 8 exit boundaries and 36 malformed contracts) and both
legacy V1 and V2 exec-failed AppKit history scenarios. Each preserves exact copy,
read-only input, selection/scroll/focus across a cross-window move and source close.
All 261 retained source hashes were checked against the exact Git objects; archive
integrity passed. Receipt SHA256
`be6af63b2bef7217c2c861edd82c0dc793cdac3465bd50f144c37e2ab2ad147d`.

The first attempt lost downloadable evidence to GitHub artifact-service timeouts;
the second supplies retained evidence. This fixture establishes native presentation
of generated saved documents, not live supervisor authentication or actual Darwin
execution-admission acceptance. The separate runtime package gate remains open.

## Retained Darwin runtime and main integration — 2026-10-16

Run `38849188824` at `b15fb47` passes the executed ARM64 owner scope: 51 tests
across six targets, nine successful untruncated phases, 293 source hashes matched
against Git, retained dependency identity and archive integrity. Review SHA256
`83221b29e52c2b3639b35e9ffad8ecae33e1b096a26e39636b820c8652d7345c`;
native receipt SHA256
`396bce42c283a698e91858b6ec62ad0f95bb3ef562716c01e3be5314f2881d67`.
The workflow does not execute the separate CLI `execution_admission` target; its
Linux evidence remains separate. No observer4 readiness, Intel, minimum OS or
default-activation claim follows from this run.

Main integration through `58fbd92` preserves the V8 and immediate-exit regression
commits, then the reviewed runtime and presentation changes. All 42 files touched
by this cohort exactly match the accepted branch. Existing browser obsolete-event
and window-backpressure work remains. Integrated repository checks pass 357 tests, strict Clippy, notices and both
foreign ABI checks.
