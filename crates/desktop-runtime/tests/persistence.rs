use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_desktop_runtime::persistence::{
    ALLOCATION_BLOCK_IDS, LoadedSession, MAX_DOCUMENT_BYTES, MAX_PENDING_OPERATIONS, OperationId,
    PersistenceCompletion, PersistenceError, PersistenceOutcome, PersistenceWorker,
};
use zmux_workspace_core::{Action, RestoreLimits, SessionDocument, WorkspaceStore};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        // macOS /tmp is a symlink. Resolve only the test root, so tested paths
        // exercise the production symlink prohibition rather than that OS alias.
        let path = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("zmux-persistence-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn worker(&self) -> PersistenceWorker {
        PersistenceWorker::start(
            self.0.join("state"),
            RestoreLimits {
                terminals: 8,
                browsers: 4,
            },
        )
        .unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn document() -> SessionDocument {
    let mut store = WorkspaceStore::with_identity_namespace(Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace {
            name: "project".into(),
        })
        .unwrap();
    store.session_document().unwrap()
}
fn write_private(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}
fn wait(worker: &PersistenceWorker, id: OperationId) -> PersistenceCompletion {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(completion) = worker.try_completion().unwrap() {
            assert_eq!(completion.operation_id, id);
            return completion;
        }
        assert!(Instant::now() < deadline, "filesystem operation timed out");
        thread::sleep(Duration::from_millis(2));
    }
}
fn loaded(worker: &PersistenceWorker, path: &Path) -> LoadedSession {
    match wait(worker, worker.submit_load(path.to_owned()).unwrap())
        .result
        .unwrap()
    {
        PersistenceOutcome::Loaded(value) => value,
        other => panic!("expected load completion, got {other:?}"),
    }
}
fn close(mut worker: PersistenceWorker) {
    worker
        .join_until(Instant::now() + Duration::from_secs(2))
        .unwrap();
}

#[test]
fn save_is_private_atomic_and_requires_explicit_replacement() {
    let directory = Directory::new();
    let worker = directory.worker();
    let path = directory.0.join("named.json");
    let original = document();
    let id = worker
        .submit_save(path.clone(), original.clone(), false)
        .unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Ok(PersistenceOutcome::Saved)
    ));
    let bytes = fs::read(&path).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    let mut updated = original.clone();
    updated.workspaces[0].name = "updated".into();
    let id = worker
        .submit_save(path.clone(), updated.clone(), false)
        .unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::AlreadyExists)
    ));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let id = worker.submit_save(path.clone(), updated, true).unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Ok(PersistenceOutcome::Saved)
    ));
    let saved: SessionDocument = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.workspaces[0].name, "updated");
    assert!(!fs::read_dir(&directory.0).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
    close(worker);
}

