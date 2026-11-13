# Native response-buffer contention repair

**Source:** `7f6d5ac29c4d51d00d4f08f416d2dae7b1d41557` (checkout remains on this SHA; no commit created).

## Repair

`crates/native-bridge/src/lib.rs` now separates desktop/core handles from response-buffer slots:

- `HANDLE_REGISTRY` contains only desktop/core entries and keeps its existing nonblocking `try_lock` behavior for handle lookup, creation, and release.
- `BUFFER_REGISTRY` owns all reserved and published response slots. `Reservation::new` uses its nonblocking `try_lock`, counts reserved plus published entries against the unchanged 16-slot limit, and allocates the token before any effect. Publish and failed-reservation cleanup use this same buffer store.
- `zmux_buffer_length`, `zmux_buffer_copy`, and `zmux_buffer_release` use the dedicated normal mutex. Lookup only clones the response `Arc` under lock; copy happens after unlock. Release removes the response under lock and drops the `Arc` after unlock. No desktop, process, filesystem, terminal, or network work runs under the buffer lock.
- A shared `AtomicU64` allocator now issues every core, desktop, and buffer ID. It issues nonzero IDs in monotonically increasing order, returns `CAPACITY` before integer overflow, and never reuses an ID. The public C ABI signatures are unchanged.

No caller retry or request replay was added. The operation admission path still reserves response capacity before effects; contention/capacity there remains a pre-effect refusal. Only already-owned response-token access is decoupled from the handle registry.

## Test-authoring gate

1. **Observable contract:** a returned response token remains readable and releasable while the unrelated desktop/core handle registry is contended; IDs are unique across handle and response types, and exhaustion fails with `CAPACITY` without reuse.
2. **Credible regression:** after a successful FFI operation returns a response token, another native call owns the handle mutex. The old buffer lookup then returns `BUSY`, losing the receipt; if release also sees contention, the token can occupy one of the bounded slots indefinitely.
3. **Why existing tests did not catch it:** existing ABI coverage consumed tokens sequentially and tested capacity before effects. File-channel tests serialize users of the process-wide registry, and the Swift file contract tests cover operation-level pre-effect refusals. None consumed a real returned token while the handle registry was held.
4. **Production seam:** none was added. The in-module regression obtains the actual private handle mutex, then calls the actual exported length/copy/release functions on a real response token. It observes exact response bytes, successful release, and invalid-handle status after release.

## Intended-red control

Before changing the owner, I added `response_buffer_registry_tests::response_buffer_access_is_independent_of_handle_registry_contention` and ran:

```text
cargo test --locked -p zmux-native-bridge --lib response_buffer_registry_tests::response_buffer_access_is_independent_of_handle_registry_contention -- --exact --nocapture
```

The old owner failed for the intended reason while the test held `HANDLE_REGISTRY` (then named `REGISTRY`):

```text
running 1 test
thread 'response_buffer_registry_tests::response_buffer_access_is_independent_of_handle_registry_contention' panicked at crates/native-bridge/src/lib.rs:420:9:
assertion `left == right` failed
  left: -4
 right: 0
test response_buffer_registry_tests::response_buffer_access_is_independent_of_handle_registry_contention ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out
```

The test drops the handle lock before cleanup and before asserting, so this red control neither deadlocks cleanup nor leaves the old response token reserved in the test process.

## Green and scoped verification

The same owner test passed after the repair:

```text
cargo test --locked -p zmux-native-bridge --lib response_buffer_registry_tests::response_buffer_access_is_independent_of_handle_registry_contention -- --exact --nocapture
running 1 test
test response_buffer_registry_tests::response_buffer_access_is_independent_of_handle_registry_contention ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out
```

The final `cargo test --locked -p zmux-native-bridge` run passed **50 tests**: 5 unit, 1 downloads, 7 files, 2 native frames, 33 Swift file-contract, 1 terminal-input, and 1 windows test; 0 doc tests. This includes the unchanged capacity-before-mutation, cross-type invalid-handle, and stale-release assertions.

The existing native bridge C ABI script passed:

```text
scripts/check-native-bridge
C ABI checks passed: wire decoding, ownership, stale handles, bounded copy, capacity-before-mutation and the framed trusted file channel.
Native profile lease C ABI passed: capacity before acquisition, independent-process contention, original-owner status and lease retention after Desktop shutdown.
```

Additional scoped gates passed:

- `cargo fmt --package zmux-native-bridge -- --check`
- `cargo clippy --locked -p zmux-native-bridge --all-targets -- -D warnings`
- `git diff --check -- crates/native-bridge/src/lib.rs`

No full `scripts/check`, macOS app build, workflow, commit, or push was run.

## Proof limits and handoff

The deterministic control proves a returned buffer is independent of the handle registry mutex and its existing buffer assertions prove copy/release behavior on the actual exported functions. It does not identify which native call held the registry during hosted token 12. The proposed file-panel connection remains a hypothesis; this repair did not rerun the packaged app or FilePanel smoke. The exact-source universal app still requires parent-owned aggregate validation and fresh hosted acceptance before delivery.

Only `crates/native-bridge/src/lib.rs` and this reference were changed for this repair. Pre-existing dirty/index/task-plan/status work and the earlier `native-buffer-busy-diagnosis.md` remain untouched.
