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
  `--workspace workspace:1` so they do not depend on ambient `ZMUX_*` variables, and the file
  operand is the absolute `/README.md` so the catalog does not depend on the invoker's working
  directory (fix round 1, below).
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

- `apps/cli/tests/open.rs` (new, 12 in the first round, 14 after fix round 1). Wire tests use a one-shot fake app that owns the listener
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
  `scripts/license-inventory --check`, `scripts/check-native-bridge`): PASS, exit 0 (first round: 542 tests
  passed, 0 failed, 2 ignored (the `lease_child` and `mutation_child` subprocess fixtures,
  unrelated) across 78 test binaries. No dependency change).
- `zmux commands --check` passes inside the `cli` test (inventory entries 428).

Determinism, executed against the final test binaries (24 CPUs; Linux; the harness ran each binary
20 times serially, then 3 rounds of 8 concurrent copies, then 3 rounds of 24 concurrent copies
alongside 24 busy-loop CPU hogs):

| binary | serial | 8 concurrent x3 | 24 copies + 24 hogs x3 |
| --- | --- | --- | --- |
| `open` (apps/cli/tests/open.rs, 12 tests, first round) | 20/20 | 24/24 | 72/72 |
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

### Fix round 1 (two blocking findings, both verified valid)

Checks run in this round are in the last bullets; the sections above describe the first round, with
the counts corrected where this round changed them (`open.rs` is 14 tests, `scripts/check` is 546
passed / 0 failed / 2 ignored across 78 binaries).

1. Upper-case scheme rejected by the server. Verified from the code path: the CLI and
   `classify_url` accepted `HTTPS://` case-insensitively, but `validate_browser_url`
   (`crates/workspace-core/src/surfaces.rs`) strips `https://`/`http://` case-sensitively, so the
   item came back `invalid_url`. Fix: `files::classify_url` now returns the canonical URL, with
   only the scheme lower-cased (RFC 3986: schemes are case-insensitive; host, path, query and
   fragment are byte for byte), validates that canonical form, and `lib.rs` plans the tab with it,
   so the receipt `url` and the tab carry `https://`. `validate_browser_url` is shared with the
   browser actions and is unchanged. The CLI keeps sending the operand as typed (the server is the
   owner of the rule, and other control-socket clients benefit). Tests: `classify_url` unit tests
   in `crates/desktop-runtime/src/files.rs` (any-case scheme accepted and canonicalised; `FILE://`,
   `ftp://`, bare host, empty authority, credentials, whitespace, non-ASCII near the prefix and
   the markdown force are still refused with the same codes) and the real-Desktop test
   `an_upper_case_scheme_opens_a_browser_tab_with_the_canonical_scheme` in `apps/cli/tests/open.rs`
   (`HTTPS://Example.org/Docs?Q=1` and `HtTp://example.net/` open two browser tabs, receipt
   urls canonical, host case kept).
2. Catalog depended on the working directory. Verified: the probes used the relative `README.md`,
   which the CLI joins to `env::current_dir()`, so `zmux commands` embedded the cwd and
   `commands`/`commands --check` failed from a removed directory. Fix: both probes use the
   absolute `/README.md`. Test `the_command_catalog_does_not_depend_on_the_working_directory`:
   `commands` from two different directories is identical once the per-request random
   `request_id` values (already random before this slice) are ignored and contains neither
   directory; `commands` and `commands --check` succeed from a directory that was removed under
   the process. Mutation check: restoring `README.md` makes this test fail (the catalog embeds the
   cwd); restored.

Checks (fix round 1): `scripts/check` PASS (546 passed, 0 failed, 2 ignored, 78 binaries).
Determinism for the changed/new tests, executed against the final binaries: the `open` binary
(14 tests) 20/20 serial, then 3 rounds of 8 concurrent copies (24/24), the last two rounds beside
4 busy-loop CPU hogs; the `zmux-desktop-runtime` lib `classify_url` filter 20/20 serial. The
determinism table above is from the first round; its other rows were not rerun in this round.

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

## S4 native panel: Swift/AppKit file editor and preview surface

Base: `b135b7532331cebd89c8c16aaac7eb86fda6b4a6`.

This machine is Linux with no Swift toolchain. Every line of Swift below was written but never
compiled or run here; macOS CI (`scripts/check-macos-sources`, `scripts/build-macos-dev`,
`scripts/check-macos-app`, the in-app smoke) is the only verifier and none of it has run.

### What changed

New Swift (all under `apps/macos/Sources`, system frameworks only: AppKit, PDFKit, ImageIO, Foundation):

- `FileHostClient.swift`: `FileHostLimits` (mirrors `zmux.h`), `FileOwnerToken`, `FileHostRequest`
  (frame codec `u32 BE header_len | header JSON | raw body`, header and per-op size checks before
  anything is sent), `FileHostReply.decode` (an `ok:false` header becomes a `FileHostFailure` with
  the stable code, `outcomeUnknown`, `opId` and the conflict detail; never a success), the
  `FileOpenResult` shapes, and `NativeBridge.fileDetach` / `fileOpen`.
- `FilePanelRendering.swift`: `FilePanelMode`, `FileImageDecoder` (ImageIO off the main actor,
  100 megapixel guard, EXIF orientation applied, decode errors are explicit failures) and
  `FileMarkdownRenderer` (`AttributedString(markdown:)` with `interpretedSyntax = .full`, mapped to
  fonts and paragraph styles; headings, emphasis, strong, code, strikethrough, quotes, ordered and
  bulleted lists; links clickable only for http, https, mailto; a parser failure shows the plain
  source plus a visible message).
- `FilePanelView.swift`: `FilePanelView` (one retained view per file surface) hosting a monospaced
  editable `NSTextView` (substitutions, smart quotes and link detection off, undo on), a rendered
  Markdown text view with a Preview/Source segmented control and a shared source toggle, an
  `NSImageView`, a `PDFView`, and a status view for loading, error (stable code and Retry),
  empty Markdown and unsupported mode. Header shows the name, "Edited", mode details, read-only and
  worker-stall notes; a banner row carries the external-change, deleted, unreadable, save-unknown,
  owner-lost and load-failure states with explicit buttons. The view owns the unsaved text, so its
  own dirty bit is the truth; the model's `file.dirty` is reconciled by an `observe` loop (bounded
  backoff 250 ms to 2 s, one in flight, stops on a non-retryable refusal and says so; an acknowledged-but-unconfirmed
  observation is retried with no count limit, at a rate that slows to 2 s, and shows an informational
  banner after 8 such attempts, see "Fix round 1"). Save (`Cmd-S`) goes through the bridge
  with `base_generation`; `conflict` opens Cancel / Reload from Disk / Overwrite (Recreate File for a
  deleted file); `stale_view` and owner loss keep the text and offer Reload or Copy Text; a lost
  acknowledgement (`outcome_unknown`) is never replayed, the banner waits for `last_op` in a later
  snapshot. External change: a clean view reloads once per disk generation, a dirty view shows the
  banner; if the person starts typing while an automatic reload is in flight the text is kept and
  the next Save asks before overwriting (`externalPending`). Word wrap toggle (user default
  `filePanel.wordWrap`). Accessibility labels on the editor, rendered view, image, PDF, header
  controls and banner.
- `FilePanelSmoke.swift`: the in-app journey (below).

Changed Swift:

- `Models.swift`: `FileOperationReceipt`, `FileEntrySnapshot`, `FileWorkerSnapshot`,
  `FilesSnapshot`, `DesktopSnapshot.files`; `PaneTree.key` now includes the surface kind and
  deliberately not dirty state or file mode.
- `NativeBridge.swift`: actor method `file(_:body:)` (detached blocking call, `Self.consume`).
- `ApplicationCoordinator.swift`: `files` registry, `reconcileFilePanels` (create, update, prune,
  re-create when the path differs, reset on identity-namespace change), `FilePanelOwner`
  conformance, `FileCommand.open` and `.detach` on their own bounded queue and task (fix round 2; the
  original slice put them in `UICommand`/`commandTask`, which stalled terminal input), orphan handling
  (a panel whose tab vanished while holding unsaved text is kept aside and the person chooses Copy
  Text or Discard; quit is refused until then), `resolveFilesBeforeClose` / `resolveFilesBeforeQuit`
  (Save / Don't Save / Cancel per dirty file, then a forced dirty-mirror sync, a failed save or
  sync aborts the close), shutdown releases panels without calling the file service.