#[test]
fn repeated_imports_reserve_durable_generations_without_mutating_source() {
    let directory = Directory::new();
    let path = directory.0.join("import.json");
    let document = document();
    let source = serde_json::to_vec(&document).unwrap();
    write_private(&path, &source);
    let worker = directory.worker();
    let first = loaded(&worker, &path);
    assert_eq!(first.session_ids[0].0, document.next_id);
    close(worker);
    // Reopening a process/worker still consumes a fresh durable generation.
    let worker = directory.worker();
    let second = loaded(&worker, &path);
    assert!(second.session_ids[0].0 > first.session_ids[0].0);
    assert_eq!(fs::read(&path).unwrap(), source);
    let prepared = WorkspaceStore::prepare_session_restore(
        second.document,
        &second.session_ids,
        second.window_id,
    )
    .unwrap();
    assert_eq!(prepared.store.snapshot().workspaces[0].name, "project");
    assert_eq!(
        fs::metadata(directory.0.join("state")).unwrap().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(directory.0.join("state/session-generations.json"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    close(worker);
}

#[test]
fn legacy_window_and_sessions_share_durable_generations_even_without_terminals() {
    for empty in [false, true] {
        let directory = Directory::new();
        let path = directory.0.join("legacy.json");
        let mut original = document();
        if empty {
            original.workspaces.clear();
            original.focused_workspace = None;
        }
        let mut wire = serde_json::to_value(&original).unwrap();
        wire["version"] = serde_json::json!(3);
        wire.as_object_mut().unwrap().remove("window_layout");
        let source = serde_json::to_vec(&wire).unwrap();
        write_private(&path, &source);

        let worker = directory.worker();
        let first = loaded(&worker, &path);
        let first_window = first.window_id.expect("legacy window must be reserved");
        assert_eq!(
            first_window.0,
            first.allocation_start + first.session_ids.len() as u64
        );
        assert!(first_window.0 < first.allocation_end);
        let ledger: serde_json::Value = serde_json::from_slice(
            &fs::read(directory.0.join("state/session-generations.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            ledger["next_ids"][original.identity_namespace.to_string()],
            first.allocation_end
        );
        close(worker);

        let worker = directory.worker();
        let second = loaded(&worker, &path);
        let second_window = second
            .window_id
            .expect("restart must reserve a new legacy window");
        assert!(second_window.0 >= first.allocation_end);
        assert!(second_window.0 > original.next_id);
        assert_eq!(
            second_window.0,
            second.allocation_start + second.session_ids.len() as u64
        );
        assert_eq!(fs::read(&path).unwrap(), source);
        let prepared = WorkspaceStore::prepare_session_restore(
            second.document,
            &second.session_ids,
            second.window_id,
        )
        .unwrap();
        assert_eq!(prepared.effects.len(), usize::from(!empty));
        let snapshot = prepared.store.snapshot();
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].id, second_window);
        assert_eq!(
            snapshot.windows[0].workspace_ids,
            original.workspaces.iter().map(|w| w.id).collect::<Vec<_>>()
        );
        let exported = prepared.store.session_document().unwrap();
        let v4_path = directory.0.join("v4.json");
        let save = worker
            .submit_save(v4_path.clone(), exported, false)
            .unwrap();
        assert!(matches!(
            wait(&worker, save).result,
            Ok(PersistenceOutcome::Saved)
        ));
        close(worker);

        let worker = directory.worker();
        let v4 = loaded(&worker, &v4_path);
        assert!(v4.window_id.is_none());
        assert!(v4.session_ids.iter().all(|id| id.0 > second_window.0));
        let restored =
            WorkspaceStore::prepare_session_restore(v4.document, &v4.session_ids, v4.window_id)
                .unwrap();
        assert_eq!(restored.store.snapshot().windows, snapshot.windows);
        assert_eq!(
            restored.store.snapshot().identity_namespace,
            original.identity_namespace
        );
        close(worker);
    }
}

#[test]
fn unsupported_window_topology_is_rejected_before_durable_reservation() {
    let directory = Directory::new();
    let path = directory.0.join("two-windows.json");
    let mut wire = serde_json::to_value(document()).unwrap();
    wire["window_layout"]["windows"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": 99, "workspace_ids": [], "selected_workspace": null
        }));
    wire["next_id"] = serde_json::json!(100);
    let source = serde_json::to_vec(&wire).unwrap();
    write_private(&path, &source);
    let worker = directory.worker();
    let id = worker.submit_load(path.clone()).unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::InvalidDocument(_))
    ));
    assert!(!directory.0.join("state/session-generations.json").exists());
    assert_eq!(fs::read(&path).unwrap(), source);
    close(worker);
}

#[test]
fn corrupt_private_and_oversize_sources_are_preserved() {
    let directory = Directory::new();
    let worker = directory.worker();
    let corrupt = directory.0.join("corrupt.json");
    let source = b"{broken";
    write_private(&corrupt, source);
    let id = worker.submit_load(corrupt.clone()).unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::InvalidDocument(_))
    ));
    assert_eq!(fs::read(&corrupt).unwrap(), source);
    let public = directory.0.join("public.json");
    write_private(&public, &serde_json::to_vec(&document()).unwrap());
    fs::set_permissions(&public, fs::Permissions::from_mode(0o644)).unwrap();
    let id = worker.submit_load(public).unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::UnsafeFile(_))
    ));
    let oversize = directory.0.join("large.json");
    write_private(&oversize, &[]);
    OpenOptions::new()
        .write(true)
        .open(&oversize)
        .unwrap()
        .set_len(MAX_DOCUMENT_BYTES as u64 + 1)
        .unwrap();
    let id = worker.submit_load(oversize).unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::TooLarge(_))
    ));
    let id = worker
        .submit_load(directory.0.join("missing.json"))
        .unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::NotFound)
    ));
    close(worker);
}

