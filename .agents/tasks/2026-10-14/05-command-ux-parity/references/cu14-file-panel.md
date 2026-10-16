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
  (`MAX_FILE_SAVE_FRAME_BYTES`), 2 s waits (5 s save). Ops: attach, reload, save, observe,
  detach, open (path items). `zmux.h`, `docs/native-bridge.md`, `tests/abi.c` and
  `scripts/check-native-bridge` updated. `ABI_VERSION` stays 1 (one additive export).
- No new dependency: `sha2`, `rustix`, `uuid`, `serde_json` were already direct dependencies;
  `scripts/license-inventory --check` and `THIRD_PARTY_NOTICES.md` are unchanged.

### Red then green

Red: `crates/desktop-runtime/tests/files.rs` (25 behavioural tests) was written before
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

Green: `scripts/check` exit 0 (490 tests passed, 0 failed across the workspace). New tests:

- `file_io_tests.rs` (21): extension table incl. case, dotfile, no extension, `svg`/`html` as
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
- `tests/files.rs` desktop-runtime (25, real `Desktop`, gated and counting backend):
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
- `native-bridge/tests/files.rs` (5): frame validation (null, short, header over cap, header
  past the frame, not JSON, unknown op, unknown field, missing field, body on a non-save op,
  non-save over 16384, URL item, save body one byte over 2 MiB, wrong handle) all before any
  work; open -> attach -> 1.5 MiB save -> conflict -> reload -> observe -> detach round trip
  through the real exports; forged owner changes nothing on disk; `CAPACITY` with 16 buffers
  held and no tab created; `observe_returns_busy_not_ok_when_the_command_queue_is_full`.
- `crates/native-bridge/tests/abi.c`: independent C caller does open, attach (raw body check),
  framed save, stale-view refusal, detach and malformed-frame rejection through the shared library;
  `scripts/check-native-bridge` asserts its completion line.

Flake check: the desktop-runtime `files` binary passed 20 serial runs and 18 runs executed 6 at a
time under CPU contention. Two races found and fixed while writing the queue-full tests: the
executor must be proven parked in the hook before flooding, and C-registry `BUSY` under parallel
bridge tests is handled by serialising those tests (the registry intentionally uses `try_lock`).

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

- Control-socket route not built in this slice: no `TargetOperation::OpenItems`, no
  `control.rs` branch, no `capabilities.file_panel`, no `list-pane-surfaces` change. The slice
  scope named desktop-runtime files, the bridge and the C checks; the CLI/socket wiring belongs
  with S3. The same probe -> executor pipeline is available in process as `Desktop::file_open`
  and through the bridge `open` op, and `Command::OpenFiles` is `#[serde(skip)]`, so nothing new is
  reachable from the socket yet.
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
