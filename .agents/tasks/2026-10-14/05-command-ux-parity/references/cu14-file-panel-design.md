# CU-14 / CV-01 / CV-05 / CV-06 file panel: authoritative design

Status: design only, written 2026-10-16. Nothing here is implemented. This document supersedes
nothing in the research digest ([cu14-file-panel-research.md](cu14-file-panel-research.md)); it
decides the open questions the digest leaves and fixes exact wire shapes, limits and slices.
Evidence is recorded in [cu14-file-panel.md](cu14-file-panel.md) as slices land.

Deliverable: `zmux open <path-or-url>...` opens a file preview/editor panel (a tab in a pane,
like the browser panel): editable text, rendered Markdown, image and PDF preview, save with
conflict detection, live reload, close/reopen and session restore. One PR, real end to end,
delivered as five validated slices (S1..S5, section 6).

Licensing: original Zmux code, MIT. Nothing under the cmux checkout was read for this design
and none may be consulted for the implementation. Behaviour comes from the digest functional
spec and the audit acceptance rows (`audit-browser-content.json` CV-01, CV-02, CV-05, CV-06,
CU-14), which are Zmux's own records.

## 1. Facts confirmed against the code

I re-read every cited site. The digest is accurate; these additional facts shape the design.

- `SurfaceKind { Terminal, Browser }` and the flat `Surface` struct are in
  `crates/workspace-core/src/surfaces.rs:5-31`; `Surface` is `Serialize` only. Literals that
  must gain a field: `Surface::terminal` (:27), `CreateBrowser` (:210), `fresh_surface` (:661),
  restore literals `restore.rs:649-686`. `SavedTree::capture` matches `SurfaceKind`
  exhaustively (`restore.rs:564-621`), so the compiler forces the new arm.
- Every close path funnels through `ensure_settled` (`surfaces.rs:315,331,400,424`;
  `CloseWindow` reaches it through `CloseWorkspace`, `windows.rs:173`). One new guard beside
  it covers CloseSurface, CloseOtherSurfaces, CloseWorkspace, CloseWindow and ClosePane.
- `apply` clones the store and swaps only on success (`lib.rs:463-484`); it already runs
  `validate_terminal_descriptors` on the candidate, which is where file-shape and
  path-uniqueness validation belongs.
- `Command::Action{ObserveBrowser}` is rejected at `desktop-runtime/src/lib.rs:1574-1579`; the
  trusted path is `BrowserHostEvent` (`browser.rs:573`). Commands queue through a bounded
  `sync_channel(MAX_COMMANDS=32)` to one worker (`lib.rs:463`). While a session restore is
  pending only listed commands are admitted (`lib.rs:1400-1412`); the list must gain the new
  trusted file commands.
- The local control listener is ONE thread that serves one connection at a time
  (`control.rs:316-336`), with a 500 ms client I/O timeout, a 2 s executor wait
  (`COMMAND_TIMEOUT`), 16 KiB request and 1 MiB response caps. Read-only/authorization gating
  (`gate.admit`, `is_read`) runs before `dispatch` (`control.rs:561-585`). Consequence: file
  probing for `open` must be bounded to about one second and must run on a separate worker, not
  on the listener thread and not on the workspace executor.
- `mutate` renders a fixed receipt (`created_surfaces`, `focused_*`, `model_revision`;
  `control.rs:1178-1215`). `Execution` carries optional per-feature results
  (`download_result`, `window_result`, ...); an `open_result` follows that pattern.
- `shutdown.rs` joins workers against a deadline; `PathObservers` (`download_paths.rs`) is the
  precedent for a single bounded poll worker with `request_shutdown/is_finished/shutdown`.
- Native bridge: `Reservation::new()` is taken before work (`native-bridge/src/lib.rs:93-135`),
  `MAX_RESPONSE_BYTES = 8 MiB`, `MAX_BUFFERS = 16`, ordinary request cap 16 KiB. The only
  precedent for a larger request is the paste envelope (`:25`).
- No dependency is needed: `sha2` and `rustix` (features `fs`, `process`, `time`) are already
  direct dependencies of `zmux-desktop-runtime`; `uuid`, `serde_json` too. Tests use
  `std::env::temp_dir()` + `uuid` (no `tempfile` dev-dependency exists).
- Swift builds all `apps/macos/Sources/*.swift` by glob (`scripts/check-macos-sources`), so new
  Swift files need no project edits. `zmux.h` is imported through `-import-objc-header`.
- Swift trap (digest, confirmed): `App.swift:paneView` treats every surface that is neither a
  terminal nor a completed-history surface as a browser (`App.swift:965-995`), and the tab icon
  is `kind == "browser" ? "globe" : "terminal"` (`WorkspaceViews.swift:643`).
- A second trap found while reading: `PaneTree.key` (`Models.swift:226`) drives full pane
  rebuild and `paneView` calls `view.removeFromSuperview()`. If dirty state were folded into
  that key, the first keystroke in an editor would rebuild the pane and drop first responder.
  The design therefore keeps `dirty` out of the key (D23).
- Registry facts: `CMD-006 open` and `CMD-114 markdown open` are both `unresolved`/
  `local_contract: null` in `apps/cli/src/registry/frozen-coverage.json`; `validated()`
  (`registry/contracts.rs:1188`) requires every mapped `local_contract` to be a real leaf whose
  probe parses through `crate::parse` into an allowed `Parsed` variant (`parsed()`, :1144). The
  tracker uses only `planned`/`implementing` as `delivery_status` values today.

## 2. Scope and non-goals

In: everything in the deliverable sentence, plus `zmux markdown open`, `File > Open...` and
`File > Save` in the native app, dirty-close protection, dedupe, bounded polling live reload,
overload/recovery states, and honest Linux test coverage of all Rust-owned behaviour.

Out (section 7 is the full list): CV-02 typography/find/zoom/copy-as-HTML, syntax highlighting,
line numbers, Quick Look media, PDF thumbnails/outline, Markdown tables/images/relative links,
non-UTF-8 text, remote (SSH) files, autosave/crash recovery of unsaved text, Save As, three-way
merge, FSEvents/inotify watching, preferred external editor, directory-to-workspace routing.

A portable prototype is not a native release: S1-S3 are fully verifiable on Linux; S4 (Swift)
cannot be built or run here and is recorded UNAVAILABLE until macOS CI runs it.

## 3. Decisions

Each decision states the rationale and the rejected alternative.

### D1. Surface model: `SurfaceKind::File` and `Surface.file`

```rust
pub enum SurfaceKind { Terminal, Browser, File }          // wire: "terminal" | "browser" | "file"
pub enum FileMode { Text, Markdown, Image, Pdf }          // wire: "text" | "markdown" | "image" | "pdf"
pub struct FileSurface { pub path: String, pub mode: FileMode, pub dirty: bool }
// Surface gains:  pub file: Option<FileSurface>          // wire: "file": null | {path, mode, dirty}
```

Invariants (checked in `validate_terminal_descriptors`, also run on restore):
`kind == File` iff `file.is_some()`. A file surface has `session_id == None`, `url == None`,
`navigation == 0`, `browser_profile_id == None`, no `cwd/remote/local_tmux/local_zellij*`.
Terminal and browser surfaces have `file == None`. `path` is absolute, UTF-8, at most 4096
bytes, no control characters or NUL, no `.`/`..` component, no trailing `/`. `dirty` is true
only for `Text`/`Markdown`. Canonical paths are unique across all file surfaces in the store
(D18). Title defaults to the basename (at most `MAX_NAME_CHARS` chars) and may be renamed with
the existing `RenameSurface`.

The core stores ONLY identity and a trusted dirty bit. Content, hashes, fingerprints, external
state and errors never enter `Snapshot` or the persisted document (D5, D9).

Rejected: (a) reusing `Browser` with a `file:` URL: it would defeat `validate_browser_url`,
which deliberately forbids local files, and drag WebKit's file-access policy into the editor
path; (b) one `SurfaceKind` per mode: four near-identical kinds multiply every kind-aware site
(`inspection`, restore, Swift) for no behavioural gain, since mode only picks a native view;
(c) a content/fingerprint field in `Surface`: it would make the model, the closed-item history
and the 8 MiB snapshot reply carry file bytes.

### D2. Path and symlink policy

1. The CLI makes operands absolute by joining `env::current_dir()` (as `paste-files` does,
   `apps/cli/src/targets.rs:232-253`); it does not canonicalize and does not expand `~`.
2. The Rust probe canonicalizes once with `std::fs::canonicalize` (realpath) on the file
   worker. The canonical path is what `Action::CreateFile` stores. macOS `/var -> /private/var`,
   `/tmp`, `/etc` and user symlinks therefore work; opening a symlink edits its target.