- `App.swift`: `paneView` file branch before the browser fallback (an unknown kind still gets an
  error label), `completePresentation` / `releaseContentResponder` / `toggleSidebar` focus
  handling, `onFocus -> focus_pane`, header detail and `isDocumentEdited`, workspace-row subtitle
  and window title for file tabs, presentation key includes the file panel host id, File menu
  (below), Edit menu Undo / Redo / Cut through the responder chain, `validateMenuItem` arms, dirty
  checks before Close Tab, Close Other Tabs, Close Pane, Close Workspace, Close Window and window
  close, `confirmClose` copy that counts terminals, browsers and files separately and no longer
  calls a file tab a terminal session, Quit hook in `applicationShouldTerminate`, smoke stage
  `file_panel` and evidence key `file_panel`.
- `WorkspaceViews.swift`: tab icon by mode (`doc.text`, `doc.richtext`, `photo`, `doc`), tooltip
  shows the file path, in-place edited marker ("• ") and accessibility "edited" (no pane rebuild).
- `docs/native-bridge.md`: a paragraph on the Swift client.
- `crates/native-bridge/tests/swift_file_contract.rs` (new, Linux-runnable).

Menus (existing key equivalents audited: n t l b U d D w { } 1-9 T [ ] r f + - 0 c v a q and the
Command-Option arrows/braces/brackets; none of the new ones collide):

| Item | Key | Notes |
| --- | --- | --- |
| File > Open File... | Cmd-O | `NSOpenPanel`, up to 16 files, then bridge `open` through the file command queue (`FileCommand.open`) |
| File > Save | Cmd-S | enabled only for a dirty, writable, attached text or Markdown panel |
| File > Reload from Disk | none | confirms when the buffer is dirty; every banner Reload button takes the same route (fix round 2) |
| File > Toggle Word Wrap | Opt-Cmd-L | checkmark follows the preference |
| File > Show Markdown Source / Show Rendered Markdown | Opt-Cmd-P | title follows state |
| Edit > Undo / Redo / Cut | Cmd-Z / Shift-Cmd-Z / Cmd-X | no target: responder chain, disabled elsewhere |

Copy, Paste and Select All already forward to the responder chain when the first responder is not a
terminal, so they reach the file text views unchanged.

### Audit: every place that tests `kind == "browser"` or assumes non-terminal means browser

| Site | Before | Now for `kind == "file"` |
| --- | --- | --- |
| `App.swift` `paneView` (fallthrough `kind != "browser"` after terminal and history) | any other kind became a browser | explicit `file` arm first; an unknown kind shows an error label; a file without a panel shows an error label |
| `WorkspaceViews.swift` `TerminalTabControl.symbolName` | `browser ? globe : terminal` | by mode; tooltip shows the path |
| `App.swift` `updateHeader` detail | browser URL or terminal cwd | file arm shows the abbreviated path and "edited" |
| `App.swift` `terminalTitle` | title unless it is the default "Terminal" | a file tab always shows its own title |
| `App.swift` `updateWorkspaceRows` subtitle | cwd or "Local shell" | the file path for a selected file pane |
| `App.swift` `presentationKey` browser hosts | selected browser host ids | unchanged; a parallel `file-hosts` part replaces a panel whose host id changed |
| `App.swift` `confirmClose` | "closes N terminal session(s)" for every tab | terminals, browsers, files and saved-output tabs are counted separately; file tabs say the files are untouched |
| `App.swift` `completePresentation`, `releaseContentResponder`, `toggleSidebar` | terminal, history, browser only | file branches; focus is restored to the editor, rendered view or PDF view |
| `App.swift` `focusedBrowser`, `focusBrowserAddress` | browser dictionary lookups | a file tab is not in `browsers`; Cmd-L on a file tab opens a new browser tab like it does on a terminal |
| `ApplicationCoordinator.swift` (4 sites: live set, view creation, profile `isNeeded`, update loop) | browser only | unchanged, correct: file surfaces are reconciled by `reconcileFilePanels` |
| `App.swift` smoke wait and `SurfaceOrderSmoke`, `WorkspaceNumberSmoke`, `WorkspaceOrderSmoke` | select the browser the smoke created | unchanged: fixtures |
| `Models.swift` `PaneTree.key` | surface id, session, completion digest | adds the kind; no dirty state, no mode |
| `Pane.sessionId`, `focusedViewport`, `retainedHistories`, `NotificationDelivery` session checks | nil session means "not a terminal" | already correct for files (no session, no history) |

`crates/native-bridge/tests/swift_file_contract.rs` pins this table: it counts the browser-kind
tests per Swift file, asserts the `file` arm precedes the fallthrough, the tab-icon `file` case and
that `PaneTree.key` carries no dirty state; adding a site fails it with instructions.

### Red then green

Swift cannot be compiled or executed here, so there is no red-then-green for the Swift itself and
none is claimed. The Linux-testable part is the contract between Rust and Swift:
`crates/native-bridge/tests/swift_file_contract.rs`, three contract tests in the original slice
(eight tests after fix round 1, which added five structure tests; twelve after fix round 2, which
added four more; sixteen after fix round 4; eighteen after fix round 5, which also renamed every
source-text test with a `pin_` prefix, see below; twenty after fix round 6), all green. Test names quoted in earlier fix-round
sections are the names at the time; the structure tests now carry the `pin_` prefix:

- `swift_decodables_name_exactly_the_fields_the_real_rust_wire_carries`: runs a real Desktop
  (open with one good and one missing path, attach, save, observe, a content conflict, a refused
  detach), then asserts the Swift `Decodable` field sets equal the real JSON keys (converted with
  `convertFromSnakeCase`) for `FilesSnapshot`, `FileWorkerSnapshot`, `FileEntrySnapshot`,
  `FileOperationReceipt`, `FileSurface`, `FileOwnerToken`, `FileConflict`, `FileHostReplyHeader`
  (union of every reply kind), its error body, `FileOpenResult`, `FileOpenItem` and its failure,
  and that `Surface.file` and `DesktopSnapshot.files` exist.
- `swift_frame_limits_and_operations_match_the_rust_channel`: Swift `FileHostLimits` equal the Rust
  constants and the file service caps; every op and request key is written by the Swift codec.
- `every_browser_kind_test_in_the_swift_sources_is_audited`: see above.

The tests were written after the Swift (they test a contract, not behaviour), so their strength is
shown by mutation. Each mutation was applied to the Swift, the test run, then the file restored
(byte-identical, checked with `cmp`):

| Mutation | Result |
| --- | --- |
| rename `FileEntrySnapshot.diskGeneration` to `diskGen` | decodables test FAILED (field sets differ) |
| `maxSaveBody` 2_097_152 -> 2_097_153 | limits test FAILED (`2097153` vs `2097152`) |
| add one more `kind == "browser"` site to `ApplicationCoordinator.swift` | audit test FAILED |
| add `dirty` to the `PaneTree.key` string | audit test FAILED |

### Determinism

The new test binary (`swift_file_contract`, 3 tests in the original slice, one real Desktop, unique
temp directories, the usual bounded snapshot poll) was executed against the sources of that slice: 20
serial runs, 0 failures; then 5 rounds of 8 concurrent copies (40 runs) beside 24 busy-loop CPU hogs
on a 24-core machine, 0 failures, every log carrying the 3-passed line. No other Rust test changed in
that slice. Fix round 1 re-ran the 8-test binary, see its own Determinism paragraph.

### In-app smoke (`FilePanelSmoke`, UNAVAILABLE here)

Stage `file_panel` at the end of `runSmokeTest` (after the browser download checks), driven through
the real bundled CLI, the real File menu key equivalents and a real directory; only the conflict
close and (fix round 2) discard sheets are answered through the panel's own seams (`conflictChooser`,
`fileCloseChoiceOverride`, `discardConfirmation`) so no human is needed:
1. `zmux open` of md, txt, png, pdf, a binary, a folder and a missing path: exit failure with seven
   per-item results, four tabs, error codes `binary_file`, `is_directory`, `not_found`.
