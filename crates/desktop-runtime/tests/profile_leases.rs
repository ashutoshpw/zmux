use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_desktop_runtime::{
    persistence::{OperationId, PersistenceError, PersistenceOutcome, PersistenceWorker},
    profile_leases::{LeaseError, LeaseMode, LeaseOwner, LeaseService},
};
use zmux_workspace_core::{Action, RestoreLimits, SessionDocument, WorkspaceStore};

struct Fixture {
    root: PathBuf,
    worker: Option<PersistenceWorker>,
    profiles: [Uuid; 2],
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("zmux-profile-leases-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let worker = PersistenceWorker::start(
            root.join("state"),
            RestoreLimits {
                terminals: 8,
                browsers: 4,
            },
        )
        .unwrap();
        let PersistenceOutcome::Profiles(catalog) = wait(
            &worker,
            worker.submit_profiles(Some("Work".into())).unwrap(),
        )
        .unwrap() else {
            panic!("missing catalog")
        };
        let first = catalog.profiles[0].id;
        let PersistenceOutcome::Profiles(catalog) = wait(
            &worker,
            worker.submit_profiles(Some("Other".into())).unwrap(),
        )
        .unwrap() else {
            panic!("missing catalog")
        };
        Self {
            root,
            worker: Some(worker),
            profiles: [first, catalog.profiles[1].id],
        }
    }
    fn state(&self) -> PathBuf {
        self.root.join("state")
    }
    fn lease_root(&self) -> PathBuf {
        self.root.join("leases")
    }
    fn path(&self) -> PathBuf {
        self.lease_root()
            .join(format!("browser-profile-{}.lock", self.profiles[0]))
    }
    fn stop_catalog(&mut self) {
        if let Some(mut worker) = self.worker.take() {
            worker
                .join_until(Instant::now() + Duration::from_secs(2))
                .unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop_catalog();
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn owner() -> LeaseOwner {
    LeaseOwner {
        epoch: Uuid::new_v4(),
        store: Uuid::new_v4(),
    }
}
fn wait(
    worker: &PersistenceWorker,
    id: OperationId,
) -> Result<PersistenceOutcome, PersistenceError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = worker.try_completion().unwrap() {
            assert_eq!(value.operation_id, id);
            return value.result;
        }
        assert!(Instant::now() < deadline, "catalog I/O did not complete");
        thread::sleep(Duration::from_millis(2));
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
