use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
use uuid::Uuid;
use zmux_desktop_runtime::{
    persistence::PersistenceError,
    profile_leases::{
        LeaseError, LeaseMode, LeaseOwner, LeaseService, MutationRequest, NativeMutationOutcome,
        journal::{MutationKind, MutationPhase, QUARANTINE_SENTINEL},
    },
};
use zmux_workspace_core::{Action, SessionDocument, WorkspaceStore};

#[path = "support/profile_fixture.rs"]
mod profile_fixture;
use profile_fixture::{Fixture, wait};

fn owner() -> LeaseOwner {
    LeaseOwner {
        epoch: Uuid::new_v4(),
        store: Uuid::new_v4(),
    }
}

fn document() -> SessionDocument {
    let mut store = WorkspaceStore::with_identity_namespace(Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace {
            name: "fixture".into(),
        })
        .unwrap();
    store.session_document().unwrap()
}
struct ChildLease(Child);
impl ChildLease {
    fn start(catalog: PathBuf, leases: PathBuf, profile: Uuid) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "lease_child", "--nocapture"])
            .env("ZMUX_LEASE_CHILD_STATE", catalog)
            .env("ZMUX_LEASE_CHILD_ROOT", leases)
            .env("ZMUX_LEASE_CHILD_PROFILE", profile.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                match line {
                    Ok(line) if line.contains("PROFILE_LEASE_READY") => {
                        let _ = sender.send(());
                        return;
                    }
                    Ok(_) => (),
                    Err(error) => panic!("lease child output failed: {error}"),
                }
            }
        });
        let mut child = Self(child);
        if receiver.recv_timeout(Duration::from_secs(4)).is_err() {
            child.crash();
            panic!("independent process never acquired its shared lease");
        }
        child
    }
    fn crash(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            self.0.kill().unwrap();
        }
        self.0.wait().unwrap();
    }
}
impl Drop for ChildLease {
    fn drop(&mut self) {
        self.crash();
    }
}

#[test]
#[ignore = "real subprocess fixture invoked only by the lease ownership test"]
fn lease_child() {
    // Independent finite lifetime if the owning test process disappears.
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(10));
        std::process::exit(3);
    });
    let service = LeaseService::start(PathBuf::from(
        std::env::var_os("ZMUX_LEASE_CHILD_ROOT").unwrap(),
    ))
    .unwrap();
    let token = service
        .acquire(
            PathBuf::from(std::env::var_os("ZMUX_LEASE_CHILD_STATE").unwrap()),
            owner(),
            std::env::var("ZMUX_LEASE_CHILD_PROFILE")
                .unwrap()
                .parse()
                .unwrap(),
            LeaseMode::Shared,
        )
        .unwrap();
    service.constructed(token).unwrap();
    println!("PROFILE_LEASE_READY");
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
    service.release(token).unwrap();
}