2. Rendered Markdown shows no syntax and an https link; Source toggle shows the exact source;
   image decodes to 3x2; PDF decodes to 1 page.
3. Opening an open file again reports `deduplicated`, no new tab.
4. Select the text tab, type: dirty marker in the tab, model dirty mirror true, first responder
   unchanged, and `zmux close-surface` on the dirty tab is refused ("unsaved").
5. Cmd-S through the menu writes the file and clears dirty (panel and model).
6. External write while clean reloads the panel by itself.
7. External write while dirty keeps the edits and shows the banner; Cmd-S gives the conflict
   prompt: Cancel changes nothing, Overwrite writes the buffer, then (second round) Reload adopts
   the disk version.
8. Close of a dirty tab: Cancel keeps it and the edits, Don't Save closes it with no orphan; Reopen
   Closed restores the tab from disk.
9. Deleting an open image shows the deleted banner and keeps the last good preview.
10. (fix round 1) Two dirty panels, `resolveFilesBeforeClose` answered Don't Save then Cancel: the
    close is refused and both panels keep their text, dirty marker and model dirty bit; then two
    back-to-back `closeTerminalTab` calls with Don't Save both close their tabs.
    (fix round 4) Between those two, a two-file Don't Save close is settled and the model must
    stay clean across four `status` polls for both panels, and new edits must mark both dirty again.
    (fix round 6) That settled close is a pending settlement: the panels keep their text and edited
    marker while the model reads clean; `finish(closeSucceeded: false)` (the core refused) leaves both
    panels dirty and re-marks both in the model; a second settlement finished with `true` then clears
    both.
11. (fix round 2) A dirty panel with the changed-on-disk banner: pressing the banner's Reload button
    asks first; Cancel keeps the text, Discard adopts the disk version. (fix round 6) The `zmux open`
    that reopens the text file first is a plain successful open (`expectFailure` false); it had been
    passed `true`, which makes the harness throw, so this step and step 12 could never run.
12. (fix round 2) A closed clean tab whose file was deleted is reopened: the panel shows the
    `not_found` error state; the file is written back and `retry()` loads it.
    Cleanup then checks with `lsof` that this process holds no descriptor under the fixture
    directory after every tab is closed (polled up to 5 s because detach is asynchronous).
Session quit/restore (the `files.epoch` rotation) is NOT exercised by the smoke, see "Fix round 2".
Step 2 also (fix round 1) waits for the keyboard to land in the rendered Markdown view of the tab
that `zmux open` selected while it was loading.
Cleanup closes the tabs, removes the directory and restores the original selection and responder.
`scripts/check-macos-app` does not yet list `file_panel` among its required evidence keys (scripts
are outside this slice); the key is emitted.

### UNAVAILABLE gates

Not run, because there is no Swift toolchain or macOS here; none of these is recorded as passed:

- `scripts/check-macos-sources` (Swift 6 strict-concurrency, warnings-as-errors type check of all
  sources), `scripts/build-macos-dev`, `scripts/check-macos-app`, the `file_panel` smoke stage and
  `.github/workflows/macos-development.yml` / the macOS job of `check.yml`.
- Behaviour that only a Mac can show: first-responder retention across the first keystroke, real
  Cmd-S / Cmd-O / Cmd-Z handling, `NSOpenPanel`, the Save / Don't Save and conflict sheets, the
  quit-with-dirty prompt, IME composition, VoiceOver and the accessibility labels, large-file
  `NSTextView` performance, ImageIO and PDFKit decode of real files, `AttributedString(markdown:)`
  fidelity (fonts, list markers, quotes), word wrap layout, dark-appearance contrast of the banner.
- Hosted GitHub Actions results of any kind (no push in this phase).

Swift API names used without a compiler that CI must confirm first, in the order most likely to
need a one-line fix: `PresentationIntent.components` / `.kind` / `.identity` and the `Kind` cases
(`header(level:)`, `codeBlock`, `blockQuote`, `listItem(ordinal:)`, `orderedList`, `unorderedList`,
`tableCell`, `tableRow`, `tableHeaderRow`) in `FileMarkdownRenderer`; `NSTextViewDelegate`
conformance isolation on the MainActor view; building `NSFont` / `NSColor` / `NSParagraphStyle`
inside a detached task; autolinking of `import PDFKit` (see deviations); `withCheckedContinuation`
around `beginSheetModal` in `presentSheet`.

### Deviations from the design and the task, with reasons

- Markdown uses `AttributedString(markdown:)` (design D20) rather than `NSAttributedString(markdown:)`
  named in the task. It is the same system parser; the typed `presentationIntent` and
  `inlinePresentationIntent` runs are what the style mapping needs, and the result is converted to
  `NSAttributedString` for the text view. No dependency either way.
- Word wrap was listed as deferred in design section 7 but the task requires it: implemented as a
  global user default (`filePanel.wordWrap`), not stored per file and not in the session document.
- Rendered Markdown known gaps (design section 7, unchanged): tables degrade to tab-separated text,
  thematic breaks, images and raw HTML are not drawn, relative links are inert, only http, https and
  mailto links open. Scroll position is not kept across a reload (selection is clamped and kept).
- File open and detach are fire-and-forget commands on a dedicated `FileCommand` queue (fix round 2;
  they were first added to `UICommand`). Save, reload and observe need their reply (conflict,
  generation, op id) so the panel awaits `NativeBridge.file` through the `FilePanelOwner` protocol
  instead of a command.
- The smoke is a stage of the existing `--smoke-test` run instead of a new launch flag, so it needs
  no new workflow, `AppStartup` argument list or storage case. `scripts/check-macos-app` was not
  edited (scripts are outside this slice), so it does not yet require the `file_panel` evidence
  key; adding it to its key list is a one-line follow-up for S5 or CI hardening.
- `-framework PDFKit` was not added to `scripts/build-macos-dev` for the same reason. `swiftc`
  autolinks imported system frameworks, as it does for the other imports there; if the macOS link
  step reports undefined `PDFView` / `PDFDocument` symbols, add the flag.
- The detach command bypasses the "clear the last error" step of `enqueue`/`enqueueFile(.open)` so an
  automatic release cannot erase an error message the person has not read; a full file queue is
  reported.
- Tab dirty marker and header edited state read the view's own dirty bit first and the model
  mirror second, so they react before the `observe` acknowledgement (design D7a).
- Orphan sheet: Copy Text and Discard only. Save As does not exist in v1, as designed.
- `Open File...` is capped at 16 selections with a visible message instead of splitting requests.

### Known limits and risks carried forward (not hidden)

- The window between the first keystroke and the acknowledged `observe(true)` (D7a) still lets a
  socket `close-surface` through; the orphan sheet then offers Copy Text. Not testable here.
- If the file service is stalled, Save, Reload and `observe` surface `file_worker_unavailable`
  or `busy`; `observe` retries at a bounded rate, the others show a banner and wait for the person.
- The Swift cannot prove first-responder retention, sheet flows or decode; CI is the verifier.
- No test, native or Linux, drives the Swift reaction to a `files.epoch` rotation (session restore)
  end to end; see "Fix round 2" for what is pinned structurally and why the smoke omits it.

### Checks run

- `scripts/check` (fmt, `cargo test --workspace --locked`, clippy `-D warnings`,
  `scripts/license-inventory --check`, `scripts/check-native-bridge`): PASS, exit 0; 549 tests
  passed, 0 failed, 2 ignored, 79 test binaries (546 before the slice plus the 3 new tests). The
  license inventory and `THIRD_PARTY_NOTICES.md` are unchanged (no dependency was added).
- `cargo test -p zmux-native-bridge --test swift_file_contract`: 3 passed; mutation and
  determinism evidence above.
- `cargo fmt --all` reformatted the new test file once; clippy found nothing to fix.
- Fix round 1 checks are in "Fix round 1" below.
- Brace, bracket and string balance of every touched Swift file was checked with a script (all
  balanced). This is not a type check and is not evidence that the Swift compiles.

### Fix round 1 (S4 review findings, all five verified and fixed)

Base of the round: `bfa8e66`. Swift still cannot be compiled or run here; every fix below is
verified by reading the code path, by a Linux-runnable structure test that fails when the fix is
reverted, and by the strengthened in-app smoke, which is UNAVAILABLE here and has not run.

