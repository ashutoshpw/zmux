use super::{
    START_TIMEOUT, check_deadline, fail,
    files::{self, LocalZellij, Record},
    process,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::DirBuilderExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_session_runtime::child::{
    ExitObservation, ProcessIdentity, observe_child, process_identity,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bootstrap {
    socket: PathBuf,
    starter: ProcessIdentity,
    program: PathBuf,
    nonce: String,
    cwd: PathBuf,
    shell: PathBuf,
    command: Option<String>,
}
pub(super) fn clean(command: &mut Command) {
    super::super::local_multiplexer::clear_context(command);
    command.env_remove("ZMUX_LOCAL_TMUX");
}
pub(super) fn environment(command: &mut Command, root: &Path) {
    clean(command);
    command
        .env_remove("ZELLIJ")
        .env_remove("ZELLIJ_SESSION_NAME")
        .env("ZELLIJ_SOCKET_DIR", root.join("s"))
        .env("XDG_CACHE_HOME", root.join("cache"));
}
fn configure(stream: &UnixStream) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))
}
fn send<T: Serialize>(stream: &mut UnixStream, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > 4096 {
        return Err(fail("bootstrap message exceeded4KiB"));
    }
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)
}
fn receive<T: for<'de> Deserialize<'de>>(stream: &mut UnixStream) -> io::Result<T> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > 4096 {
        return Err(fail("bootstrap message exceeded4KiB"));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

pub(super) fn create(
    owner: &LocalZellij,
    record: &mut Record,
    cwd: &Path,
    command: Option<&str>,
    terminal: &mut process::ForegroundTerminal,
    mut persist: impl FnMut(&Record) -> io::Result<()>,
) -> io::Result<()> {
    terminal.startup_events()?;
    let root = record.runtime()?;
    fs::DirBuilder::new().mode(0o700).create(&root)?;
    files::directory(&root, true)?;
    let root_identity = files::inode(&root)?;
    for name in ["s", "cache", "config"] {
        files::directory(&root.join(name), true)?;
    }
    let socket = root.join("bootstrap.sock");
    files::socket_path(&socket)?;
    let listener = UnixListener::bind(&socket)?;
    listener.set_nonblocking(true)?;
    let shell = super::executable(
        &std::env::var_os("SHELL")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| PathBuf::from("/bin/sh")),
    )?;
    let spec = Bootstrap {
        socket,
        starter: process_identity(std::process::id())?,
        program: owner.program.clone(),
        nonce: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
        cwd: cwd.into(),
        shell,
        command: command.map(str::to_owned),
    };
    let path = root.join("bootstrap.json");
    files::write_private(&path, &serde_json::to_vec(&spec).map_err(io::Error::other)?)?;
    let layout = root.join("layout.kdl");
    // Only internal paths enter KDL. For this bounded subset, JSON quoting uses
    // the same quote/backslash escapes; reject control/non-UTF8 paths rather
    // than silently emitting JSON-only Unicode escapes or changing path bytes.
    let quote = |path: &Path| {
        let text = path
            .to_str()
            .filter(|text| !text.chars().any(char::is_control))
            .ok_or_else(|| {
                fail("supervisor layout paths require UTF-8 without control characters")
            })?;
        serde_json::to_string(text).map_err(io::Error::other)
    };
    files::write_private(&layout, format!("layout {{\n default_tab_template {{ pane; }}\n pane command={} close_on_exit=true {{\n args \"__local-zellij-supervisor\" {}\n }}\n}}\n", quote(&owner.program)?, quote(&path)?).as_bytes())?;
    let config = root.join("config/config.kdl");
    files::write_private(&config, super::OWNED_CONFIG)?;
    let backend = record.backend()?;
    files::socket_path(&backend)?;
    let deadline = Instant::now() + START_TIMEOUT;
    let mut launch = Command::new(&owner.binary);
    environment(&mut launch, &root);
    launch
        .args(["--config"])
        .arg(&config)
        .arg("--layout")
        .arg(&layout)
        .args(["attach", "--create-background"])
        .arg(record.session_name())
        .args([
            "options",
            "--session-serialization",
            "false",
            "--on-force-close",
            "detach",
        ]);
    process::capture(&mut launch, deadline, || terminal.startup_events())?;
    let mut connection = loop {
        terminal.startup_events()?;
        check_deadline(deadline, "supervisor bootstrap")?;
        match listener.accept() {
            Ok((connection, _)) => break connection,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(error) => return Err(error),
        }
    };
    configure(&connection)?;
    let supervisor = super::broker::peer(&connection)?;
    if supervisor.executable != owner.program || supervisor.uid != spec.starter.uid {
        return Err(fail(
            "bootstrap peer is not the pinned production supervisor",
        ));
    }
    let nonce: String = receive(&mut connection)?;
    if nonce != spec.nonce {
        return Err(fail("bootstrap reservation challenge rejected"));
    }
    let backend_connection = super::broker::connect(&backend, deadline)?;
    let server = super::broker::peer(&backend_connection)?;
    if server.executable != owner.binary
        || server.uid != spec.starter.uid
        || supervisor.parent_pid != server.pid
        || process_identity(supervisor.parent_pid)? != server
    {
        return Err(fail(
            "bootstrap kernel parent does not match the connected new backend",
        ));
    }
    if files::inode(&root)? != root_identity {
        return Err(fail("bootstrap runtime namespace changed"));
    }
    record.namespace = Some(files::NamespaceIdentity {
        root: root_identity,
        socket: files::inode(&backend)?,
    });
    record.server = Some(server);
    record.supervisor = Some(supervisor);
    record.state = "starting".into();
    persist(record)?;
    terminal.startup_events()?;
    send(&mut connection, &"spawn_once")?;
    let pid: u32 = receive(&mut connection)?;
    let initial = process_identity(pid)?;
    if initial.parent_pid != record.supervisor.as_ref().expect("bound supervisor").pid
        || initial.uid != spec.starter.uid
    {
        return Err(fail(
            "workload is not the supervisor's directly owned child",
        ));
    }
    // A process can retain PID/start/parent across exec. Do not commit its
    // inherited supervisor image while the selected shell is still starting.
    // An immediately exec-changing shell remains uncertain, never adopted.
    let workload = loop {
        terminal.startup_events()?;
        check_deadline(deadline, "selected workload shell exec")?;
        let current = process_identity(pid)?;
        if current.pid != initial.pid
            || current.uid != initial.uid
            || current.parent_pid != initial.parent_pid
            || current.start != initial.start
            || current.boot != initial.boot
        {
            return Err(fail("workload identity changed before selected shell exec"));
        }
        if current.executable == spec.shell {
            break current;
        }
        thread::sleep(Duration::from_millis(5));
    };
    record.workload = Some(workload);
    record.state = "bound".into();
    record.verify()?;
    persist(record)?;
    send(&mut connection, &"committed")?;
    // The workload no longer needs startup text or the one-use challenge file.
    fs::remove_file(path)?;
    fs::remove_file(layout)?;
    fs::remove_file(&spec.socket)?;
    Ok(())
}