#[test]
fn actual_process_shared_ownership_blocks_delete_lease_until_crash_and_keeps_inode() {
    let mut fixture = Fixture::new();
    let service = LeaseService::start(fixture.lease_root()).unwrap();
    let shared = service
        .acquire(
            fixture.state(),
            owner(),
            fixture.profiles[0],
            LeaseMode::Shared,
        )
        .unwrap();
    let identity = fs::metadata(fixture.path()).unwrap();
    assert_eq!(identity.nlink(), 1);
    assert_eq!(identity.mode() & 0o777, 0o600);
    // A copied catalog at another state path still addresses the same WebKit
    // UUID. The independent process must share the namespace root, not derive
    // its lock pathname from that catalog's location.
    let mut copied = Fixture::new();
    copied.stop_catalog();
    fs::copy(
        fixture
            .state()
            .join(zmux_desktop_runtime::profiles::CATALOG_NAME),
        copied
            .state()
            .join(zmux_desktop_runtime::profiles::CATALOG_NAME),
    )
    .unwrap();
    let mut child = ChildLease::start(copied.state(), fixture.lease_root(), fixture.profiles[0]);
    assert!(matches!(
        service.acquire(
            fixture.state(),
            owner(),
            fixture.profiles[0],
            LeaseMode::Exclusive
        ),
        Err(LeaseError::Busy)
    ));
    let other = service
        .acquire(
            fixture.state(),
            owner(),
            fixture.profiles[1],
            LeaseMode::Exclusive,
        )
        .unwrap();
    service.release(other).unwrap();
    service.release(shared).unwrap();
    assert!(matches!(
        service.acquire(
            fixture.state(),
            owner(),
            fixture.profiles[0],
            LeaseMode::Exclusive
        ),
        Err(LeaseError::Busy)
    ));
    let worker = fixture.worker.as_ref().unwrap();
    wait(
        worker,
        worker
            .submit_profile_command(zmux_desktop_runtime::profiles::ProfileCommand::Rename {
                profile: fixture.profiles[0].to_string(),
                name: "Office".into(),
            })
            .unwrap(),
    )
    .unwrap();
    // Ordinary export must never replace this stable inode, including case
    // aliases on the default case-insensitive macOS filesystem.
    for path in [
        fixture.path(),
        fixture.lease_root().join(format!(
            "browser-profile-{}.admission.lock",
            fixture.profiles[0]
        )),
        fixture.lease_root().join(format!(
            "browser-profile-{}.intent.json",
            fixture.profiles[0]
        )),
        fixture.lease_root().join(
            fixture
                .path()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_ascii_uppercase(),
        ),
    ] {
        assert!(matches!(
            wait(worker, worker.submit_save(path, document(), true).unwrap()),
            Err(PersistenceError::ReservedStatePath)
        ));
    }
    // A second desktop may use another state directory while exporting into
    // this one. It must not replace a held lease merely because it owns a
    // different persistence worker.
    let mut exporter = Fixture::new();
    let exporter_worker = exporter.worker.as_ref().unwrap();
    assert!(matches!(
        wait(
            exporter_worker,
            exporter_worker
                .submit_save(fixture.path(), document(), true)
                .unwrap()
        ),
        Err(PersistenceError::ReservedStatePath)
    ));
    exporter.stop_catalog();
    fixture.stop_catalog();
    assert_eq!(fs::metadata(fixture.path()).unwrap().ino(), identity.ino());
    assert!(fs::read(fixture.path()).unwrap().is_empty());
    assert!(matches!(
        service.acquire(
            fixture.state(),
            owner(),
            fixture.profiles[0],
            LeaseMode::Exclusive
        ),
        Err(LeaseError::Busy)
    ));
    child.crash();
    let exclusive_owner = owner();
    let exclusive = service
        .acquire(
            fixture.state(),
            exclusive_owner,
            fixture.profiles[0],
            LeaseMode::Exclusive,
        )
        .unwrap();
    assert_eq!(service.status(exclusive_owner).unwrap(), Some(exclusive));
    assert_eq!(fs::metadata(fixture.path()).unwrap().ino(), identity.ino());
    service.release(exclusive).unwrap();
    let successor = service
        .acquire(
            fixture.state(),
            owner(),
            fixture.profiles[0],
            LeaseMode::Shared,
        )
        .unwrap();
    assert_ne!(successor.id, exclusive.id);
    assert!(matches!(
        service.release(exclusive),
        Err(LeaseError::StaleToken)
    ));
    assert_eq!(service.status(successor.owner).unwrap(), Some(successor));
    service.release(successor).unwrap();
}

#[test]
fn lease_open_refuses_aliases_unsafe_inodes_and_unknown_profile_without_replacement() {
    for kind in ["symlink", "hardlink", "public", "nonempty"] {
        let fixture = Fixture::new();
        fs::create_dir(fixture.lease_root()).unwrap();
        fs::set_permissions(fixture.lease_root(), fs::Permissions::from_mode(0o700)).unwrap();
        let outside = fixture.root.join("outside");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&outside)
            .unwrap();
        drop(file);
        match kind {
            "symlink" => symlink(&outside, fixture.path()).unwrap(),
            "hardlink" => fs::hard_link(&outside, fixture.path()).unwrap(),
            "public" => {
                fs::write(fixture.path(), b"").unwrap();
                fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o644)).unwrap();
            }
            "nonempty" => {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(fixture.path())
                    .unwrap();
                file.write_all(b"do not replace").unwrap();
            }
            _ => unreachable!(),
        }
        let before = fs::symlink_metadata(fixture.path()).unwrap();
        let bytes = fs::read(fixture.path()).unwrap();
        let service = LeaseService::start(fixture.lease_root()).unwrap();
        assert!(
            service
                .acquire(
                    fixture.state(),
                    owner(),
                    fixture.profiles[0],
                    LeaseMode::Shared
                )
                .is_err(),
            "accepted {kind}"
        );
        assert_eq!(
            fs::symlink_metadata(fixture.path()).unwrap().ino(),
            before.ino()
        );
        assert_eq!(fs::read(fixture.path()).unwrap(), bytes);
    }
    let fixture = Fixture::new();
    let service = LeaseService::start(fixture.lease_root()).unwrap();
    let unknown = Uuid::new_v4();
    assert!(matches!(
        service.acquire(fixture.state(), owner(), unknown, LeaseMode::Shared),
        Err(LeaseError::UnknownProfile)
    ));
    assert!(
        !fixture
            .lease_root()
            .join(format!("browser-profile-{unknown}.lock"))
            .exists()
    );
}