| # | Finding | Verified | Fix |
| --- | --- | --- | --- |
| 1 | `closeAfterFileCheck` silently dropped a close made while another check was pending (smoke cleanup closes four tabs in a row, so `file_panel` could never finish; Cmd-W twice) | Yes: `guard !closePromptActive else { return }` ran before the first Task could run | `App.swift`: a bounded FIFO (`pendingFileCloses`, 64) drained by one task; each request runs in order. A repeat of the same gesture (same `key`, for example `surface:<id>`) is folded into the one still waiting or being asked about, which is the only case that does not queue; a full queue shows an error. `hasPendingFileCloses` lets the smoke wait for a request to settle before repeating it. |
| 2 | Keyboard focus stayed on the panel after an async load replaced the loading state | Yes: `present()` swapped the subview without touching the responder chain | `FilePanelView.present`: reads `ownsFocus(in:)` before the outgoing view leaves the window and calls `makeFirstResponder(preferredResponder)` afterwards; focus elsewhere is never taken. |
| 3 | Don't Save was applied immediately, so a later Cancel or failed Save left an earlier file clean | Yes: `discardLocalChanges()` ran inside the per-file loop | `resolveFilesBeforeClose` only records discards, settles every mirror (a discarded panel reports clean while keeping its text, `settleDirtyMirror(assumingDiscarded:)`), and applies `discardLocalChanges()` last. If a settle fails, `resumeDirtyMirror()` on every panel restores the model's dirty bit. SUPERSEDED in part by fix round 4: this version still let a snapshot poll re-mark a discarded panel dirty between settles, and the decision now lives in `FileCloseResolution`. SUPERSEDED in part by fix round 6: `applyDiscard` is no longer reached from `resolve`; the settlement applies it only after the core closed the tab. |
| 4 | Dirty detection used `String !=` (canonical equivalence) | Yes: `"\u{E9}" == "e\u{301}"` is true in Swift | `sameBytes` compares `utf8.elementsEqual`; used on every keystroke and after a save. |
| 5 | The dirty observation gave up after 8 acknowledged-but-unconfirmed attempts | Yes, and it contradicted design D7a; it was also missing from this receipt's deviations | The count no longer stops the loop: the confirmation wait backs off 150 ms to 2 s with no limit, an informational banner (`dirtyUnconfirmed`, no action, clears when the model agrees) appears after 8 attempts. Non-retryable refusals still stop the loop and say so, as designed. |

Tests added or strengthened (Swift behaviour cannot run here, so none of these is claimed to prove
the Swift works):

- `crates/native-bridge/tests/swift_file_contract.rs`, five new tests: `close_requests_are_queued_never_dropped`
  (no `closePromptActive`, no unexplained early return in the request path, every call site keyed,
  bounded with a visible refusal, smoke closes tabs back to back),
  `keyboard_focus_follows_content_that_replaces_the_loading_state` (focus read before the swap, then
  moved), `dont_save_is_applied_only_after_the_whole_close_is_settled` (one `discardLocalChanges`
  call, after every `return false` and after the settle loop, restore on abort; REPLACED in fix
  round 4 because it could not see a failed Save no longer aborting the close),
  `dirty_detection_compares_utf8_bytes_not_canonical_equivalence`, and
  `the_dirty_observation_never_gives_up_after_a_count`.
- Mutation evidence: each fix was reverted in a copy of the Swift, the binary run, then the sources
  restored (checked with `cmp`). The close queue guard replaced by a silent early return, the focus
  move removed, the discard moved back into the loop, `editor.string != baseline` restored and the
  count-based give-up restored each made exactly the matching new test FAIL (7 passed, 1 failed).