#[test]
fn symlink_components_and_unsafe_state_never_redirect_or_overwrite_files() {
    let directory = Directory::new();
    let source = directory.0.join("source.json");
    let source_bytes = serde_json::to_vec(&document()).unwrap();
    write_private(&source, &source_bytes);
    let alias = directory.0.join("alias.json");
    symlink(&source, &alias).unwrap();
    let worker = directory.worker();
    let id = worker.submit_load(alias.clone()).unwrap();
    assert!(wait(&worker, id).result.is_err());
    let id = worker.submit_save(alias, document(), true).unwrap();
    assert!(wait(&worker, id).result.is_err());
    let parent_alias = directory.0.join("linked-parent");
    symlink(&directory.0, &parent_alias).unwrap();
    let id = worker
        .submit_load(parent_alias.join("source.json"))
        .unwrap();
    assert!(wait(&worker, id).result.is_err());
    assert_eq!(fs::read(&source).unwrap(), source_bytes);
    // The bounded open retry must not follow an existing or dangling lock symlink,
    // create its target, or reach ledger mutation before descriptor ownership.
    let lock = directory.0.join("state/session-generations.lock");
    for existing in [false, true] {
        let target = directory.0.join(format!("lock-target-{existing}"));
        if existing {
            write_private(&target, &[]);
        }
        symlink(&target, &lock).unwrap();
        let id = worker.submit_load(source.clone()).unwrap();
        assert!(matches!(
            wait(&worker, id).result,
            Err(PersistenceError::Io {
                operation: "open persistence generation lock",
                ..
            })
        ));
        assert!(
            fs::symlink_metadata(&lock)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!directory.0.join("state/session-generations.json").exists());
        if existing {
            assert!(fs::read(&target).unwrap().is_empty());
        } else {
            assert!(!target.exists());
        }
        fs::remove_file(&lock).unwrap();
    }
    close(worker);
    fs::set_permissions(directory.0.join("state"), fs::Permissions::from_mode(0o755)).unwrap();
    let worker = directory.worker();
    let id = worker.submit_load(source).unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::UnsafeStateDirectory)
    ));
    close(worker);
}

#[test]
fn corrupt_ledger_and_descriptor_lock_fail_closed_without_resetting_generations() {
    let directory = Directory::new();
    let source = directory.0.join("source.json");
    write_private(&source, &serde_json::to_vec(&document()).unwrap());
    let worker = directory.worker();
    let _first = loaded(&worker, &source);
    for name in ["session-generations.lock", "session-generations.json"] {
        let id = worker
            .submit_save(directory.0.join("state").join(name), document(), true)
            .unwrap();
        assert!(matches!(
            wait(&worker, id).result,
            Err(PersistenceError::ReservedStatePath)
        ));
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.0.join("state/session-generations.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let id = worker.submit_load(source.clone()).unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::LedgerBusy)
    ));
    drop(lock);
    let ledger = directory.0.join("state/session-generations.json");
    fs::write(&ledger, b"broken ledger").unwrap();
    let id = worker.submit_load(source).unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::CorruptLedger)
    ));
    assert_eq!(fs::read(&ledger).unwrap(), b"broken ledger");
    close(worker);
}

#[test]
fn accepted_operations_and_unpolled_completions_share_one_bound() {
    let directory = Directory::new();
    let worker = directory.worker();
    let mut ids = Vec::new();
    for index in 0..MAX_PENDING_OPERATIONS {
        ids.push(
            worker
                .submit_load(directory.0.join(format!("absent-{index}.json")))
                .unwrap(),
        );
    }
    assert!(matches!(
        worker.submit_load(directory.0.join("overflow.json")),
        Err(PersistenceError::QueueFull)
    ));
    for id in ids {
        assert!(matches!(
            wait(&worker, id).result,
            Err(PersistenceError::NotFound)
        ));
    }
    let id = worker
        .submit_load(directory.0.join("capacity-returned.json"))
        .unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::NotFound)
    ));
    worker.request_shutdown();
    assert!(matches!(
        worker.submit_load(directory.0.join("after-stop.json")),
        Err(PersistenceError::Closed)
    ));
    close(worker);
}

