use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_desktop_runtime::persistence::{
    OperationId, PersistenceError, PersistenceOutcome, PersistenceWorker,
};
use zmux_workspace_core::RestoreLimits;
pub struct Fixture {
    pub root: PathBuf,
    pub worker: Option<PersistenceWorker>,
    pub profiles: [Uuid; 2],
}
impl Fixture {
    pub fn new() -> Self {
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
    pub fn state(&self) -> PathBuf {
        self.root.join("state")
    }
    pub fn lease_root(&self) -> PathBuf {
        self.root.join("leases")
    }
    pub fn path(&self) -> PathBuf {
        self.lease_root()
            .join(format!("browser-profile-{}.lock", self.profiles[0]))
    }
    pub fn stop_catalog(&mut self) {
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
pub fn wait(
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

// Command/PTY spawning can inherit any thread's flock descriptor until exec.
// Isolate host test cases, while preserving each case's real subprocess owners.
static PROCESS_FIXTURES: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub fn process_fixture_isolation() -> std::sync::MutexGuard<'static, ()> {
    PROCESS_FIXTURES.lock().unwrap()
}
