# Isolated kernel PTY reply contract

Original MIT fixture; no app engine, metadata-input or production dependency change.
The base lock pins 28 unchanged accepted metadata inputs plus the separately
revised subprocess runner, shared lifecycle helper and regression tests. Every
locked hash must match before/after execution; the runner delta is recorded in
the lock.

- One accepted Resource/Integrated/Metadata owns the sole engine and normalizer.
  One separate POSIX transport owns one actual PTY, one Python child and two pipes.
  The child uses a raw no-echo slave; it stays in the fixture process group. This
  is not a controlling-terminal, shell/job-control or native input acceptance.
- Fixed transport input storage is 8,192 bytes; each user admission caps at 4,096,
  engine reply batches at the accepted 4,096. Reserve before reading at most 512
  output bytes. No parser outside Ghostty, second terminal or per-step host growth.
- Prepare uses the accepted staged metadata/frame export and validator. A validated
  result stays sealed while replies advance by actual nonblocking write counts.
  Commit+swap occurs only after all reply bytes are accepted by the kernel. Busy
  rejects user/resize admission without consuming it; caller retains that request.
- One admitted input/reply segment has a 250-ms experiment lifetime, including
  stalling. Deadline/cancel quarantines the owner, stops only its child and reaps
  it with TERM/50ms then KILL/200ms escalation. Kernel waits/scheduling are not
  guaranteed by application memory accounting; outer subprocess-group containment
  also reaches the inherited child on panic/timeout. No infinite Busy loop occurs.
- Irreversible partial writes cannot be retracted. A failed pending result leaves
  last-good frame/metadata/notices unchanged and rejects further input. No replay,
  reset/fallback or consumed-output receipt is promised. Earlier successful chunks
  remain committed; a child write is not an engine operation atomicity boundary.
- Kernel write success is acceptance, not consumption. Child compares independent
  literal cursor/status bytes between actual PRE and POST input, and separately
  confirms a consumed three-byte prefix before close/cancel. Real kernel EAGAIN
  and short-write counters are reported; a deterministic syscall cap exercises
  partial progress even when the kernel never returns a short write. The basic
  journey requires positive partial reply writes with a three-byte syscall cap
  and checks unchanged publication after each incomplete write. The flood path
  checks the same invariant after a successful kernel short write, before later
  EAGAIN/commit/failure can obscure the negative control's first violation.
- Idle resize stages/validates engine state, performs TIOCSWINSZ, then commits.
  Pending replies make resize Busy; retry is explicit after progress. A real
  ioctl error quarantines without rollback claims. The child checks kernel size.
- Idle EOF is distinct from losing a peer with an unpublished reply suffix. The
  latter fails the staged result. Close/kill/reap and physical descriptor closure
  are observed before owner destruction; RAII is the failure cleanup backstop.
  Partial construction drains reports only after nonblocking setup succeeds.
  A separate bounded subprocess exhausts real descriptors after the report pipe,
  requiring prompt failure and closure of every owned descriptor before return.
  Cleanup syscall/wait errors are surfaced; successful reaping is checked in the
  exercised paths, not guaranteed under every possible operating-system error.
  Cleanup first reaps an already-exited direct child before signalling. A wait
  error grants no signalling authority; EPERM/ESRCH is benign only after that
  exact direct child is actually reaped. Failures retain the first cleanup
  operation and errno. Kernel-boundary cases cover an exited unreaped child and
  a real ECHILD after external reaping; neither authorizes a stale-PID signal.
  After signalling a live child, close the owned PTY master before bounded
  reaping waits so an output-blocked Darwin child can finish exiting. Retain
  the report descriptor until after reaping. A real stalled child and physical
  descriptor checks guard this ordering without extending cleanup deadlines.
- The original child has fixed modes and finite transcripts. Its stdout is actual
  terminal bytes; report/control pipes contain only bounded protocol bytes, not
  VT data. No shell or arbitrary code is injected into a terminal.
- Original-coordinator negative controls duplicate a reply byte or publish a
  partially delivered candidate. The actual child/order and last-good assertions
  must fail specifically. Every locked input stays fixed during the controls;
  the accepted terminal/metadata implementation and patches remain unchanged.
- Reserved reply/input payload capacity totals 24,576 bytes, including the unused
  original Probe 4,096, active collector 4,096, two Rust publication slots 8,192
  and transport input 8,192. These are already included in their existing C/slot/
  inline storage charges and must not be added again or replaced by observed
  high-water. Temporary engine reply allocations remain charged by its allocator.
- Engine mappings, existing Rust slots, extra inline coordinator/transport storage,
  child heap/threads and kernel PTY/pipe buffers remain separate accounting classes.
  Nothing here bounds total app RSS or establishes production/native/workload parity.