#[test]
fn racing_no_replace_saves_have_exactly_one_winner() {
    let directory = Directory::new();
    let first = directory.worker();
    let second = directory.worker();
    let path = directory.0.join("race.json");
    let first_document = document();
    let mut second_document = first_document.clone();
    second_document.workspaces[0].name = "other".into();
    let a = first
        .submit_save(path.clone(), first_document, false)
        .unwrap();
    let b = second
        .submit_save(path.clone(), second_document, false)
        .unwrap();
    let a = wait(&first, a).result;
    let b = wait(&second, b).result;
    assert!(matches!(
        (&a, &b),
        (
            Ok(PersistenceOutcome::Saved),
            Err(PersistenceError::AlreadyExists)
        ) | (
            Err(PersistenceError::AlreadyExists),
            Ok(PersistenceOutcome::Saved)
        )
    ));
    let _: SessionDocument = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    close(first);
    close(second);
}

#[test]
fn generation_capacity_overflow_and_duplicate_ledger_keys_fail_closed() {
    let directory = Directory::new();
    let source = directory.0.join("source.json");
    let document = document();
    write_private(&source, &serde_json::to_vec(&document).unwrap());
    let worker = directory.worker();
    let _first = loaded(&worker, &source);
    let ledger = directory.0.join("state/session-generations.json");
    let namespaces: serde_json::Map<String, serde_json::Value> = (0..128)
        .map(|_| (Uuid::new_v4().to_string(), serde_json::json!(100)))
        .collect();
    let full = serde_json::to_vec(&serde_json::json!({"version":1,"next_ids":namespaces})).unwrap();
    fs::write(&ledger, &full).unwrap();
    let id = worker.submit_load(source.clone()).unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::LedgerFull)
    ));
    assert_eq!(fs::read(&ledger).unwrap(), full);
    fs::write(&ledger, serde_json::to_vec(&serde_json::json!({"version":1,"next_ids":{document.identity_namespace.to_string():u64::MAX - 4}})).unwrap()).unwrap();
    let id = worker.submit_load(source.clone()).unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::GenerationExhausted)
    ));
    let duplicate = format!(
        r#"{{"version":1,"next_ids":{{"{}":200,"{}":1}}}}"#,
        document.identity_namespace, document.identity_namespace
    );
    fs::write(&ledger, &duplicate).unwrap();
    let id = worker.submit_load(source).unwrap();
    assert!(matches!(
        wait(&worker, id).result,
        Err(PersistenceError::CorruptLedger)
    ));
    assert_eq!(fs::read_to_string(&ledger).unwrap(), duplicate);
    close(worker);
}

