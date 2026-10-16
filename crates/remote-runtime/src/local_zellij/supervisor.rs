use super::{
    check_deadline, fail,
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
    owner_uuid: Uuid,
    owner_generation: Uuid,
    profile: PathBuf,
    #[serde(default)]
    admitted: bool,
}
pub(super) fn clean(command: &mut Command) {
    super::super::local_multiplexer::clear_context(command);
    command.env_remove("ZMUX_LOCAL_TMUX");
    command.env_remove(super::observer::ENV);
}
pub(super) fn environment(command: &mut Command, root: &Path) {
    clean(command);
    command
        .env_remove("ZELLIJ")
        .env_remove("ZELLIJ_SESSION_NAME")
        .env("ZELLIJ_SOCKET_DIR", root.join("s"))
        .env("XDG_CACHE_HOME", root.join("cache"));
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    nonce: String,
    completion_socket: (u64, u64),
}

pub(super) fn configure(stream: &UnixStream) -> io::Result<()> {
    // Darwin accept inherits O_NONBLOCK from its listener. Framed bootstrap I/O
    // uses socket timeouts and must wait for the authenticated peer's next field.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))
}
pub(super) fn encode<T: Serialize>(value: &T) -> io::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > 4096 {
        return Err(fail("bootstrap message exceeded4KiB"));
    }
    let mut frame = (bytes.len() as u32).to_le_bytes().to_vec();
    frame.extend(bytes);
    Ok(frame)
}
pub(super) fn send<T: Serialize>(stream: &mut UnixStream, value: &T) -> io::Result<()> {
    stream.write_all(&encode(value)?)
}
pub(super) fn receive<T: for<'de> Deserialize<'de>>(stream: &mut UnixStream) -> io::Result<T> {
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

pub(super) struct CreationAdmission {
    pub deadline: Option<Instant>,
    pub admitted: bool,
}

pub(super) fn create(
    owner: &LocalZellij,
    record: &mut Record,
    cwd: &Path,
    command: Option<&str>,
    terminal: &mut process::ForegroundTerminal,
    admission: CreationAdmission,
    mut persist: impl FnMut(&Record) -> io::Result<()>,
) -> io::Result<()> {
    let CreationAdmission { deadline, admitted } = admission;
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
        owner_uuid: record.id,
        owner_generation: Uuid::new_v4(),
        profile: owner.root.clone(),
        admitted,
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
    let deadline = deadline.unwrap_or_else(|| Instant::now() + super::START_TIMEOUT);
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
    let hello: Hello = receive(&mut connection)?;
    if hello.nonce != spec.nonce
        || files::inode(&root.join("completion.sock"))? != hello.completion_socket
    {
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
    if admitted {
        let birth: ProcessIdentity = receive(&mut connection)?;
        if birth.parent_pid != record.supervisor.as_ref().expect("bound supervisor").pid
            || birth.uid != spec.starter.uid
            || birth.executable != owner.program
            || process_identity(birth.pid)? != birth
        {
            return Err(fail("admission birth is not the actual blocked helper"));
        }
        let plan = super::admission_gate::InvocationPlan::new(
            &spec.shell,
            &spec.cwd,
            spec.command.as_deref(),
        );
        let binding = super::admission::ExecutionBindingV2 {
            version: 2,
            domain: super::admission::EXECUTION_DOMAIN.into(),
            owner_uuid: record.id,
            owner_generation: spec.owner_generation,
            namespace: record.namespace.clone().expect("bound namespace"),
            completion_socket: hello.completion_socket,
            server: record.server.clone().expect("bound server"),
            supervisor: record.supervisor.clone().expect("bound supervisor"),
            lifetime: (&birth).into(),
            workload_birth: birth.clone(),
            invocation: plan.authorization()?,
            admission_id: Uuid::new_v4(),
        };
        binding.validate()?;
        record.workload = Some(birth);
        record.state = "bound".into();
        record.completion = Some(super::completion::Capability {
            execution_contract: Some(super::admission::ExecutionContract::AdmittedLifetimeV2),
            admission: Some(Box::new(binding.clone())),
            owner_generation: spec.owner_generation,
            completion_socket: hello.completion_socket,
            binding_digest: binding.digest()?,
        });
        record.verify_live()?;
        persist(record)?;
        send(&mut connection, &binding)?;
        let accepted: String = receive(&mut connection)?;
        if accepted != binding.digest()? {
            return Err(fail("admission publisher acknowledgment rejected"));
        }
    } else {
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
        record.verify_live()?;
        let binding = super::completion::CommittedBinding {
            version: 1,
            owner_uuid: record.id,
            owner_generation: spec.owner_generation,
            namespace: record.namespace.clone().expect("bound namespace"),
            completion_socket: hello.completion_socket,
            server: record.server.clone().expect("bound server"),
            supervisor: record.supervisor.clone().expect("bound supervisor"),
            workload: record.workload.clone().expect("bound workload"),
        };
        record.completion = Some(super::completion::Capability {
            execution_contract: None,
            admission: None,
            owner_generation: spec.owner_generation,
            completion_socket: hello.completion_socket,
            binding_digest: binding.digest()?,
        });
        persist(record)?;
        send(&mut connection, &binding)?;
        let accepted: String = receive(&mut connection)?;
        if accepted != binding.digest()? {
            return Err(fail("supervisor binding acknowledgment rejected"));
        }
    }
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
    configure(&connection)?;
    if super::broker::peer(&connection)? != spec.starter {
        return Err(fail("bootstrap starter identity changed"));
    }
    let root = path
        .parent()
        .ok_or_else(|| fail("missing supervisor runtime root"))?
        .to_path_buf();
    let mut publisher =
        super::completion_control::Publisher::create(root.clone(), spec.profile.clone())?;
    send(
        &mut connection,
        &Hello {
            nonce: spec.nonce.clone(),
            completion_socket: publisher.endpoint(),
        },
    )?;
    let instruction: String = receive(&mut connection)?;
    if instruction != "spawn_once" {
        return Err(fail("supervisor was not authorized to start workload"));
    }
    let mut gate = if spec.admitted {
        Some(super::admission_gate::Gate::spawn(
            &root,
            &spec.program,
            super::admission_gate::InvocationPlan::new(
                &spec.shell,
                &spec.cwd,
                spec.command.as_deref(),
            ),
            spec.owner_uuid,
            spec.owner_generation,
        )?)
    } else {
        None
    };
    let (mut child, committed) = if let Some(gate) = &mut gate {
        let mut child = gate.take_child();
        let committed = (|| {
            send(&mut connection, &gate.birth)?;
            let binding: super::admission::ExecutionBindingV2 = receive(&mut connection)?;
            if binding.owner_uuid != spec.owner_uuid
                || binding.owner_generation != spec.owner_generation
                || binding.workload_birth != gate.birth
            {
                return Err(fail("admitted supervisor binding mismatch"));
            }
            let digest = binding.digest()?;
            publisher.bind_execution(super::completion::ExecutionBinding::V2(binding.clone()))?;
            gate.release(&binding)?;
            send(&mut connection, &digest)
        })();
        if committed.is_err() && !gate.released() {
            super::admission_gate::reap_unreleased(&mut child);
            return Err(fail(
                "workload admission failed before Run; shell was not executed",
            ));
        }
        (child, committed)
    } else {
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
        let child = command.spawn()?;
        // Once spawn can have executed user code, startup uncertainty must preserve
        // its sole physical owner. Continue waiting even if parent commit failed.
        let committed = (|| {
            send(&mut connection, &child.id())?;
            let binding: super::completion::CommittedBinding = receive(&mut connection)?;
            if binding.owner_uuid != spec.owner_uuid
                || binding.owner_generation != spec.owner_generation
                || binding.workload != process_identity(child.id())?
            {
                return Err(fail("supervisor workload binding mismatch"));
            }
            let digest = binding.digest()?;
            publisher.bind(binding)?;
            send(&mut connection, &digest)
        })();
        (child, committed)
    };
    if committed.is_err() {
        eprintln!(
            "zmux workload bootstrap completion is uncertain; workload was not replayed or terminated"
        );
    }
    drop(connection);
    let mut terminal = None;
    let mut control_issue_reported = false;
    let mut admission_issue = None;
    loop {
        if terminal.is_none()
            && admission_issue.is_none()
            && let Some(gate) = &mut gate
            && let Err(error) = gate.poll()
        {
            eprintln!("admission observation failed; actual Child owner retained: {error}");
            admission_issue = Some(error);
        }
        if terminal.is_none()
            && let Some(status) = observe_child(child.id(), false)?
        {
            use std::os::unix::process::ExitStatusExt;
            let waited = child.wait()?;
            let actual = if let Some(code) = waited.code() {
                ExitObservation::Exited(code)
            } else if let Some(signal) = waited.signal() {
                ExitObservation::Signaled(signal)
            } else {
                return Err(fail("missing actual terminal wait status"));
            };
            if status != actual {
                return Err(fail("retained workload observe/wait mismatch"));
            }
            let exit = match status {
                ExitObservation::Exited(code) => super::completion::WorkloadExit::Exited(code),
                ExitObservation::Signaled(signal) => {
                    super::completion::WorkloadExit::Signaled(signal)
                }
                _ => return Err(fail("unexpected workload wait status")),
            };
            terminal = Some(exit);
            if committed.is_err() && gate.is_none() {
                return exit.mapped();
            }
            if admission_issue.is_none()
                && let Some(gate) = &mut gate
                && let Err(error) = gate.finish()
            {
                eprintln!("admission terminal observation failed; publication withheld: {error}");
                admission_issue = Some(error);
            }
            if let Some(error) = admission_issue.take() {
                return Err(fail(format!(
                    "actual Child reaped ({exit:?}); admission observation unavailable: {error}; no terminal receipt published; retained registry/runtime require operator inspection"
                )));
            }
            let publication = if let Some(gate) = &gate {
                publisher.publish_observed(exit, gate.observation.clone())
            } else {
                publisher.publish(exit)
            };
            if let Err(error) = publication {
                eprintln!("supervisor terminal publication unknown; retained publisher: {error}");
            }
        }
        match publisher.poll() {
            Ok(true) => {
                return terminal
                    .ok_or_else(|| fail("cannot retire a live workload"))?
                    .mapped();
            }
            Ok(false) => (),
            Err(error) => {
                if !control_issue_reported {
                    eprintln!("supervisor control service issue; owner retained: {error}");
                    control_issue_reported = true;
                }
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_bootstrap_waits_for_fragmented_frames_on_an_initially_nonblocking_socket() {
        let (mut reader, mut peer) = UnixStream::pair().unwrap();
        // Darwin accept inherits a nonblocking listener's flag. Force that
        // actual socket mode here so the same bootstrap contract runs on Linux.
        reader.set_nonblocking(true).unwrap();
        configure(&reader).unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            started_tx.send(()).unwrap();
            result_tx.send(receive::<String>(&mut reader)).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            matches!(
                result_rx.recv_timeout(Duration::from_millis(20)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ),
            "bootstrap returned before the connected peer sent a frame"
        );
        // Independently encoded JSON string and length, fragmented in both fields.
        for fragment in [b"\x07".as_slice(), b"\0\0\0\"n", b"once\""] {
            peer.write_all(fragment).unwrap();
        }
        assert_eq!(
            result_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap(),
            "nonce"
        );
        worker.join().unwrap();
    }
}