3. Every later operation (attach, reload, save, poll) re-opens the stored canonical path with a
   component walk from `/`: each directory `openat(O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)`,
   the leaf `O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC`, then `fstat` must say regular file. If any
   component has since become a symlink the operation fails with `path_changed` ("path no longer
   resolves to the stored location; reopen it with `zmux open`"); it never silently follows.
   No file descriptor is held between operations, so close releases everything (CV-05
   "releases files").
4. Non-regular targets (directory, FIFO, socket, device) are rejected; `O_NONBLOCK` stops a
   FIFO from hanging the worker. Non-UTF-8 paths are rejected (`path_not_utf8`): the model path
   is a `String`. Hard-linked files (`nlink > 1`) can be read but not atomically saved (D13).

Rejected: reusing `persistence.rs` `open_directory`/`private_file`/`check_file` wholesale. Its
walk is the right pattern but its error type is `PersistenceError` and its owner/`0600` checks
are for private state and MUST NOT apply to user files. I re-implement the ~30-line walk in a
new `file_io.rs` rather than refactor the heavily tested persistence worker. Rejected: rejecting
all symlinks like persistence does (macOS `/var` and `/tmp` are symlinks); rejected: following
symlinks at every operation (a swap between open and save would redirect a write).

### D3. Operand classification and routing

Lexical, in the CLI (and re-validated by the server):
`http://` / `https://` (ASCII case-insensitive scheme) => URL item, handled by the existing
`Action::CreateBrowser` (per-host private profile; no `--profile` in v1). Any other
`scheme://` operand, including `file://`, is `unsupported_scheme` (a plain path is the one way
to name a file; avoids percent-decoding/host edge cases). `about:blank` is not an `open`
operand. Everything else is a path (`foo:bar.txt` is a path). Empty operand, NUL/control
characters, or a path over 4096 bytes is `invalid_operand`.

Directories: per-item error `is_directory` with the message "directories cannot be opened; use
`zmux new-workspace` and cd, or open a file". There is no safe existing workspace-create-with-cwd
action (`Action::CreateWorkspace` takes only a name; cwd is only observed from a live terminal),
so inventing one is out of scope. Binary, oversize, unreadable, missing: per-item errors, never
a success-shaped placeholder.

Rule for failures: purely lexical errors (D3 list, more than 16 operands, request over the
16 KiB cap) fail the whole invocation before connecting (fail fast, no side effects). Anything
that needs the filesystem or the model is a per-item error; the other items still proceed.

### D4. Mode selection by extension, then content

Rust owns this (`file_io::classify`). Extension is the last `.`-suffix of the basename,
ASCII-lowercased; a basename with no dot or a leading dot only (`.bashrc`) has no extension.

| mode | extensions | extra check at probe |
| --- | --- | --- |
| `markdown` | `md`, `markdown`, `mdown`, `mkd`, `mkdn` | text sniff |
| `image` | `png`, `jpg`, `jpeg`, `gif`, `webp`, `bmp`, `tif`, `tiff`, `heic`, `heif`, `ico` | signature for png/jpg/jpeg/gif only; mismatch is `content_mismatch` |
| `pdf` | `pdf` | `%PDF-` within the first 1024 bytes, else `content_mismatch` |
| `text` | everything else, including no extension, `html`, `htm`, `svg`, `xml`, all source code | text sniff |

Text validation at probe time (text and markdown modes): the probe reads the WHOLE file (the
size is first checked with `fstat` against `MAX_TEXT_BYTES`, so at most 2 MiB is read) inside
the 1 s probe budget and validates it completely: any NUL byte anywhere is `binary_file`, any
invalid UTF-8 anywhere is `invalid_utf8`, size over the cap is `too_large`. The 8192-byte
window is only the fast pre-check that rejects obviously binary files without reading the rest.
A failure therefore always becomes an explicit per-item error and never a tab with a
success-shaped `{ok: true}`. If the full read does not finish by the deadline the item is
`probe_timeout` (explicit, no tab). The probe's sha256 and size are handed to the attach as a
hint only; attach still reads and validates again (the file can change in between: that case is
the attach error view, not a probe result). Image and PDF modes are validated only by size
(`fstat` against `MAX_PREVIEW_BYTES`) and the signature check above; a file with a valid header
but corrupt body opens a tab and the native decoder reports the failure in the error state.
That is a documented deviation from "decode errors are per-item errors" (the Rust side has no
decoder and adding one would be a new dependency); it is recorded in the receipt and
`remaining_gates`. HTML and SVG open as editable text source because the browser
path rejects `file:` URLs and rendering SVG in a web view would execute embedded script;
rendering them is a deferred feature. `zmux markdown open` forces `markdown` for any file that
passes the text sniff (so `README` without an extension works) and rejects image/PDF/binary.
Media (mp3, mov, ...) fails the sniff and reports `binary_file` with "media preview is not
implemented"; there is no Quick Look in v1.

Mode is fixed at surface creation. The Markdown preview/source toggle is native view state only
(not persisted; restore always opens the Markdown preview).

### D5. File I/O lives in a Rust-owned bounded worker in desktop-runtime

New `FileService` (`crates/desktop-runtime/src/files.rs`) owning one OS thread `zmux-files`, a
bounded `sync_channel` of 8 jobs, and all per-surface state (D6). OS functions live in
`file_io.rs` behind a `FileBackend` trait (real implementation `RealFs`) so tests inject gated
and failing backends deterministically (explicit dependency injection, no globals).

- `workspace-core` stays I/O-free (`lib.rs:1`).
- The workspace executor never does file I/O. It only calls `FileService::reconcile(&model)` and
  `FileService::poll(&model)` (short mutex-guarded bookkeeping updates; the lock is never held
  across I/O, see D6a) and, for dirty observations, applies a core action.
- The control listener thread never does file I/O: it submits a probe job and waits at most
  `OPEN_PROBE_TIMEOUT` (1 s), then reports the unfinished items as `probe_timeout`.
- Content crosses to Swift only through the trusted bridge export (D15), never through
  `Snapshot`, `ControlResponse` or closed-item history.

Public injection point (needed because the acceptance tests live in external integration crates
and `DesktopOptions` today has no file field): `DesktopOptions` gains
`pub files: Option<FilesConfig>` (`None` = production defaults), with `FilesConfig`,
`FileBackend` and the value types its methods use exported `pub` from `zmux-desktop-runtime`:

```rust
#[derive(Clone)]
pub struct FilesConfig {
    pub backend: Arc<dyn FileBackend>,   // default: Arc::new(RealFs)
    pub poll_interval: Duration,         // default 1000 ms per attached entry (D14)
    pub poll_tick: Duration,             // default 250 ms worker tick; tests inject 5 ms
    pub stall_after: Duration,           // default 5 s (D21)
    pub probe_timeout: Duration,         // default 1 s (D16)
    pub executor_tick_hook: Option<Arc<dyn Fn() + Send + Sync>>, // default None; tests only
}
impl Default for FilesConfig { /* the production values above */ }
```

`executor_tick_hook` is called once per executor tick inside `FileService::poll`; its only
purpose is to let a test hold the executor deterministically so the bounded command queue can
be filled (D7a test). Production hosts leave it `None`. The same `FilesConfig` values feed
`capabilities.file_panel` (4.3), so the reported limits cannot drift from the enforced ones.
`FileBackend` is a narrow trait (walk-open, fstat, read-bounded, write-temp, rename, fsync,
unlink, stat-at) so a test backend can wrap `RealFs` and gate, count or fail individual calls.
A `CountingBackend` test double records every call; the read-only-socket test asserts zero
calls.

Rejected: Swift reads/writes with `FileManager`/`NSFileCoordinator` (violates "Rust owns
workspace/session logic", forks conflict logic, untestable on Linux, and a blocked read would
hit the UI thread); I/O on the executor (stalls every terminal frame); thread-per-request
(unbounded threads on a hung network volume); a third-party watcher crate (new dependency, see
D14).

### D6. Ownership and stale-view fencing

Modeled on `BrowserRuntime` (host UUID + restore epoch) without its receipt retention.

```text
FileOwner { epoch: Uuid, identity_namespace: Uuid, surface_id: u64, host_id: Uuid }
```

- `epoch` is random per Desktop and rotated by `FileService::reset` on session restore (the same
  place `browser.reset(...)` is called, `lib.rs:1560-1566`). It is exported in the snapshot
  (`files.epoch`).
- `host_id` is a UUID the native view generates for ONE view instance. `attach` binds it to the
  surface; attaching a different host to an owned surface is `host_conflict`.
- `reconcile(&model)` runs inside `Worker::publish` BEFORE the new snapshot is stored for
  readers (and also from the per-tick `FileService::poll`, D6a), so the registry is never older
  than any snapshot Swift has seen. A surface that
  disappears retires its owner into a ring of 256 (so late calls get `stale_owner`, not
  `unknown_surface`), drops its entry, and any in-flight job result is discarded.
- At most one operation in flight per surface (`busy` otherwise). A second queue bound: 8 jobs.
- Every state-changing call carries a full owner and is checked twice: at admission, and again
  on the worker immediately before the side effect that cannot be undone (the save rename,
  D13), so a view closed or replaced while its save waited in the queue never writes.
- Every call is assigned a monotonically increasing `op_id`, returned in the reply and recorded
  in the snapshot (`files.entries[].last_op`) so a caller that timed out can learn the outcome
  from later snapshots without ever replaying (D15, D21).

### D6a. How file-worker state reaches a published snapshot

Swift only sees a change when `revision` advances, and the executor tick advances it only when
its `dirty` flag is set (`Worker::run`, after the `dirty |= x.poll(...)` chain; `publish` does
`self.revision += 1`). Calling `FileService::reconcile` only inside `publish` would therefore
leave `changed`, `deleted`, `unreadable`, `stalled`, completed `last_op` and a resolved
`outcome_unknown` invisible until something unrelated published. So:

- `FileService::poll(&mut self, model: &Snapshot, now: Instant) -> bool` is called once per
  executor tick beside `downloads.poll` / `browser.reconcile`, as `dirty |= self.files.poll(
  &self.model.snapshot(), Instant::now())`. It (1) runs `reconcile` (retire owners and entries of
  vanished surfaces, discard their in-flight results), (2) compares the service's monotonically
  increasing `state_version` (bumped under the mutex by the file worker whenever an entry's
  `state`, `generation`, `disk_generation`, `size`/`mtime_ns`, `attached`, or `last_op` changes,
  or the worker state flips) with the last version this poll observed, and (3) recomputes stall
  age from `now` and the in-flight job start time. It returns `true` iff the published
  `FilesSnapshot` would differ from the previous one. It never does I/O and never holds the lock
  across anything but field reads.
- Stall age is quantized: while stalled, `stalled_ms` is reported in whole seconds and `poll`
  returns `true` only when the stalled flag flips or the whole-second bucket changes, so a hung
  volume costs at most one revision per second rather than one per 100 ms tick (`UPDATE_INTERVAL`).
- `Worker::publish` still calls `reconcile` (cheap, idempotent) before storing the snapshot, so a
  snapshot that Swift has seen is never newer than the registry, but the tick `poll` is what makes
  file-worker changes cause a publish. The executor never waits for the file worker: results flow
  worker -> mutex-guarded state -> `state_version` -> next tick.
- Latency from a worker state change to a visible snapshot is at most `UPDATE_INTERVAL`.

Test (S2, `crates/desktop-runtime/tests/files.rs`): `file_worker_changes_advance_the_revision`
asserts that with no other activity, an external modification observed by the poller, a delete,
a completed save `last_op`, and a stall flip each advance `Snapshot.revision` and appear in
`files.entries`, and that an idle service (nothing changing) does NOT advance the revision.

### D7. Dirty-state trust model: trusted-host-only observe, like `ObserveBrowser`

`Action::ObserveFile { surface_id, dirty }` sets `file.dirty` (text/markdown only). Public
paths reject it: `Command::Action{ObserveFile}` and therefore `ControlCommand::Action`
(`lib.rs:1574` pattern) and any socket client. The only producer is
`Command::FileHost { event }` (`#[serde(skip)]`), reached solely from the native bridge
(`FileHostEvent::Observed{owner, dirty}` / `Detach{owner}`), which validates the owner against
`FileService` on the bridge thread and again on the executor. Detach clears `dirty` (the view
that held the unsaved text is gone, so the bit would otherwise stick). Session restore and
`fresh_surface` always produce `dirty = false`. The native view changes its observation only on
a dirty-state transition (clean->dirty on first edit, dirty->clean after a save/discard/reload),
never per keystroke, so snapshot revisions do not churn. Delivery of that observation is
reliable by construction, see D7a.

The dirty bit lives in the core model (not only in the runtime) so the close guard (D8) holds
for EVERY caller of `apply`, including the socket and `close-workspace`.

Rejected: dirty only in `FileService` (core could not refuse closes, so each of five close
paths would need a runtime pre-check on the executor); trusting socket-supplied dirty (any local
client could clear it and lose a user's edits); a per-keystroke `observe` (revision churn).

### D7a. The dirty bit is a reconciled mirror, not an event; the view-local bit is the truth

The native view holds the unsaved text, so the VIEW-LOCAL dirty state (text differs from the
last saved/loaded text) is the source of truth. The model's `file.dirty` is only a guard for
callers that cannot see the view (socket `close-surface`, `close-workspace`, `close-window`).
`observe` can fail transiently (the executor queue is `sync_channel(MAX_COMMANDS = 32)`, and
the bridge maps `DesktopError::QueueFull | Busy` to `BUSY`, as for browser events), so it is
specified as an idempotent absolute-state write that is retried until the model agrees:

1. `observe(owner, dirty)` sets `file.dirty` to the stated value; repeating it with the same
   value is a no-op that returns `ok` and does not advance the revision. It is therefore EXEMPT
   from the "never replay" rule of D15 (that rule stays for `save`, `open` and `reload`).
2. The view runs a reconciliation loop: on every snapshot it compares its local dirty state with
   `snapshot.surface.file.dirty` for its surface. While they differ, and no observe is in flight,
   it issues `observe(local)`. A failure that is retryable (`BUSY`, `CAPACITY`, `timeout`,
   `busy`) is retried with bounded exponential backoff (250 ms, doubling to a 2 s cap, no jitter
   needed, one observe in flight per view) for as long as the states differ; there is no retry
   count limit because a lost observation is a data-loss hazard, but the RATE is bounded. A
   non-retryable error (`stale_owner`, `unknown_surface`, `host_conflict`, `closed`) stops the
   loop and takes the orphan path in point 4. Success is confirmed by the next snapshot, not by
   the ack alone.
3. Native close, Close Tab/Pane/Workspace/Window and Quit decide from the VIEW-LOCAL bit and the
   views' own list of dirty files, never from the snapshot. They also first force a
   reconciliation (`observe(local)` and wait for success within the 2 s bridge wait) before
   asking the core to close, so a core close issued by the app itself cannot race a pending
   observe. A native close that cannot make the model agree within the wait aborts with an
   error sheet ("could not record unsaved changes; try again"), it does not close.
4. A view whose surface disappears from the snapshot (any path that bypassed the guard: a
   non-native caller that won the pre-ack race below, a restore) while the view is locally
   dirty is NOT pruned. It moves to an orphan list retained by the coordinator, and the app
   shows a sheet with "Copy Text" (to the pasteboard) and "Discard". Quit also prompts while an
   orphan exists. Orphans are in-memory only; there is no Save As in v1 (D12), so Copy Text is
   the recovery path. A clean view whose surface disappears is pruned as usual.

Residual window, stated honestly: between the first edit and the acknowledged `observe(true)`
(normally milliseconds; unbounded only while the executor queue stays full) a socket
`close-surface` sees `dirty = false` and succeeds. Point 4 limits the damage to "text is
recoverable via Copy Text" instead of "text is lost". This window and the Swift-only parts
(points 2-4 are UNAVAILABLE to run here) are recorded in the receipt.

Rust-side tests (S2, `crates/desktop-runtime/tests/files.rs` and `crates/native-bridge/tests/files.rs`):
`observe_is_idempotent_and_does_not_advance_revision_when_unchanged`;
`observe_true_then_false_converges_after_queue_full` (uses `FilesConfig::executor_tick_hook` to
hold the executor, floods the 32-slot queue so the next `observe(true)` returns `QueueFull` /
`BUSY` with the model unchanged, asserts `close-surface` still SUCCEEDS in that window per the
documented residual, then releases the hook, retries `observe(true)` and asserts `file.dirty` is
true and that `close-surface`, `close-workspace` and `close-window` are now refused with the
dirty message and leave the store untouched); `retried_observe_after_stale_owner_is_not_applied`;
and the bridge test `observe_returns_busy_not_ok_when_queue_full`. The Swift reconciliation,
backoff and orphan flow are specified for S4 and recorded UNAVAILABLE.

### D8. Close/reopen policy for dirty files

- Core `ensure_closeable(ids)` (new, called beside every `ensure_settled` in the five close
  paths) fails with `InvalidOperation("file has unsaved changes; save or discard it in the app")`
  while any affected file surface is dirty (the model bit, kept honest by D7a). The store is untouched (atomic `apply`). `zmux
  close-surface` on a dirty file therefore fails with a clear message; there is no CLI force
  flag in v1 (rejected: a force flag is a data-loss footgun; the user resolves it in the app).
- Native flow for Close Tab/Pane/Workspace/Window and Quit with dirty files: the list of dirty
  files comes from the VIEW-LOCAL dirty state of the affected `FileView`s (D7a), never from the
  snapshot; per dirty file show Save / Don't Save / Cancel. Save = bridge `save`, then `observe(dirty:false)`, then
  close; Don't Save = `observe(dirty:false)` then close; Cancel aborts the whole operation. A
  failed save aborts the close. Quit does the same before session save.
- Because a dirty file cannot be closed without a decision, the closed-item history can never
  hold unsaved text. `ReopenClosed` therefore only needs path + mode: `fresh_surface` copies
  `file` with `dirty = false` and the view reloads from disk when it attaches. The closed list
  stays in memory only (unchanged).
- Reopen of a file whose path is open elsewhere now fails explicitly (D18).

### D9. Persisted-path privacy

Persisted: absolute canonical `path`, `mode`, and the tab title, in `session.json` inside the
private state directory (mode `0600`, owner-checked by `persistence.rs` on both read and write).
This is the same class of data as the terminal `cwd` already persisted (`SavedContent::LocalTerminal`).
Never persisted: file content or hash, unsaved text, dirty bit, external-change/error state,
viewport, selection, zoom, Markdown source/preview toggle. Closed-item history is in memory only.
Runtime error text returned to callers names the path only because the caller supplied it; the
service writes no path or content to stderr/logs. `list-pane-surfaces` exposes `file.path` just
as it exposes `cwd`/`url` (same-uid socket; read-only sockets already read those fields).
Session export carries file paths like cwd; no secrets or contents are exported.

Rejected: persisting only a hash or an opaque bookmark (impossible to restore without the path;
macOS security-scoped bookmarks apply to a sandbox this app does not use); omitting the path from
persistence (restore would silently drop tabs).

### D10. Version bumps

- `MODEL_VERSION` 6 -> 7 (`lib.rs:50`), and Swift `NativeBridge.swift:133` `== 6` -> `== 7` in
  the SAME commit as the Rust bump (S1) so no intermediate tree has a mismatched app.
- `SESSION_DOCUMENT_VERSION` 9 -> 10. `SavedContent::File { path, mode }` (tag `"file"`) is
  accepted only for `version >= 10` ("file surfaces require session version 10", mirroring the
  gating style of versions 3/6/7). All older versions still load. Capture always writes version
  10. Update the module doc comment at the top of `restore.rs` ("V10 adds file surfaces").
- `RestoreLimits` gains `files` and `ValidatedSessionDocument` a `file_count()`; fed from
  `MAX_FILE_SURFACES` in `sessions.rs:90-96`.
- Native bridge `ABI_VERSION` stays 1: one additive export (`zmux_desktop_file`, D15).
- `CONTROL_VERSION` stays 2: the new `open_items` operation is an additive variant. A new CLI
  against an old app gets that app's `invalid_request` (unknown variant) error; no replay.

### D11. Caps and limits

See the table in section 5. Principles: every queue and size is bounded and named; overload is
an explicit error (`busy`, `file_capacity`, `file_worker_unavailable`, `too_large`), never a
silent drop or an unbounded queue; one global cap on file surfaces beside `MAX_BROWSERS`.

### D12. Conflict detection and fingerprint algorithm

A fingerprint is `{ dev, ino, size, mtime_ns, ctime_ns }` (cheap, from `fstat`) plus a content
`sha256` (hex). The service keeps ONE baseline per attached surface:
`Baseline { generation: u64, sha256, size, mtime_ns, dev, ino, bom: bool }`.

- `generation` starts at 0, becomes 1 on the first successful read, and increments on every
  successful read (attach/reload) or save. The native view remembers the generation of the text
  it shows.
- The conflict criterion is CONTENT, not mtime: save proceeds without `force` only if the file
  on disk is present and its SHA-256 equals `baseline.sha256`. Metadata is used only to skip
  re-hashing in the poller and for the narrow pre-rename re-check. A `touch`, or an editor that
  rewrites identical bytes or replaces the inode with equal content, is NOT a conflict.
- `save` is fenced by owner AND by `base_generation == baseline.generation`; a mismatch is
  `stale_view` (the view is behind a reload it did not take) and writes nothing.
- A conflict reply is `conflict` with `kind: "modified" | "deleted"` and the disk size/sha so
  the sheet can say what changed. Resolutions available in v1: Overwrite (`save` with
  `force: true`, still owner- and generation-fenced), Reload (discard mine), Cancel (keep
  editing). Save As and merge are deferred; copying the text out is the workaround.
- `force` after a delete recreates the file if the parent directory still exists.

Residual window, stated honestly: between the final metadata re-check and `renameat` there is no
atomic compare-and-swap in POSIX; a writer that lands in those microseconds is overwritten.
Advisory locks are not used (most editors ignore them). The check is performed as late as
possible to keep the window minimal; this limitation is also recorded in the receipt.

### D13. Atomic save mechanics

`file_io::save(dir, name, bytes, expect, force)`, all anchored to the parent directory fd:

1. Walk to the parent directory (NOFOLLOW). Fresh `fstatat(name, NOFOLLOW)` => `D` (missing or
   metadata). If `D` exists and `nlink > 1`: `hard_linked` (an atomic replace would silently
   detach the other names); not written. If `D` is not a regular file: `not_regular_file`.
2. Unless `force`: missing => `conflict(deleted)`; present => read at most `MAX_TEXT_BYTES + 1`
   bytes, hash, differ from baseline => `conflict(modified)` (a file that grew past the cap is
   also `conflict(modified)` since equality cannot be proven).
3. Create temp `.zmux-save-<uuid>.tmp` in the same directory, `O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC`, mode
   `0600` initially; write all bytes (re-adding the UTF-8 BOM if the baseline had one); `fchmod`
   to the original permission bits (`mode & 0o7777`, or `0o644` for a file that did not exist);
   `fsync`. Ownership, ACLs and extended attributes are NOT preserved (documented limitation).
4. Re-fence on the worker (D6): owner still current and entry live, else unlink temp, reply
   `stale_owner`, nothing written.
5. Unless `force`: `fstatat` again; if `(dev, ino, size, mtime_ns, ctime_ns)` differ from `D`,
   unlink temp, reply `conflict(modified)`.
6. `renameat(temp -> name)`, then `fsync` the directory. A failure before the rename unlinks the
   temp and reports the error (known not written). A directory-`fsync` failure after the rename
   reports `sync_failed` with `outcome_unknown: true`; the baseline is already updated to the
   visible content, so a retry is a safe no-op-equivalent save.
7. Success updates the baseline (`generation + 1`, new sha/size/mtime/dev/ino) under the service
   mutex and returns them.

If the directory is not writable or the temp cannot be created the error is
`directory_not_writable` / `permission_denied`; there is NO in-place fallback write (it would
make torn writes possible and defeat conflict safety). Text and Markdown only; the body must be
valid UTF-8 (`invalid_utf8`) and at most `MAX_TEXT_BYTES`. The service never converts line
endings or normalizes Unicode: bytes in = bytes out (the BOM is the only transformation, and it
is lossless). Native text components must not normalize line endings either.

### D14. Live reload: bounded polling, no new dependency

There is no inotify/FSEvents/`notify` in the tree and adding one needs `docs/dependencies.md`,
`scripts/license-inventory` and `THIRD_PARTY_NOTICES.md` changes plus a macOS-only FSEvents
owner. Polling reuses the proven `download_paths.rs` shape and is testable on Linux.

- The worker polls ATTACHED entries only, interval `POLL_INTERVAL = 1000 ms` per entry, at most
  4 `fstatat` per 250 ms tick, at most 16 entries. Jobs always run before polls. The interval
  and tick are `FilesConfig.poll_interval` / `poll_tick` (D5), which is how tests run it at
  20 ms / 5 ms. Poll results reach Swift through `FileService::poll` and the revision (D6a).
- A poll compares the cheap fingerprint to the last seen one. On a change it waits for the
  metadata to be stable across one more poll (coalesces a writer mid-write, 1-2 s total
  latency), then reads (bounded by the mode cap), hashes, and compares with `baseline.sha256`.
  Equal => silently adopt the new metadata (touch, identical rewrite). Different => `changed`
  and `disk_generation += 1` (a second external write while a banner is up is visible).
- ENOENT must repeat on two consecutive polls to become `deleted` (an editor that deletes then
  recreates, or an atomic replace in progress, is not reported as deleted).
- EACCES, a non-regular replacement, or a size over the cap => `unreadable` with a code.
- States: `unattached | in_sync | changed | deleted | unreadable`. They are published in the
  snapshot (`files.entries[]`, section 4.5), never the bytes.
- Policy executed by the native view: clean + `changed` => automatic `reload`; dirty + `changed`
  => non-modal banner (Reload and discard / Keep mine); a later save without `force` reports the
  authoritative `conflict`. Rust enforces the dirty rule too: `reload` while the model says
  dirty is `dirty` unless `discard: true`.
- Preview modes (image/PDF) poll identically and reload automatically (they have no dirty
  state). A same-size, same-mtime rewrite within the filesystem timestamp granularity can be
  missed by the cheap fingerprint; a manual Reload covers it. Noted in the receipt.

### D15. Bridge export, framing and payload limits

One new trusted native-only export, additive to ABI v1 (`crates/native-bridge/include/zmux.h`,
`docs/native-bridge.md`, `tests/abi.c` symbol reference):

```c
/* Trusted native file channel. Framed request/response, see below. Off the UI thread; waits at
   most 2 s (reads/attach/reload/open/observe) or 5 s (save). A nonzero status, a timeout, or
   outcome_unknown=true must NOT be replayed automatically, EXCEPT `observe`, which is an
   idempotent absolute-state write and is retried until the model agrees (D7a). */
ZmuxResult zmux_desktop_file(uint64_t desktop, const uint8_t *request, size_t length);
```

Frame (both directions): `u32 big-endian header_len | header JSON (UTF-8) | body bytes`.
`header_len` must satisfy `4 + header_len <= length`; the body is the remaining bytes and is
raw (no JSON escaping, so a 2 MiB text save is not inflated up to sixfold by escaping and an
image/PDF needs no base64). Request header at most 16384 bytes. Request total: at most 16384
bytes for every op except `save`, whose total is at most `16384 + 2097152` (named
`MAX_FILE_SAVE_FRAME_BYTES`), the same "one strict command may exceed the ordinary cap" rule the
paste envelope uses. Response total at most the existing 8 MiB buffer cap; the largest legal
reply is `4 + 16384 + 7340032`, which fits. `Reservation::new()` is taken FIRST (before any
queueing), exactly as the download/transfer exports do; a `CAPACITY` result means nothing ran.

C status codes follow the existing set: `INVALID_ARGUMENT` for a malformed/oversized frame,
`INVALID_HANDLE`, `CAPACITY` (no buffer), `BUSY` (registry lock), `INTERNAL`. Every other
failure is an `ok:false` JSON reply with status 0.

Ops (headers in section 4.4): `attach`, `reload`, `save`, `observe`, `detach`, `open`. Blocking
waits are on the calling native thread only (the executor and file worker are never blocked by
the caller). `open` is the same pipeline as `zmux open` targeted at the focused pane and exists
for `File > Open...` and for native bridge tests.

Rejected: putting file bytes through `zmux_desktop_dispatch` JSON (16 KiB cap, escaping
inflation); base64 bodies (33% overhead for no benefit); a streaming/chunked protocol (unneeded
at these sizes; deferred); a second export per op (six exports to document and version).

### D16. Control-socket route and probe timing

`TargetOperation::OpenItems { items, focus, force_mode }` (additive variant of the existing
targeted request, `desktop-runtime/src/targets.rs`) so scope/caller/`expected_model_revision`
resolution is reused. In `control.rs` the `Targeted` arm handles `OpenItems` before the generic
`mutate`:

1. `gate.admit` has already run; `is_read` is false, so read-only sockets are rejected before any
   file is touched (no existence oracle for read-only clients).
2. Validate the request (at most 16 items, bounds, `deny_unknown_fields`).
3. `Desktop::probe_open_items(items, force_mode, OPEN_PROBE_TIMEOUT)`: file items are probed
   in order on the file worker; items not finished by the deadline become `probe_timeout`.
4. `mutate(Command::OpenFiles { request, probes })` (`#[serde(skip)]`; only constructible in
   process). The executor resolves target + revision ONCE, then applies items strictly in order,
   each as its own core transition, building `open_result`.
5. A raw `OpenItems` arriving through `Command::Targeted` (e.g. via the generic bridge dispatch)
   is rejected: "open requires the control-socket probe path".

Budget: probe 1 s + executor 2 s, so the CLI's default timeout for `open` is 5 s (explicit global
`--timeout` still wins, as for browser waits). The listener thread is blocked at most about 3 s,
the same order as other mutations.

`Action::CreateFile` is likewise rejected on public action paths (`Command::Action` and
`ControlCommand::Action`): the path must come from a probe. Only `Command::OpenFiles` (and the
bridge `open` op, which reaches the same function) construct it.

### D17. CLI grammar, results and exit status

```text
zmux open [TARGET_OPTIONS] [--focus true|false | --no-focus] [--] PATH_OR_URL...
zmux markdown open [TARGET_OPTIONS] [--focus true|false | --no-focus] [--] PATH...
TARGET_OPTIONS: --window H  --workspace H  --pane H  --surface H (or --tab H)  --expect-revision N
```

- One to 16 operands. Options may precede operands; `--` makes later operands literal. A
  leading `-` operand without `--` is an unknown-option error (same rule as `targets::parse`).
- Target defaults follow existing rules (`ZMUX_WORKSPACE_ID`, `ZMUX_SURFACE_ID`,
  `ZMUX_TAB_ID`, managed tmux); with `--surface`, tabs go in that surface's pane. New tabs are
  appended to the end of the target pane in operand order (as `browser open` appends).
- Focus: default true. With focus, only the FIRST successful item (new or deduplicated) is
  selected and the pane focused; later items open unfocused. `--focus false` / `--no-focus`
  leaves selection unchanged. Repeated or conflicting focus flags are errors (reuse
  `extract_browser_focus` semantics).
- `markdown open` accepts paths only (a URL operand is `invalid_operand`) and sends
  `force_mode: "markdown"`.
- Output: the server `result` pretty-printed on stdout (shape in 4.3). Exit status 0 iff
  `result.ok` is true (every item succeeded; a deduplicated item is a success). If any item
  failed, stdout still carries the full per-item JSON and exit status is 1 with
  `zmux: N of M items failed` on stderr. A whole-request failure (stale revision, unknown
  target, transport) prints the usual `zmux: code: message` and exits 1. Nothing is retried.

A new `Parsed::Open(ControlCommand)` variant (own exit-status handling in a new `apps/cli/src/
open.rs`), added to the `parsed()` allow-list in `registry/contracts.rs` with route `"open"`.

### D18. Dedupe: one live surface per canonical path

Opening a path that is already open anywhere in the desktop does not create a second tab (two
editors with independent dirty buffers would defeat conflict detection). The executor finds the
existing surface, and if focus applies issues `SelectSurface`; the item reports
`ok:true, deduplicated:true` with that surface's receipt and its actual `mode` (a forced
`markdown` request does not change an existing text tab). Core enforces uniqueness as an
invariant (second line of defence); `ReopenClosed` of a tab whose path is currently open fails
with "file is already open in another tab" (rejected alternative: silently focusing it, which
would make a multi-tab reopen partially succeed). Case-insensitive volumes: dedupe is by exact
canonical string; `realpath` normally returns on-disk case on macOS but this is unverified, so
case-variant aliases are listed as a known gap.