#[test]
fn concurrent_instances_reserve_disjoint_ids_or_report_a_retryable_lock_conflict() {
    let directory = Directory::new();
    let source = directory.0.join("source.json");
    write_private(&source, &serde_json::to_vec(&document()).unwrap());
    let mut first = directory.worker();
    let mut second = directory.worker();
    let first_id = first.submit_load(source.clone()).unwrap();
    let second_id = second.submit_load(source.clone()).unwrap();
    let first_result = wait(&first, first_id).result;
    let second_result = wait(&second, second_id).result;
    fn finish(
        worker: &PersistenceWorker,
        source: &Path,
        result: Result<PersistenceOutcome, PersistenceError>,
    ) -> Result<LoadedSession, String> {
        let result = if matches!(result, Err(PersistenceError::LedgerBusy)) {
            match worker.submit_load(source.to_owned()) {
                Ok(id) => wait(worker, id).result,
                Err(error) => return Err(format!("retry admission: {error:?}")),
            }
        } else {
            result
        };
        match result {
            Ok(PersistenceOutcome::Loaded(value)) => Ok(value),
            other => Err(format!("{other:?}")),
        }
    }
    let first_value = finish(&first, &source, first_result);
    let second_value = finish(&second, &source, second_result);
    // Consume both operation completions and join both filesystem workers before
    // an outcome assertion can unwind and remove their private directory.
    let first_closed = first.join_until(Instant::now() + Duration::from_secs(2));
    let second_closed = second.join_until(Instant::now() + Duration::from_secs(2));
    let context = || {
        let metadata = |path: &Path| match fs::symlink_metadata(path) {
            Ok(value) => format!(
                "dir={},symlink={},inode={},mode={:o}",
                value.is_dir(),
                value.file_type().is_symlink(),
                value.ino(),
                value.mode()
            ),
            Err(error) => format!("{error:?}"),
        };
        let state = directory.0.join("state");
        let entries = match fs::symlink_metadata(&state) {
            Ok(value) if value.is_dir() && !value.file_type().is_symlink() => {
                match fs::read_dir(&state) {
                    Ok(entries) => format!(
                        "{:?}",
                        entries
                            .take(16)
                            .map(|entry| entry.map(|value| value.file_name()))
                            .collect::<Vec<_>>()
                    ),
                    Err(error) => format!("{error:?}"),
                }
            }
            _ => "not-listed".into(),
        };
        format!(
            "fixture={:?}; root={}; state={}; state_entries={entries}",
            directory.0,
            metadata(&directory.0),
            metadata(&state)
        )
    };
    assert!(
        first_closed.is_ok() && second_closed.is_ok(),
        "filesystem workers did not finish: {first_closed:?}, {second_closed:?}; {}",
        context()
    );
    let first_value = first_value.unwrap_or_else(|error| {
        panic!(
            "unexpected first concurrent restore outcome: {error}; {}",
            context()
        )
    });
    let second_value = second_value.unwrap_or_else(|error| {
        panic!(
            "unexpected second concurrent restore outcome: {error}; {}",
            context()
        )
    });
    assert_ne!(first_value.session_ids, second_value.session_ids);
}

#[test]
fn durable_blocks_burn_unused_runtime_ids_across_restart_and_later_import() {
    let directory = Directory::new();
    let document = document();
    let source = directory.0.join("older-checkpoint.json");
    write_private(&source, &serde_json::to_vec(&document).unwrap());
    let first = directory.worker();
    let id = first
        .submit_reserve(document.identity_namespace, document.next_id)
        .unwrap();
    let reservation = match wait(&first, id).result.unwrap() {
        PersistenceOutcome::Reserved(value) => value,
        other => panic!("expected allocator reservation, got {other:?}"),
    };
    assert_eq!(reservation.start, document.next_id);
    assert_eq!(reservation.end - reservation.start, ALLOCATION_BLOCK_IDS);
    // No topology save occurs: allocations, launches, or a crash could consume
    // any portion of this range while the on-disk checkpoint remains old.
    close(first);
    let second = directory.worker();
    let id = second
        .submit_reserve(document.identity_namespace, document.next_id)
        .unwrap();
    let next = match wait(&second, id).result.unwrap() {
        PersistenceOutcome::Reserved(value) => value,
        other => panic!("expected allocator reservation, got {other:?}"),
    };
    assert_eq!(next.start, reservation.end);
    let restored = loaded(&second, &source);
    assert_eq!(restored.session_ids[0].0, next.end);
    assert_eq!(
        restored.allocation_end - restored.session_ids[0].0,
        ALLOCATION_BLOCK_IDS + 1
    );
    // An empty/browser-only checkpoint has no terminal IDs from which runtime
    // could infer the reserved allocator floor. The load result must carry it.
    let mut empty = document;
    empty.workspaces.clear();
    empty.focused_workspace = None;
    for window in &mut empty.window_layout.as_mut().unwrap().windows {
        window.workspace_ids.clear();
        window.selected_workspace = None;
    }
    let empty_source = directory.0.join("empty-checkpoint.json");
    write_private(&empty_source, &serde_json::to_vec(&empty).unwrap());
    let empty_restore = loaded(&second, &empty_source);
    assert!(empty_restore.session_ids.is_empty());
    assert_eq!(empty_restore.allocation_start, restored.allocation_end);
    assert_eq!(
        empty_restore.allocation_end - empty_restore.allocation_start,
        ALLOCATION_BLOCK_IDS
    );
    close(second);
}