- `FilePanelSmoke` (UNAVAILABLE here): step 2 waits for the keyboard to land in the rendered Markdown
  view of a tab selected while loading; step 8 waits for the close queue to settle before repeating
  a close; new step 10 (Don't Save then Cancel keeps both panels edited and guarded in the model,
  then two back-to-back Don't Save closes both); cleanup answers a leftover dirty sheet with
  Don't Save instead of waiting for a person, and still closes four tabs in a row.

Checks run: `scripts/check` PASS, exit 0 (fmt, `cargo test --workspace --locked`: 79 test binaries,
554 passed, 0 failed, 2 ignored, which is the previous 549 plus the 5 new tests; clippy `-D warnings`;
license inventory; `scripts/check-native-bridge`). Determinism of the changed binary
(`swift_file_contract`, 8 tests, one real Desktop test among them): 20 serial runs, 0 failures; then
5 rounds of 8 concurrent copies (40 runs) beside 24 busy-loop CPU hogs on a 24-core machine, 0
failures, every log carrying the 8-passed line. The five new tests read source files and use no
threads, timers or sleeps.

Deviations and limits:

- Repeats of the same close gesture while it is pending are folded rather than queued (a second Cmd-W
  on the same tab would otherwise ask the same question twice or close a vanished tab). This is
  documented in the code, and it is the only dropped request path.
- `discardLocalChanges()` (now reached through `applyDiscard()`) still takes the editor text at the moment it runs, so text typed in a
  panel between its Don't Save answer and the last settle of the same close is also discarded. The
  window is the duration of the remaining sheets and syncs; not testable here.
- UNAVAILABLE as before: `scripts/check-macos-sources` (a Swift 6 type check of these edits, in
  particular the `Banner == Banner.none` comparison and the `Task` closures capturing `self` weakly
  in `drainFileCloses`), `scripts/build-macos-dev`, `scripts/check-macos-app`, the `file_panel` smoke
  stage and the hosted macOS workflows. None is recorded as passed.

### Fix round 2 (S4 review findings, four valid and fixed, one fixed in part)

Base of the round: `9b9a848`. Swift still cannot be compiled or run here. Each fix below is checked
by reading the code path and by a Linux structure test that fails when the fix is reverted; the
smoke additions are UNAVAILABLE here and have not run.

| # | Finding | Verified | Fix |
| --- | --- | --- | --- |
| 1 | A loaded dirty panel that lost its binding and then failed to re-attach ended with Save, Reload and Copy Text all gone | Yes, by reading: `retry()` cleared `staleOwner` and the banner before the attach, `loadFailed` then raised a Dismiss-only `.loadFailed` banner, and `performReload`'s `if staleOwner { retry() }` returned silently | `retry()` no longer clears the stale state or the recovery banner. `loadFailed` keeps a loaded panel stale (`staleOwner = true`, `ownerLost` banner with Reload from Disk and Copy Text) whenever it has no token, was stale or holds unsaved text, with the failure code in the text. `performReload` with no token now awaits `reattach()` regardless of the stale flag, so the confirmed reload does something. A successful attach clears the `ownerLost` banner. |
| 2 | Banner Reload buttons (`unreadable`, `saveResolved`, `saveUnknown(0)`, also `changedOnDisk`) discarded unsaved text with no confirmation | Yes: they called `startReload(discard: true, automatic: false)`; only `reloadFromDisk()` confirmed | Every manual reload, banner or menu, goes through `reloadFromDisk()`, which asks (`confirmDiscard`) when `hasUnsavedChanges`. `startReload` remains only for the automatic reload of a view without unsaved text. A reload requested while another operation runs now says so instead of doing nothing. |
| 3 | `UICommand.fileOpen` / `.fileDetach` were awaited inside the single terminal command loop, so a probe plus executor wait (about 3 s) stalled `.input`, `.key`, `.paste`, `.wheel`, `.resize` behind it | Yes: both arms awaited the bridge inside `commandTask` | New `FileCommand` enum with its own `AsyncStream` (`bufferingOldest(32)`, `fileCommandCapacity`) and `fileCommandTask`. `enqueueFile` reports a full queue and a closed desktop (a detach during shutdown stays silent: the file service shutdown releases it). `File > Open...` and `filePanelDetach` use it; `UICommand` lost both cases; shutdown finishes the stream and cancels the task. The queue is FIFO, so an open and a later detach keep their order. |
| 4 | The owner-lost banner's Reload from Disk called `retry()`, an attach with the same host, which Rust refuses with `dirty` while the model is dirty (the `stale_view` case) | Yes: `files.rs` refuses an attach as the bound host while `model_dirty`; existing tests assert it | The button now means `reloadFromDisk()`. A kept token recovers with `reload(discard: true)` (Rust allows it); a lost token (epoch rotation, `stale_owner`, detach) re-attaches, which Rust accepts because no host is bound. New Rust test `a_dirty_view_recovers_a_kept_binding_by_reload_and_a_lost_binding_by_attach` pins both halves against the real service. |
| 5 | The smoke omitted session quit/restore, error states with Retry and descriptor release, and the receipt did not say so | Yes | Added: step 12 (error state `not_found` then `retry()`), a banner-Reload discard confirmation step 11, and an `lsof` check that no descriptor under the fixture directory remains after all tabs close. NOT added: session quit/restore. Restore needs an empty desktop (`restore-session` refuses a populated one), so it would destroy the app state the rest of the smoke runs in, and a quit/relaunch needs a harness that does not exist here. This is recorded as a known limit and a deviation. The smoke source documents it, and a Linux test pins the structure of `epochChanged` (a clean panel re-attaches, a dirty one keeps its text and says so). Fixed in part. |

Tests added (all Linux-runnable; none proves the Swift works):

- `crates/desktop-runtime/tests/files.rs`: `a_dirty_view_recovers_a_kept_binding_by_reload_and_a_lost_binding_by_attach`
  (real service: refused same-host attach while dirty, reload with discard still works, detach then
  attach succeeds while dirty, detached owner is `stale_owner`).
- `crates/native-bridge/tests/swift_file_contract.rs`, four new tests:
  `every_manual_reload_asks_before_discarding_unsaved_text`,
  `a_lost_binding_recovers_by_reattaching_and_a_kept_one_by_reloading`,
  `file_service_waits_have_their_own_bounded_queue_off_the_terminal_input_path`,
  `the_smoke_covers_error_state_retry_and_descriptor_release_and_names_the_restore_gap`.
- Mutation evidence (fix reverted in a copy, test run, sources restored and checked with `cmp`):
  a banner arm calling `startReload` again, `performReload` going back to `if staleOwner { retry() }`,
  `loadFailed` reduced to owner loss only, a bridge `fileOpen` arm back in the terminal loop and the
  smoke's `retry()` removed each made exactly the matching test FAIL (11 passed, 1 failed).

Checks run: `scripts/check` PASS, exit 0 (fmt, `cargo test --workspace --locked`: 79 test binaries,
559 passed, 0 failed, 2 ignored, which is the previous 554 plus the 5 new tests; clippy
`-D warnings`; license inventory; `scripts/check-native-bridge`). Determinism of the two changed
binaries, each executed: `swift_file_contract` (12 tests) and `desktop-runtime` `files` (46 tests):
for each, 20 serial runs, 0 failures; then 5 rounds of 8 concurrent copies (40 runs) beside 24
busy-loop CPU hogs on a 24-core machine, 0 failures, every log carrying the full passed line. The
new Rust test uses the existing deterministic fixture (no sleeps, timers or threads of its own); the
four structure tests only read source files.

Deviations and limits:

- Session quit/restore is not in the smoke (finding 5, above). The native reaction to a `files.epoch`
  change is verified only by reading and by the structure test.
- The reload confirmation also guards `changedOnDisk`'s Reload, whose banner text already says edits
  are discarded; the prompt is a second, consistent safeguard.
- A reload that ends in a failed re-attach leaves a dirty panel stale on purpose; the person retries
  from the banner or the menu. There is no automatic retry, to keep the text from being replaced.
- Between a clean panel's automatic re-attach starting and its reply, typing is still replaced by the
  reply (pre-existing, not changed here). SUPERSEDED by fix round 3: the editor is read-only while an
  attach or reload is in flight, so this no longer holds.
- The `lsof` descriptor check and the new smoke steps are UNAVAILABLE here: no Swift toolchain, so
  `scripts/check-macos-sources`, `scripts/build-macos-dev`, `scripts/check-macos-app`, the
  `file_panel` stage and the hosted macOS workflows are not run and not recorded as passed. Swift
  names used without a compiler that CI should confirm: `Process`/`Pipe` in a detached task,
  `NSButton.performClick(nil)`, and the `@discardableResult` placement on `enqueueFile`.

### Fix round 3 (S4 review finding, one blocking, valid and fixed)

Base of the round: `6e3c021`. Swift still cannot be compiled or run here.

| # | Finding | Verified | Fix |
| --- | --- | --- | --- |
| 1 | Text typed while a manual Reload from Disk or a re-attach is in flight is overwritten by the reply. Only the automatic reload was protected. | Yes, by reading: `reloadFromDisk()` on a clean panel skips the confirmation and `performReload(discard: true, automatic: false)` awaits `send(.reload)`; the editor stayed editable and `busy` did not block typing; `apply` then ran `applyText`, which replaced the buffer and called `setDirty(false)` with no check. `finishAttach` (epoch re-attach, `retry()`, `reattach()`) did the same. The receipt had admitted only the re-attach case. | The editor is read-only for the whole time a reload or attach can replace the buffer. `FilePanelView` gains `editingSuspended`, `suspendEditing()`, `resumeEditing()` and `refreshEditable()` (`isLoaded && writable && !editingSuspended`, the only place besides the initial setup that assigns `editor.isEditable`). `prepareAttach` suspends in the same step that sets `busy`; `finishAttach` resumes right after the reply; `performReload` suspends before `send(.reload)` and resumes after it; `applyText` goes through `refreshEditable`. There is no suspension point between the clean check in `reloadFromDisk` and `performReload` (same main-actor turn), so no window is left before the gate closes. The automatic-reload guard (`automatic && hasUnsavedChanges`) stays for text typed between scheduling and starting the automatic reload. Save is deliberately not gated: it compares the buffer with what it wrote (`sameBytes`) and keeps edits made during the write. |

Test added (Linux-runnable; it reads the Swift source and does not prove the Swift works):
`crates/native-bridge/tests/swift_file_contract.rs`
`the_buffer_is_read_only_while_a_reload_or_attach_reply_can_replace_it`: only `refreshEditable`
assigns the editor's editable state and it includes the suspension flag, `applyText` uses it,
`prepareAttach` suspends together with `busy = true`, `finishAttach` resumes after `busy = false` and
before `apply`, `performReload` suspends before and resumes after the awaited request, and a clean
`reloadFromDisk` reaches `performReload` without another suspension point. Mutation evidence (fix
reverted in a copy, test run, sources restored and checked with `cmp`): removing the suspension in
`performReload`, removing it in `prepareAttach`, and `applyText` assigning `isEditable = writable`
again each made the new test FAIL (12 passed, 1 failed).

Checks run: `scripts/check` PASS, exit 0 (fmt, `cargo test --workspace --locked`: 79 test binaries,
560 passed, 0 failed, 2 ignored, which is the previous 559 plus the 1 new test; clippy
`-D warnings`; license inventory; `scripts/check-native-bridge`). Determinism of the changed binary
(`swift_file_contract`, 13 tests): 20 serial runs, 0 failures; then 5 rounds of 8 concurrent copies
(40 runs) beside 24 busy-loop CPU hogs on a 24-core machine, 0 failures, every log carrying the
13-passed line. (A first attempt at the concurrent runs used a faulty wait loop in my driver script
and was discarded; the 40 runs reported here are from the rerun.) The new test only reads source
files and uses no threads, timers or sleeps.

Deviations and limits:

- Typing is blocked, not buffered, during a reload or attach (up to the 2 s file-service timeout, or
  longer while the worker is stalled; the header shows "Working…"). Keystrokes in that time are lost
  rather than replayed; no existing text is lost.
- No smoke step was added: the in-app smoke cannot run here, and a behaviour check of a window this
  narrow needs a stalled worker the harness does not provide.
- UNAVAILABLE as before: `scripts/check-macos-sources` (a Swift 6 type check of this edit),
  `scripts/build-macos-dev`, `scripts/check-macos-app`, the `file_panel` smoke stage and the hosted
  macOS workflows. None is recorded as passed.

### Fix round 4 (S4 review findings, two blocking, both verified valid and fixed at the root)

Base of the round: `a06accb`. Swift still cannot be compiled or run here (no Swift toolchain, Linux).

| # | Finding | Verified | Fix |
| --- | --- | --- | --- |
| 1 | Don't Save raced the snapshot poll: `settleDirtyMirror(assumingDiscarded: true)` cancelled the dirty-mirror task and told Rust the file was clean while the panel kept the text, with no state to stop `update()` -> `reconcileDirty()` from restarting the loop. With two or more discarded panels a poll between panel 1's settle and the final discard re-marked panel 1 dirty (`observe(true)`); the core close guard then refused the close of a panel that already read clean, and a later close lost the edits silently. | Yes, by reading: `update()` calls `reconcileDirty()` on every snapshot; `observeNeeded` had no discard state; `resolveFilesBeforeClose` awaited one bridge call per panel before the single `discards.forEach`. A second, narrower hole of the same kind was found while fixing it: a cancelled loop can still have an `observe(true)` in flight on a detached worker that lands after the settle's `observe(false)`, and the `wanted == lastModelDirty` shortcut would then skip the write. | New `DirtyMirrorGate` (a counted hold) in `FileCloseResolution.swift`. `observeNeeded` is `dirtyGate.observationNeeded(...)`, which is false while a hold is taken, so neither `update()` nor the loop's own tail can restart the mirror. `holdDiscard()` takes the hold and cancels the task synchronously. The whole close decision moved out of the coordinator into `FileCloseResolution.resolve`: all Cancel/Save decisions first, then every discarded panel is held before the first awaited settle (nothing awaited in between), then settle, then (fix round 6: after the core has closed the tab, not at the end of `resolve`) `applyDiscard()` for every discarded panel. `applyDiscard()` drops the text from dirty tracking and lifts the hold in one synchronous step. On a failed settle every hold is released and every mirror resumed. `settleDirtyMirror` now waits for the cancelled loop (`if let running { await running.value }`) before it writes, and `closeWriteNeeded` forces a write when a loop ran or the panel is held, so a late `observe(true)` cannot land after (and undo) the settle. Holds are counted so two overlapping closes of one panel cannot release each other's. |
| 2 | `dont_save_is_applied_only_after_the_whole_close_is_settled` could not detect a failed Save no longer aborting the close (D8). | Yes, reproduced as reported: replacing the Save guard with `_ = await view.performSave(force: false)` left the old test green (its `last_return_false` was found at the later settle failure). | The Save guard text is pinned exactly in the new `close_resolution_aborts_without_discarding_and_applies_dont_save_last`, together with the exact Cancel and Discard arms, hold before settle with no `await` between, release then resume on failure, `applyDiscard` last, and exactly three `return false` abort paths. Because the decision is now an ordinary generic function, a macOS fixture also executes it (below). |

Tests added or strengthened:

- `tools/native-fixtures/file-close-resolution.swift` + `scripts/check-file-close-resolution` (new,
  wired into `.github/workflows/macos-development.yml`): compiles the production
  `FileCloseResolution.swift` with scripted panels and runs the real `FileCloseResolution.resolve` and
  `DirtyMirrorGate` with a snapshot poll injected before and after every settle write (synchronous
  calls, no threads, timers or sleeps). Scenarios: two Don't Saves keep the model clean and a later
  poll does not re-mark it; a control with holds disabled shows the model re-marked dirty for a panel
  that reads clean (proves the scenario can fail); Cancel after Don't Save changes nothing; a failed
  Save aborts before anything is settled or held; Save then Don't Save; a failed settle restores the
  first panel's bit in the model and lifts every hold; a clean panel is settled but never held; the
  gate's truth table, counted holds and `closeWriteNeeded`. UNAVAILABLE here: it needs `xcrun swiftc`.
  It was never compiled or run, so even its own syntax is unproven.
- `crates/native-bridge/tests/swift_file_contract.rs`: removed
  `dont_save_is_applied_only_after_the_whole_close_is_settled`; added
  `close_resolution_aborts_without_discarding_and_applies_dont_save_last`,
  `a_panel_being_discarded_cannot_restart_the_dirty_mirror`,
  `closing_waits_for_an_in_flight_observation_before_writing` and
  `the_close_decision_fixture_is_wired_into_macos_ci`. These only read source files; they pin exact
  statements and their order. They do not execute the Swift. That the production `FilePanelView`
  actually behaves like the fixture's scripted panel (its `update()`/`reconcileDirty()` plumbing, the
  loop tail, the in-flight wait) is therefore pinned structurally only; the in-app smoke is the
  behavioural check and has not run.
