//! Persistent named text buffers. Filesystem work stays in the CLI, never on Desktop.
use super::*;
use nix::{
    fcntl::{Flock, FlockArg},
    libc,
    unistd::geteuid,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
};
use zmux_desktop_runtime::targets::{TargetOperation, TargetRequest};
use zmux_workspace_core::targets::TargetScope;

const TEXT_BYTES: usize = 4096;
const STORE_BYTES: usize = 512 * 1024;
const MAX_BUFFERS: usize = 128;
pub(super) enum Invocation {
    Set {
        name: String,
        text: Option<String>,
    },
    List,
    Paste {
        name: String,
        bracketed: bool,
        target: Box<TargetRequest>,
    },
}
impl Invocation {
    pub(super) fn discovery(&self) -> Value {
        match self {
            Self::Set { name, text } => json!({"method":"set_buffer","name":name,"text":text}),
            Self::List => json!({"method":"list_buffers"}),
            Self::Paste {
                name,
                bracketed,
                target,
            } => json!({"method":"paste_buffer","name":name,"bracketed":bracketed,"target":target}),
        }
    }
}
pub(super) fn parse(command: &str, tail: &[OsString]) -> Result<Parsed, CliError> {
    // A literal --help after -- is text, not a discovery request.
    if tail
        .iter()
        .take_while(|a| *a != "--")
        .any(|a| a == "--help" || a == "-h")
    {
        return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
    }
    let args = utf8_args(tail)?;
    let mut name = None;
    let mut bracketed = false;
    let mut scope = TargetScope::default();
    let mut text = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if command == "set-buffer" && (arg == "--" || !arg.starts_with("--")) {
            text.extend_from_slice(&args[index + usize::from(arg == "--")..]);
            break;
        }
        if command == "paste-buffer" && arg == "--bracketed" && !bracketed {
            bracketed = true;
            index += 1;
            continue;
        }
        if arg == "--json" && command == "list-buffers" {
            index += 1;
            continue;
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| CliError(format!("invalid {command} option {arg}")))?;
        match arg.as_str() {
            "--name" if command != "list-buffers" && name.is_none() => name = Some(value.clone()),
            "--window" | "--workspace" | "--pane" | "--surface" | "--tab"
                if command == "paste-buffer" =>
            {
                let slot = match arg.as_str() {
                    "--window" => &mut scope.window,
                    "--workspace" => &mut scope.workspace,
                    "--pane" => &mut scope.pane,
                    _ => &mut scope.surface,
                };
                if slot.is_some() {
                    return Err(CliError(format!("repeated {arg}")));
                }
                *slot = Some(value.parse().map_err(|e| CliError(format!("{e}")))?);
            }
            _ => {
                return Err(CliError(format!(
                    "unknown or repeated {command} option {arg}"
                )));
            }
        }
        index += 2;
    }
    let name = name.unwrap_or_else(|| "default".into());
    validate_name(&name)?;
    let invocation = match command {
        "set-buffer" => {
            let text = if text.is_empty() || text.as_slice() == ["-"] {
                None
            } else {
                Some(text.join(" "))
            };
            if let Some(text) = &text {
                validate_text(text)?;
            }
            Invocation::Set { name, text }
        }
        "list-buffers" => Invocation::List,
        _ => Invocation::Paste {
            name,
            bracketed,
            target: Box::new(TargetRequest {
                caller: targets::caller(&scope)?,
                scope,
                expected_model_revision: None,
                operation: TargetOperation::ReadScreen,
            }),
        },
    };
    Ok(Parsed::Buffers(invocation))
}
fn validate_name(name: &str) -> Result<(), CliError> {
    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err(CliError(
            "buffer name must contain 1–128 UTF-8 bytes without control characters".into(),
        ));
    }
    Ok(())
}
fn validate_text(text: &str) -> Result<(), CliError> {
    if text.is_empty() || text.len() > TEXT_BYTES || text.contains('\0') {
        return Err(CliError(
            "buffer text must contain 1–4096 UTF-8 bytes without NUL".into(),
        ));
    }
    Ok(())
}
fn directory() -> Result<PathBuf, CliError> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| CliError("HOME must be an absolute path for named buffers".into()))?;
    let parent = home.join(".config/zmux");
    fs::create_dir_all(&parent)?;
    let directory = parent.join("buffers");
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    let m = fs::symlink_metadata(&directory)?;
    if !m.is_dir() || m.uid() != geteuid().as_raw() || m.mode() & 0o077 != 0 {
        return Err(CliError(
            "buffer directory must be private and owned by this user".into(),
        ));
    }
    Ok(directory)
}
fn private_file(path: &Path, create: bool) -> Result<File, CliError> {
    let file = OpenOptions::new()
        .read(true)
        .write(create)
        .create(create)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != geteuid().as_raw()
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
        || m.len() > STORE_BYTES as u64
    {
        return Err(CliError(
            "buffer state must be a bounded private regular file with one link".into(),
        ));
    }
    Ok(file)
}
struct Store {
    directory: PathBuf,
    _lock: Flock<File>,
    values: BTreeMap<String, String>,
}
impl Store {
    fn open(timeout: Duration) -> Result<Self, CliError> {
        let directory = directory()?;
        let lock_path = directory.join("lock");
        let mut file = private_file(&lock_path, true)?;
        if file.metadata()?.len() != 0 {
            return Err(CliError("buffer lock is not empty".into()));
        }
        let deadline = Instant::now() + timeout;
        let lock = loop {
            match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
                Ok(lock) => break lock,
                Err((f, nix::errno::Errno::EWOULDBLOCK)) if Instant::now() < deadline => {
                    file = f;
                    thread::sleep(Duration::from_millis(5));
                }
                Err((_, error)) => return Err(CliError(format!("acquire buffer lock: {error}"))),
            }
        };
        let actual = lock.metadata()?;
        let named = fs::symlink_metadata(&lock_path)?;
        if actual.dev() != named.dev() || actual.ino() != named.ino() || actual.nlink() != 1 {
            return Err(CliError("buffer lock identity changed".into()));
        }
        let path = directory.join("store.json");
        let values = match private_file(&path, false) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(STORE_BYTES as u64 + 1).read_to_end(&mut bytes)?;
                if bytes.len() > STORE_BYTES {
                    return Err(CliError("buffer store exceeds limit".into()));
                }
                let value: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| CliError("invalid buffer store; existing file retained".into()))?;
                if value.as_object().is_none_or(|v| v.len() != 2) || value["version"] != 1 {
                    return Err(CliError(
                        "unsupported buffer store; existing file retained".into(),
                    ));
                }
                serde_json::from_value::<BTreeMap<String, String>>(value["buffers"].clone())
                    .map_err(|_| {
                        CliError("invalid buffer contents; existing file retained".into())
                    })?
            }
            Err(_)
                if fs::symlink_metadata(&path)
                    .is_err_and(|e| e.kind() == io::ErrorKind::NotFound) =>
            {
                BTreeMap::new()
            }
            Err(error) => return Err(error),
        };
        if values.len() > MAX_BUFFERS {
            return Err(CliError("too many saved buffers".into()));
        }
        for (name, text) in &values {
            validate_name(name)?;
            validate_text(text)?;
        }
        Ok(Self {
            directory,
            _lock: lock,
            values,
        })
    }
    fn save(&self) -> Result<(), CliError> {
        if self.values.len() > MAX_BUFFERS {
            return Err(CliError("buffer limit is 128 names".into()));
        }
        let bytes = serde_json::to_vec(&json!({"version":1,"buffers":self.values}))
            .map_err(|e| CliError(e.to_string()))?;
        if bytes.len() > STORE_BYTES {
            return Err(CliError("buffer store exceeds 512 KiB".into()));
        }
        let temporary =
            self.directory
                .join(format!(".write-{}-{}", std::process::id(), request_id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        let result = (|| -> Result<(), CliError> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, self.directory.join("store.json"))?;
            File::open(&self.directory)?.sync_all()?;
            Ok(())
        })();
        if let Err(error) = fs::remove_file(&temporary)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(CliError(format!(
                "buffer temporary cleanup failed: {error}; save result: {}",
                if result.is_ok() {
                    "published"
                } else {
                    "failed"
                }
            )));
        }
        result
    }
}
pub(super) fn run(
    invocation: Invocation,
    socket: Option<PathBuf>,
    timeout: Duration,
    password: Option<SocketPassword>,
) -> Result<Value, CliError> {
    match invocation {
        Invocation::Set { name, text } => {
            let text = match text {
                Some(text) => text,
                None => {
                    let mut bytes = Vec::new();
                    io::stdin()
                        .take(TEXT_BYTES as u64 + 1)
                        .read_to_end(&mut bytes)?;
                    String::from_utf8(bytes)
                        .map_err(|_| CliError("buffer stdin must be valid UTF-8".into()))?
                }
            };
            validate_text(&text)?;
            let mut store = Store::open(timeout)?;
            let bytes = text.len();
            store.values.insert(name.clone(), text);
            store.save()?;
            Ok(json!({"name":name,"bytes":bytes,"saved":true}))
        }
        Invocation::List => {
            let store = Store::open(timeout)?;
            Ok(
                json!({"buffers":store.values.iter().map(|(name,text)| json!({"name":name,"size":text.len(),"size_unit":"utf8_bytes"})).collect::<Vec<_>>()}),
            )
        }
        Invocation::Paste {
            name,
            bracketed,
            mut target,
        } => {
            let text = Store::open(timeout)?
                .values
                .get(&name)
                .cloned()
                .ok_or_else(|| CliError(format!("buffer not found: {name}")))?;
            target.operation = if bracketed {
                TargetOperation::PasteText { text }
            } else {
                TargetOperation::SendText { text }
            };
            let options = client_options(socket, timeout, password)?;
            response_result(request(
                &options,
                ControlCommand::Targeted { request: *target },
            )?)
        }
    }
}
