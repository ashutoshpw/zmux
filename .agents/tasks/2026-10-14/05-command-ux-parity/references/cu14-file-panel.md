# CU-14 / CV-01 / CV-05 / CV-06 file panel: evidence receipt

Status: in progress. Evidence for each slice of [cu14-file-panel-design.md](cu14-file-panel-design.md)
is appended here as the slice lands. Original Zmux code, MIT; nothing under the cmux checkout was
read or consulted. This machine is Linux without a Swift toolchain: every Swift, macOS and hosted
gate is recorded UNAVAILABLE, never passed.

## S1 core model (file surface kind, actions, validation, session persistence)

Base: `19df8e02fb23019bc83bbd287fdc3bfacf93e1ab`.

### What changed

- `workspace-core`: `SurfaceKind::File`, `FileMode` (`text|markdown|image|pdf`), `FileSurface
  {path, mode, dirty}`, `Surface.file`; `Action::CreateFile` and `Action::ObserveFile`;
  `TargetAction::CreateFile` (resolves the target pane into `Action::CreateFile`); path validation
  (absolute, UTF-8, at most 4096 bytes, no control characters, no `.`, `..`, empty or trailing
  component); descriptor and cross-kind exclusivity validation in
  `validate_terminal_descriptors` (also run on capture), including document-wide canonical-path
  uniqueness; `ensure_closeable` beside every `ensure_settled` close guard; `fresh_surface` copies
  the file with `dirty = false` and refuses a path that is open elsewhere; `MODEL_VERSION` 6 -> 7;
  `SESSION_DOCUMENT_VERSION` 9 -> 10 with `SavedContent::File {path, mode}` gated to version >= 10,
  `RestoreLimits.files`, `ValidatedSessionDocument::file_count()`.