#[test]
fn browser_profile_catalog_is_durable_locked_bounded_and_preserves_other_writers() {
    let directory = Directory::new();
    let first = directory.worker();
    let second = directory.worker();
    let profiles = |worker: &PersistenceWorker, name: Option<String>| match wait(
        worker,
        worker.submit_profiles(name).unwrap(),
    )
    .result
    .unwrap()
    {
        PersistenceOutcome::Profiles(catalog) => catalog,
        other => panic!("unexpected catalog outcome: {other:?}"),
    };
    assert!(profiles(&first, None).profiles.is_empty());
    assert!(profiles(&second, None).profiles.is_empty());
    let a = profiles(&first, Some("Work".into())).profiles[0].clone();
    let catalog = profiles(&second, Some("Personal".into()));
    assert_eq!(catalog.profiles.len(), 2);
    assert_eq!(catalog.profiles[0], a);
    let path = directory.0.join("state/browser-profiles.json");
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    let before = fs::read(&path).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.0.join("state/browser-profiles.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(matches!(
        wait(
            &first,
            first.submit_profiles(Some("Locked".into())).unwrap()
        )
        .result,
        Err(PersistenceError::LedgerBusy)
    ));
    drop(lock);
    assert_eq!(fs::read(&path).unwrap(), before);
    for name in [
        "Work",
        " bad",
        "bad\nname",
        "00000000-0000-0000-0000-000000000001",
    ] {
        assert!(
            wait(&first, first.submit_profiles(Some(name.into())).unwrap())
                .result
                .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    for index in 2..32 {
        profiles(&first, Some(format!("Profile {index}")));
    }
    let full = fs::read(&path).unwrap();
    assert!(
        wait(
            &second,
            second.submit_profiles(Some("Overflow".into())).unwrap()
        )
        .result
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), full);
    close(first);
    close(second);
    let reopened = directory.worker();
    let restored = profiles(&reopened, None);
    assert_eq!(restored.profiles.len(), 32);
    assert_eq!(restored.profiles[0], a);
    // Session export cannot overwrite the catalog or its synchronization owner.
    for name in ["browser-profiles.json", "browser-profiles.lock"] {
        assert!(matches!(
            wait(
                &reopened,
                reopened
                    .submit_save(directory.0.join("state").join(name), document(), true)
                    .unwrap()
            )
            .result,
            Err(PersistenceError::ReservedStatePath)
        ));
    }
    close(reopened);
}

#[test]
fn browser_profile_catalog_rejects_corruption_and_unsafe_files_without_replacement() {
    for bad in [b"{broken".as_slice(),b"{\"version\":99,\"profiles\":[]}",b"{\"version\":1,\"profiles\":[{\"id\":\"00000000-0000-0000-0000-000000000000\",\"name\":\"Work\"}]}"] {
        let directory=Directory::new();let worker=directory.worker();
        wait(&worker,worker.submit_profiles(None).unwrap()).result.unwrap();
        let path=directory.0.join("state/browser-profiles.json");write_private(&path,bad);
        assert!(wait(&worker,worker.submit_profiles(Some("New".into())).unwrap()).result.is_err());
        assert_eq!(fs::read(&path).unwrap(),bad);close(worker);
    }
    let directory = Directory::new();
    let worker = directory.worker();
    wait(&worker, worker.submit_profiles(None).unwrap())
        .result
        .unwrap();
    let source = directory.0.join("source");
    write_private(&source, b"{\"version\":1,\"profiles\":[]}");
    let path = directory.0.join("state/browser-profiles.json");
    symlink(&source, &path).unwrap();
    assert!(
        wait(&worker, worker.submit_profiles(Some("New".into())).unwrap())
            .result
            .is_err()
    );
    assert!(path.is_symlink());
    close(worker);
}
