# R03 foreground consumer: bounded runtime slice

2026-10-16. Original MIT Rust implementation. This slice activates no AppKit UI,
bridge configuration, default installation, or packaged distribution behavior.

The desktop launch worker owns the actual foreground Cargo CLI through Session,
with graceful bounded shutdown. Its Session resource retains the private observer
namespace and monitor until physical child/PTTY cleanup ends. Kernel credentials
and the full directly owned CLI identity authenticate the one accepted socket;
all lifecycle events retain immutable installation/profile/operation/namespace/
surface/session/generation correlation and exact sequence. Startup uses one
absolute deadline created before launch. No shell fallback or PID-adopted signals
exist. A private non-deserializable Saved value is the sole reservation ACK source.

The existing S01 FIFO writer owns the real checkpoint. The workload's independent
Python reader checks the actual saved attach-only owner/profile descriptor before
recording startup. The real CLI/Session fixture proves input, detach cleanup,
Attach input with identical original identities and startup count one, then closes
both native frontends while their original durable owner remains alive. Each close
requires full final frame, monitor join, namespace cleanup, actual outer Session
join, CLI exit zero and no diagnostic. Independent retained CLI stdin ends only the
fixture workload afterward; it is not native finite-completion acceptance.

The actual refusal control configures no persistence and proves no CLI reservation
or profile creation. The actual writer-failure control replaces the owned test
session path with a sentinel symlink after initial save; the S01 no-follow open
fails after actual CLI owner reservation. It proves no backend/supervisor/workload
identity, no workload marker, unchanged sentinel, no acknowledged checkpoint,
failed persistence receipt, disabled autosave and actual Session/monitor/namespace
cleanup. Its shared shutdown reports the retained launch issue even though physical
cleanup completes. Synthetic queue and fence controls remain at the S01 owner.

## Retained activation gates

- The current final receipt only proves original-owner preservation while live.
  The supervisor reaps a finite workload and returns its exit status but publishes
  no authenticated completion record. Normal finite completion and identity loss
  are therefore indistinguishable. The latest real native finite-exit test failed
  with `observer final original-owner/physical frontend cleanup unconfirmed` and
  shared shutdown correctly retained that issue. Do not weaken identity checks or
  present this slice as full lifecycle acceptance. The next protocol slice must
  authenticate supervisor workload exit/status against exact owner/correlation,
  retain it durably and carry it through native Session finalization.
- Upstream Zellij drops non-query input during its startup terminal-query window;
  `transport_connected` is not terminal usability. The fixture waits for actual
  workload rendering, sends one CR-terminated input token and never retries it.
  Native activation needs a real readiness contract; fixed sleeps or fabricated
  terminal-query replies do not satisfy it.
- Current consumer evidence is Linux, local real dependency and Cargo CLI. Darwin,
  AppKit Start/Attach, minimum OS, Intel and default distribution remain unverified.

Evidence is retained under `target/agent-validation/r03-native-consumer-20261016/`.
The earlier three failed fixtures and reassessment remain separate. The latest
finite-exit failure is `actual-checkpoint-latest.log`; bounded positive/actual
negative controls pass in `actual-bounded-controls-second.log`; malformed external
wire controls pass in `schema-controls.log`. Final combined source/check receipts
are recorded separately by the source freeze, not implied by these focused runs.
