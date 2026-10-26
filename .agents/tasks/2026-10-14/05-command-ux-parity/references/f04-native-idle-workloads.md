# F04 controlled native idle workloads

## Objective and scope

Deliver a separate opt-in packaged baseline run with one, then eight idle
terminals. Keep the default vt100 engine, ordinary native smoke limits and
current retained-engine gates unchanged. This slice establishes phase-owned CI
measurements; it establishes no physical-Mac performance budget or candidate
comparison.

## Ownership and approach

- A new native Swift fixture uses the real coordinator/bridge actions to create,
  select and close tabs. It records exact surface/session/PTY-child identities
  from production snapshots while each new tab is selected. Hidden tabs remain
  hidden during idle-eight; native visible-frame count is not a live-session count.
- A separate Python runner owns the build invocation, private output/socket,
  finite shell fixture, phase journal and observer. It checks the child reports
  against exact app PID and process start identities. It retains partial evidence
  and checks shared shutdown plus disappearance of its exact child identities.
- Extend the existing resource sampler with optional phase metadata and bounded
  opt-in run limits. Its normal150-sample/155-second defaults and original native
  journey stay unchanged. Record warmup and measured stages separately.
- Use a dedicated manual workflow. Build through the existing host builder;
  do not edit its independently owned packaging interface. Retain source copies,
  source hashes, host context, archive, phase/child/native/resource receipts and
  failure logs.

## Finite workload contract

Repetitions are explicit1..3, default1. Each repetition runs idle-one then
idle-eight, each with30 seconds warmup and120 seconds measurement. At most eight
terminals are live. Later repetitions close the seven extra tabs through shared
actions before returning to idle-one. Creation/readiness and cleanup have separate
bounds; no phase completion renews the outer run deadline. The shell fixture must
have its own finite lifetime if the app fails. Record observer work and phase-tag
handoff delay; do not call CLI status timing input/display latency.

## Steps and acceptance

- [x] Review A01 process repair first; preserve other agents' source and routing.
- [x] Implement native phase owner and real finite child fixture without runtime
  injection or changes to production queue/engine policy.
- [x] Add phase-aware external resource observations and independent exact-child
  validation; unavailable metrics remain explicit.
- [x] Add isolated manual runner/workflow with exact source/artifact provenance.
- [x] Validate portable contracts and Python syntax; freeze for independent review.
- [x] Obtain committed Darwin typing/execution and inspect retained evidence.

The normal app correctness gate must remain separate from optional telemetry
availability. This opt-in workload run must disclose incomplete phase/resource
evidence rather than supplying zeros or accepting a partial measurement as complete.
No commit or dispatch is performed by this owner; the parent integrates after review.

## Portable validation and remaining native gate (2026-10-16)

The runner accepts only explicit1..3 repetitions. Native phases have a fixed1200s
outer deadline,10s readiness bounds, at most18 phase events and22 total recorded
children. External app execution is bounded1230s, observation1250s/1250samples,
current tree9processes including the app, log1MiB and resource document8MiB.
Each controlled shell execs Bash at the same registered PID with no startup files,
empty prompts, no imported prompt hooks and an independent1260s idle timeout.
Measurement summaries exclude observations starting before measurement or ending
at/after its completion. Phase admission requires a fresh observation beginning
at/after the native phase timestamp and the exact registered app children.
Every included sample must retain those exact child start identities. Per-phase
receipts retain unavailable counts/reasons and observation gaps. Any unavailable
observation, fewer than two available observations, or an endpoint/interior gap
over10s produces partial measurements and a nonzero runner result, even when
native phases and shared shutdown completed. The10s observer availability bound
allows the5s phase handoff plus bounded process/socket/CLI probes and interval;
it is not a product resource or latency budget.

Portable input validation passes for the maximum three repetitions. YAML parsing,
three extracted shell syntax checks, Python outer/child parsing, eight rejected
observer-bound inputs, phase copying/validation and actual Linux unsupported-state
publication pass. An actual owned Linux PTY child publishes its PID/PPID/nonce,
execs Bash with the same PID, excludes an injected PROMPT_COMMAND, exposes1260s
TMOUT and exits normally. This validates the child/observer boundary without
mocking macOS metrics or asserting source strings. Existing native smoke coverage
cannot establish the distinct real one/eight-child phase and cleanup contract.
No production queue, runtime or engine injection seam and no duplicate unit test
are added. The portable receipt is
`target/agent-validation/f04-idle-portable/receipt.json`, SHA256
`4d8eabc72e45e45e54c0a4830d7a6c71b8f0f097c28dd9fe800da94a318ad5b8`.