- `FilePanelSmoke` (UNAVAILABLE here): new step between the Cancel check and the back-to-back close
  burst in step 10. A two-file `resolveFilesBeforeClose` answered Don't Save twice must return true,
  leave both models clean across four `status` polls, and, after new keystrokes, mark both models
  dirty again (the hold is lifted). The burst then runs from those re-dirtied panels.
- Mutation evidence (fix reverted in a copy of the Swift, the binary run, the file restored and
  checked): Save guard replaced by `_ = await panel.performSave`, the hold line removed, the gate's
  `!isDiscarding` removed, the wait for the old loop removed, and `|| assumingDiscarded` removed from
  the write decision each made exactly one matching test FAIL (15 passed, 1 failed each). The old test
  stayed green for the first of these before the round, as the finding said.

Checks run: `scripts/check` PASS, exit 0 (fmt, `cargo test --workspace --locked`: 79 test binaries,
563 passed, 0 failed, 2 ignored, which is the previous 560 minus 1 removed plus 4 added; clippy
`-D warnings`; license inventory; `scripts/check-native-bridge`). Determinism of the changed binary
(`swift_file_contract`, 16 tests, one real Desktop test among them): 20 serial runs, 0 failures; then
3 rounds of 8 concurrent copies at once (24 runs), 0 failures. This is fewer concurrent rounds than
earlier rounds and was run on an otherwise idle machine without busy-loop CPU hogs; the new tests only
read source files and use no threads, timers or sleeps. (A first concurrent attempt used a faulty
`wait` loop in my driver and every copy reported failure by the driver's own bug; it was discarded
and the 24 runs reported here are from the rerun with a bash driver.)

Deviations and limits specific to this round:

- The decision logic is executed by a fixture only on macOS CI. On this machine the only evidence is
  source-structure pinning, stated as such above.
- After a successful `resolve`, the tab is closed by the caller. If the core still refuses that close
  for a reason other than the dirty bit, the discarded panel stays open reading clean while still
  holding the text (the hold is already lifted, so edits made from then on are tracked normally).
  Pre-existing in kind; not changed.
- `settleDirtyMirror` waits for a cancelled loop's in-flight `observe`, bounded by the file call
  timeout (2 s), so Don't Save can take that long to close when the file service is stalled. There is
  no progress UI for that wait.
- `resolveFilesBeforeQuit` goes through the same function; no smoke step quits the app.

#### Remaining known limitations of the native file panel (for the PR description)

Every Swift/macOS gate is UNAVAILABLE locally: `scripts/check-macos-sources` (Swift 6 strict
concurrency, warnings-as-errors type check of all sources including `FileCloseResolution.swift`),
`scripts/check-file-close-resolution`, `scripts/check-browser-download-files`, `scripts/build-macos-dev`,
`scripts/check-macos-app`, the `file_panel` smoke stage and every hosted macOS workflow. None is
recorded as passed; CI is the verifier and none of the Swift in this slice has ever been compiled.

- Unconfirmed Swift API use that CI must confirm first: `PresentationIntent` components/kinds in
  `FileMarkdownRenderer`; `NSTextViewDelegate` isolation on the MainActor view; `NSFont`/`NSColor`/
  `NSParagraphStyle` built inside a detached task; autolinking of `import PDFKit` (add
  `-framework PDFKit` to `scripts/build-macos-dev` if the link fails); `withCheckedContinuation` around
  `beginSheetModal`; `Process`/`Pipe` in a detached task; `NSButton.performClick(nil)`; `@discardableResult`
  on `enqueueFile`; `Banner == Banner.none`; and, new, the `@MainActor` closure parameter of the generic
  `FileCloseResolution.resolve`, the `[self]` capture in its coordinator call, and `@main @MainActor struct`
  in the new fixture.
- Behaviour only a Mac can show: first-responder retention, Cmd-S/Cmd-O/Cmd-Z, `NSOpenPanel`, the
  Save/Don't Save/conflict/discard/orphan sheets, quit-with-dirty, IME, VoiceOver labels, large-file
  `NSTextView` performance, ImageIO/PDFKit decode of real files, Markdown fidelity, word-wrap layout,
  dark-appearance banner contrast.