- `desktop-runtime`: `MAX_FILE_SURFACES = 16`, `ensure_surface_caps` (browser and file caps) in the
  candidate transition, `RestoreLimits.files` fed from it, `Command::Action` and
  `TargetOperation::Action` reject `CreateFile`/`ObserveFile` ("trusted file opener and native
  document owner"), `list-pane-surfaces` entries gain `"file"`. `PersistenceOutcome::Loaded` is now
  boxed (clippy `large_enum_variant` after `ValidatedSessionDocument` grew a field).
- Swift (UNAVAILABLE to compile): `NativeBridge.swift` model version check `== 7`, `Models.swift`
  `FileSurface` plus `Surface.file`, `App.swift` explicit `paneView` kind dispatch (a `file` surface
  shows "File panel is not available in this build"; an unknown kind shows an error label instead of
  silently becoming a browser), `WorkspaceViews.swift` tab icon (`doc.text` for files).
- `docs/native-bridge.md`: workspace-model section updated to version 7 with the file shape.
- Persisted privacy (D9): path, mode and the (renameable) title only; never content, hash, dirty
  bit or view state.

### Red then green

Red: tests were written first and `cargo test -p zmux-workspace-core --test surfaces` failed to
compile against the old API (`error[E0433]: cannot find type FileMode`, `error[E0609]: no field
file on type Surface`, ... `could not compile zmux-workspace-core (test "surfaces") due to 48
previous errors`). Behavioural red was not separable from the missing types on this tree, and a
non-compiling red commit is forbidden, so S1 lands as one cohesive validated commit.

Green: all new tests pass after the implementation. One mutation check confirms the close guard test
is not vacuous: making `ensure_closeable` ignore the dirty bit fails
`close_paths_refuse_dirty_file_atomically_and_succeed_once_clean` (restored afterwards).

New tests (all run by `cargo test --workspace --locked`):

- `crates/workspace-core/tests/surfaces.rs`: `create_file_appends_a_tab_without_session_effects_and_honours_focus`,
  `create_file_rejects_bad_paths_and_modes_without_touching_the_store`,
  `create_file_rejects_duplicate_paths_and_a_full_pane`,
  `observe_file_sets_and_clears_dirty_only_on_editable_file_surfaces`,
  `observe_file_is_idempotent_and_does_not_advance_the_revision_when_unchanged`,
  `close_paths_refuse_dirty_file_atomically_and_succeed_once_clean` (CloseSurface,
  CloseOtherSurfaces, ClosePane, CloseWorkspace, CloseWindow),
  `reopen_file_resets_dirty_and_rejects_open_path`,
  `reopening_a_closed_pane_or_workspace_keeps_file_paths_clean`, `move_surface_keeps_file_state`,
  `snapshot_serializes_file_surface_shape`, `model_version_is_seven`.
- `crates/workspace-core/tests/session_document.rs`: `v10_round_trips_file_surfaces`,
  `v9_rejects_file_content`, `legacy_versions_still_load`, `capture_omits_dirty_and_content`,
  `restore_rejects_invalid_or_duplicate_file_paths`, `restore_enforces_file_limit`,
  `model_version_is_seven`.
- `crates/workspace-core/tests/targets.rs`:
  `target_create_file_resolves_the_selected_pane_into_a_core_action` (also asserts there is no
  targeted `observe_file`).
- `crates/workspace-core/src/local_zellij.rs` unit test `descriptor_validation_rejects_mixed_kinds`
  (needs private store state: nine cross-kind corruptions, dirty image, bad path, duplicate path).
- `crates/desktop-runtime/tests/desktop.rs`: `public_action_rejects_create_and_observe_file`
  (`Command::Action` and the targeted wire form, model unchanged).
- `crates/desktop-runtime/src/lib.rs` unit test
  `file_surface_cap_is_enforced_independently_of_the_browser_cap` (16 allowed, 17th rejected).
- `crates/desktop-runtime/tests/session_persistence.rs`:
  `file_surface_persists_and_restores_through_the_private_state_directory`,
  `restore_over_file_limit_is_rejected_with_recoverable_state`.
- Existing tests adjusted for the version bump / new `RestoreLimits.files` field only
  (`workspace_groups.rs`, `session_persistence.rs` version assertions, `RestoreLimits` literals).

### Checks run

- `scripts/check` (fmt, `cargo test --workspace --locked`, clippy `-D warnings`,
  `scripts/license-inventory --check`, `scripts/check-native-bridge`): PASS, exit 0. Earlier runs in
  this slice failed on two version assertions, one `clippy::type_complexity` and one
  `clippy::large_enum_variant`; each was fixed in the source, nothing disabled.

### UNAVAILABLE gates

- `scripts/check-macos-sources`, `scripts/build-macos-dev`, `scripts/check-macos-app`, the in-app
  smokes and `.github/workflows/macos-development.yml`: need macOS/Swift. The four Swift edits are
  unbuilt and unrun; macOS CI is the verifier.
- Native keyboard, focus and rendering behaviour of the file panel: S4, not started.

### Deviations from the design, with reasons

- Path validation also rejects empty (`//`) components. The design lists `.`/`..`/trailing `/`;
  a canonical `realpath` never contains an empty component, so this is strictly consistent.
- `TargetAction::CreateFile` was added (task requirement for CLI routing) although the design routes
  `open` through `TargetOperation::OpenItems` in S2/S3. Because `TargetOperation::Action` is a public
  wire form, `desktop-runtime` rejects the resolved `CreateFile` there as well, preserving D16
  ("`Action::CreateFile` is rejected on public action paths"). There is no targeted `observe_file`.
- `seventeenth_file_surface_is_rejected` is a unit test of `ensure_surface_caps` in `desktop-runtime`
  rather than a `tests/desktop.rs` test: until S2 there is no trusted in-process constructor, and
  the public path correctly rejects `CreateFile`. The restore-side cap is covered end to end by
  `restore_over_file_limit_is_rejected_with_recoverable_state`.
- `capabilities` does not yet carry `file_panel` and `inspection.rs` is unchanged (its `kind` field
  already serializes as `"file"` and `session` is null for non-terminals); both belong with the
  S2/S3 surface they describe.
- `ObserveFile` that does not change the dirty bit returns the current revision without a new one
  (D7a point 1); this is implemented in `apply_inner`, not in `surface_action`.
- `CloseOtherSurfaces` checks only the tabs that will close, so a dirty tab that stays open does not
  block closing its siblings (the design says "affected file surface").
- Reopen duplicate-path protection lives in `fresh_surface`, so it covers reopened tab groups,
  panes and whole workspaces, not only single tabs.
- `PersistenceOutcome::Loaded` is boxed to satisfy clippy after `ValidatedSessionDocument` gained
  `files`.

## S2 file service (bounded worker, trusted file I/O, bridge ABI)

Base: `e66b1675e079db4c566f2a7d881ebb833ffd6d84`.

### What changed

- `desktop-runtime/src/file_io.rs` (new): `FileBackend` trait plus `RealFs`; component-by-component
  `openat` walk with `O_NOFOLLOW|O_DIRECTORY`, leaf open `O_NOFOLLOW|O_NONBLOCK` and a regular-file
  check (never the 0600/owner rule of `persistence.rs`); `classify` (extension table, forced
  Markdown), `sniff_text`/`validate_text` (whole file, NUL and UTF-8), signature checks, `load`,
  `probe` (lexical operand rules, one `realpath`, canonical UTF-8 path), `observe`, and the atomic
  same-directory `save` (private `0600` exclusive temp, `fchmod` to the original bits, `fsync`,
  worker re-fence, last-moment fingerprint re-check, `renameat`, directory `fsync`, no in-place
  fallback, hard-link refusal, BOM round trip). Sizes: 2 MiB text, 7 MiB preview, 4096-byte path.
- `desktop-runtime/src/files.rs` (new): `FileService` owning one `zmux-files` thread, a bounded
  8-job queue plus one running job, per-tab single operation in flight, host/epoch/surface owner
  fencing with a 256-entry retired ring, monotonically increasing `op_id`, bounded `last_op`
  receipts (`pending|completed|failed|outcome_unknown`), stall detection (`file_worker_unavailable`
  instead of queueing behind a hung volume), content-hash conflict detection, `stale_view`
  generations, and low-rate polling (per-tab interval, 4 stats per tick, metadata must be stable
  across two polls before a read, a delete needs two consecutive misses). `FilesConfig` is public
  (`DesktopOptions.files`) so tests inject backends and intervals. Executor side: trusted
  `Command::FileHost` (`Observed`, `Detach`, `#[serde(skip)]`) and `Command::OpenFiles`
  (`#[serde(skip)]`, applied in order: dedupe, focus-once, `file_capacity`, `tab_capacity`).
- `desktop-runtime/src/lib.rs`: `Snapshot.files` (epoch, worker health, at most 16 entries, never
  content or hashes), `Execution.open_result`, `DesktopError::File`, `Desktop::{file_open,
  file_attach, file_reload, file_save, file_observe, file_detach}`, the executor tick
  `dirty |= files.poll(..)` so worker-side changes advance `Snapshot.revision`, `files.reconcile`
  in `publish`, `files.reset()` on session restore, and the new commands in both restricted-state
  allow-lists. `MAX_FILE_SURFACES` is enforced in `Worker::action` (S1 `ensure_surface_caps`) and
  pre-checked per item as `file_capacity`.
- `desktop-runtime/src/shutdown.rs`: the file worker is woken, joined and reported as
  `pending.files` against the shared deadline; a stuck worker keeps the ticket pending ("IO may
  still be running") instead of being abandoned.
- `native-bridge`: trusted export `zmux_desktop_file` (`src/file_channel.rs`): framed
  `u32 BE header_len | header JSON | raw body`, `Reservation::new()` before any queueing, header
  at most 16384 bytes, non-save frames at most 16384, `save` frame at most 16384 + 2097152
  (`MAX_FILE_SAVE_FRAME_BYTES`), 2 s waits (5 s save; `open` is about 3 s worst case, a 1 s probe
  of all operands plus the 2 s native command). Ops: attach, reload, save, observe,
  detach, open (path items). `zmux.h`, `docs/native-bridge.md`, `tests/abi.c` and
  `scripts/check-native-bridge` updated. `ABI_VERSION` stays 1 (one additive export).
- No new dependency: `sha2`, `rustix`, `uuid`, `serde_json` were already direct dependencies;
  `scripts/license-inventory --check` and `THIRD_PARTY_NOTICES.md` are unchanged.

### Red then green

Red: `crates/desktop-runtime/tests/files.rs` (the original 25 behavioural tests; 44 after the third fix round, 45 after the fourth) was written before
`files.rs`. Run against a clean export of the base commit (`git archive e66b167`, separate target
directory, so no branch, worktree or stash was involved) it fails to build:
`error[E0432]: unresolved imports zmux_desktop_runtime::file_io, zmux_desktop_runtime::files`
and `error[E0433]: cannot find files in desktop_runtime` (`could not compile zmux-desktop-runtime
(test "files") due to 3 previous errors`). As in S1 the behavioural red is inseparable from the
missing types, and a non-compiling red commit is forbidden, so S2 lands as one cohesive commit.
The `file_io` unit tests (`file_io_tests.rs`) were authored with `file_io.rs`, not before it;
their strength is shown by mutation instead. Each mutation was applied, observed failing, then
restored (diff against a backup is empty):

- save fence forced true: `a_save_queued_behind_a_closed_surface_never_writes` and
  `a_refused_fence_writes_nothing_and_removes_the_temporary_file` fail.
- leaf open without `O_NOFOLLOW`: `later_operations_refuse_symlink_swaps_with_path_changed` fails.
- conflict hash comparison disabled: `conflict_modified_and_deleted_require_explicit_force` fails.
- delete reported after one miss: `one_miss_is_not_a_delete_but_two_are` fails.

Green at the original slice commit: `scripts/check` exit 0 (490 tests passed at that time). The
current total is in the third fix round below. Tests as of the third fix round (counts are the
`#[test]` functions in each file):

- `file_io_tests.rs` (31, of which 5 are third-round additions listed below): extension table incl. case, dotfile, no extension, `svg`/`html` as
  text; sniff window versus whole-file validation (NUL at offset 8193 and at the last byte of a
  2 MiB file => `binary_file`, stray invalid byte at offset 20000 => `invalid_utf8`, exactly
  2 MiB ok, one over `too_large`); PNG/JPEG/GIF/PDF signature mismatch; 7 MiB boundary; media
  => `binary_file`; operand rules; symlink resolved once to the canonical target; leaf and
  directory-component swapped to a symlink => `path_changed`; FIFO (`mknodat`), directory and
  `/dev/null` rejected without blocking; hard-linked file reads but `hard_linked` on save;
  permission bits preserved (`0755`, `0600`, `0640`, `0444`) and a recreated file gets `0644`;
  no leftover temp after injected failure at create/write/torn write/mode/fsync/rename;
  post-rename directory sync failure is `outcome_unknown` with the committed content reported;
  content-based conflict, touch and identical rewrite are not conflicts, over-cap growth is a
  conflict, change between check and rename is a conflict; BOM round trip; save body validation.
- `files.rs` unit tests (5): fingerprint-stable-for-two-polls rule, miss counting, epoch reset
  fences owners, idle shutdown join without waiting a full tick.
- `tests/files.rs` desktop-runtime (44; the original 25 are described here, later rounds below; real `Desktop`, gated and counting backend):
  attach/reload/save generations and hash of the disk bytes (CRLF kept); binary/late-NUL/late
  invalid UTF-8/oversize/missing/directory/relative never create a tab; mixed batch in order,
  symlink opens its target, focus only the first success, dedupe re-focuses, `focus=false` keeps
  selection; forced Markdown; `file_capacity` at 17; conflict modified and deleted with
  `force`; touch is not a conflict; `stale_view`; owner fencing by host, epoch, namespace,
  detach and surface close; a save blocked at `fsync` while its surface is closed never writes
  and leaves no temp; `busy` for a second operation on a tab; bounded queue (1 running + 8
  queued, the 10th is `busy`, nothing left pending for it); stalled worker fails fast then
  recovers and is published; save timeout is `outcome_unknown` with `op_id`, resolves to
  `completed` in the snapshot, exactly one rename ever happens and the view's next save is
  `stale_view`; reload refused while dirty unless `discard`; dirty only through the trusted
  host event and cleared on detach; `observe` idempotent (model revision unchanged); refused
  observe with the executor parked and the 32-slot queue full, then convergence and the close
  guard; polling (changed with `disk_generation`, second write, atomic replace restoring the
  baseline heals, delete only after two misses, recreate, touch stays `in_sync`, directory in
  place => `unreadable`/`is_directory`); worker changes advance `Snapshot.revision` and an idle
  service does not; executor and host events progress while the backend is gated; probe timeout
  per item; shutdown keeps the ticket pending while a worker is blocked and joins it after
  release; idle shutdown within the deadline; a unique sentinel never appears in the serialized
  snapshot or open result.
- `native-bridge/tests/files.rs` (7; 5 original, 2 added in the third round): frame validation (null, short, header over cap, header
  past the frame, not JSON, unknown op, unknown field, missing field, body on a non-save op,
  non-save over 16384, URL item, save body one byte over 2 MiB, wrong handle) all before any
  work; open -> attach -> 1.5 MiB save -> conflict -> reload -> observe -> detach round trip
  through the real exports; forged owner changes nothing on disk; `CAPACITY` with 16 buffers
  held and no tab created; `observe_returns_busy_not_ok_when_the_command_queue_is_full`.
- `crates/native-bridge/tests/abi.c`: independent C caller does open, attach (raw body check),
  framed save, stale-view refusal, detach and malformed-frame rejection through the shared library;
  `scripts/check-native-bridge` asserts its completion line.

Flake check (corrected in the third fix round): the original claim of 18 runs executed 6 at a
time was wrong in effect. A review reproduced intermittent failures of
`the_request_queue_is_bounded_and_overload_is_explicit` with eight concurrent copies (2 of 8 and
1 of 24 failed), because the test never proved the first job had left the queue before it
submitted the ninth. That is fixed and re-measured in the third fix round below. Two other races
found while writing the queue-full tests were fixed earlier: the executor must be proven parked
in the hook before flooding, and C-registry `BUSY` under parallel bridge tests is handled by
serialising those tests (the registry intentionally uses `try_lock`).

### Checks run

- `scripts/check` (fmt, `cargo test --workspace --locked`, clippy `-D warnings`,
  `scripts/license-inventory --check`, `scripts/check-native-bridge`): PASS, exit 0.
  Intermediate failures that were fixed in source, nothing disabled: `clippy::result_large_err`
  (boxed `DesktopError::File`, `FileFailure.conflict`, `SaveFailure`), `result_unit_err`,
  `manual_range_contains`, a shutdown-test deadline misunderstanding (root deadline had already
  expired), and the executor-parked race above.
- `scripts/check-native-bridge` on its own: PASS (C ABI including the framed file channel).

### UNAVAILABLE gates

- No Swift toolchain on this machine: `scripts/check-macos-sources`, `scripts/build-macos-dev`,
  `scripts/check-macos-app`, the in-app smokes and `.github/workflows/macos-development.yml`.
  The Swift side of the file channel (frame client, reconciliation loop for `observe`, orphan
  views, dirty-close sheets) is S4 and not started.
- macOS filesystem behaviour (APFS case folding, `/var` realpath, `renameat`/`fsync` durability,
  network volumes) and hosted GitHub Actions results: not run.

### Deviations from the design, with reasons

- The control-socket route was first deferred to S3 and is now built in S2 (second fix round, finding 3 below); the original deferral is withdrawn.
- `state_version` is replaced by value comparison: `FileService::poll` builds the `FilesSnapshot`
  and compares it with the last published one (stall age quantised to whole seconds). It cannot
  miss a bump and keeps the idle case silent; behaviour matches D6a.
- `FilesConfig` additionally carries `read_timeout` and `save_timeout` (defaults 2 s / 5 s) so
  tests can exercise the documented waits without sleeping seconds.
- One capacity-1 reply channel per request replaces a shared bounded completion channel; the
  admission bound is the 8-job queue plus one in flight per tab, so memory stays bounded.
- Bridge `open` accepts path items only; a URL item is `INVALID_ARGUMENT` (URLs are routed by the
  CLI to the browser action). A save body over 2 MiB is `INVALID_ARGUMENT` at the frame cap; the
  JSON `too_large` error exists for direct Rust callers.
- A refused `observe` is reported as an `ok:false` header with `error.code: "busy"` and C status 0
  (D15: every other failure is a JSON reply), not as a C `BUSY` status.
- The D7a test cannot also assert "a socket close still succeeds in the window": with the queue
  full the close cannot be submitted either. The test asserts the refused observe changes nothing,
  the retry converges, the close guard then refuses `close-surface`, and the close succeeds after
  `observe(false)`. The socket-close residual window stays documented, unproven.
- `tab_capacity` has no test: the reachable maximum is 8 terminals + 4 browsers + 16 files = 28,
  below the 32 per-pane cap. The code path exists and shares the core check.
- `FileFailure`/`SaveFailure`/`DesktopError::File` are boxed or shrunk for `clippy::result_large_err`.
  `Pending` (shutdown report) gained `files`, and entries gained `error_code`.
- A save that completes after its tab was closed answers its stale caller with the truthful
  success and `generation: 0`; nothing is adopted into a vanished entry.
- `create_desktop_with_options` is a `#[doc(hidden)]` Rust-only seam (not a C export) so the bridge
  tests can inject `FilesConfig`; the foreign ABI offers only the configured constructors.

### S2 fix round (review findings, all verified valid)

1. Reload (or attach) completing after the view detached re-bound the detached host. Fixed with a
   per-entry `detaches` counter bumped by `detach` and captured when a load is admitted; the worker
   discards the result (receipt `failed` / `stale_owner`, in-flight cleared, nothing adopted) when
   the counter moved. This covers a timed-out reload as well. Tests:
   `a_reload_that_outlives_a_detach_never_rebinds_the_old_view` and
   `a_timed_out_reload_finishing_after_detach_does_not_rebind_the_old_view`.
2. A queued save whose owner detached returned `stale_owner` without clearing `in_flight`, wedging
   the tab as `busy` with a `pending` receipt. The early-refusal path now clears `in_flight` and
   records `failed` / `stale_owner`. Test:
   `a_save_queued_behind_work_then_detached_releases_the_tab_and_never_writes`.
3. The 2 MiB cap ignored the preserved UTF-8 BOM, so a save could produce a file `load` refuses.
   The cap now applies to the on-disk size (`file_io::saved_size`), checked in `file_io::save` and
   at service admission using the baseline's BOM flag (`too_large`, nothing written). Tests:
   `a_preserved_bom_counts_against_the_save_cap_so_the_file_stays_readable` (file_io) and
   `a_preserved_bom_counts_against_the_save_cap_end_to_end` (Desktop).
4. Added the mandated `retried_observe_after_stale_owner_is_not_applied`: after detach, for a
   different view's later dirty bit, for forged epoch/namespace owners and after surface close,
   stale-owner observes are refused with `stale_owner` and change neither the dirty bit nor the
   revision. Deviation: a queued-then-superseded observe cannot be staged, because detach and
   close travel through the same FIFO executor queue as observe, so the test drives the retries
   after each supersession instead. The executor re-check (`check_owner` in `file_host_event`)
   remains as defence in depth.

Red/green: with the `files.rs` source fix reverted, the three detach tests fail (27 pass, 3 fail);
restored, all 30 `tests/files.rs` tests pass, repeatedly. Full `scripts/check` result recorded in
the fix commit message. Swift/macOS gates remain UNAVAILABLE as above.

### S2 second fix round (review findings, verified one by one)

Fix commits follow the first fix round (`af761de`). Every finding was reproduced or confirmed in
the code before changing it; none was rejected.

1. Terminal open (blocking, `file_io.rs`). Confirmed: `RealFs::open_leaf` opened a node before
   inspecting it and without `O_NOCTTY`. Now `open_leaf` first runs `fstatat(NOFOLLOW)` and refuses
   anything that is not a regular file, directory or symlink with `not_regular_file` before
   `openat` (design D13 step 1; a directory still reaches the caller's `is_directory`, a symlink
   still reaches `O_NOFOLLOW` and `path_changed`), and the open flags are the named constant
   `LEAF_OPEN_FLAGS` including `NOCTTY`. Tests: `opening_a_terminal_never_acquires_a_controlling_terminal`
   (Linux: a real pty master/slave pair; the child is the test binary started under `setsid -w`, a
   session leader without a controlling terminal, which opens the slave through `RealFs::open_leaf`
   three times and then asserts `/dev/tty` still cannot be opened; `ctty_child_probe` is that child
   half and is a no-op unless launched by the parent) and
   `the_leaf_open_flags_forbid_a_controlling_terminal_symlinks_and_blocking`. Mutation: removing
   both the stat check and `NOCTTY` fails the child test and the flag test; removing only `NOCTTY`
   fails the flag test. `rustix` gains the `pty` feature as a dev-dependency only.
2. Swallowed unlink failure (blocking, `file_io.rs`). Confirmed. The last-moment conflict branch
   now keeps the `conflict` detail but returns the message of `cleanup`, which names a temporary
   that could not be removed, like every other cleanup path. Test:
   `a_conflict_found_at_the_last_moment_still_reports_a_temporary_it_could_not_remove` (modified and
   deleted variants, `Faulty` backend gained an `unlink` failure). Mutation: restoring the
   `let _ =` fails it.
3. Control route owned by no slice (blocking). Confirmed against design section 6 and the receipt.
   Built now: `TargetOperation::OpenItems { items, focus, force_mode }` with `OpenItem`,
   `OpenItemKind` (`path|url`) and `OpenForceMode` (`markdown`); `control.rs` `open_items`:
   `gate.admit` has already refused read-only sockets (`is_read` is false), an early
   `expected_model_revision` check, `Desktop::probe_open_items` (file items on the worker under the
   shared probe deadline, URL items classified lexically: only `http(s)`, `unsupported_scheme`,
   `invalid_url`, paths only for `markdown`), then `Command::OpenFiles` (`#[serde(skip)]`, now
   carrying scope, caller and `expected_model_revision`). The executor resolves the target and the
   revision once (`prepare_target`, extracted from `targeted`, plus `scope.resolve`) before any
   item, appends tabs to the resolved pane in operand order, dedupes files desktop-wide, and
   applies the first-success focus rule across files and URLs. `mutate` merges `ok`/`items` into
   the standard receipt (`created_surfaces`, `focused_surface`, ...). A raw `OpenItems` through
   `Command::Targeted` is rejected with "open requires the control-socket probe path".
   `capabilities.file_panel` reports the limits in force (`Desktop::files_config`).
   `list-pane-surfaces` already carried `"file"` and `current` already reports `kind: file` with a
   null session since S1; both are now pinned by a socket test. `workspace-core` exports
   `validate_browser_url` so URL items use the one browser URL rule. Tests (real `ControlServer`,
   `tests/files.rs`): `a_mixed_batch_over_the_socket_returns_exact_ordered_item_results`,
   `reopening_dedupes_and_focus_false_keeps_the_selection`,
   `the_seventeenth_file_and_a_forced_mode_are_item_results_over_the_socket`,
   `a_read_only_socket_is_denied_before_any_file_is_touched` (zero backend calls),
   `a_stale_revision_is_a_whole_request_error_that_changes_nothing`,
   `the_executor_rechecks_the_revision_and_target_before_any_item_runs`,
   `a_gated_probe_times_out_per_item_over_the_socket_while_urls_still_open`,
   `open_items_validates_its_request_and_the_executor_path_refuses_it_raw`,
   `new_tabs_go_to_the_resolved_pane_and_inspection_reports_file_surfaces`,
   `capabilities_advertise_the_file_panel_limits_actually_in_force`. Mutations: skipping
   `prepare_target` fails the executor recheck test; removing the early socket check fails the
   stale-revision test.
   Deviations: a Desktop-wide browser cap hit is the new item code `browser_capacity` (added to the
   design's code list); `tab_capacity` stays unreachable by test (8 + 4 + 16 < 32). The CLI side
   (`apps/cli`, `docs/local-control.md`) remains S3.
4. Attach bypassing the dirty guard (blocking, `files.rs`). Confirmed. Attaching again as the
   bound host re-reads and re-baselines exactly like a reload, so `attach` now refuses with `dirty`
   (before any read) when that host's tab is dirty; unsaved text is dropped only by `reload` with
   `discard`. A different host is still `host_conflict`, an unbound entry attaches normally.
   Test: `attaching_again_as_the_bound_host_obeys_the_dirty_rule_and_never_loses_an_update`
   (refused, no read, generation unchanged, the old generation's save reports the external edit as a
   conflict, `reload(discard)` then re-attach works). Mutation: disabling the guard fails it.
5. Unpinned fencing (blocking). Confirmed by both mutations. Tests:
   `a_session_restore_rotates_the_epoch_and_fences_every_earlier_view` (export, close all, restore;
   the epoch changes, the old epoch and old owner get `stale_owner` for attach, reload, save,
   observe and detach, the file is untouched, a new-epoch attach works) and
   `the_registry_is_never_older_than_a_published_snapshot` (30 rounds of open then immediate attach,
   close then immediate stale owner and no entry). Mutations: commenting out `files.reset()` fails
   the first; commenting out `files.reconcile` in `publish` fails the second (and two older tests).

Swift, macOS and hosted gates remain UNAVAILABLE as above. Full `scripts/check` result is recorded
in the fix commit messages.

### S2 third fix round (review findings, verified one by one)

Two blocking findings and five minor items. Each was reproduced or confirmed before changing it;
none was rejected. The earlier red-test-commit protocol deviation (tests and implementation land
together because the first red does not compile) is unchanged and not repeated here.

1. Flaky `the_request_queue_is_bounded_and_overload_is_explicit` (blocking, `tests/files.rs`).
   Confirmed by reading the test: it submitted nine reloads without `wait_blocked(1)`, so the ninth
   `try_send` could run before the worker dequeued the first job, fill the 8-slot queue and be
   refused `busy`. Fixed exactly as prescribed: arm the gate, submit one reload, `wait_blocked(1)`,
   then submit the other eight (nine pending entries, then the tenth is `busy`). Determinism, all
   executed on this machine (24 CPUs, Linux), against the final test binaries:
   - desktop-runtime `files` binary (44 tests): 20 serial runs, all passed; 3 rounds of 8
     concurrent copies (24 runs), all passed; additionally 5 rounds of 8 concurrent copies (40
     runs) with 48 busy-loop CPU hogs running, all passed.
   - desktop-runtime unit-test binary (`file_io`, `files`, ... 85 tests): 20 serial, 24 concurrent
     (3 x 8), all passed.
   - native-bridge `files` binary (7 tests): 20 serial, 24 concurrent (3 x 8), all passed.
   I did not re-run the pre-fix binary under the same harness, so the 2-of-8 reproduction above is
   the reviewer's, not mine.
2. Search-only ancestors (blocking, `file_io.rs`). Confirmed: `RealFs::open_dir` opened every
   ancestor `O_RDONLY|O_DIRECTORY|O_NOFOLLOW`, which needs read permission. Now each component is
   tried that way first; on `EACCES` it is reopened traversal-only, keeping `O_NOFOLLOW|O_DIRECTORY`:
   `O_PATH` on Linux, `O_SEARCH` (`O_EXEC|O_DIRECTORY`, `0x4000_0000` from `<sys/fcntl.h>`) on
   macOS, nothing elsewhere (the original `EACCES` stands). Rationale is documented at
   `search_only_flags` and in design D13. A traversal-only directory descriptor cannot be fsynced,
   so `sync_dir` retries with a readable reopen and, only when that is also `EACCES`, treats the
   directory fsync as unavailable (the rename stays atomic; durability of that single directory
   entry across power loss is not guaranteed in a `--x`/`-wx` directory). Deviation from "never
   swallow errors", documented: every other `sync_dir` error still fails the save as
   `outcome_unknown`. The macOS branch is UNAVAILABLE (not compiled or run here).
   Tests: `search_only_ancestors_do_not_block_probe_load_observe_save_or_the_walk` (a `0311`
   directory as the file's parent and as a grandparent: probe, load, observe, missing leaf,
   symlink refusal inside it, save, save below it, conflict detection, no temporary left) and
   `a_directory_with_no_search_permission_is_still_permission_denied` (`0000` is still refused).
   Both skip with an `eprintln!` when running as root; this run was uid 1000, so they ran.
   Mutation: disabling the fallback fails the first.
3. Bridge `open` wait bound (minor). Confirmed: probe 1 s plus the 2 s native command. The
   documented bound is corrected to "about 3 s worst case" in `zmux.h`, `docs/native-bridge.md`,
   design D15 and this receipt.
4. `open_items` code (minor, `control.rs`). Confirmed: every `probe_open_items` failure was labelled
   `invalid_request`. Now `whole_request_code` keeps `invalid_request` for a malformed request,
   maps the file service's `closed` to the socket's existing `desktop_closed` (the code
   `desktop_error` already uses for the same condition) and any other whole-request failure to
   `file_error`. Deviation: the code is mapped rather than copied verbatim because the error type
   is `&'static str` and `desktop_closed` is the socket vocabulary. Test:
   `a_closed_desktop_is_reported_as_closed_not_as_an_invalid_open_request` (real socket; mutation
   restoring `invalid_request` fails it).
5. Committed fingerprint (minor, `file_io.rs`). Confirmed: `save` re-opened the path after
   `renameat`, so an external replacement in that window became the baseline. The temp file's
   descriptor now stays open across the rename and `fstat` on it supplies the fingerprint (it also
   carries the rename's ctime update, so an undisturbed save still matches the next poll). Tests:
   `the_committed_fingerprint_is_taken_from_the_written_descriptor_not_the_path` (a backend that
   atomically replaces the path right after the real rename; the reported inode is the written
   one, and `observe` sees a different fingerprint) and
   `an_undisturbed_save_reports_the_fingerprint_the_next_poll_sees`. Mutation: path lookup fails
   the first.
6. Preview probe (minor, `file_io.rs`). Confirmed: `probe` called `load`, reading and hashing up to
   7 MiB inside the 1 s budget. Image and PDF probes now use `fstat` size (via the new shared
   `open_regular`) plus one read of at most 1024 bytes for the signature, as D4 states.
   `Probe.sha256` is now `Option<String>` (`None` for previews; it was never consumed). Test:
   `image_and_pdf_probes_use_fstat_size_and_signature_without_reading_or_hashing_the_file` (a read
   spy: 7 MiB PNG and PDF read at most 1024 bytes in one read; one byte over reads nothing and is
   `too_large`; text still reads and hashes everything). Mutation: reading 8 MB fails it.
7. End-to-end maximum payloads (minor, `native-bridge/tests/files.rs`), both with 60 s file waits
   so they depend on the caps and not on machine speed:
   `the_largest_legal_preview_attaches_through_the_bridge_and_one_more_byte_does_not_open`
   (open -> attach of a 7 340 032-byte PNG, raw body compared byte for byte, preview save refused
   `invalid_request`, file unchanged, one byte more is a per-item `too_large` with no tab) and
   `a_two_mib_save_in_a_frame_exactly_at_the_cap_is_written_and_one_byte_more_is_refused` (the
   header is padded with JSON whitespace to 16380 bytes so the frame is exactly
   `MAX_FILE_SAVE_FRAME_BYTES`; header 16381 is `INVALID_ARGUMENT` with the file untouched; the
   exact frame writes 2 097 152 bytes, generation 2, and reload returns them).

Receipt corrections: the original "Green" count of 490 tests, the "(21)", "(25)" and "(5)" test
counts and the 18-run flake claim above were stale and are corrected in place.

### Checks run (third fix round)

- `scripts/check` (fmt, `cargo test --workspace --locked`, clippy `-D warnings`,
  `scripts/license-inventory --check`, `scripts/check-native-bridge`): PASS, exit 0; 521 tests
  passed, 0 failed, 2 ignored (the `lease_child` and `mutation_child` subprocess fixtures, unrelated) across 77 test binaries. No dependency change.
- The native bridge C ABI check passed, including the framed file channel and profile lease checks.

### S2 fourth fix round (determinism under load, verified one by one)

One blocking finding: the threaded `tests/files.rs` suite flaked at 2-4x CPU oversubscription.
Reproduced first on the unmodified tests (harness: N concurrent copies of the test binary plus N
`while :; do :; done` hogs on this 24-CPU box): 14 of 96 whole-suite runs failed at 48 copies with
48 hogs, and 35 of 384 on a diagnostic build. The earlier "deflaked" and determinism claims in the
third round covered only 8 concurrent copies (and 8 with 48 hogs); that is superseded by the
measurements below. Sub-findings (a)-(f), each confirmed before changing anything:

- (a) `probe_timeout_is_an_explicit_item_error_and_other_items_complete` (confirmed, 13 failures
  in the diagnostic run) and its socket twin. After the release the recovery `open_item` ran under
  the fixture's own 150 ms probe deadline. Worse, the same tiny deadline can expire before the
  first probe is even queued (the deadline check precedes `try_send`), so the worker never reaches
  the gate and `wait_blocked` fails (seen at 4x as "no operation reached the gate"). Now
  `Fixture::pin_worker` repeats short attempts until one provably parked the worker in the gate,
  and only then is the call under test made, so both items must time out whatever the speed;
  recovery after release is eventual (retry only on the explicit `probe_timeout`, 60 s patience).
- (b) `shutdown_waits_for_a_busy_worker_and_joins_it_once_released` (confirmed, 3 failures). Two
  causes. Test: `pending.files == 1` is written by the executor's own drain loop, which a loaded
  machine may not have run yet; the test now observes `shutdown_status` until it does. Product
  (a real, small race): `wait_shutdown` took its report, then read the clock, so a wait that ended
  by expiry could return `deadline_expired == false`. It now reads the clock first and reports
  after, so an expiry it observed is always in the report (`shutdown.rs`). The test asserts it on
  a wait that ends by expiry. A real interleaving cannot be forced to fail deterministically, so
  the fix is structural and I did not demonstrate it by mutation.
- (c) `file_worker_changes_advance_the_revision_and_an_idle_service_does_not` (confirmed, 10
  failures): the idle assertion is on the global revision and the `/bin/sh -i` starter terminal
  printed its prompt after the 150 ms sleep. The fixture's starter terminal is now
  `/bin/sh -c "exec cat"`, which never writes a byte, the sleep is gone, and "idle" is measured in
  counted polls (`Fixture::wait_polls` uses the backend's own `open_leaf` counter as the clock).
  The same helper replaced the sleeps in the touch assertion.
- (d) `a_timed_out_reload_finishing_after_detach_does_not_rebind_the_old_view` (confirmed, 5
  failures): the 100 ms read timeout also governed the setup and final attaches. Those now use
  `attach_patiently` (a timed-out attach still completes on the worker and binds the same host,
  so retrying with that host is idempotent); only the reload under test keeps the tiny timeout,
  and the test waits until the worker is inside the read before it detaches.
- (e) Broken pipe on the control socket (confirmed: 2 in the baseline run, and 2 more tests,
  `a_closed_desktop...` and `a_mixed_batch...`, returning `invalid_frame`, "Resource temporarily
  unavailable", in the diagnostic run). The server gives a client 500 ms from accept to deliver a frame; a descheduled
  test client missed it and wrote to a closed socket. A request the server did not read was never
  executed, so `Socket::call` reconnects and resends on `invalid_frame` or a broken pipe on
  write and returns every other answer untouched. The 500 ms server constant is unchanged.
- (f) `a_save_queued_behind_work_then_detached_...` (confirmed, 1 failure; the reviewer's
  question about a recreated entry that loses its receipt is answered NO). A diagnostic build
  printed the entry at the failure: `last_op = {attach, op_id 2, completed}`, `generation 1`,
  `attached false`, i.e. the same entry, never recreated, and the save was never admitted. The
  published snapshot lags the registry by an executor tick, so `wait_entry(.. "pending")` was
  satisfied by a stale snapshot taken while the earlier ATTACH was pending; the test then
  detached before its save thread had enqueued, `file_save` refused with `stale_owner` before any
  receipt existed, and `state != "pending"` matched the old completed attach. Tests now wait for
  the operation's own receipt (`wait_receipt(kind, state)`) in every place that did this
  (`reload`/`save` receipts, the queue test, the timed-out save test).

Found by the 4x runs after those fixes and also fixed:

- Polling test, product imprecision (`files.rs`): `disk_generation` advanced on every load that
  differed from the baseline. The fingerprint is taken before the read, so a write landing in
  between made the poller read the same new bytes twice and count two edits. It now advances only
  for content that differs from the hash the poller last examined (`Entry.disk_sha`: baseline
  after an adopt, replaced per load, forgotten when the file is declared deleted, so a recreated
  file always counts). Tests: `an_edit_landing_between_a_polls_stat_and_read_is_one_disk_generation`
  (a backend hook writes the edit between the stat and the read, deterministically; with the old
  rule the test fails with the production flake's exact message, `disk_generation` 3 instead of
  2) and the unit test `only_new_disk_content_is_a_new_disk_generation`. Design D14 states it.
  The polling and revision tests now make external edits with an atomic rename
  (`replace_atomically`): an in-place `write(2)` or `std::fs::write` (truncate first) can be seen
  half-written by a poller whose writer is descheduled, which is a different, unwinnable race.
- `FilesConfig.command_timeout` (default 2 s, the previous constant) is the wait for the
  executor's acknowledgement of `open`/`observe`/`detach`; tests set 60 s. The 2 s constant was
  hit at 8x load by `file_open`/`file_detach`. Behaviour at the default is unchanged.
- The session-restore test retries only the persistence layer's explicit "retry" answer
  (generation ledger lock held by another holder); the unit test
  `shutdown_joins_an_idle_worker_and_refuses_new_work` no longer uses a stopwatch (a 600 s tick
  makes only the wake-up able to end the worker inside a 30 s budget).
- Generic: all failure-only waits and the fixture's file timeouts are `PATIENCE` (60 s) instead of
  5-8 s; shutdowns use the 30 s maximum. `capabilities_advertise...` sets its own 5000 ms probe
  timeout because it asserts the advertised value.

Determinism, executed against the final test binaries (24 CPUs; Linux):

| binary | serial | 8 concurrent x3 | 48 copies + 48 hogs | 96 copies + 96 hogs |
| --- | --- | --- | --- | --- |
| desktop-runtime `files` (45 tests) | 20/20 | 24/24 | 288/288 (6 rounds) | 191/192 (see below) |
| native-bridge `files` (7 tests) | 20/20 | 24/24 | 96/96 | not run |
| desktop-runtime lib, `files::` filter | 20/20 (whole lib) | 24/24 (whole lib) | 96/96 | not run |

Before the changes the same harness gave 14/96 at 48+48 (above). The single 96+96 failure is
`a_mixed_batch_over_the_socket_...` returning `outcome_unknown`: the control server's own 2 s
`COMMAND_TIMEOUT` for the executor expired at 8x oversubscription. That constant is the control
server's and is unchanged; an earlier 96+96 run of an almost identical binary passed 288/288.
Residual, not fixed because it is outside the file service: at 48+48 the *whole* lib unit-test
binary still flakes in other modules' tests (`agent_messages::tests::pending_worker_credits_and_
shutdown_remain_bounded_during_locked_io`, `control::tests::message_io_and_queued_policy_writer_
do_not_block_the_workspace_executor`); both predate and are untouched by this slice.

Checks: `scripts/check`: PASS, exit 0; 523 tests passed, 0 failed, 2 ignored (the `lease_child`
and `mutation_child` subprocess fixtures) across 77 test binaries. Swift, macOS and hosted gates
remain UNAVAILABLE. Corrections to earlier text: the `files` binary now has 45 tests (not 44), and
the determinism evidence is the table above, not the 8-copy runs.

### UNAVAILABLE gates (unchanged)

Swift, macOS and hosted gates remain UNAVAILABLE: `scripts/check-macos-sources`,
`scripts/build-macos-dev`, `scripts/check-macos-app`, the in-app smokes and
`.github/workflows/macos-development.yml`. The macOS `O_SEARCH` fallback is compiled only there.

## S3 CLI and control socket: `zmux open` and `zmux markdown open`

Base: `4eddc9ce07bac31a66570d9b5dc8421db41cba64`.

### What changed

- `apps/cli/src/open.rs` (new): lexical operand classification, absolute paths, the request bound
  and result reporting. `http://`/`https://` (ASCII case-insensitive) become `url` items; any other
  `scheme://` is `unsupported_scheme`; everything else, including `foo:bar.txt` and `about:blank`,
  is a path made absolute by joining `env::current_dir()` (no canonicalisation, `~` untouched).
  An empty operand, a control character, an operand over 4096 bytes (after absolutising), no
  operand, more than 16 operands, or an encoded request over 16384 bytes fails the whole
  invocation before any connection (`invalid_operand`, `unsupported_scheme`, `request_too_large`,
  "split the operands across invocations"). The size check runs twice: at parse time without a
  password (a lower bound that already decides the common case) and in `run` with the caller's
  real password. `run` sends one request, prints the server `result` pretty-printed, and returns
  0 only if `result.ok` is true; otherwise it prints `zmux: N of M items failed` on stderr and
  returns 1. A whole-request failure (`stale_revision`, unknown target, `policy_denied`,
  transport) stays an ordinary `zmux: code: message` error with exit status 1 and no stdout.
  A result without an `items` array is itself an error (fail fast, never a success-shaped guess).
- `apps/cli/src/targets.rs`: `open` and `markdown open` reuse the selector loop and the focus
  extraction (`extract_browser_focus` became `extract_focus` with message labels; the browser
  messages are unchanged). Selector options must precede operands; `--` makes later operands
  literal; a leftover option-looking operand is an unknown-option error. `request` was split into
  `command_request` (returns the `ControlCommand`) so the new `Parsed::Open` variant does not
  have to unwrap `Parsed::App`.
- `apps/cli/src/main.rs`: `Parsed::Open(ControlCommand)`, a `run` arm, and a 5 s default client
  budget for these commands (1 s probe + 2 s executor wait) unless the global `--timeout` is given.
- `apps/cli/src/registry.rs`: `open` and `markdown` entries (`targeted`, socket) in a new `files`
  group; the help text states the 2 MiB / 7 MiB limits, extension modes, dedupe, per-item errors,
  focus rules, exit status and the 5 s default.
- `apps/cli/src/registry/contracts.rs`: leaves `open` and `markdown open` (route `open`, pointers
  `/request/operation/operation = open_items` and, for markdown, `force_mode = markdown`, default
  `focus` read from the live parse); `parsed()` allows `Parsed::Open`. The probes carry an explicit
  `--workspace workspace:1` so they do not depend on ambient `ZMUX_*` variables.
- `apps/cli/src/registry/frozen-coverage.json`: `CMD-006` and `CMD-114` moved from `unresolved`
  to `local_counterpart_compatibility_unverified` with `local_contract` `open` / `markdown open`.
  `behavioral_parity_verified` stays false; nothing under `apps/cli/contracts/baseline/` changed.
- `crates/desktop-runtime/src/control_policy.rs`: no behaviour change. `is_read` already treats
  `Targeted` as a mutation unless the operation is on its short read list, so `open_items` is a
  mutation without edits; a unit test now pins that (see below). The route, error codes
  (including `whole_request_code`) and per-item codes were delivered in S2 and are unchanged.
- `docs/local-control.md`: new "Opening files and web pages" section (wire shape, bounds, result
  shape, per-item codes, exit status, no retry).

### Red then green

Red: `apps/cli/tests/open.rs` was written first and run against the unmodified CLI. All 12 tests
failed (`unknown command \`open\``), i.e. 0 passed / 12 failed. A separate red appeared in
`scripts/check` after the implementation: the catalog contract check
(`offline_topics_and_agent_discovery_do_not_require_a_valid_socket`, which runs `commands --check`
with `ZMUX_LOCAL_TMUX=1` and no `TMUX`) failed with `command contract drift: open: managed tmux
caller is missing TMUX`, because my first leaf probe had no explicit target. The probes now name
`--workspace workspace:1`, as the other targeted leaves do; that test is the regression guard.

Green: the 12 `open.rs` tests pass, as do the changed `cli.rs` tests and the new unit tests.

New and changed tests (all run by `cargo test --workspace --locked`):

- `apps/cli/tests/open.rs` (new, 12). Wire tests use a one-shot fake app that owns the listener
  and records the single request; "never connected" is proved after the child exits.
  `open_sends_the_designed_wire_request_with_cwd_absolute_paths` (exact `open_items` operation:
  relative paths joined to the fixture cwd, `..` and `~` untouched, absolute passthrough,
  case-insensitive `HTTPS://`, `about:blank` as a path);
  `open_options_select_scope_focus_and_literal_operands` (all four selectors, `--tab`,
  `--expect-revision`, `--no-focus`, `--focus true|false` before and after operands, `--` and
  option-looking operands); `open_defaults_follow_the_existing_target_environment_rules`
  (`ZMUX_WORKSPACE_ID`/`ZMUX_SURFACE_ID` reach `caller`; an explicit selector replaces them);
  `markdown_open_forces_markdown_and_accepts_paths_only`;
  `lexical_errors_fail_the_whole_invocation_before_connecting` (22 rejections, each asserting the
  message, empty stdout and that nothing connected: no/empty operands, control character, 4097-byte
  path, `file://`, `ftp://`, unknown and misplaced options, focus conflicts and bad values,
  repeated/invalid selectors, `markdown` without `open`, URL operands for markdown, 17 operands,
  an oversize request);
  `item_failures_print_the_full_result_and_exit_one_without_retrying` and
  `whole_request_failures_exit_one_with_the_server_error_and_no_items` (exit statuses, stdout and
  stderr contracts); `help_describes_both_commands_offline` (five help spellings with an invalid
  relative socket, so none can connect; `Files:` group in the top-level help).
  Real Desktop behind a real `ControlServer` (production file service with 60 s deadlines):
  `a_mixed_batch_creates_ordered_tabs_and_reports_each_item` (markdown, text, image, a directory,
  a missing file, a binary file and an https URL in one call: exit 1, `3 of 7 items failed`,
  exact per-item codes `is_directory`/`not_found`/`binary_file`, tabs appended in operand order
  only for successes, `kind` file/browser, first success selected, then an all-success call that
  exits 0 with quiet stderr); `dedupe_no_focus_and_the_first_success_focus_rule` (`--no-focus`
  keeps the starter terminal selected, re-opening is `deduplicated:true` with no new surface,
  with focus the first success is selected and a later new tab is not);
  `explicit_targets_environment_defaults_markdown_force_and_stale_revisions` (a caller workspace
  wins over the focused one; `markdown open --pane` forces an extensionless file to markdown and
  reports `content_mismatch` for a PNG; a stale `--expect-revision` and an unknown pane are
  whole-request failures with no stdout, no `items` and no new tab);
  `a_read_only_socket_is_refused_before_any_file_is_probed` (`policy_denied` through the CLI).
- `apps/cli/tests/cli.rs`: `open` and `markdown` added to the help command list (each also runs
  `--help`); the catalog assertions check `CMD-006`/`CMD-114` rows, both leaves
  (`contract_status` partial, dispatcher, `focus` true, 5000 ms client timeout, `open_items`
  pointer, `force_mode`, argument names) and that inventory stays 428.
- `apps/cli/src/open.rs` unit tests (6): scheme detection table, order and kinds, markdown
  forcing and URL refusal, the 16-operand / 4096-byte / request-size boundaries (exact bound
  accepted, one over refused), control characters, and the 5 s default versus an explicit
  `--timeout`.
- `crates/desktop-runtime/src/control_policy.rs`:
  `open_items_is_a_mutation_for_every_item_kind_and_force_mode` (read-only is refused, full is
  admitted, the remote relay is denied, for path, URL and forced-markdown operations).

Mutation checks (each applied, observed failing, then restored): putting `OpenItems` on the
`is_read` list fails the control_policy test; exit status 0 on item failure fails 3 open tests;
dropping the cwd join fails 6; treating every operand as scheme-less (so `file://` becomes a path)
fails 3 including the lexical-rejection table.

### Checks run

- `scripts/check` (fmt, `cargo test --workspace --locked`, clippy `-D warnings`,
  `scripts/license-inventory --check`, `scripts/check-native-bridge`): PASS, exit 0; 542 tests
  passed, 0 failed, 2 ignored (the `lease_child` and `mutation_child` subprocess fixtures,
  unrelated) across 78 test binaries. No dependency change.
- `zmux commands --check` passes inside the `cli` test (inventory entries 428).

Determinism, executed against the final test binaries (24 CPUs; Linux; the harness ran each binary
20 times serially, then 3 rounds of 8 concurrent copies, then 3 rounds of 24 concurrent copies
alongside 24 busy-loop CPU hogs):

| binary | serial | 8 concurrent x3 | 24 copies + 24 hogs x3 |
| --- | --- | --- | --- |
| `open` (apps/cli/tests/open.rs, 12 tests) | 20/20 | 24/24 | 72/72 |
| `cli` (apps/cli/tests/cli.rs, 13 tests) | 20/20 | 24/24 | 72/72 |
| `zmux` bin unit tests (apps/cli, incl. `open::tests`) | 20/20 | 24/24 | 72/72 |
| `zmux-desktop-runtime` lib, `control_policy` filter | 20/20 | 24/24 | 72/72 |

The new tests contain no sleeps used as synchronisation. The fake app spins on a non-blocking
accept until the child has exited, and the child cannot finish without the reply the fake owes it.
End-to-end waits are a bounded condition loop (60 s) on the Desktop's published snapshot, the
established pattern. Server-side deadlines in the end-to-end fixture are 60 s (probe, command),
so the assertions are about behaviour and cannot be turned into `probe_timeout` by load; each CLI
call passes `--timeout 30000`.

### UNAVAILABLE gates

Unchanged from S2 and not run on this Linux machine: `scripts/check-macos-sources`,
`scripts/build-macos-dev`, `scripts/check-macos-app`, the in-app smokes and
`.github/workflows/macos-development.yml`. S3 touches no Swift. Rendering, editing, saving and
tab presentation of the tabs that `zmux open` creates are S4 and are not verified here.

### Deviations from the design, with reasons

- `unsupported_scheme` (and `invalid_operand` for a URL given to `markdown open`) fail the whole
  invocation before connecting, rather than becoming per-item results. D3 lists them as lexical
  errors and the failure rule says purely lexical errors fail fast with no side effects; the
  server still reports the same codes per item for other clients (bridge and raw socket).
- The parse-time request-size check cannot know the password, so it is a lower bound; the exact
  check runs in `run`, still before connecting. Both use the shared 16384-byte constant.
- `is_read` and the server error vocabulary needed no change (the S2 route already classifies
  `open_items` as a mutation); only a pinning unit test was added to `control_policy.rs`.
- The end-to-end tests do not inject a gated `FilesConfig` backend; overload, stall and
  `probe_timeout` behaviour are covered at their owner in `crates/desktop-runtime/tests/files.rs`
  (S2), and the CLI test only asserts that the CLI relays whatever the server returned.
- The `markdown` registry entry is the dispatcher name; its usage reads `markdown open ...` and
  `zmux markdown` without `open` is an explicit error, as `browser` without a verb is.