Swift6 compilation, actual AppKit coordinator phase actions, Darwin child-tree
observations and shared shutdown remain required hosted gates. This Linux check
does not establish native idle resource values or physical-Mac budgets.

## First native idle receipt and start-column repair

Run38827961978 at exacta85dc43 compiled the host app and completed idle-one,
then created all eight real children. The observer rejected idle-eight warmup:
the same first PID10820 had the same `Fri Oct 16 00:15:10 2026` start date,
but Darwin ps appended four column-padding spaces once more child rows followed
it. The existing process-table wrapper strips the whole output, so only the final
row loses padding; splitting each row into five fields preserves padding on the
remaining start column. This formatting change is not evidence of a child restart.

Normalize only leading/trailing presentation whitespace when storing the start
column in the existing process parser. Keep exact PID/date/start/registration
identity checks and all coverage requirements. Retain the failed run without
accepting idle-eight measurements or shared shutdown. Artifact12016251194 ZIP
SHA256 `aff7f18933eda4d05d4cae2fbdbf2568cff1da7af7213be7cd7621d083ac63f1`
is under `target/agent-validation/f04-idle-darwin-38827961978/`.

All221 retained source copies match exacta85dc43 Git blobs. A replay of the
actual retained multirow process columns preserves both observed padding forms:
the original parser rejects the same PID/date, the repaired parser accepts it,
and a one-second start change remains distinct. PID, PPID, RSS and CPU fields
remain unchanged. Raw ps stdout was not retained by the observer; replay inputs
reassemble only the actual retained columns and make no new native metric claim.
Receipt `parser-repair-receipt.json` in that evidence directory has SHA256
`253246c77bc4f31f320318d7c12b08468ee4fa4b8ea4f40a8944126f0d68a6a3`.
Actual corrected Darwin execution of both phases and shared shutdown remains open.

## Accepted one-repetition CI run

Corrected run38829001880 at `aac6677c02e4f28ceb4714f36acc2abe24433c07`
passes. Independent artifact review verifies all221 exact source copies against
Git, the archive hash/source plist, six ordered native phase events, eight actual
PTY registrations, fresh observations before each active phase handoff, and exact
PID/start identity in every measured sample. Both120s windows have complete
resource coverage with no unavailable sample. Shared native shutdown completes,
the app exits0, and the executed runner's fresh identity check finds no matching
child alive. Raw final ps stdout is not retained; its absence check is enforced
by the exact executed runner rather than a separate raw-process-table artifact.

Host: macOS15.7.9, ARM64 VirtualMac2,1, three logical CPUs, GitHub image
`macos15` version `20260907.0337.1`. One explicit repetition only:

| Observation | idle1 | idle8 |
| --- | ---: | ---: |
| Available measured samples | 100 | 101 |
| Native measurement window, seconds | 120.116 | 120.781 |
| Actual sampled interval, seconds | 117.992 | 118.113 |
| App CPU over sampled interval, percent | 0.4661 | 0.4149 |
| App RSS, KiB | 48320–48400 | 51424–54656 |
| Current app/child tree RSS, KiB | 50800–50880 | 69696–73472 |
| Available CLI launch+status probe, ms | 7.124–220.017 | 9.840–173.914 |
| Maximum observation gap, seconds | 1.3745 | 1.6708 |

Artifact12016976149 ZIP SHA256
`f2db8307b94543973f24be46bd9bda2602c867ba0665b5d794459e20f3191236`.
Independent receipt under
`target/agent-validation/f04-idle-darwin-38829001880/independent-review.json`,
SHA256 `e0cf0b81f9e0c054ed302af9406bbe2ef9698d8e78676ebe4e09513a174c4ea0`.
All measurement arithmetic/ranges and coverage were recomputed from retained
observations. The accepted ARM host archive SHA256 is
`ee812bf2490851863e39a53b2fe0c7de5d8336e9f1a118093a8b255a4365e828`.

These are named CI observations, not dedicated physical-Mac budgets, physical
footprint, input/display latency or engine-comparison evidence. Default vt100 is
unchanged; no optional-engine, Intel runtime or minimum-OS claim follows. Repeated
repetition2/3 lifecycle execution, output/hidden-output and remote stress remain
separate gates.

## Later work

Visible1MiB/s output, seven hidden producers with a foreground send-once marker,
authenticated SSH stall/disconnect/cleanup, browser attribution, physical footprint
and engine/queue counters are separate later slices. Five paired repetitions on a
named dedicated Mac with fixed display/font/power context must inform a versioned
numeric baseline budget before candidate comparison. GitHub VirtualMac results
remain named CI evidence, not dedicated physical-hardware budgets.