- Between the first keystroke and the acknowledged `observe(true)` (D7a), a socket `close-surface` can
  still get through; the orphan sheet then offers Copy Text.
- A stalled file service surfaces `file_worker_unavailable`/`busy` on Save, Reload and observe; observe
  retries at a bounded rate, the others show a banner and wait.
- Session quit/restore (`files.epoch` rotation) and the quit-with-dirty prompt are not exercised by the
  smoke; the epoch reaction is pinned structurally only.
- Typing is blocked, not buffered, while a reload or attach is in flight; keystrokes then are lost.
- Text typed into a panel between its Don't Save answer and the end of the same close is also discarded.
- Repeats of the same close gesture while it is pending are folded into the pending request.
- Rendered Markdown: tables degrade to tab-separated text; thematic breaks, images and raw HTML are not
  drawn; relative links are inert; only http, https and mailto open; scroll position is not kept across
  a reload.
- Word wrap is one global user default, not per file and not in the session document.
- `Open File...` is capped at 16 selections; the orphan sheet offers Copy Text and Discard only (no
  Save As).
- `scripts/check-macos-app` does not list `file_panel` among its required evidence keys (the key is
  emitted).
- The dirty-mirror gate and the in-flight wait in the production `FilePanelView` are pinned by source
  structure on Linux and run only by the macOS fixture (against scripted panels, not `FilePanelView`)
  and the in-app smoke. (Fix round 5 narrowed this: the dirty, save, reload and banner decisions now
  live in `FileEditorModel`, which its own fixture runs directly; the dirty-mirror plumbing, the
  observe loop and `settleDirtyMirror` remain in the view and remain structure-pinned.)

### Fix round 5 (S4 review finding, one blocking, valid and fixed by extraction)

Base of the round: `06dcf64`. Swift still cannot be compiled or run here (no Swift toolchain, Linux).

| # | Finding | Verified | Fix |
| --- | --- | --- | --- |
| 1 | Almost all S4 native-panel "behaviour" tests were substring pins on Swift source. The data-loss-critical logic (`hasUnsavedChanges`, save payload, dirty tracking, reload, banner) had no runnable test at its production owner, and the receipt's claim that each assertion fails if its fix is reverted held only for the exact pinned text. | Yes, by reading: `hasUnsavedChanges`, `Data(text.utf8)`, `setDirty`, `reconcileExternal`, `performReload`'s keep-typed-text branch and `bannerAction` were inline in the AppKit `FilePanelView`, so no fixture could reach them; the sixteen tests of the binary read source text for all of them. The reviewer's mutations (`hasUnsavedChanges { false }`, dropping the last byte of the save body) would have left the Linux suite green, and they still do: Linux can only pin structure. They are caught only by the new macOS fixture. | New Foundation-only `apps/macos/Sources/FileEditorModel.swift` (`FileEditorModel`, `FileBanner` and four small answer enums). It owns the baseline, dirty bit, base generation, kept-edits flag, editing suspension and the external-change and banner bookkeeping, and answers: byte-exact `textEdited`/`adopt`/`saved`, `hasUnsavedChanges(editable:)`, `canSave`, `savePayload(for:)`, `saveNeedsConflictPrompt`, `isEditable`, `reloadReplied` (an automatic reload that finds typed text keeps it), `externalStateReported`, `saveOutcomeBanner`, `bannerAction`, and which banners a load or a save obsoletes. `FilePanelView` holds one `model` and applies the answers (`dirtyChanged`, `setBanner`, `reloadFromDisk`); its `isDirty` and `banner` are now read-only views of the model and `FilePanelView.Banner` is a typealias of `FileBanner`. No behaviour was intentionally changed; the move is a re-homing of the same statements. |

Tests added or changed:

- `tools/native-fixtures/file-editor-model.swift` + `scripts/check-file-editor-model` (new, wired into
  `.github/workflows/macos-development.yml` after the close-resolution step): compiles the production
  `FileEditorModel.swift` alone (no AppKit) with Swift 6 strict concurrency and warnings as errors and
  runs it synchronously (no threads, timers, files or sleeps). Scenarios: canonical-equivalent text is
  dirty (with a control proving Swift `==` would call it equal), undo returns to clean; save payload
  equals the UTF-8 bytes for empty, multibyte, CRLF and decomposed text; the `canSave` truth table;
  a confirmed save rebases baseline and generation, text typed during the save stays dirty, a reply
  without a generation keeps the old base; kept edits over a newer disk ask before saving and a
  confirmed save ends that state; the editor is read-only exactly while a reload or attach is in
  flight; an automatic reload that finds typed text keeps it and shows the changed-on-disk banner while
  manual and clean reloads replace the buffer; external-state reactions (automatic reload only for a
  clean buffer and once per disk generation, banners for dirty, deleted and unreadable, nothing when
  detached or busy, dismissals remembered until the disk moves or re-syncs, banner cleared on sync
  unless edits were kept); save-outcome resolution; and every banner button (only reload answers
  reach `reloadFromDisk`, a load failure over unsaved text only dismisses, a pending save has no
  buttons). UNAVAILABLE here: it needs `xcrun swiftc`. It was never compiled or run, so even its own
  syntax is unproven; its expected values were traced by hand against the model.
- `crates/native-bridge/tests/swift_file_contract.rs`: every source-text test was renamed with a
  `pin_` prefix and the module documentation now separates contract tests (execute real Rust against
  the Swift text) from structure pins (assert statements exist and their order, never run the Swift),
  so no test name claims a behaviour it does not execute. The pins for dirty detection, banner
  reload, lost-binding recovery and the read-only buffer now read `FileEditorModel.swift` for the
  decision and `FilePanelView.swift` for the delegation. New:
  `the_file_editor_model_fixture_is_wired_into_macos_ci` (script compiles the model alone, strictly;
  CI runs it; eleven named fixture scenarios exist) and
  `pin_the_file_editor_model_is_appkit_free_and_the_panel_asks_it` (the model imports Foundation only;
  the panel no longer contains the moved state or `Data(text.utf8)`; the panel asks the model at each
  decision site). These cannot show that the Swift is correct; they keep the model runnable by the
  fixture and keep decisions from drifting back into the view.
- No mutation of the model is caught on Linux, by design: the reviewer's two mutations now land in
  `FileEditorModel.swift` and only `scripts/check-file-editor-model` on macOS can fail them. This is
  stated rather than claimed as covered.

Checks run: `scripts/check` PASS, exit 0 (fmt, `cargo test --workspace --locked`: 79 test binaries,
565 passed, 0 failed, 2 ignored, which is the previous 563 plus 2 added; clippy `-D warnings`;
license inventory; `scripts/check-native-bridge`). Determinism of the changed binary
(`swift_file_contract`, 18 tests, one real Desktop test among them): 20 serial runs, 0 failures, every
log carrying `18 passed; 0 failed`; then 5 rounds of 8 copies started together beside 24 busy-loop CPU
hogs, 40 runs, 0 failures, every log carrying `18 passed; 0 failed`. The driver's `wait` loop was
faulty under zsh (it reported `job not found` and did not block per round), so rounds may have
overlapped more than 8 copies; pass/fail was taken from the 60 logs, not from the driver's counters.

Still UNAVAILABLE: `scripts/check-file-editor-model`, `scripts/check-file-close-resolution`,
`scripts/check-macos-sources` (which type-checks `FileEditorModel.swift` and the edited
`FilePanelView.swift` with the other sources), `scripts/build-macos-dev`, `scripts/check-macos-app`,
the in-app smoke and every hosted macOS workflow. New unconfirmed Swift points for CI: the
`@discardableResult` mutating methods, the `Optional` implicit-member comparisons such as
`model.saveOutcomeBanner(...) == .saveResolved` in the fixture, and `typealias Banner = FileBanner`
inside the `@MainActor` view.

Limits: the observe loop, `settleDirtyMirror`, `epochChanged`, the attach and reload plumbing and the
sheets are still in the view and still only structure-pinned (plus the existing close fixture for the
gate and the resolution); the extraction covers the decisions, not the asynchronous sequencing.

### Fix round 6 (S4 review findings, two blocking, both verified valid and fixed)