fn mutation(
    fixture: &Fixture,
    controller: Uuid,
    stores: Vec<zmux_desktop_runtime::profile_leases::LeaseToken>,
    kind: MutationKind,
) -> MutationRequest {
    MutationRequest {
        catalog: fixture.state(),
        controller,
        intent: Uuid::new_v4(),
        profile: fixture.profiles[0],
        kind,
        local_stores: stores,
    }
}

#[test]
fn construction_permits_and_foreign_store_prevent_writer_without_losing_local_share() {
    let fixture = Fixture::new();
    let service = LeaseService::start(fixture.lease_root()).unwrap();
    let owner = owner();
    let shared = service
        .acquire(
            fixture.state(),
            owner,
            fixture.profiles[0],
            LeaseMode::Shared,
        )
        .unwrap();
    assert!(matches!(
        service.mutation_start(mutation(
            &fixture,
            owner.epoch,
            vec![shared],
            MutationKind::Clear
        )),
        Err(LeaseError::Busy)
    ));
    service.constructed(shared).unwrap();
    let construction = service
        .reuse(fixture.state(), shared, Uuid::new_v4())
        .unwrap();
    assert!(matches!(
        service.mutation_start(mutation(
            &fixture,
            owner.epoch,
            vec![shared],
            MutationKind::Clear
        )),
        Err(LeaseError::Busy)
    ));
    let mut forged = construction;
    forged.lease.owner.store = Uuid::new_v4();
    assert!(matches!(
        service.construction_done(forged),
        Err(LeaseError::StaleToken)
    ));
    assert!(matches!(service.release(shared), Err(LeaseError::Busy)));
    service.construction_done(construction).unwrap();
    let mut child = ChildLease::start(fixture.state(), fixture.lease_root(), fixture.profiles[0]);
    let request = mutation(&fixture, owner.epoch, vec![shared], MutationKind::Clear);
    let intent = request.intent;
    assert!(matches!(
        service.mutation_start(request),
        Err(LeaseError::Busy)
    ));
    assert_eq!(
        service
            .intent_status(fixture.profiles[0], intent)
            .unwrap()
            .record
            .phase,
        MutationPhase::FailedBeforeClear
    );
    // The foreign process is gone; only the restored original local share now
    // prevents another independent service from acquiring lifetime exclusive.
    child.crash();
    let competitor = LeaseService::start(fixture.lease_root()).unwrap();
    assert!(matches!(
        competitor.acquire(
            fixture.state(),
            super_owner(),
            fixture.profiles[0],
            LeaseMode::Exclusive
        ),
        Err(LeaseError::Busy)
    ));
    assert_eq!(service.status(owner).unwrap(), Some(shared));
    service.release(shared).unwrap();
    let exclusive = competitor
        .acquire(
            fixture.state(),
            super_owner(),
            fixture.profiles[0],
            LeaseMode::Exclusive,
        )
        .unwrap();
    competitor.release(exclusive).unwrap();
}
fn super_owner() -> LeaseOwner {
    owner()
}