/// Internal production root-pane command. It never reads terminal input or emits
/// a shell ACK. A bootstrap disconnect after spawn never kills/restarts workload.
pub fn run_supervisor(path: &Path) -> io::Result<i32> {
    let spec: Bootstrap =
        serde_json::from_slice(&files::read_private(path, 16384)?).map_err(io::Error::other)?;
    if spec.nonce.len() != 64
        || !spec.nonce.bytes().all(|c| c.is_ascii_hexdigit())
        || super::executable(&std::env::current_exe()?)? != spec.program
        || !spec.cwd.is_absolute()
    {
        return Err(fail("invalid supervisor bootstrap descriptor"));
    }
    let mut connection =
        super::broker::connect(&spec.socket, Instant::now() + Duration::from_secs(3))?;
    connection.set_nonblocking(false)?;
    configure(&connection)?;
    if super::broker::peer(&connection)? != spec.starter {
        return Err(fail("bootstrap starter identity changed"));
    }
    send(&mut connection, &spec.nonce)?;
    let instruction: String = receive(&mut connection)?;
    if instruction != "spawn_once" {
        return Err(fail("supervisor was not authorized to start workload"));
    }
    let mut command = Command::new(&spec.shell);
    clean(&mut command);
    command
        .current_dir(&spec.cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(script) = &spec.command {
        command.args(["-lc", script]);
    } else {
        command.arg("-i");
    }
    let mut child = command.spawn()?;
    // Once spawn can have executed user code, startup uncertainty must preserve
    // its sole physical owner. Continue waiting even if parent commit failed.
    let committed = (|| {
        send(&mut connection, &child.id())?;
        let reply: String = receive(&mut connection)?;
        if reply == "committed" {
            Ok(())
        } else {
            Err(fail("unexpected bootstrap completion"))
        }
    })();
    if committed.is_err() {
        eprintln!(
            "zmux workload bootstrap completion is uncertain; workload was not replayed or terminated"
        );
    }
    drop(connection);
    loop {
        if let Some(status) = observe_child(child.id(), false)? {
            child.wait()?;
            return Ok(match status {
                ExitObservation::Exited(code) => code,
                ExitObservation::Signaled(number) => 128 + number,
                _ => return Err(fail("unexpected workload wait status")),
            });
        }
        thread::sleep(Duration::from_millis(10));
    }
}
