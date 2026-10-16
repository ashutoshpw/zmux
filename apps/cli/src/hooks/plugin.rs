//! Private, session-only Claude plugin files. No provider settings are read or edited.
use super::*;
use rustix::fs::{self as at, AtFlags, Mode, OFlags};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::File,
    os::unix::fs::MetadataExt,
    path::{Component, Path},
};
use uuid::Uuid;

pub(super) const SUPPORTED_VERSION: &str = "2.1.301";
const RECORD: &str = "installation.json";
const LIMIT: usize = 65536;
pub(super) struct Directory(pub File);
impl Directory {
    pub fn open(path: &Path, create: bool) -> Result<Self, CliError> {
        if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(CliError(
                "plugin directory must be absolute without parent traversal".into(),
            ));
        }
        let mut dir = Self(File::from(
            at::open(
                "/",
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?,
        ));
        for component in path.components() {
            if let Component::Normal(name) = component {
                dir = dir.child(name, create)?;
            }
        }
        dir.private()?;
        Ok(dir)
    }
    fn private(&self) -> Result<(), CliError> {
        let m = self.0.metadata()?;
        if m.uid() != rustix::process::geteuid().as_raw() || m.mode() & 0o7777 != 0o700 {
            return Err(CliError(
                "plugin directory must be owned by this user with mode0700".into(),
            ));
        }
        Ok(())
    }
    pub fn child(&self, name: &OsStr, create: bool) -> Result<Self, CliError> {
        if create {
            match at::mkdirat(&self.0, name, Mode::from_raw_mode(0o700)) {
                Ok(()) => self.0.sync_all()?,
                Err(e) if e == rustix::io::Errno::EXIST => (),
                Err(e) => return Err(io::Error::from(e).into()),
            }
        }
        Ok(Self(File::from(
            at::openat(
                &self.0,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?,
        )))
    }
    pub fn exists(&self, name: &str) -> Result<bool, CliError> {
        match at::statat(&self.0, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => Ok(true),
            Err(e) if e == rustix::io::Errno::NOENT => Ok(false),
            Err(e) => Err(io::Error::from(e).into()),
        }
    }
    fn file(&self, name: &str) -> Result<File, CliError> {
        let file = File::from(
            at::openat(
                &self.0,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?,
        );
        let m = file.metadata()?;
        if !m.is_file()
            || m.uid() != rustix::process::geteuid().as_raw()
            || m.mode() & 0o7777 != 0o600
            || m.nlink() != 1
            || m.len() > LIMIT as u64
        {
            return Err(CliError("unsafe or oversized owned plugin file".into()));
        }
        Ok(file)
    }
    fn identity(&self, name: &str) -> Result<FileIdentity, CliError> {
        Ok(FileIdentity::of(&self.file(name)?.metadata()?))
    }
    pub fn read(&self, name: &str) -> Result<Vec<u8>, CliError> {
        read_file(self.file(name)?)
    }
    fn verify_owned(
        &self,
        name: &str,
        identity: &FileIdentity,
        expected: &[u8],
        missing: bool,
    ) -> Result<(), CliError> {
        if missing && !self.exists(name)? {
            return Ok(());
        }
        let file = self.file(name)?;
        if FileIdentity::of(&file.metadata()?) != *identity || read_file(file)? != expected {
            return Err(CliError(
                "changed owned plugin inode/content retained".into(),
            ));
        }
        Ok(())
    }
    fn remove_owned(
        &self,
        name: &str,
        identity: &FileIdentity,
        expected: &[u8],
    ) -> Result<(), CliError> {
        if !self.exists(name)? {
            return Ok(());
        }
        self.verify_owned(name, identity, expected, false)?;
        at::unlinkat(&self.0, name, AtFlags::empty()).map_err(io::Error::from)?;
        self.0.sync_all()?;
        Ok(())
    }
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<(), CliError> {
        let temporary = format!(".{}", Uuid::new_v4());
        let mut file = File::from(
            at::openat(
                &self.0,
                temporary.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )
            .map_err(io::Error::from)?,
        );
        file.write_all(bytes)?;
        file.sync_all()?;
        at::renameat(&self.0, temporary.as_str(), &self.0, name).map_err(io::Error::from)?;
        self.0.sync_all().map_err(|_|CliError("owned plugin file published but directory sync failed; durability unknown, inspect hooks claude status before retry".into()))?;
        Ok(())
    }
    fn remove(&self, name: &str, expected: &[u8]) -> Result<(), CliError> {
        if self.read(name)? != expected {
            return Err(CliError(
                "changed owned file retained; uninstall incomplete".into(),
            ));
        }
        at::unlinkat(&self.0, name, AtFlags::empty()).map_err(io::Error::from)?;
        self.0.sync_all()?;
        Ok(())
    }
    fn try_runtime_lock(&self, expected: &FileIdentity) -> Result<Option<File>, CliError> {
        if !self.exists("runtime.lock")? {
            return Ok(None);
        }
        self.verify_owned("runtime.lock", expected, b"", false)?;
        let file = File::from(
            at::openat(
                &self.0,
                "runtime.lock",
                OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?,
        );
        if FileIdentity::of(&file.metadata()?) != *expected {
            return Err(CliError("changed runtime lock retained".into()));
        }
        at::flock(&file,at::FlockOperation::NonBlockingLockExclusive).map_err(|_|CliError("active provider retains its plugin generation; adapter disabled, retry uninstall after provider exit".into()))?;
        Ok(Some(file))
    }
    pub fn lock(&self, name: &str, shared: bool) -> Result<File, CliError> {
        let file = File::from(
            at::openat(
                &self.0,
                name,
                OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )
            .map_err(io::Error::from)?,
        );
        let m = file.metadata()?;
        if !m.is_file()
            || m.uid() != rustix::process::geteuid().as_raw()
            || m.mode() & 0o7777 != 0o600
            || m.nlink() != 1
            || m.len() != 0
        {
            return Err(CliError("unsafe plugin ownership lock".into()));
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let op = if shared {
            at::FlockOperation::NonBlockingLockShared
        } else {
            at::FlockOperation::NonBlockingLockExclusive
        };
        loop {
            match at::flock(&file, op) {
                Ok(()) => return Ok(file),
                Err(e) if e == rustix::io::Errno::WOULDBLOCK && Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(e) => return Err(io::Error::from(e).into()),
            }
        }
    }
}
fn read_file(mut file: File) -> Result<Vec<u8>, CliError> {
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        return Err(CliError("owned plugin file exceeds64KiB".into()));
    }
    Ok(bytes)
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileIdentity {
    device: u64,
    inode: u64,
}
impl FileIdentity {
    fn of(m: &std::fs::Metadata) -> Self {
        Self {
            device: m.dev(),
            inode: m.ino(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedFiles {
    manifest: FileIdentity,
    hooks: FileIdentity,
    owner: FileIdentity,
    runtime_lock: FileIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binary {
    pub path: PathBuf,
    pub sha256: String,
    identity: FileIdentity,
    size: u64,
    modified: [i64; 2],
}
pub(super) fn binary(path: &Path) -> Result<Binary, CliError> {
    let path = path.canonicalize()?;
    let mut file = File::from(
        at::open(
            &path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?,
    );
    let m = file.metadata()?;
    if !m.is_file() || m.mode() & 0o022 != 0 || m.mode() & 0o111 == 0 || m.len() > 512 * 1024 * 1024
    {
        return Err(CliError(
            "provider/adapter must be a bounded, non-writable executable file".into(),
        ));
    }
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    let mut count = 0;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n;
        if count > 512 * 1024 * 1024 {
            return Err(CliError("provider executable grew beyond512MiB".into()));
        }
        digest.update(&buffer[..n]);
    }
    if file.metadata()?.len() != m.len()
        || [file.metadata()?.mtime(), file.metadata()?.mtime_nsec()] != [m.mtime(), m.mtime_nsec()]
    {
        return Err(CliError("executable changed while hashing".into()));
    }
    Ok(Binary {
        path,
        sha256: format!("{:x}", digest.finalize()),
        identity: FileIdentity::of(&m),
        size: m.len(),
        modified: [m.mtime(), m.mtime_nsec()],
    })
}
pub(super) fn metadata_matches(binary: &Binary) -> bool {
    fs::symlink_metadata(&binary.path).is_ok_and(|m| {
        m.is_file()
            && m.mode() & 0o022 == 0
            && FileIdentity::of(&m) == binary.identity
            && m.len() == binary.size
            && [m.mtime(), m.mtime_nsec()] == binary.modified
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Installation {
    pub generation: Uuid,
    pub adapter: Binary,
    pub provider: Binary,
    pub provider_version: String,
    files: OwnedFiles,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    format: u32,
    enabled: bool,
    generations: Vec<Installation>,
}
fn bytes(value: &impl Serialize) -> Result<Vec<u8>, CliError> {
    serde_json::to_vec(value).map_err(|e| CliError(e.to_string()))
}
fn record(root: &Directory) -> Result<Record, CliError> {
    let value: Record = serde_json::from_slice(&root.read(RECORD)?)
        .map_err(|_| CliError("invalid plugin installation record; retained unchanged".into()))?;
    let mut unique = std::collections::HashSet::new();
    if value.format != 1
        || value.generations.is_empty()
        || value.generations.len() > 8
        || value.generations.iter().any(|g| {
            g.generation.is_nil()
                || !unique.insert(g.generation)
                || g.provider_version != SUPPORTED_VERSION
        })
    {
        return Err(CliError(
            "unsupported plugin installation record; retained unchanged".into(),
        ));
    }
    Ok(value)
}
fn owner_bytes(i: &Installation) -> Result<Vec<u8>, CliError> {
    bytes(
        &json!({"generation":i.generation,"adapter":i.adapter,"provider":i.provider,"provider_version":i.provider_version}),
    )
}
fn artifacts(i: &Installation) -> (Value, Value) {
    let mut hooks = serde_json::Map::new();
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PermissionRequest",
        "Elicitation",
        "PreToolUse",
        "Stop",
        "SessionEnd",
    ] {
        let mut hook = json!({"type":"command","command":i.adapter.path,
            "args":["hooks","claude","event",event],"timeout":3});
        // stdout from lifecycle events must never be exposed as additional context.
        hook["statusMessage"] = json!("zmux inbox");
        hooks.insert(event.into(), json!([{"hooks":[hook]}]));
    }
    (
        json!({"name":"zmux-local-inbox","version":"0.1.0","description":"Opt-in local inbox context; provider acceptance unconfirmed"}),
        json!({"hooks":hooks}),
    )
}
fn generation(root: &Directory, i: &Installation) -> Result<Directory, CliError> {
    let dir = root.child(OsStr::new(&i.generation.to_string()), false)?;
    dir.private()?;
    let (plugin, hooks) = artifacts(i);
    let manifest = dir.child(OsStr::new(".claude-plugin"), false)?;
    let config = dir.child(OsStr::new("hooks"), false)?;
    manifest.private()?;
    config.private()?;
    manifest.verify_owned("plugin.json", &i.files.manifest, &bytes(&plugin)?, false)?;
    config.verify_owned("hooks.json", &i.files.hooks, &bytes(&hooks)?, false)?;
    dir.verify_owned("owner.json", &i.files.owner, &owner_bytes(i)?, false)?;
    dir.verify_owned("runtime.lock", &i.files.runtime_lock, b"", false)?;
    Ok(dir)
}
pub(super) fn install(path: &Path, provider: Binary) -> Result<Value, CliError> {
    let root = Directory::open(path, true)?;
    let _lock = root.lock("installation.lock", false)?;
    let adapter = binary(&env::current_exe()?)?;
    let mut owned = if root.exists(RECORD)? {
        record(&root)?
    } else {
        Record {
            format: 1,
            enabled: false,
            generations: vec![],
        }
    };
    if let Some(current) = owned.generations.last() {
        generation(&root, current)?;
        if owned.enabled && current.adapter == adapter && current.provider == provider {
            return Ok(
                json!({"installed":true,"changed":false,"generation":current.generation,"session_only":true}),
            );
        }
    }
    if owned.generations.len() >= 8 {
        return Err(CliError(
            "plugin generation capacity reached; explicitly uninstall retained generations first"
                .into(),
        ));
    }
    let mut installed = Installation {
        generation: Uuid::new_v4(),
        adapter,
        provider,
        provider_version: SUPPORTED_VERSION.into(),
        files: OwnedFiles::default(),
    };
    let dir = root.child(OsStr::new(&installed.generation.to_string()), true)?;
    dir.private()?;
    let (plugin, hooks) = artifacts(&installed);
    dir.child(OsStr::new(".claude-plugin"), true)?
        .write("plugin.json", &bytes(&plugin)?)?;
    dir.child(OsStr::new("hooks"), true)?
        .write("hooks.json", &bytes(&hooks)?)?;
    dir.write("owner.json", &owner_bytes(&installed)?)?;
    drop(dir.lock("runtime.lock", true)?);
    installed.files = OwnedFiles {
        manifest: dir
            .child(OsStr::new(".claude-plugin"), false)?
            .identity("plugin.json")?,
        hooks: dir
            .child(OsStr::new("hooks"), false)?
            .identity("hooks.json")?,
        owner: dir.identity("owner.json")?,
        runtime_lock: dir.identity("runtime.lock")?,
    };
    generation(&root, &installed)?;
    owned.enabled = true;
    owned.generations.push(installed.clone());
    root.write(RECORD, &bytes(&owned)?)?;
    Ok(
        json!({"installed":true,"changed":true,"generation":installed.generation,"session_only":true,"previous_generations_retained":owned.generations.len()-1}),
    )
}
pub(super) fn publication_matches(path: &Path, id: Uuid) -> bool {
    Directory::open(path, false)
        .and_then(|root| record(&root))
        .is_ok_and(|r| r.enabled && r.generations.last().is_some_and(|i| i.generation == id))
}
pub(super) fn active(path: &Path) -> Result<Installation, CliError> {
    let root = Directory::open(path, false)?;
    let r = record(&root)?;
    if !r.enabled {
        return Err(CliError("local Claude adapter is disabled".into()));
    }
    let current = r
        .generations
        .last()
        .ok_or_else(|| CliError("missing plugin generation".into()))?;
    generation(&root, current)?;
    Ok(current.clone())
}
pub(super) fn status(path: &Path) -> Result<Value, CliError> {
    if !path.try_exists()? {
        return Ok(json!({"installed":false,"session_only":true}));
    }
    let root = Directory::open(path, false)?;
    if !root.exists(RECORD)? {
        return Ok(json!({"installed":false,"session_only":true}));
    }
    let r = record(&root)?;
    if r.enabled {
        for i in &r.generations {
            generation(&root, i)?;
        }
    }
    Ok(
        json!({"installed":r.enabled,"session_only":true,"generations":r.generations,"provider_authenticated":false,"provider_acceptance_confirmed":false}),
    )
}
pub(super) fn lease(path: &Path, i: &Installation) -> Result<File, CliError> {
    let root = Directory::open(path, false)?;
    let dir = generation(&root, i)?;
    let lock = dir.lock("runtime.lock", true)?;
    if FileIdentity::of(&lock.metadata()?) != i.files.runtime_lock {
        return Err(CliError("plugin runtime lock inode changed".into()));
    }
    Ok(lock)
}
pub(super) fn uninstall(path: &Path) -> Result<Value, CliError> {
    if !path.try_exists()? {
        return Ok(json!({"installed":false,"cleanup_pending":false}));
    }
    let root = Directory::open(path, false)?;
    if !root.exists(RECORD)? {
        return Ok(json!({"installed":false,"cleanup_pending":false}));
    }
    let _lock = root.lock("installation.lock", false)?;
    let mut r = record(&root)?;
    // Disable admission durably before attempting removal. A running wrapper
    // observes this publication, retires its exact capability, and keeps its Child.
    r.enabled = false;
    root.write(RECORD, &bytes(&r)?)?;
    let mut retained = Vec::new();
    let mut diagnostics = Vec::new();
    for i in &r.generations {
        let result = (|| -> Result<(), CliError> {
            let name = i.generation.to_string();
            if !root.exists(&name)? {
                return Ok(());
            }
            let dir = root.child(OsStr::new(&name), false)?;
            dir.private()?;
            let _lease = dir.try_runtime_lock(&i.files.runtime_lock)?;
            let (plugin, hooks) = artifacts(i);
            // Missing files/directories can be a prior interrupted uninstall.
            // Every surviving file is revalidated before the first unlink.
            let manifest = if dir.exists(".claude-plugin")? {
                Some(dir.child(OsStr::new(".claude-plugin"), false)?)
            } else {
                None
            };
            let config = if dir.exists("hooks")? {
                Some(dir.child(OsStr::new("hooks"), false)?)
            } else {
                None
            };
            if let Some(manifest) = &manifest {
                manifest.private()?;
                manifest.verify_owned("plugin.json", &i.files.manifest, &bytes(&plugin)?, true)?;
            }
            if let Some(config) = &config {
                config.private()?;
                config.verify_owned("hooks.json", &i.files.hooks, &bytes(&hooks)?, true)?;
            }
            dir.verify_owned("owner.json", &i.files.owner, &owner_bytes(i)?, true)?;
            if let Some(manifest) = manifest {
                manifest.remove_owned("plugin.json", &i.files.manifest, &bytes(&plugin)?)?;
                at::unlinkat(&dir.0, ".claude-plugin", AtFlags::REMOVEDIR)
                    .map_err(io::Error::from)?;
            }
            if let Some(config) = config {
                config.remove_owned("hooks.json", &i.files.hooks, &bytes(&hooks)?)?;
                at::unlinkat(&dir.0, "hooks", AtFlags::REMOVEDIR).map_err(io::Error::from)?;
            }
            dir.remove_owned("owner.json", &i.files.owner, &owner_bytes(i)?)?;
            dir.remove_owned("runtime.lock", &i.files.runtime_lock, b"")?;
            at::unlinkat(&root.0, name.as_str(), AtFlags::REMOVEDIR).map_err(io::Error::from)?;
            root.0.sync_all()?;
            Ok(())
        })();
        if let Err(e) = result {
            retained.push(i.clone());
            diagnostics.push(e.0);
        }
    }
    if retained.is_empty() {
        root.remove(RECORD, &bytes(&r)?)?;
    } else {
        r.generations = retained;
        root.write(RECORD, &bytes(&r)?)?;
    }
    Ok(
        json!({"installed":false,"disabled":true,"cleanup_pending":!diagnostics.is_empty(),"diagnostics":diagnostics,
        "retirement":"live wrappers retire only their own capability; unavailable wrappers cannot be adopted", "rollback":"install with a new private --directory after reviewing retained artifacts; old capabilities and exposure attempts are never restored"}),
    )
}
