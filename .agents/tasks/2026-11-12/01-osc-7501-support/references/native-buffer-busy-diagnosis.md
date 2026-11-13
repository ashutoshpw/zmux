# Native response-buffer BUSY diagnosis

**Scope:** read-only diagnosis at `main` / `7f6d5ac29c4d51d00d4f08f416d2dae7b1d41557`. No tests, checks, builds, workflows, source edits, staging, commits, or pushes were run. Only this reference was added. The pre-existing dirty task/status and profile-lifecycle paths were left alone.

## Verdict

The hosted error is a response-consumption failure after the shared-terminal FFI call returned a response token. `NativeBridge.consume` successfully read the token length, then `zmux_buffer_copy` returned `-4` (`BUSY`) while looking up that same token in the process-wide Rust registry. This is evidence of registry contention during response copying, not evidence that the shared-terminal request itself was refused.

The operation must not be replayed. A shared-terminal mutation can already have been admitted to the terminal worker before Rust returns its success response. The existing Cloud owner preserves this uncertainty: `CloudTerminalHost.process` catches the bridge error, sets `outcomeUnknown` for a mutation, and responds to the existing claim; it does not submit the native request again.

## Source evidence and interleaving

- `apps/macos/Sources/NativeBridge.swift:233-253` runs `zmux_desktop_shared_terminal` in a detached task and calls `Self.consume` only after `result.status == 0`.
- `apps/macos/Sources/NativeBridge.swift:833-845` consumes a token with one `buffer_length`, one `buffer_copy`, and a deferred single `buffer_release`. A failed length/copy throws. Release failure is only logged.
- `crates/native-bridge/src/lib.rs:41-46,165-183` maps a contended `try_lock` to `BUSY = -4`; `buffer(id)` looks up and clones the response `Arc` under the global `REGISTRY` mutex.
- `crates/native-bridge/src/lib.rs:447-485` uses that lookup for length/copy and uses the same global registry's `try_lock` for release. Copy itself occurs after the lookup has released the mutex.
- `crates/native-bridge/src/lib.rs:730-779` reserves response storage before calling the shared-terminal owner and publishes a response token before returning. `crates/desktop-runtime/src/shared_terminals.rs:463-508` validates a mutation, sends it to the executor, waits for its receipt, and returns `accepted:true` after executor admission.
- `apps/macos/Sources/CloudTerminalHost.swift:624-640,666-672` responds to the original Cloud claim after a bridge error. It marks a mutation's result unknown; there is no automatic replay.

The observed sequence is therefore:

1. Rust returns a successful `ZmuxResult` containing token `12`.
2. Swift's `buffer_length` lookup succeeds.
3. A different native call acquires `REGISTRY` before the subsequent copy lookup.
4. The copy's `try_lock` returns `WouldBlock`, mapped to `-4`; `consume` throws after the original operation ran or was admitted.
5. The deferred release attempts one more registry `try_lock`. If that call also returns `-4`, the owned token remains in the registry and consumes one of the 16 response-buffer slots. That leak is possible in the source contract, but it is **not established for this run**: the retained lifecycle stderr contains no `Zmux buffer release failed` line.

The exact lock owner at step 3 is unknown. One concrete source path can hold the same registry across comparatively long work: `create_configured_desktop_with_leases` acquires `REGISTRY` at `crates/native-bridge/src/lib.rs:614` and retains the guard across `Desktop::with_options` and optional `ControlServer::start_with_policy` through line 653. This demonstrates an owner capable of delaying unrelated registry access; it does not prove it was active during token 12's copy. Other registry insert/publish/remove critical sections are short.

The retained packaged log records the first native failure as `Native sharedTerminalOperation(request:grant:):buffer_copy token=12 failed (-4)`. It occurred before `FilePanelSmoke`; no file-panel stage or per-tab state was produced in that run.

## Repair boundary

The narrowest robust owner is the Rust native response-buffer store, because that owner controls token lifetime, the 16-buffer capacity, and the global lock that produced `BUSY`. Keep request admission semantics unchanged: `Reservation::new` must still reserve a response slot before any effect and may return `BUSY`/`CAPACITY` before work. Do not retry `zmux_desktop_shared_terminal` or any other already-dispatched request.

Recommended design for parent acceptance:

- Store response buffers behind a dedicated bounded buffer registry instead of the global desktop/core handle registry. Preserve one process-wide monotonically increasing token allocator, for example with an atomic ID source, so splitting storage does not introduce token collisions or reuse.
- Keep the existing 16-buffer cap and pre-effect reservation. Publish and release entries through the response-buffer registry.
- Make `buffer_length`, `buffer_copy`, and `buffer_release` acquire that small buffer registry with a normal lock rather than mapping incidental lock contention to `BUSY`. Keep all work under that lock to bounded map/count/`Arc` operations. Clone the response `Arc` under lock and copy bytes after unlock; remove a released entry under lock and drop its value after unlock. No filesystem, process, terminal, or network work belongs under this lock.
- Keep handle/core lookups and effect-bearing admission APIs on their existing nonblocking `try_lock` paths. The unrelated desktop startup lock then cannot make a successfully returned response token unreadable, and `-4` retains its pre-effect meaning at operation admission.

This design addresses both the observed copy failure and the one-shot release leak without changing any caller's request, extending operation deadlines, or adding a retry around an ambiguous mutation. A narrower alternative is a bounded retry in `NativeBridge.consume` of only the token's `length/copy/release` operations, always using the same token. That crosses the shared consumer used by many bridges, needs cleanup behavior when release remains busy, and must remain separate from all request-level `-4` retry logic; it is less robust than removing unrelated handle-registry contention from token reads.

`NativeBridge` is an actor documented as keeping Rust operations off AppKit's main actor (`NativeBridge.swift:81-82`); the Cloud call and several receipt paths already use detached tasks. A Swift retry could therefore be bounded and yielding, but it would require changing the shared consumer/call sites and would not by itself solve a release failure. A Rust response-store boundary keeps this behavior centralized for Swift and C consumers and avoids adding waits to the desktop executor.

## Existing coverage and limits

- `crates/native-bridge/tests/abi.c:20-35,80-90` exercises successful sequential length/copy/release and invalid-after-release behavior. Its snapshot helper retries `BUSY` from the operation call, then performs buffer calls once; it does not force contention inside length/copy/release.
- `crates/native-bridge/tests/abi.c:455-475,742-773` checks the 16-buffer capacity contract and that a mutation is refused before applying when no response slot is available. This is valuable admission coverage, not response-copy contention coverage.
- `crates/native-bridge/tests/swift_file_contract.rs:2355-2419` verifies that `-4`/`-5` from the file operation map to known pre-work refusals. Those assertions do not define `-4` from `zmux_buffer_copy`, which is a separate post-success operation.
- `crates/native-bridge/tests/files.rs:13-20` serializes tests that share the process-wide nonblocking registry. The existing C ABI tests include no deterministic lock interleaving for a returned response token.

The earlier file-panel timeout is mechanistically related but not attributable from available evidence. `NativeBridge.file` also calls `Self.consume` after a successful `zmux_desktop_file`; if its response copy fails, `FilePanelView.send` wraps the error as an unknown internal failure and `FileLoadGate` does not retry that attach. That could leave a tab unloaded until the packaged smoke's existing timeout. The earlier failure artifact had no per-tab/bridge state proving this happened, and the current run failed before the file-panel smoke. Treat this only as a hypothesis.

## Test-authoring gate for a regression

1. **Observable contract:** after an FFI operation returns an owned response token, unrelated desktop/core registry contention must not make the response unreadable or unreleasable. The operation must remain single-dispatch; response handling may act only on the returned token.
2. **Credible regression:** a successful or mutating FFI call returns a token, a concurrent handle-registry owner holds `REGISTRY`, and a subsequent token read/release returns `BUSY`; the caller loses an already-executed receipt, and a contended release can exhaust the bounded 16-token pool.
3. **Why current tests miss it:** current tests cover sequential buffer operations, pre-effect capacity refusals, and retryable pre-effect operation `BUSY`; none holds the actual handle registry while consuming an already-returned buffer token.
4. **Production seam:** none is needed for the recommended Rust owner test. A unit test inside `crates/native-bridge/src/lib.rs` can create a real response token, hold the private handle `REGISTRY` mutex, and call the actual exported `zmux_buffer_length`, `zmux_buffer_copy`, and `zmux_buffer_release` functions. On the current code, the first call deterministically returns `BUSY`; after moving buffers to their own registry it should return the exact bytes and release successfully while the unrelated handle lock remains held. This is an intended-red control at the C ABI owner and adds no production hook. Do not add the test before parent accepts the repair design.

No tests/builds were run here. Current hosted evidence remains a failed packaged native acceptance with no final universal artifact; this diagnosis is not a repair or delivery.