#[test]
fn exact_native_phases_precede_catalog_commit_and_terminal_sentinel_release() {
    for kind in [MutationKind::Clear, MutationKind::Delete] {
        let fixture = Fixture::new();
        let service = LeaseService::start(fixture.lease_root()).unwrap();
        let owner = owner();
        let shared = service
            .acquire(
                fixture.state(),
                owner,
                fixture.profiles[0],
                LeaseMode::Shared,
            )
            .unwrap();
        service.constructed(shared).unwrap();
        let path = fixture
            .state()
            .join(zmux_desktop_runtime::profiles::CATALOG_NAME);
        let before = fs::read(&path).unwrap();
        let identity = fs::metadata(fixture.path()).unwrap().ino();
        let token = service
            .mutation_start(mutation(&fixture, owner.epoch, vec![shared], kind))
            .unwrap();
        assert_eq!(fs::read(fixture.path()).unwrap(), QUARANTINE_SENTINEL);
        assert!(matches!(
            service.acquire(
                fixture.state(),
                super_owner(),
                fixture.profiles[0],
                LeaseMode::Shared
            ),
            Err(LeaseError::Busy)
        ));
        let other = service
            .acquire(
                fixture.state(),
                super_owner(),
                fixture.profiles[1],
                LeaseMode::Shared,
            )
            .unwrap();
        service.release(other).unwrap();
        let mut forged = token;
        forged.controller = Uuid::new_v4();
        assert!(matches!(
            service.mutation_advance(forged, MutationPhase::ClearDispatched),
            Err(LeaseError::StaleToken)
        ));
        assert!(matches!(
            service.mutation_advance(token, MutationPhase::RemoveCompleted),
            Err(LeaseError::StaleToken)
        ));
        assert!(matches!(
            service.mutation_finish(token, NativeMutationOutcome::Completed),
            Err(LeaseError::StaleToken)
        ));
        for phase in [
            MutationPhase::ClearDispatched,
            MutationPhase::ClearCompleted,
            MutationPhase::RemoveDispatched,
            MutationPhase::RemoveCompleted,
        ] {
            service.mutation_advance(token, phase).unwrap();
            if phase == MutationPhase::ClearDispatched {
                assert!(matches!(
                    service.mutation_finish(
                        token,
                        NativeMutationOutcome::AbortedBeforeClear {
                            message: "generic timeout after dispatch".into()
                        }
                    ),
                    Err(LeaseError::StaleToken)
                ));
                assert_eq!(fs::read(fixture.path()).unwrap(), QUARANTINE_SENTINEL);
            }
            assert_eq!(fs::read(&path).unwrap(), before);
            assert_eq!(
                service
                    .intent_status(token.profile, token.intent)
                    .unwrap()
                    .record
                    .phase,
                phase
            );
        }
        let worker = fixture.worker.as_ref().unwrap();
        wait(
            worker,
            worker
                .submit_profile_command(zmux_desktop_runtime::profiles::ProfileCommand::Rename {
                    profile: fixture.profiles[1].to_string(),
                    name: "Concurrent rename".into(),
                })
                .unwrap(),
        )
        .unwrap();
        service
            .mutation_finish(token, NativeMutationOutcome::Completed)
            .unwrap();
        let catalog: zmux_desktop_runtime::profiles::Catalog =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            catalog
                .profiles
                .iter()
                .find(|p| p.id == fixture.profiles[1])
                .unwrap()
                .name,
            "Concurrent rename"
        );
        assert_eq!(
            catalog.profiles.iter().any(|p| p.id == fixture.profiles[0]),
            kind == MutationKind::Clear
        );
        if kind == MutationKind::Clear {
            assert_eq!(
                catalog
                    .profiles
                    .iter()
                    .find(|p| p.id == fixture.profiles[0])
                    .unwrap()
                    .name,
                "Work"
            );
        }
        assert!(fs::read(fixture.path()).unwrap().is_empty());
        assert_eq!(fs::metadata(fixture.path()).unwrap().ino(), identity);
        let status = service.intent_status(token.profile, token.intent).unwrap();
        assert_eq!(status.record.phase, MutationPhase::Completed);
        assert!(!status.outcome_unknown && !status.retry_native);
        assert!(matches!(
            service.release(shared),
            Err(LeaseError::StaleToken)
        ));
    }
}

#[test]
#[ignore = "real subprocess fixture invoked only by the crash quarantine test"]
fn mutation_child() {
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(10));
        std::process::exit(3);
    });
    let service = LeaseService::start(PathBuf::from(
        std::env::var_os("ZMUX_LEASE_CHILD_ROOT").unwrap(),
    ))
    .unwrap();
    let token = service
        .mutation_start(MutationRequest {
            catalog: PathBuf::from(std::env::var_os("ZMUX_LEASE_CHILD_STATE").unwrap()),
            controller: Uuid::new_v4(),
            intent: std::env::var("ZMUX_MUTATION_INTENT")
                .unwrap()
                .parse()
                .unwrap(),
            profile: std::env::var("ZMUX_LEASE_CHILD_PROFILE")
                .unwrap()
                .parse()
                .unwrap(),
            kind: MutationKind::Delete,
            local_stores: vec![],
        })
        .unwrap();
    service
        .mutation_advance(token, MutationPhase::ClearDispatched)
        .unwrap();
    println!("PROFILE_MUTATION_READY");
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
}