### D19. `zmux markdown open` is included

Rationale: CMD-114 is its own inventory row, the cost is one registry entry, one parse arm, one
contract leaf and a `force_mode` field on an operation that already exists, and without it
CV-01's "CLI shorthand" gap stays open for no engineering reason. It is mapped to the local
contract `markdown open` with status `local_counterpart_compatibility_unverified`;
`behavioral_parity_verified` stays false. `open` is mapped to local contract `open` the same way.

### D20. Native composition (system frameworks only)

One `FileView` container (NSView) per file surface, retained by the coordinator in a
`files: [UInt64: FileView]` dictionary exactly like `browsers`, created/pruned in `reconcile`,
re-parented (never recreated) across windows. Modes:

- Text and Markdown-source: `NSScrollView` + `NSTextView` (monospaced system font, plain text,
  undo enabled, automatic quote/dash/text-replacement/spell substitution OFF so typing never
  alters bytes, editable only when the attach reply says `writable`). Accessibility label
  "File editor: <name>".
- Markdown preview: parsed off the main thread with `AttributedString(markdown:)`
  (`interpretedSyntax: .full`) and mapped to fonts/paragraph styles in our own code, shown in a
  non-editable selectable `NSTextView`. No `WKWebView`, so there is no script surface and no
  second web configuration. Newest parse wins (generation counter). Preview shows the current
  buffer, including unsaved edits. Links: only `http`, `https` and `mailto` open (via
  `NSWorkspace`); everything else is inert text. Tables, images, task lists and raw HTML degrade
  to plain text (known gap).
