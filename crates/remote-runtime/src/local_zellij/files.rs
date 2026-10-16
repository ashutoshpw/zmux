use super::{START_TIMEOUT, check_deadline, executable, fail, process, supervisor};
use nix::{
    fcntl::{Flock, FlockArg},
    unistd::geteuid,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_session_runtime::child::ProcessIdentity;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: Uuid,
    pub name: String,
    pub state: String,
    pub server: Option<ProcessIdentity>,
    pub supervisor: Option<ProcessIdentity>,
    pub workload: Option<ProcessIdentity>,
    pub namespace: Option<NamespaceIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<super::completion::Capability>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamespaceIdentity {
    pub root: (u64, u64),
    pub socket: (u64, u64),
}
pub(super) fn inode(path: &Path) -> io::Result<(u64, u64)> {
    let metadata = path.symlink_metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}
/// Socket namespaces must fit Darwin's sockaddr_un after Zellij appends its
/// version and full session UUID. The system TMPDIR may already exceed that
/// budget; use the canonical short /tmp base with private random child roots.
pub(super) fn runtime_base() -> io::Result<PathBuf> {
    let base = Path::new("/tmp").canonicalize()?;
    let info = fs::symlink_metadata(&base)?;
    // A shared base must preserve private children against other users' unlink
    // or rename operations. Root or this user may own it; writable shared bases
    // require the sticky bit. Each random child is still created exclusively.
    if !info.is_dir()
        || (info.uid() != 0 && info.uid() != geteuid().as_raw())
        || (info.mode() & 0o022 != 0 && info.mode() & 0o1000 == 0)
    {
        return Err(fail("unsafe short socket runtime base owner/mode/type"));
    }
    Ok(base)
}
impl Record {
    pub(super) fn session_name(&self) -> String {
        format!("zmux-{}", self.id)
    }
    pub(super) fn runtime(&self) -> io::Result<PathBuf> {
        Ok(runtime_base()?.join(format!("z{}", self.id.simple())))
    }
    pub(super) fn backend(&self) -> io::Result<PathBuf> {
        Ok(self.runtime()?.join("s/0.43.0").join(self.session_name()))
    }
    /// Pure original binding reconstruction; this is not live authentication.
    pub fn committed_binding(&self) -> io::Result<super::completion::CommittedBinding> {
        let capability = self
            .completion
            .as_ref()
            .ok_or_else(|| fail("owner has no completion capability"))?;
        let binding = super::completion::CommittedBinding {
            version: 1,
            owner_uuid: self.id,
            owner_generation: capability.owner_generation,
            namespace: self
                .namespace
                .clone()
                .ok_or_else(|| fail("missing owner namespace"))?,
            completion_socket: capability.completion_socket,
            server: self
                .server
                .clone()
                .ok_or_else(|| fail("missing owner server"))?,
            supervisor: self
                .supervisor
                .clone()
                .ok_or_else(|| fail("missing owner supervisor"))?,
            workload: self
                .workload
                .clone()
                .ok_or_else(|| fail("missing owner workload"))?,
        };
        if binding.digest()? != capability.binding_digest {
            return Err(fail("owner completion binding digest mismatch"));
        }
        Ok(binding)
    }
    /// Revalidate the original owner without adopting or signalling its processes.
    pub fn verify_live(&self) -> io::Result<()> {
        if self.state != "bound" {
            return Err(fail(
                "owner is not committed; inspect status, never replay creation",
            ));
        }
        let server = self
            .server
            .as_ref()
            .ok_or_else(|| fail("missing server identity"))?;
        let supervisor = self
            .supervisor
            .as_ref()
            .ok_or_else(|| fail("missing supervisor identity"))?;
        let workload = self
            .workload
            .as_ref()
            .ok_or_else(|| fail("missing workload identity"))?;
        if supervisor.parent_pid != server.pid
            || workload.parent_pid != supervisor.pid
            || [server.uid, supervisor.uid, workload.uid]
                .iter()
                .any(|uid| *uid != geteuid().as_raw())
        {
            return Err(fail("owner process lineage changed"));
        }
        files_runtime(self)?;
        for identity in [&self.server, &self.supervisor, &self.workload] {
            super::same(
                identity
                    .as_ref()
                    .ok_or_else(|| fail("owner process binding is incomplete"))?,
            )?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct LocalZellij {
    pub(super) root: PathBuf,
    pub(super) binary: PathBuf,
    pub(super) program: PathBuf,
}
impl LocalZellij {
    pub fn from_environment(
        mut terminal: Option<&mut process::ForegroundTerminal>,
    ) -> io::Result<Self> {
        let root = if let Some(root) = std::env::var_os("ZMUX_LOCAL_ZELLIJ_ROOT") {
            PathBuf::from(root)
        } else {
            let home =
                PathBuf::from(std::env::var_os("HOME").ok_or_else(|| fail("HOME is missing"))?)
                    .canonicalize()?;
            let config = home.join(".config");
            directory(&config, false)?;
            let product = config.join("zmux");
            directory(&product, true)?;
            product.join("local-zellij")
        };
        if !root.is_absolute() {
            return Err(fail("local-zellij profile must be absolute"));
        }
        directory(&root, true)?;
        let selected = std::env::var_os("ZMUX_LOCAL_ZELLIJ_BIN")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("PATH").and_then(|path| {
                    std::env::split_paths(&path)
                        .map(|base| base.join("zellij"))
                        .find(|path| path.is_file())
                })
            })
            .ok_or_else(|| fail("install supported zellij0.43.0 or set ZMUX_LOCAL_ZELLIJ_BIN"))?;
        let binary = executable(&selected)?;
        let mut version = std::process::Command::new(&binary);
        super::super::local_multiplexer::clear_context(&mut version);
        version.env_remove(super::observer::ENV);
        if process::capture(
            version.arg("--version"),
            Instant::now() + Duration::from_secs(5),
            || {
                if let Some(terminal) = terminal.as_mut() {
                    terminal.startup_events()
                } else {
                    Ok(())
                }
            },
        )?
        .trim()
            != "zellij 0.43.0"
        {
            return Err(fail("only zellij0.43.0 is supported"));
        }
        Ok(Self {
            root,
            binary,
            program: executable(&std::env::current_exe()?)?,
        })
    }
    pub(super) fn lock(&self) -> io::Result<Flock<File>> {
        self.lock_while(Instant::now() + START_TIMEOUT, || Ok(()))
    }
    fn lock_while(
        &self,
        deadline: Instant,
        mut tick: impl FnMut() -> io::Result<()>,
    ) -> io::Result<Flock<File>> {
        directory(&self.root, true)?;
        let path = self.root.join("registry.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(path)?;
        private_file(&file)?;
        let mut file = file;
        loop {
            tick()?;
            check_deadline(deadline, "registry lock")?;
            match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
                Ok(lock) => return Ok(lock),
                Err((returned, error)) if error == nix::errno::Errno::EWOULDBLOCK => {
                    file = returned;
                    check_deadline(deadline, "registry lock")?;
                    thread::sleep(Duration::from_millis(10));
                }
                Err((_, error)) => return Err(error.into()),
            }
        }
    }
    pub(super) fn records(&self) -> io::Result<Vec<Record>> {
        let path = self.root.join("registry.json");
        let records: Vec<Record> = match read_private(&path, 128 * 1024) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        if records.len() > 64
            || records
                .iter()
                .any(|record| record.id.is_nil() || !valid_name(&record.name))
        {
            return Err(fail("invalid bounded zellij registry"));
        }
        Ok(records)
    }
    pub(super) fn save(&self, records: &[Record]) -> io::Result<()> {
        let bytes = serde_json::to_vec(records).map_err(io::Error::other)?;
        if bytes.len() > 128 * 1024 {
            return Err(fail("zellij registry capacity reached"));
        }
        let pending = self
            .root
            .join(format!("pending-{}", Uuid::new_v4().simple()));
        write_private(&pending, &bytes)?;
        fs::rename(pending, self.root.join("registry.json"))?;
        File::open(&self.root)?.sync_all()
    }
    pub fn list(&self) -> io::Result<Vec<Record>> {
        let _lock = self.lock()?;
        let mut records = self.records()?;
        for record in &mut records {
            if record.verify_live().is_err() {
                record.state = "uncertain".into();
            } else {
                record.state = "identity_present".into();
            }
        }
        Ok(records)
    }
    pub fn status(&self, id: Uuid) -> io::Result<Record> {
        self.list()?
            .into_iter()
            .find(|record| record.id == id)
            .ok_or_else(|| fail("local zellij record was not found"))
    }
    pub fn start(
        &self,
        name: &str,
        cwd: &Path,
        command: Option<&str>,
        terminal: &mut process::ForegroundTerminal,
    ) -> io::Result<i32> {
        terminal.startup_events()?;
        if !valid_name(name)
            || !cwd.is_absolute()
            || !cwd.is_dir()
            || command.is_some_and(|text| {
                text.len() > 8192 || text.contains('\0') || text.trim().is_empty()
            })
        {
            return Err(fail("invalid bounded zellij name/cwd/start command"));
        }
        let deadline = Instant::now() + super::START_TIMEOUT;
        let mut observer =
            super::observer::Observer::configured(deadline, || terminal.startup_events())?;
        let lock = if observer.is_some() {
            self.lock_while(deadline, || terminal.startup_events())?
        } else {
            self.lock()?
        };
        let mut records = self.records()?;
        if records.len() >= 64 {
            return Err(fail(
                "registry is full (64 owner slots); inspect list/status and reclaim only completed generations with cleanup --id UUID --generation UUID; do not replay start",
            ));
        }
        if records.iter().any(|record| record.name == name) {
            return Err(fail(
                "session name exists; inspect list/status, do not replay start",
            ));
        }
        let mut record = Record {
            id: Uuid::new_v4(),
            name: name.into(),
            state: "reserved".into(),
            server: None,
            supervisor: None,
            workload: None,
            namespace: None,
            completion: None,
        };
        records.push(record.clone());
        self.save(&records)?;
        let result = (|| {
            if let Some(observer) = &mut observer {
                observer.reserved(record.id, deadline, || terminal.startup_events())?;
            }
            supervisor::create(
                self,
                &mut record,
                cwd,
                command,
                terminal,
                observer.as_ref().map(|_| deadline),
                |record| {
                    *records
                        .last_mut()
                        .ok_or_else(|| fail("reservation disappeared"))? = record.clone();
                    self.save(&records)
                },
            )
        })();
        if let Err(error) = result {
            record.state = "uncertain".into();
            *records
                .last_mut()
                .ok_or_else(|| fail("reservation disappeared"))? = record;
            let saved = self.save(&records);
            return Err(fail(format!(
                "creation outcome unknown for{}: {error}; reservation retained, do not replay; registry_update={saved:?}",
                records.last().expect("reserved record").id
            )));
        }
        drop(lock);
        eprintln!(
            "zmux local-zellij reserved owner{} (foreground transport only)",
            record.id
        );
        self.attach_record(record, terminal, observer, deadline)
    }
    pub fn attach(&self, id: Uuid, terminal: &mut process::ForegroundTerminal) -> io::Result<i32> {
        let deadline = Instant::now() + super::START_TIMEOUT;
        let observer =
            super::observer::Observer::configured(deadline, || terminal.startup_events())?;
        let record = {
            let _lock = if observer.is_some() {
                self.lock_while(deadline, || terminal.startup_events())?
            } else {
                self.lock()?
            };
            self.records()?
                .into_iter()
                .find(|record| record.id == id)
                .ok_or_else(|| fail("local zellij record was not found"))?
        };
        self.attach_record(record, terminal, observer, deadline)
    }
    pub(super) fn consume_terminal(
        &self,
        record: &Record,
        receipt: &super::completion::TerminalReceiptV1,
    ) -> io::Result<()> {
        receipt.matches(record)?;
        let _lock = self.lock()?;
        let current = self
            .records()?
            .into_iter()
            .find(|r| r.id == record.id)
            .ok_or_else(|| fail("completion owner missing from profile"))?;
        receipt.matches(&current)?;
        super::completion_control::publish(
            &self.root,
            &format!("consumed-{}.json", record.id),
            &serde_json::to_vec(receipt).map_err(io::Error::other)?,
        )
    }
    fn attach_record(
        &self,
        record: Record,
        terminal: &mut process::ForegroundTerminal,
        mut observer: Option<super::observer::Observer>,
        deadline: Instant,
    ) -> io::Result<i32> {
        if record.completion.is_none() {
            record.verify_live()?;
        }
        if record
            .server
            .as_ref()
            .is_none_or(|server| server.executable != self.binary)
            || record
                .supervisor
                .as_ref()
                .is_none_or(|supervisor| supervisor.executable != self.program)
        {
            return Err(fail("configured executable differs from reserved owner"));
        }
        if let Some(observer) = &mut observer {
            observer.bound(&record, deadline, || terminal.startup_events())?;
        }
        super::broker::attach(self, record, terminal, observer, deadline)
    }
}

pub(super) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
pub(super) fn directory(path: &Path, private: bool) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error),
    }
    validate_directory(path, private)
}
pub(super) fn validate_directory(path: &Path, private: bool) -> io::Result<()> {
    let info = fs::symlink_metadata(path)?;
    if !info.is_dir()
        || info.uid() != geteuid().as_raw()
        || info.mode() & if private { 0o077 } else { 0o022 } != 0
    {
        return Err(fail("unsafe namespace owner/mode/type"));
    }
    Ok(())
}
fn private_file(file: &File) -> io::Result<()> {
    let info = file.metadata()?;
    if !info.is_file()
        || info.uid() != geteuid().as_raw()
        || info.mode() & 0o077 != 0
        || info.nlink() != 1
    {
        return Err(fail("unsafe private file"));
    }
    Ok(())
}
pub(super) fn read_private(path: &Path, bound: usize) -> io::Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)?;
    private_file(&file)?;
    let mut bytes = Vec::new();
    file.take((bound + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > bound {
        return Err(fail("private file exceeds its bound"));
    }
    Ok(bytes)
}
pub(super) fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
pub(super) fn socket_path(path: &Path) -> io::Result<()> {
    if path.as_os_str().len() >= 100 {
        Err(fail("zellij Unix socket exceeds portable99-byte limit"))
    } else {
        Ok(())
    }
}

fn files_runtime(record: &Record) -> io::Result<()> {
    let root = record.runtime()?;
    for path in [
        &root,
        &root.join("s"),
        &root.join("s/0.43.0"),
        &root.join("cache"),
        &root.join("config"),
    ] {
        validate_directory(path, true)?;
    }
    let namespace = record
        .namespace
        .as_ref()
        .ok_or_else(|| fail("missing immutable namespace binding"))?;
    if inode(&root)? != namespace.root || inode(&record.backend()?)? != namespace.socket {
        return Err(fail("reserved namespace/socket was replaced"));
    }
    let backend = record.backend()?.symlink_metadata()?;
    if !backend.file_type().is_socket()
        || backend.uid() != geteuid().as_raw()
        || backend.mode() & 0o077 != 0
    {
        return Err(fail("unsafe backend socket"));
    }
    for entry in fs::read_dir(root.join("s/0.43.0"))?.take(2) {
        if entry?.file_name() != std::ffi::OsStr::new(&record.session_name()) {
            return Err(fail("unexpected backend socket or prefix shadow"));
        }
    }
    let mut pending = vec![(root.join("cache"), 0)];
    let mut count = 0;
    while let Some((directory, depth)) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            count += 1;
            if count > 256 || depth > 8 {
                return Err(fail("cache inspection bound exceeded"));
            }
            let entry = entry?;
            let metadata = entry.path().symlink_metadata()?;
            if metadata.file_type().is_symlink() || entry.file_name() == "session-layout.kdl" {
                return Err(fail("unsafe resurrection cache entry"));
            }
            if metadata.is_dir() {
                pending.push((entry.path(), depth + 1));
            }
        }
    }
    let config = read_private(&root.join("config/config.kdl"), 4096)?;
    if config != super::OWNED_CONFIG {
        return Err(fail("owned non-resurrecting configuration changed"));
    }
    Ok(())
}