#[test]
fn dispatched_operation_survives_process_death_and_quarantines_only_original_uuid() {
    let fixture = Fixture::new();
    let intent = Uuid::new_v4();
    let mut process = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "mutation_child", "--nocapture"])
        .env("ZMUX_LEASE_CHILD_ROOT", fixture.lease_root())
        .env("ZMUX_LEASE_CHILD_STATE", fixture.state())
        .env("ZMUX_LEASE_CHILD_PROFILE", fixture.profiles[0].to_string())
        .env("ZMUX_MUTATION_INTENT", intent.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let output = process.stdout.take().unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            if line.unwrap().contains("PROFILE_MUTATION_READY") {
                sender.send(()).unwrap();
                return;
            }
        }
    });
    let mut child = ChildLease(process);
    receiver
        .recv_timeout(Duration::from_secs(4))
        .expect("writer did not durably dispatch");
    let inode = fs::metadata(fixture.path()).unwrap().ino();
    child.crash();
    let service = LeaseService::start(fixture.lease_root()).unwrap();
    let status = service.intent_status(fixture.profiles[0], intent).unwrap();
    assert_eq!(status.record.phase, MutationPhase::ClearDispatched);
    assert!(
        status.outcome_unknown
            && status.recovery_required
            && !status.original_owner_available
            && !status.retry_native
    );
    assert!(
        matches!(service.acquire(fixture.state(), owner(), fixture.profiles[0], LeaseMode::Shared), Err(LeaseError::Quarantined { intent: original, .. }) if original == intent)
    );
    assert!(matches!(
        service.intent_status(fixture.profiles[0], Uuid::new_v4()),
        Err(LeaseError::StaleToken)
    ));
    assert_eq!(fs::metadata(fixture.path()).unwrap().ino(), inode);
    assert_eq!(fs::read(fixture.path()).unwrap(), QUARANTINE_SENTINEL);
    let other = service
        .acquire(
            fixture.state(),
            owner(),
            fixture.profiles[1],
            LeaseMode::Shared,
        )
        .unwrap();
    service.release(other).unwrap();
}

#[test]
fn known_partial_failure_restores_shared_ownership_and_catalog_retry_never_replays_native() {
    let fixture = Fixture::new();
    let service = LeaseService::start(fixture.lease_root()).unwrap();
    let owner = owner();
    let shared = service
        .acquire(
            fixture.state(),
            owner,
            fixture.profiles[0],
            LeaseMode::Shared,
        )
        .unwrap();
    service.constructed(shared).unwrap();
    let request = mutation(&fixture, owner.epoch, vec![shared], MutationKind::Clear);
    let first = service.mutation_start(request).unwrap();
    for phase in [
        MutationPhase::ClearDispatched,
        MutationPhase::ClearCompleted,
        MutationPhase::RemoveDispatched,
    ] {
        service.mutation_advance(first, phase).unwrap();
    }
    service
        .mutation_finish(
            first,
            NativeMutationOutcome::RemovalFailed {
                message: "known namespace removal failure".into(),
            },
        )
        .unwrap();
    assert_eq!(
        service
            .intent_status(first.profile, first.intent)
            .unwrap()
            .record
            .phase,
        MutationPhase::FailedPartial
    );
    assert_eq!(service.status(owner).unwrap(), Some(shared));
    assert!(fs::read(fixture.path()).unwrap().is_empty());
    let second = service
        .mutation_start(mutation(
            &fixture,
            owner.epoch,
            vec![shared],
            MutationKind::Delete,
        ))
        .unwrap();
    assert!(matches!(
        service.intent_status(first.profile, first.intent),
        Err(LeaseError::StaleToken)
    ));
    assert!(matches!(
        service.mutation_advance(first, MutationPhase::RemoveCompleted),
        Err(LeaseError::StaleToken)
    ));
    for phase in [
        MutationPhase::ClearDispatched,
        MutationPhase::ClearCompleted,
        MutationPhase::RemoveDispatched,
        MutationPhase::RemoveCompleted,
    ] {
        service.mutation_advance(second, phase).unwrap();
    }
    let original = fixture.root.join("original-state");
    fs::rename(fixture.state(), &original).unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(fixture.state())
        .unwrap();
    assert!(matches!(
        service.mutation_finish(second, NativeMutationOutcome::Completed),
        Err(LeaseError::UnsafeIntent)
    ));
    assert_eq!(
        service
            .intent_status(second.profile, second.intent)
            .unwrap()
            .record
            .phase,
        MutationPhase::CatalogCommitting
    );
    assert_eq!(fs::read(fixture.path()).unwrap(), QUARANTINE_SENTINEL);
    assert!(matches!(
        service.mutation_advance(second, MutationPhase::ClearDispatched),
        Err(LeaseError::StaleToken)
    ));
    fs::remove_dir(fixture.state()).unwrap();
    fs::rename(original, fixture.state()).unwrap();
    service
        .mutation_finish(second, NativeMutationOutcome::Completed)
        .unwrap();
    assert_eq!(
        service
            .intent_status(second.profile, second.intent)
            .unwrap()
            .record
            .phase,
        MutationPhase::Completed
    );
}