Base of the round: `58abc37`. Swift still cannot be compiled or run here (no Swift toolchain, Linux).

| # | Finding | Verified | Fix |
| --- | --- | --- | --- |
| 1 | `FilePanelSmoke` step 11 called `cli(["open", f.text], true)`; the second argument is `expectFailure`, so a successful single-file open threw "Bundled browser CLI failed its expected outcome" and the stage could never pass. | Yes, by reading: `nativeCLI` has `guard (process.terminationStatus != 0) == expectFailure`; `apps/cli/tests/open.rs` asserts exit 0 for an all-success open (design D17); the text tab was closed in step 8 and its reopened copy in the step 10 burst, so nothing is deduplicated. The mixed batch (step 1, `true`) and the dedupe call (step 3, `false`) are consistent; this was the only successful open passed `true`. | The call passes `false`. New Linux-runnable `pin_the_smoke_expects_a_plain_open_of_a_valid_file_to_succeed` checks every `cli(["open", ...], X)` line in the smoke: only the batch containing `f.missing` may expect failure. |
| 2 | Data loss: `FileCloseResolution.resolve` ended with `discards.forEach { $0.applyDiscard() }` before the core had closed anything, so a close the core refused (`ensure_settled`: "terminal restoration is in progress") or one that never reached the core (`enqueue` returns false on a full 256-slot queue; the result was ignored) left the tab open with its abandoned edits shown as clean, no model dirty bit and no prompt on the next close. | Yes, by reading: `drainFileCloses` called `next.proceed()` after `resolveFilesBeforeClose` returned true, and `proceed` was a `Void` closure around `enqueue`, whose `Bool` and the command's later failure (`reportError` in the command loop) were never seen by the close. `applyDiscard` -> `discardLocalChanges` -> `model.adopt(text:)` resets baseline and dirty bit. The receipt's "applied only once the whole close is settled" meant the sheets and mirror writes, not the core close. | Don't Save is now two phases. `FileCloseResolution.resolve` still asks, holds, and settles (the model reads clean so the core accepts the close), but returns a `FileCloseSettlement` (nil on Cancel, failed Save or failed settle, as before) and applies nothing. The settlement's `finish(closeSucceeded:)` decides once: `true` applies every discard, `false` releases every hold, which resumes the dirty mirror and re-marks the model dirty (the panel never lost its text or its edited marker). To know the answer, closes are sent as `UICommand.tracked(command, CommandOutcome)`: `ApplicationCoordinator.perform` (the extracted command switch) finishes the outcome `true` after the command ran and `false` when it threw (still reported to the person), `enqueueTracked` fails it when the queue refuses the command, and the command loop fails a tracked command it drops at shutdown so the close queue cannot hang. `MainWindowHost.drainFileCloses` awaits `next.proceed().wait()`, finishes the settlement, and only then dequeues, so a repeat of the gesture is still folded and a following close sees the settled state. All five close gestures (surface, other surfaces, pane, workspace, window) use the tracked send. A snapshot poll that sees the tab gone while Don't Save is in flight retires the panel quietly (`isDiscardPending`, no orphan prompt); `applyDiscard` on an already retired panel only gives its hold back. Quit still applies the discard before returning because termination cannot be refused by the core and the next `applicationShouldTerminate` must see no dirty panel. |

Tests added or changed:

- `tools/native-fixtures/file-close-resolution.swift` (UNAVAILABLE here, needs `xcrun swiftc`): the
  scripted panel now counts applies and releases. New scenarios run the real `FileCloseResolution`,
  `FileCloseSettlement` and `CommandOutcome`: a settled Don't Save leaves the text, edited marker and a
  held gate on the panel with the model clean, and polls start no observation until the core answers;
  a refused close releases (never applies), re-marks the model dirty and the next close prompts
  again; the first resolution wins (a late success after a refusal cannot drop text, a late refusal
  after a success cannot revive it, no hold underflow); two overlapping closes of one panel, first
  refused and second accepted, keep the hold counted; Save answers leave nothing pending; the outcome
  returns the first result, a failed outcome, and a waiter suspended before the answer. The wait uses
  `Task.yield()` until the waiter is registered, no sleeps.
- `crates/native-bridge/tests/swift_file_contract.rs` is 20 tests (18 before; fmt'd). Structure pins,
  not behaviour: `pin_close_resolution_aborts_without_discarding_and_applies_dont_save_last` was
  renamed `pin_close_resolution_aborts_without_discarding_and_defers_dont_save_to_the_core` and now
  pins that `resolve` returns nil on its three abort paths, never calls `applyDiscard`, and that the
  settlement's `finish` guards once, then applies on success and releases on failure.
  New `pin_a_close_the_core_refuses_gives_the_discard_back` (queue order: resolve, `wait()`, `finish`,
  `removeFirst`; five tracked close gestures each with a `.failed()` fallback; `enqueueTracked`,
  `perform` and the loop's shutdown path finish the outcome; retire-before-orphan; quit applies; the
  fixture scenarios and the smoke's refuse-then-accept step exist) and
  `pin_the_smoke_expects_a_plain_open_of_a_valid_file_to_succeed`. The dirty-mirror pin for
  `applyDiscard` now accounts for the closed-panel guard. None of these runs the Swift; they keep the
  fixture-covered structure from drifting.
- `apps/macos/Sources/FilePanelSmoke.swift`: step 10 resolves the two-file Don't Save, checks the
  panels still hold text and the model stays clean across four polls, finishes the settlement with
  `false` and checks both panels keep their text and the model is dirty again, then resolves and
  finishes with `true` and checks both are clean. Steps 8 and the burst already exercise the real
  accepted close through `drainFileCloses` (no orphan panel, tab gone). Not run (no macOS).

Mutation check on Linux for the structure pins (applied, run, restored): making `resolve` call
`discards.forEach { $0.applyDiscard() }` before returning fails the renamed pin; dropping the
`settlement.finish(closeSucceeded: closed)` line in `drainFileCloses` fails the new refusal pin. No
mutation of the Swift behaviour is caught on Linux, by design.

Checks run: `scripts/check` PASS, exit 0 (fmt, `cargo test --workspace --locked`: 79 test result
lines, 567 passed, 0 failed, 2 ignored, the previous 565 plus 2 added; clippy `-D warnings`; license
inventory; `scripts/check-native-bridge`). Determinism of the changed binary (`swift_file_contract`,
20 tests, one real Desktop test among them): 20 serial runs, 0 failures; then 5 rounds of 8 copies
each started together and waited for individually, beside 24 busy-loop CPU hogs, 40 runs, 0 failures;
60 of 60 logs carry `20 passed; 0 failed`.

Deviations and limits:

- A close whose core command succeeds applies the discard even if the snapshot has not yet shown the
  tab gone. The core reported the close done, so the tab is gone by contract; if a poll retires the
  panel first, `isDiscardPending` closes it quietly, and the settlement later only gives back its hold.
- If the application stops before a queued tracked close runs, the close fails and the discard is
  released, then the app is closing anyway. A tracked close queued behind the first dropped one at
  shutdown is not individually failed; its waiter ends with the process.
- Text typed into a panel after its Don't Save answer and before the core closes the tab is dropped
  with the tab (unchanged limit from fix rounds 2 and 4, now a slightly longer window).
- The refusal path is exercised in the smoke by finishing the settlement with `false`, not by making
  the core genuinely refuse a close (a pending terminal restore cannot be staged there). The
  `perform` / `enqueueTracked` plumbing that produces that `false` is pinned and unrun.
- Still UNAVAILABLE: `scripts/check-file-close-resolution`, `scripts/check-file-editor-model`,
  `scripts/check-macos-sources` (type-checks the new `CommandOutcome` / `FileCloseSettlement` /
  `indirect case tracked` and the extracted `perform`), `scripts/build-macos-dev`,
  `scripts/check-macos-app`, the `file_panel` smoke and every hosted macOS workflow. New unconfirmed
  Swift points for CI: `indirect case tracked(UICommand, CommandOutcome)` in a `Sendable` enum (a
  `@MainActor` final class is Sendable), the `any FileCloseParticipant` array built from `[Panel]`,
  `?? .failed()` on an optional `CommandOutcome`, and `withCheckedContinuation` storing its
  continuation in a `@MainActor` property.