- Image: `NSImageView` in a scroll view, fit-to-window, decoded off the main thread through
  ImageIO with a 100 megapixel guard (`image_too_large` state), no zoom in v1.
- PDF: PDFKit `PDFView` over `PDFDocument(data:)`, continuous, auto-scale; no thumbnails/outline.
- Error/unavailable state (not found, permission, too large, binary, `path_changed`,
  `stalled`): centered label with the stable error code and a Retry button (re-`attach`).
- A thin top bar shows the external-change/conflict banner and the Markdown Preview/Source
  segmented control.

Menus: a new File menu with `Open File...` (Cmd-O, `NSOpenPanel`, then bridge `open`) and
`Save` (Cmd-S, first-responder action), and `Reload from Disk` (no shortcut; Cmd-R stays the
browser's). The Edit menu's Copy/Paste/Select All currently call custom coordinator selectors;
they must forward to the responder chain when a `FileView` text view is first responder, and
Undo/Redo/Cut are added through standard `NSText` selectors.

### D21. Overload and recovery are explicit

- Worker queue full => `busy` (nothing ran). Per-surface in-flight => `busy`.
- Worker stalled: if one job has been running longer than `STALL_AFTER = 5 s`, `files.worker`
  reports `stalled` with `stalled_ms`, and new jobs/probes fail immediately with
  `file_worker_unavailable` instead of queueing behind a hung volume. The state clears when the
  job returns. The thread cannot be killed; shutdown reports "IO may still be running" exactly
  like `PathObservers::shutdown` if it does not return within the deadline.
- Timeouts: reads/probes 2 s => `timeout` (outcome known: nothing delivered, safe to retry);
  saves 5 s => `timeout` with `outcome_unknown: true` and the `op_id`. The worker still
  finishes, updates the baseline under the mutex, and records `last_op.state`; the view learns
  the result from the snapshot and then `reload`s or re-saves. Never replayed automatically.
- Session restore bumps the epoch; all old owners get `stale_owner`; views re-attach.
- Shutdown: `FileService::request_shutdown/is_finished/shutdown(deadline)` joined from
  `shutdown.rs`. Pending jobs are dropped with `closed`; a save already past its rename point
  completes.

### D22. Licensing and dependencies

System frameworks only (AppKit, Foundation, PDFKit, ImageIO, UniformTypeIdentifiers if needed);
Rust: existing `sha2`, `rustix`, `uuid`, `serde_json`. No new crate, no bundled asset, no
cmux-derived code. `scripts/license-inventory --check` and `THIRD_PARTY_NOTICES.md` must stay
unchanged; if either changes, a dependency slipped in and must be justified or removed.

### D23. Native presentation invariants

Dirty/mode are NOT part of `PaneTree.key`; the tab control updates its dirty marker in place from
each snapshot (a pane rebuild on the first keystroke would resign first responder). File views
are never recreated by model changes, only by surface creation. `paneView` gets an explicit
`kind == "file"` branch and the old "everything else is a browser" fallthrough becomes an explicit
`kind == "browser"` test with a visible error label for unknown kinds. Tab icon: `doc.text` /
`doc.richtext` / `photo` / `doc` by mode. Close-confirmation copy in `confirmClose` (currently
"closes N terminal session(s)") must count only terminals and handle dirty files per D8.

### D24. Remote workspaces

A file surface always names a file on THIS Mac, even inside an SSH workspace. Remote file open
is not in scope (belongs to R02). Documented in `zmux open --help` and the receipt.

## 4. Exact wire and JSON shapes

All JSON is UTF-8, `snake_case`, and `deny_unknown_fields` on every inbound type.

### 4.1 Workspace model (`Snapshot`, `Action`, session document)

Surface JSON (additive; every other surface has `"file": null`):

```json
{"id": 7, "kind": "file", "session_id": null, "url": null, "navigation": 0,
 "browser_profile_id": null, "title": "README.md", "cwd": null, "remote": null,
 "local_tmux": null, "local_zellij": null, "local_zellij_completion": null,
 "file": {"path": "/Users/a/proj/README.md", "mode": "markdown", "dirty": false}}
```

Actions (`tag = "action"`, internal/trusted, rejected on public paths per D7/D16):

```json
{"action": "create_file", "pane_id": 3, "path": "/abs/x.rs", "mode": "text", "focus": true}
{"action": "observe_file", "surface_id": 7, "dirty": true}
```

`create_file` errors: `InvalidOperation` for a bad path/mode/duplicate, `Capacity{tabs per pane,32}`.
`observe_file` errors: unknown surface, not a file surface, `dirty:true` on image/pdf.

Session document v10 surface content (title persisted as for other kinds):

```json
{"id": 7, "title": "README.md", "content": {"kind": "file", "path": "/abs/README.md", "mode": "markdown"}}
```

### 4.2 Control request (CLI -> app), `CONTROL_VERSION` 2

```json
{"version": 2, "request_id": 41, "command": {"method": "targeted", "request": {
  "scope": {"window": null, "workspace": null, "pane": null, "surface": null},
  "caller": {"window": null, "local_tmux": null, "workspace": "workspace:1", "surface": "surface:4"},
  "expected_model_revision": null,
  "operation": {"operation": "open_items", "focus": true, "force_mode": null,
    "items": [{"kind": "path", "value": "/abs/README.md"},
              {"kind": "url",  "value": "https://example.org/docs"},
              {"kind": "path", "value": "/abs/src"}]}}}}
```

`items[].kind` is `"path" | "url"`; `force_mode` is `null | "markdown"`. At most 16 items; the
whole encoded request must stay within the existing 16384-byte cap (the CLI checks before
connecting and reports "split the operands across invocations").

### 4.3 Control response (`ControlResponse.result`)

The standard mutation receipt (`created_surfaces`, `focused_surface`, `focused_*_uuid`,
`model_revision`, `revision`, ...) plus:

```json
{
  "ok": false,
  "items": [
    {"index": 0, "kind": "file", "ok": true, "path": "/abs/README.md", "mode": "markdown",
     "deduplicated": false,
     "surface": {"window_id": 1, "window_uuid": "...", "workspace_id": 2, "workspace_uuid": "...",
                 "pane_id": 3, "pane_uuid": "...", "surface_id": 9, "surface_uuid": "..."}},
    {"index": 1, "kind": "url", "ok": true, "url": "https://example.org/docs", "surface": {"...": "..."}},
    {"index": 2, "kind": "file", "ok": false, "path": "/abs/src",
     "error": {"code": "is_directory", "message": "directories cannot be opened; ..."}}
  ]
}
```

`result.ok` is true iff every item is `ok`. `surface` appears on every `ok` item. `deduplicated`
appears on file items. Item `error.code` is drawn from: `invalid_operand`, `unsupported_scheme`,
`invalid_url`, `not_found`, `permission_denied`, `is_directory`, `not_regular_file`, `too_large`,
`binary_file`, `invalid_utf8`, `content_mismatch`, `path_not_utf8`, `io_error`, `probe_timeout`,
`file_worker_unavailable`, `busy`, `file_capacity`, `browser_capacity`, `tab_capacity`. Whole-request failures
(`stale_revision`, `invalid_target`, `unauthorized`, `read_only`, `request_too_large`, ...) are
ordinary control errors with no `items`. Message text never contains file content.

`list-pane-surfaces` surface entries gain `"file": null | {path, mode, dirty}`; `current` gains
nothing but already reports `kind`. `capabilities` gains:

```json
"file_panel": {"max_file_surfaces": 16, "max_open_items": 16, "max_text_bytes": 2097152,
               "max_preview_bytes": 7340032, "poll_interval_ms": 1000, "open_probe_timeout_ms": 1000}
```

### 4.4 Native bridge `zmux_desktop_file` headers

`Owner` = `{"epoch": "<uuid>", "identity_namespace": "<uuid>", "surface_id": 9, "host_id": "<uuid>"}`.

Requests (header JSON; `body` only for `save`):

```json
{"op": "attach",  "epoch": "<uuid>", "surface_id": 9, "host_id": "<uuid>"}
{"op": "reload",  "owner": {...}, "discard": false}
{"op": "save",    "owner": {...}, "base_generation": 3, "force": false}      // body = UTF-8 text
{"op": "observe", "owner": {...}, "dirty": true}
{"op": "detach",  "owner": {...}}
{"op": "open",    "items": [{"kind": "path", "value": "/abs/x.md"}], "focus": true}
```

Replies (header JSON; `body` only for successful `attach`/`reload`, the file bytes):

```json
{"ok": true, "op_id": 12, "owner": {...}, "mode": "markdown", "size": 4096, "generation": 1,
 "sha256": "<64 hex>", "mtime_ns": 1760000000000000000, "writable": true, "bom": false}
{"ok": true, "op_id": 13, "generation": 4, "sha256": "<64 hex>", "size": 4100, "mtime_ns": 1760000001000000000}
{"ok": true, "revision": 88}                                   // observe, detach
{"ok": true, "result": { /* same object as 4.3 result */ }}    // open
{"ok": false, "op_id": 13, "outcome_unknown": false,
 "error": {"code": "conflict", "message": "file changed on disk since it was loaded"},
 "conflict": {"kind": "modified", "disk_sha256": "<64 hex>", "disk_size": 4101}}
```

`writable` is a hint (`accessat W_OK` on the file and its directory); save errors are
authoritative. Error codes in addition to section 4.3's: `unknown_surface`, `stale_owner`,
`host_conflict`, `busy`, `timeout`, `dirty`, `stale_view`, `conflict`, `hard_linked`,
`directory_not_writable`, `path_changed`, `sync_failed`, `invalid_request`, `closed`.
`outcome_unknown` is true only for a save/sync whose result could not be confirmed.

### 4.5 Desktop snapshot addition (never content)

```json
"files": {
  "epoch": "<uuid>",
  "worker": {"state": "ok", "stalled_ms": 0},
  "entries": [
    {"surface_id": 9, "state": "changed", "attached": true, "generation": 3, "disk_generation": 4,
     "size": 4101, "mtime_ns": 1760000002000000000,
     "last_op": {"op_id": 13, "kind": "save", "state": "outcome_unknown", "code": "timeout"}}
  ]
}
```

`state` in `unattached|in_sync|changed|deleted|unreadable`; `last_op.state` in
`pending|completed|failed|outcome_unknown`; at most 16 entries and one `last_op` each. Swift
decodes with `convertFromSnakeCase`, so these map directly.

## 5. Caps and limits

| Name | Value | Owner / enforcement point |
| --- | --- | --- |
| `MAX_FILE_SURFACES` | 16 desktop-wide | `desktop-runtime/src/lib.rs` candidate check beside `MAX_BROWSERS`; `RestoreLimits.files`; `capabilities` |
| `MAX_SURFACES_PER_PANE` | 32 (existing, shared with terminals/browsers) | core `check_tab_capacity` |
| `MAX_OPEN_ITEMS` | 16 per request | CLI parse + server validation |
| Control request / response | 16384 / 1 MiB (existing) | CLI pre-check, `control.rs` |
| Path | 4096 bytes UTF-8, no control/NUL | CLI, probe, core, restore |
| Title | `MAX_NAME_CHARS` 128 | core |
| `MAX_TEXT_BYTES` | 2,097,152 (text + markdown, read and save) | `file_io` |
| `MAX_PREVIEW_BYTES` | 7,340,032 (image + pdf read) | `file_io` |
| Sniff window (fast pre-check only) | 8192 bytes | `file_io::classify` |
| Probe full validation (text/markdown) | whole file, at most `MAX_TEXT_BYTES`, inside the 1000 ms probe budget | `file_io::probe` |
| `FilesConfig` | backend, `poll_interval`, `poll_tick`, `stall_after`, `probe_timeout`, `executor_tick_hook`; defaults equal the values in this table | `DesktopOptions.files` (pub) |
| Observe retry (native) | backoff 250 ms doubling to 2 s cap, 1 in flight, until the snapshot agrees | Swift `FileView` (D7a) |
| Bridge request header | 16384 bytes | `native-bridge` |
| Bridge request total | 16384; `save` 16384 + 2,097,152 | `native-bridge` |
| Bridge response total | 8,388,608 (existing); largest file reply 4 + 16384 + 7,340,032 | `Reservation` |
| Bridge buffers | 16 live (existing) | `Reservation::new()` |
| File worker queue | 8 jobs, 1 in flight per surface | `FileService` |
| Probe deadline (`open`) | 1000 ms total | control path |
| Bridge waits | read/attach/reload/open/observe 2000 ms; save 5000 ms | `Desktop` file methods |
| Worker stalled threshold | 5000 ms | `FileService` |
| Poll | 1000 ms per attached entry; 4 stats per 250 ms tick; 16 entries; settle 1 extra poll; delete confirm 2 polls | worker |
| Retired-owner ring | 256 | `FileService` |
| Snapshot `files.entries` | 16 + one `last_op` each | `FileService::snapshot` |
| Save temp name | `.zmux-save-<uuid>.tmp` in the target directory | `file_io::save` |
| Native image decode | 100 megapixels | Swift |
| Native Markdown parse | text cap (2 MiB); newest-wins, off main thread | Swift |

## 6. Slices

Order is strict (S2 needs S1, S3 needs S2, S4 needs S2, S5 last). Each slice: red `test:` commit
first where Linux can express the behaviour, then the `feat:` commit; run `scripts/check` (tests,
fmt, clippy `-D warnings`, `license-inventory --check`, `check-native-bridge`) before every
commit with a 600000 ms timeout; commit only green; stage only files changed; lowercase
conventional messages; never bypass hooks; no push (Deliver phase only).

### S1 core-model

Goal: file surfaces exist, persist, restore and cannot be closed dirty, with no I/O.

Files:
- `crates/workspace-core/src/lib.rs`: `MODEL_VERSION = 7`; `Action::CreateFile`, `Action::ObserveFile`; re-exports `FileMode`, `FileSurface`.
- `crates/workspace-core/src/surfaces.rs`: `SurfaceKind::File`, `FileMode`, `FileSurface`, `Surface.file`, all literals, `CreateFile`/`ObserveFile` arms, `ensure_closeable` in the five close paths, `fresh_surface` (`dirty=false`), reopen duplicate-path error.
- `crates/workspace-core/src/local_zellij.rs`: extend `validate_terminal_descriptors`/`validate_terminal_descriptor` with file-shape and path-uniqueness rules.
- `crates/workspace-core/src/restore.rs`: `SESSION_DOCUMENT_VERSION = 10`, `SavedContent::File`, version gate, validation, capture/restore arms, `RestoreLimits.files`, `file_count()`, doc comment.
- `crates/desktop-runtime/src/lib.rs`: `MAX_FILE_SURFACES`, candidate-transition cap check, reject `Action::CreateFile`/`ObserveFile` in `Command::Action`; `sessions.rs` feeds the limit.
- `crates/desktop-runtime/src/control.rs`: `list_surfaces` adds `file`; `inspection.rs` prints kind `file` (no terminal session).
- `apps/macos/Sources/NativeBridge.swift` (`== 7`), `Models.swift` (`Surface.file`, `FileSurface`), `App.swift` (explicit `kind` dispatch: a `file` surface shows a "File panel is not available in this build" label until S4; unknown kinds show an error label instead of becoming a browser), `WorkspaceViews.swift` (icon). UNAVAILABLE to compile here.

Acceptance:
- `create_file` appends a file tab without any `StartSession`/`StopSession` effect, honours `focus`, rejects relative/`..`/control/oversize/NUL paths, duplicate canonical paths, and a full pane.
- `observe_file` sets/clears dirty only on text/markdown file surfaces; rejects terminals, browsers, image/pdf.
- CloseSurface, CloseOtherSurfaces, ClosePane, CloseWorkspace and CloseWindow fail atomically (revision and store unchanged) while any affected file is dirty, and succeed once clean.
- Close then `ReopenClosed` yields the same path/mode with `dirty=false` and a fresh surface id; reopen fails explicitly if the path is open.
- Session document: v10 round-trips file surfaces; v9 documents with a `file` content are rejected with the version message; every older document still loads; `dirty` is never captured; invalid paths and duplicates are rejected; the file limit is enforced.
- `Command::Action{CreateFile|ObserveFile}` is rejected over the Desktop; the 17th file tab is rejected with the cap message; restore of a document over the limit is rejected.

Linux tests (all `cargo test --workspace`):
- `crates/workspace-core/tests/surfaces.rs`: `create_file_*` (shape, no effects, focus, bad paths, duplicate, capacity), `observe_file_*`, `close_paths_refuse_dirty_file_atomically` (one case per close path), `reopen_file_resets_dirty_and_rejects_open_path`, `move_surface_keeps_file_state`, `snapshot_serializes_file_surface_shape`, `descriptor_validation_rejects_mixed_kinds`.
- `crates/workspace-core/tests/session_document.rs`: `v10_round_trips_file_surfaces`, `v9_rejects_file_content`, `legacy_versions_still_load`, `capture_omits_dirty_and_content`, `restore_rejects_invalid_or_duplicate_file_paths`, `restore_enforces_file_limit`, `model_version_is_seven`.
- `crates/desktop-runtime/tests/desktop.rs`: `public_action_rejects_create_and_observe_file`, `seventeenth_file_surface_is_rejected`.
- `crates/desktop-runtime/tests/session_persistence.rs`: `file_surface_persists_and_restores_through_the_private_state_directory`, `restore_over_file_limit_is_rejected_with_recoverable_state`.

### S2 file-service

Goal: bounded Rust file worker, fencing, atomic save, conflict, polling, the open pipeline and the
native bridge export, all testable on Linux with real temp files.

Files:
- NEW `crates/desktop-runtime/src/file_io.rs`: `FileBackend` trait + `RealFs`; `classify`, `probe`, `read`, `save`, `stat`, NOFOLLOW walk, sniff, signature checks, fingerprint, error mapping.
- NEW `crates/desktop-runtime/src/files.rs`: `FileService` (worker, queue, owners, entries, retired ring, poller, stall detection, op ids, snapshot), frame header types, `OpenRequest/OpenProbe/OpenItemResult`, `FileHostEvent`.
- `crates/desktop-runtime/src/lib.rs`: `DesktopOptions.files: Option<FilesConfig>` (pub, D5; `FilesConfig`, `FileBackend` and their value types re-exported from the crate root), `FileService::poll` in the executor tick (`dirty |= self.files.poll(...)`, D6a), `Shared.files`, `Command::OpenFiles` + `Command::FileHost` (`#[serde(skip)]`), `Execution.open_result`, open application loop (dedupe, focus-once, per-item error mapping), `publish` reconcile + `Snapshot.files`, `files.reset` on restore, allow-list during pending restore, `Desktop::{probe_open_items, file_request, ...}` pub methods, `targeted()` rejects raw `OpenItems`, error-code mapping.
- `crates/desktop-runtime/src/targets.rs`: `TargetOperation::OpenItems { items, focus, force_mode }`.
- `crates/desktop-runtime/src/control.rs` + `control_policy.rs`: `Targeted/OpenItems` branch, `open_result` merge in `mutate`, `capabilities.file_panel`.
- `crates/desktop-runtime/src/shutdown.rs`: join the file worker.
- `crates/native-bridge/src/lib.rs`, `include/zmux.h`, `tests/abi.c` (symbol reference), `docs/native-bridge.md`: `zmux_desktop_file` and the frame codec with the limits above.
- Tests listed below.

Acceptance:
- Every behaviour in D2, D4, D5, D6, D12-D15, D21 holds against real files.
- Content never appears in `Snapshot`, control JSON or logs (asserted by scanning serialized snapshot for a unique sentinel string written in the file).
- Executor never blocks on file I/O: with a gated backend that blocks a read, snapshots/commands still progress.
- File-worker state changes (changed, deleted, unreadable, stalled, `last_op` resolution) advance `Snapshot.revision` with no other activity (D6a), and an idle service does not.
- A lost or refused `observe` is retried to convergence on the Rust side of the contract and the close guard holds afterwards (D7a).
- A text or markdown file with a NUL or invalid UTF-8 at any offset (including after byte 8192) is a per-item error at probe time and creates no tab (D4).
- `open` pipeline returns correct per-item results for a mixed batch and applies items in order.

Linux tests:
- `file_io` unit tests (`#[cfg(test)]`, real temp dirs): extension table incl. case, dotfile, no extension, `svg`/`html` => text; sniff NUL/invalid UTF-8/boundary multibyte; probe full validation: NUL at offset 8193 and at the last byte of a 2 MiB file => `binary_file`, a stray invalid byte at offset 20000 => `invalid_utf8`, exactly 2 MiB valid => ok, one over => `too_large`; probe returns `probe_timeout` when a gated backend stalls the full read; PDF and PNG/JPEG/GIF signature mismatch; size boundaries exactly at and one over 2 MiB and 7 MiB; symlink resolves to target on probe; component swapped to a symlink after probe => `path_changed`; FIFO/directory/device rejected without blocking; hard link read ok but save `hard_linked`; atomic save preserves permission bits, leaves no temp file after every injected failure (read-only dir, write error, rename error), never partially replaces content; BOM round trip; `invalid_utf8` save body; fingerprint equality vs touch.
- `crates/desktop-runtime/tests/files.rs` (new; builds `Desktop::with_options(shell, DesktopOptions { files: Some(FilesConfig { backend: gated_or_counting, poll_interval: 20 ms, poll_tick: 5 ms, .. }), .. })` so every injection below uses the public `FilesConfig`): `file_worker_changes_advance_the_revision` and the D7a observe tests (`observe_is_idempotent_and_does_not_advance_revision_when_unchanged`, `observe_true_then_false_converges_after_queue_full`, `retried_observe_after_stale_owner_is_not_applied`); attach/reload/save happy path with generation increments; conflict `modified`, conflict `deleted`, `force` overwrite and recreate; touch/identical rewrite is not a conflict; `stale_view`; stale owner after detach, after surface close (file unchanged), after epoch reset; `host_conflict`; second op on same surface => `busy`; queue full => `busy`; stalled gated backend => `file_worker_unavailable` then recovery; save timeout => `outcome_unknown` then `last_op` resolves in the snapshot; reload refused while dirty unless `discard`; dirty observed only through the host event and cleared on detach; polling (interval injected to 20 ms): modify => `changed` with `disk_generation` increment, atomic rename replace, delete => `deleted` only after two polls, recreate, touch stays `in_sync`, unreadable; executor not blocked while backend gated; shutdown joins; sentinel-not-in-snapshot test.
- Open pipeline (real `ControlServer`, pattern of `crates/desktop-runtime/tests/browser.rs`/`apps/cli/tests/windows.rs`): mixed batch (md, rs, png, pdf, directory, missing, binary, text with invalid UTF-8 after byte 8192, over-cap, http URL, `file://`, URL with credentials, relative-path rejection) with exact item results; dedupe focuses existing; `focus:false` keeps selection; first-success-focus rule; 17th file => `file_capacity`; full pane => `tab_capacity`; stale revision is a whole-request error with no mutation; read-only socket denied before any probe (assert no backend call); gated probe => `probe_timeout` while other items complete.
- `crates/native-bridge/tests/files.rs` (new; uses the same public `FilesConfig`): `observe_returns_busy_not_ok_when_queue_full` (D7a), frame validation (short frame, `header_len` overflow, oversize non-save, oversize save, unknown op, unknown field => `INVALID_ARGUMENT`); `open` -> attach -> save -> reload round trip through the real exports; wrong owner; reply buffer reservation (`CAPACITY` with 16 buffers held, nothing executed); `scripts/check-native-bridge` still passes with the header addition.

### S3 cli-open

Goal: `zmux open` and `zmux markdown open` on top of S2.

Files:
- NEW `apps/cli/src/open.rs`: operand classification, absolutization, limits, request build, `run` with per-item output and exit status.
- `apps/cli/src/main.rs`: `Parsed::Open(ControlCommand)`, `run` arm, default timeout 5 s for `open`.
- `apps/cli/src/targets.rs`: route `open` / `markdown open` (reuse selector loop and `extract_browser_focus`).
- `apps/cli/src/registry.rs`: `command!` entries `open` and `markdown` (`targeted: true`, `socket: true`), a new `files` group, usage/description text (states the 2 MiB / 7 MiB limits, dirs rejected, local files only, exit status, dedupe).
- `apps/cli/src/registry/contracts.rs`: leaves `open` and `markdown open`; `parsed()` allows `Parsed::Open` with route `"open"`; expected pointers `/request/operation/operation = open_items`, and for `markdown open` `/request/operation/force_mode = markdown`.
- `apps/cli/src/registry/frozen-coverage.json`: `CMD-006` and `CMD-114` -> `status: local_counterpart_compatibility_unverified`, `local_contract: "open"` / `"markdown open"`. `behavioral_parity_verified` stays false. Do NOT touch `apps/cli/contracts/baseline/*`.
- `docs/local-control.md`: `open_items`, result shape, error codes, exit status.

Acceptance:
- Grammar of D17 exactly; relative operands become absolute against the CWD; `~` untouched; `--` literal; lexical errors fail before connecting with no request sent; more than 16 operands or an oversize request is a clear client error.
- Wire capture equals section 4.2 for the covered cases (no `ZMUX_*` ambient ids leak into tests: `env_remove` `ZMUX_WORKSPACE_ID`, `ZMUX_SURFACE_ID`, `ZMUX_TAB_ID`).
- Against a real Desktop: all-success exits 0 with full JSON; any item failure exits 1 with full JSON and the stderr summary; whole-request failure exits 1.
- `zmux commands` / catalog check pass; inventory count stays 428; `CMD-006` and `CMD-114` are the only coverage rows that change.

Linux tests: `apps/cli/tests/open.rs` (new): parse table (operand classification, absolutization with a fixture cwd, URL/scheme rules, `--`, focus flag conflicts, target selectors, `--expect-revision`, limits), exact wire capture via the `capture_request` helper, end-to-end with `Fixture` (real Desktop + ControlServer; the fixture passes a `FilesConfig` through `DesktopOptions.files` when a test needs a gated or counting backend) for mixed batch, dedupe, `--no-focus`, markdown force, exit statuses and stderr; `apps/cli/tests/cli.rs`: update `help_and_version_are_complete_and_offline` command list and group expectations, catalog/contract assertions; unit tests in `registry/contracts.rs` already run through `validated()`.

### S4 native-panel

Goal: the AppKit presentation of D20/D23. Cannot be built or run on this Linux machine.

Files (new): `apps/macos/Sources/FileView.swift` (container, mode switching, banners, retry),
`FileTextEditor.swift` (NSTextView editor, dirty transitions, writable handling),
`FileMarkdownView.swift` (parse + attribute mapping + link allow-list), `FilePreviews.swift`
(image and PDF), `FileHostClient.swift` (frame encode/decode, owner, op ids, off-main bridge
calls), `FileSmoke.swift` (in-app smoke). Modified: `NativeBridge.swift` (actor `file(...)`
wrapper, `open`), `Models.swift` (`FileSnapshot` decode), `ApplicationCoordinator.swift` (`files`
dictionary, create/prune/attach in `reconcile`, `UICommand` cases save/open/observe, bounded
queue unchanged), `App.swift` (`paneView` file branch, File menu with Open/Save/Reload,
`validateMenuItem`, `completePresentation`/`releaseContentResponder` file branches,
`onFocus -> focus_pane`, dirty-close sheets for tab/other/pane/workspace/window/quit,
`applicationShouldTerminate`, Edit-menu responder forwarding, smoke wiring near the existing
`SurfaceOrderSmoke` call), `WorkspaceViews.swift` (mode icons, in-place dirty marker).

Acceptance (all on macOS CI; none verifiable here):
- `swiftc -typecheck -swift-version 6 -strict-concurrency=complete -warnings-as-errors` over all
  sources passes (via `scripts/check-macos-sources`).
- Smoke journey through the real CLI and bridge: open md/txt/png/pdf/dir/missing mix; dedupe; edit
  -> dirty marker without losing first responder; Cmd-S saves and clears dirty; external write
  while clean reloads, while dirty shows the banner, and a plain save then reports the conflict
  sheet; Overwrite/Reload paths; close dirty tab prompts and Cancel keeps it; Don't Save closes;
  reopen restores from disk; session quit/restore reopens tabs; image/PDF decode; error states
  with Retry; no file descriptor held after close.

### S5 closure-docs

Goal: truthful evidence and tracker updates; no code.

Files: `references/cu14-file-panel.md` (receipt: commits, logs, test names, UNAVAILABLE list,
residual limitations); `references/closure-tracker.json` (features `CU-14`, `CV-01`, `CV-05`,
`CV-06`: `delivery_status` -> `implementing` (the only non-planned value the tracker uses; never
`verified`), `implementation_commits`, `verification_receipts`, `remaining_gates` naming each
deferred item; `CV-02` stays `planned`; package `C01` commit_slices note); `status.md` (newest
first under "Active implementation"); `plan.md` step line; `docs/native-bridge.md` and
`docs/local-control.md` consistency check; a final full `scripts/check` run recorded with its
log path.

Acceptance: `scripts/license-inventory --check` and `THIRD_PARTY_NOTICES.md` unchanged
(proves D22); `frozen-coverage.json` validation still passes; every native/hosted gate is listed
as UNAVAILABLE or pending CI, none as passed; `git diff --stat` shows no edits under
`apps/cli/contracts/baseline/*` or historical audit/inventory copies.

## 7. Deferred, and gates unavailable on Linux

Explicitly NOT delivered by this work (each stays a `remaining_gates` entry):
- CV-02: font family/size/max width, per-panel zoom, rendered find and next/previous, copy as
  Markdown/HTML, default-following settings, shared find/zoom actions and shortcuts.
- CV-06 editor settings: word-wrap toggle, syntax highlighting, line numbers, indent guides,
  current-line highlight, tab width, encoding selection/conversion (non-UTF-8 text is rejected),
  editor zoom, shared save/wrap/find/zoom command-palette actions.
- CV-05: Quick Look and media sessions, PDF thumbnails/outline/zoom/rotation/share/viewport
  restore, image zoom/anchor, native drag, remote preview refresh, preserved scroll position
  across reload.
- CV-01: relative file links and images in rendered Markdown, tables/task lists/raw HTML
  rendering, GitHub-flavoured extensions, an in-panel "unavailable file" recovery beyond Retry.
- CU-14: preferred external editor, system opener fallback, directory-to-workspace creation,
  `file://` operands, `file:LINE` anchors, `--profile` for URL items, shell-quoted editor launch.
- Save As, three-way merge/diff of a conflict, backup of overwritten content, autosave and crash
  recovery of unsaved text (closing is blocked instead).
- Real file watching (FSEvents/inotify); polling can miss a same-size same-timestamp rewrite.
- Ownership/ACL/xattr preservation on save, hard-linked file saves, in-place fallback writes,
  network-volume semantics, case-variant alias dedupe on case-insensitive volumes.
- Remote (SSH) workspace files (R02), non-UTF-8 paths, persisted viewport/Markdown view mode.
- Any claim of full parity: `behavioral_parity_verified` stays false; mapping `open` and
  `markdown open` is disclosure, not a compatibility receipt.

UNAVAILABLE on this Linux machine (record as UNAVAILABLE, never as passed; CI is the verifier):
- No Swift toolchain: `scripts/check-macos-sources`, `scripts/build-macos-dev`,
  `scripts/check-macos-app` and the `FileSmoke` in-app journey; Swift 6 strict-concurrency type
  check; the `.github/workflows/macos-development.yml` and the macOS job in `check.yml`.
- Physical keyboard (Cmd-S/Cmd-O/Cmd-W), first-responder retention, IME composition,
  VoiceOver/accessibility audit, large-file `NSTextView` performance, ImageIO/PDFKit decode,
  `AttributedString(markdown:)` fidelity, `NSOpenPanel` behaviour, quit-with-dirty prompt.
- macOS filesystem specifics: APFS case folding, `/var` realpath behaviour, `renameat`/`fsync`
  durability, Finder-created files, network volumes.
- Hosted GitHub Actions results of any kind (no push in this phase).

Available and required here: the whole of S1-S3 via `scripts/check` (fmt, tests, clippy,
license inventory, C ABI check), including real-file I/O, symlink/FIFO/hardlink behaviour on
Linux, the control socket, and the bridge exports called from Rust.

## 8. Licensing statement

Original Zmux code under MIT. Rust: existing dependencies only (`sha2`, `rustix`, `uuid`,
`serde`, `serde_json`, `thiserror`); no new crate, no `Cargo.lock` change expected. Swift: Apple
system frameworks only (AppKit, Foundation, PDFKit, ImageIO; UniformTypeIdentifiers if needed).
No fonts, icons or assets are added beyond SF Symbols names. No cmux GPL/BUSL source, tests or
assets were read, translated or imported; the implementation must be authored fresh from this
document and the digest. `docs/dependencies.md` needs no change; if any dependency is added the
slice must update it, `scripts/license-inventory` and `THIRD_PARTY_NOTICES.md` and justify it.

## 9. Risks to watch during implementation

- The 1 s probe plus 2 s executor wait occupies the single control listener thread for up to
  about 3 s; a hung volume therefore delays other control clients until the deadline. The stall
  detector (D21) turns later opens into immediate errors rather than repeated 3 s waits.
- `Surface` has several positional literal sites; grep `local_zellij_completion:` to find them all.
- `help_and_version_are_complete_and_offline` and the catalog assertions enumerate commands and
  groups; expect to edit them in S3.
- The close guard must not apply to `finish_restore_inner` removals or other internal
  `remove_surface` calls; it belongs only to the user-initiated close actions.
- Swift `Edit` menu currently uses custom selectors; verify Undo/Redo and clipboard reach the
  file text view before declaring any keyboard behaviour done (UNAVAILABLE here).
- The probe reads up to 2 MiB per text item inside a 1 s budget on the single control listener
  thread's behalf (the read itself is on the file worker); several 2 MiB items on a slow disk
  produce `probe_timeout` items rather than a longer wait. Watch this during S2 tuning.
- `FilesConfig::executor_tick_hook` runs on the executor; it must stay `None` outside tests and
  any test using it must release it in a drop guard so a failing test cannot wedge the executor.

## 10. Review resolutions

Five blocking review findings were verified against `crates/desktop-runtime/src/lib.rs` and
`crates/native-bridge/src/lib.rs` and all were correct. Resolutions:

1. Lossy dirty mirror, and 5. dirty bit fed by a no-replay channel (same root cause). Confirmed:
   `MAX_COMMANDS = 32` sync channel, `QueueFull | Busy => BUSY` in the bridge, and a socket close
   passes `ensure_closeable` after a lost `observe(true)`. Resolved by new D7a: `observe` is an
   idempotent absolute-state write exempt from the no-replay rule (D15 comment amended); the view
   re-issues it with bounded-rate backoff until the snapshot's `file.dirty` equals its own state;
   native close/quit flows read the view-local dirty bit (D8 amended) and force a reconciliation
   first; a locally dirty view whose surface vanishes is kept as an orphan with Copy Text instead
   of being pruned; the model bit is documented as the guard for non-native callers only. The
   residual pre-ack window is stated rather than hidden. Tests added, including the queue-full
   case via `FilesConfig::executor_tick_hook`.
2. Probe validated only 8 KiB. Confirmed: it contradicted the digest's explicit per-item errors.
   Resolved in D4: text and markdown probes read and validate the whole file (at most 2 MiB,
   inside the 1 s budget); `probe_timeout` when it cannot finish. Image and PDF corrupt-body
   decode failure remains a documented deviation (no Rust decoder, no new dependency).
3. No injection point. Confirmed: `DesktopOptions` has five fields and none for files, and
   `PathObservers::start()` takes no configuration. Resolved in D5: `DesktopOptions.files:
   Option<FilesConfig>` with pub `FilesConfig`/`FileBackend`, listed in the S2 files; tests use it.
   `poll_tick` and `executor_tick_hook` were added beyond the suggested fields because the 250 ms
   tick would otherwise make 20 ms polling untestable and the executor could not be held.
4. Missing publish trigger. Confirmed: `publish` runs only `if dirty` in the tick and bumps the
   revision itself, so `reconcile` inside `publish` alone could never surface worker changes.
   Resolved by new D6a: `FileService::poll(&model, now) -> bool` in the tick chain, driven by a
   `state_version`, with quantized stall age; plus a revision-advances test and an idle-does-not
   test.
