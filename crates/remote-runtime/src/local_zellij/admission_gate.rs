//! Private one-use execution admission. No user shell runs before committed Run.
use super::{
    admission::{
        self, AuthorizedInvocation, ExecFailureStage, ExecutionBindingV2, ExecutionObservation,
    },
    broker, files,
    supervisor::{clean, configure, encode, receive, send},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_session_runtime::child::{ProcessIdentity, process_identity};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InvocationPlan {
    version: u8,
    pub shell: PathBuf,
    argv: Vec<String>,
    cwd: PathBuf,
    environment_policy: String,
}
impl InvocationPlan {
    pub fn new(shell: &Path, cwd: &Path, command: Option<&str>) -> Self {
        Self {
            version: 1,
            shell: shell.into(),
            argv: command.map_or_else(
                || vec!["-i".into()],
                |script| vec!["-lc".into(), script.into()],
            ),
            cwd: cwd.into(),
            environment_policy: "inherit_clean_v1".into(),
        }
    }
    pub fn authorization(&self) -> io::Result<AuthorizedInvocation> {
        if self.version != 1
            || self.environment_policy != "inherit_clean_v1"
            || !self.cwd.is_absolute()
            || !self.shell.is_absolute()
            || !(self.argv == ["-i"]
                || self.argv.len() == 2
                    && self.argv[0] == "-lc"
                    && self.argv[1].len() <= 8192
                    && !self.argv[1].contains('\0'))
        {
            return Err(super::fail("invalid exact shell invocation plan"));
        }
        Ok(AuthorizedInvocation {
            shell: self.shell.clone(),
            invocation_digest: admission::canonical_digest_bounded(
                "zmux.local-zellij.invocation.v1",
                self,
                16384,
            )?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    version: u8,
    program: PathBuf,
    supervisor: ProcessIdentity,
    owner_uuid: Uuid,
    owner_generation: Uuid,
    nonce: String,
    socket: PathBuf,
    invocation: InvocationPlan,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    nonce: String,
    binding: ExecutionBindingV2,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecFailure {
    owner_uuid: Uuid,
    owner_generation: Uuid,
    binding_digest: String,
    errno: i32,
}

pub(super) struct Gate {
    child: Option<Child>,
    pub birth: ProcessIdentity,
    stream: UnixStream,
    nonce: String,
    released: bool,
    input: Vec<u8>,
    eof: bool,
    error_seen: bool,
    pub observation: ExecutionObservation,
    binding: Option<ExecutionBindingV2>,
}
pub(super) fn reap_unreleased(child: &mut Child) {
    match child.try_wait() {
        Ok(Some(_)) => return,
        Ok(None) => (),
        Err(e) => eprintln!("admission helper status inspection failed: {e}"),
    }
    if let Err(e) = child.kill() {
        eprintln!("unreleased admission helper cleanup signal failed: {e}");
    }
    if let Err(e) = child.wait() {
        eprintln!("unreleased admission helper reap failed: {e}");
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        // Before any Run write, this is our held helper and user code is impossible.
        if let Some(child) = &mut self.child
            && !self.released
        {
            reap_unreleased(child);
        }
    }
}
impl Gate {
    pub fn spawn(
        root: &Path,
        program: &Path,
        invocation: InvocationPlan,
        owner_uuid: Uuid,
        owner_generation: Uuid,
    ) -> io::Result<Self> {
        invocation.authorization()?;
        let socket = root.join("admission.sock");
        files::socket_path(&socket)?;
        let listener = UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let nonce = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let spec = Spec {
            version: 1,
            program: program.into(),
            supervisor: process_identity(std::process::id())?,
            owner_uuid,
            owner_generation,
            nonce: nonce.clone(),
            socket: socket.clone(),
            invocation,
        };
        let descriptor = root.join("admission.json");
        let bytes = serde_json::to_vec(&spec).map_err(io::Error::other)?;
        if bytes.len() > 16384 {
            return Err(super::fail("admission descriptor exceeds16KiB"));
        }
        files::write_private(&descriptor, &bytes)?;
        let mut command = Command::new(program);
        clean(&mut command);
        let mut child = command
            .args(["__local-zellij-workload"])
            .arg(&descriptor)
            .current_dir(&spec.invocation.cwd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        let accepted = (|| {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        super::check_deadline(deadline, "workload admission connection")?;
                        if child.try_wait()?.is_some() {
                            return Err(super::fail("workload helper ended before admission"));
                        }
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => return Err(e),
                }
            };
            configure(&stream)?;
            let birth = broker::peer(&stream)?;
            if birth.pid != child.id()
                || birth.parent_pid != spec.supervisor.pid
                || birth.uid != spec.supervisor.uid
                || birth.executable != spec.program
                || process_identity(child.id())? != birth
            {
                return Err(super::fail(
                    "admission peer is not the retained original helper",
                ));
            }
            let hello: String = receive(&mut stream)?;
            if hello != nonce {
                return Err(super::fail("admission helper challenge rejected"));
            }
            fs::remove_file(&descriptor)?;
            fs::remove_file(&socket)?;
            Ok((stream, birth))
        })();
        let (stream, birth) = match accepted {
            Ok(v) => v,
            Err(e) => {
                reap_unreleased(&mut child);
                return Err(e);
            }
        };
        Ok(Self {
            child: Some(child),
            birth,
            stream,
            nonce,
            released: false,
            input: Vec::new(),
            eof: false,
            error_seen: false,
            observation: ExecutionObservation::NotObserved,
            binding: None,
        })
    }
    pub fn take_child(&mut self) -> Child {
        self.child.take().expect("retained helper transferred once")
    }
    pub fn released(&self) -> bool {
        self.released
    }
    pub fn release(&mut self, binding: &ExecutionBindingV2) -> io::Result<()> {
        binding.validate()?;
        if binding.workload_birth != self.birth {
            return Err(super::fail("Run changed retained helper birth"));
        }
        // Validate the complete wrapper before any write can release the helper.
        let frame = encode(&Run {
            nonce: self.nonce.clone(),
            binding: binding.clone(),
        })?;
        // An unsuccessful write/ACK may still have delivered Run. Never kill/replay.
        self.released = true;
        self.binding = Some(binding.clone());
        self.stream.write_all(&frame)?;
        let accepted: String = receive(&mut self.stream)?;
        if accepted != binding.digest()? {
            return Err(super::fail("workload admission acknowledgment rejected"));
        }
        self.stream.set_nonblocking(true)?;
        Ok(())
    }
    pub fn finish(&mut self) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(1);
        while !self.eof {
            self.poll_channel(false)?;
            super::check_deadline(deadline, "retained exec error channel drain")?;
            if !self.eof {
                thread::sleep(Duration::from_millis(2));
            }
        }
        if !self.input.is_empty() {
            return Err(super::fail("truncated configured-shell exec error frame"));
        }
        Ok(())
    }
    pub fn poll(&mut self) -> io::Result<()> {
        self.poll_channel(true)
    }
    fn poll_channel(&mut self, observe_image: bool) -> io::Result<()> {
        let Some(binding) = &self.binding else {
            return Ok(());
        };
        if observe_image
            && matches!(self.observation, ExecutionObservation::NotObserved)
            && process_identity(self.birth.pid).is_ok_and(|p| observed_shell(&p, binding))
        {
            self.observation = ExecutionObservation::ShellObserved;
        }
        self.stream.set_nonblocking(true)?;
        let mut buffer = [0u8; 4096];
        match self.stream.read(&mut buffer) {
            Ok(0) => self.eof = true,
            Ok(n) => self.input.extend_from_slice(&buffer[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(e),
        }
        if self.input.len() > 4100 {
            return Err(super::fail("exec error frame exceeds4KiB"));
        }
        if self.error_seen && !self.input.is_empty() {
            return Err(super::fail("unexpected duplicate exec-error bytes"));
        }
        if self.input.len() >= 4 {
            let size =
                u32::from_le_bytes(self.input[..4].try_into().expect("four-byte header")) as usize;
            if size == 0 || size > 4096 {
                return Err(super::fail("invalid exec error frame bound"));
            }
            if self.input.len() == size + 4 {
                let error: ExecFailure =
                    serde_json::from_slice(&self.input[4..]).map_err(io::Error::other)?;
                if error.owner_uuid != binding.owner_uuid
                    || error.owner_generation != binding.owner_generation
                    || error.binding_digest != binding.digest()?
                    || error.errno <= 0
                {
                    return Err(super::fail("exec error correlation rejected"));
                }
                self.observation = ExecutionObservation::ExecFailed {
                    stage: ExecFailureStage::ConfiguredShellExec,
                    errno: error.errno,
                };
                self.error_seen = true;
                self.input.clear();
            } else if self.input.len() > size + 4 {
                return Err(super::fail("unexpected trailing exec-error bytes"));
            }
        }
        Ok(())
    }
}

fn observed_shell(identity: &ProcessIdentity, binding: &ExecutionBindingV2) -> bool {
    super::admission::ChildLifetime::from(identity) == binding.lifetime
        && identity.executable == binding.invocation.shell
}

/// Internal original executable entrypoint; exec keeps the same actual Child.
pub fn run_workload(path: &Path) -> io::Result<i32> {
    let spec: Spec =
        serde_json::from_slice(&files::read_private(path, 16384)?).map_err(io::Error::other)?;
    if spec.version != 1
        || spec.nonce.len() != 64
        || !spec.nonce.bytes().all(|v| v.is_ascii_hexdigit())
        || super::executable(&std::env::current_exe()?)? != spec.program
        || spec.owner_uuid.is_nil()
        || spec.owner_generation.is_nil()
    {
        return Err(super::fail("invalid workload admission descriptor"));
    }
    let own = process_identity(std::process::id())?;
    if own.parent_pid != spec.supervisor.pid || own.uid != spec.supervisor.uid {
        return Err(super::fail("helper is not the supervisor direct Child"));
    }
    let mut stream = broker::connect(&spec.socket, Instant::now() + Duration::from_secs(3))?;
    configure(&stream)?;
    if broker::peer(&stream)? != spec.supervisor {
        return Err(super::fail("workload admission supervisor changed"));
    }
    send(&mut stream, &spec.nonce)?;
    let run: Run = receive(&mut stream)?;
    run.binding.validate()?;
    if run.nonce != spec.nonce
        || run.binding.owner_uuid != spec.owner_uuid
        || run.binding.owner_generation != spec.owner_generation
        || run.binding.supervisor != spec.supervisor
        || run.binding.workload_birth != own
        || run.binding.invocation != spec.invocation.authorization()?
        || broker::peer(&stream)? != spec.supervisor
    {
        return Err(super::fail("workload Run authorization rejected"));
    }
    send(&mut stream, &run.binding.digest()?)?;
    let mut command = Command::new(&spec.invocation.shell);
    clean(&mut command);
    command
        .args(&spec.invocation.argv)
        .current_dir(&spec.invocation.cwd);
    let error = command.exec();
    let errno = error.raw_os_error().unwrap_or(nix::libc::EIO);
    send(
        &mut stream,
        &ExecFailure {
            owner_uuid: spec.owner_uuid,
            owner_generation: spec.owner_generation,
            binding_digest: run.binding.digest()?,
            errno,
        },
    )?;
    Err(super::fail(format!(
        "configured shell exec failed: {error}"
    )))
}

#[cfg(test)]
mod tests {
    use super::super::{
        admission::{ChildLifetime, EXECUTION_DOMAIN},
        files::NamespaceIdentity,
    };
    use super::*;

    fn fixture() -> (Gate, UnixStream, ExecutionBindingV2) {
        let own = process_identity(std::process::id()).unwrap();
        let mut server = own.clone();
        server.pid = own.parent_pid;
        let mut birth = own.clone();
        birth.pid += 1;
        birth.parent_pid = own.pid;
        let binding = ExecutionBindingV2 {
            version: 2,
            domain: EXECUTION_DOMAIN.into(),
            owner_uuid: Uuid::new_v4(),
            owner_generation: Uuid::new_v4(),
            namespace: NamespaceIdentity {
                root: (1, 2),
                socket: (1, 3),
            },
            completion_socket: (1, 4),
            server,
            supervisor: own,
            lifetime: ChildLifetime::from(&birth),
            workload_birth: birth.clone(),
            invocation: AuthorizedInvocation {
                shell: birth.executable.clone(),
                invocation_digest: "a".repeat(64),
            },
            admission_id: Uuid::new_v4(),
        };
        binding.validate().unwrap();
        let (stream, peer) = UnixStream::pair().unwrap();
        let gate = Gate {
            child: None,
            birth,
            stream,
            nonce: "a".repeat(64),
            released: true,
            input: Vec::new(),
            eof: false,
            error_seen: false,
            observation: ExecutionObservation::NotObserved,
            binding: Some(binding.clone()),
        };
        (gate, peer, binding)
    }
    #[test]
    fn oversized_run_wrapper_emits_no_bytes_and_does_not_release_the_held_helper() {
        let (mut gate, mut peer, mut binding) = fixture();
        gate.released = false;
        gate.binding = None;
        let size = (1..4096)
            .find(|size| {
                binding.invocation.shell = PathBuf::from(format!("/{}", "s".repeat(*size)));
                serde_json::to_vec(&binding).unwrap().len() <= 4096
                    && serde_json::to_vec(&Run {
                        nonce: gate.nonce.clone(),
                        binding: binding.clone(),
                    })
                    .unwrap()
                    .len()
                        > 4096
            })
            .expect("binding fits but wrapper exceeds bootstrap frame");
        assert!(size > 0);
        binding.validate().unwrap();
        assert!(
            gate.release(&binding)
                .unwrap_err()
                .to_string()
                .contains("exceeded4KiB")
        );
        assert!(!gate.released());
        assert!(gate.binding.is_none());
        peer.set_nonblocking(true).unwrap();
        assert_eq!(
            peer.read(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }
    #[test]
    fn shell_image_diagnostic_requires_every_pinned_lifetime_field() {
        let (_, _, binding) = fixture();
        assert!(observed_shell(&binding.workload_birth, &binding));
        for field in 0..4 {
            let mut changed = binding.workload_birth.clone();
            match field {
                0 => changed.pid += 1,
                1 => changed.parent_pid += 1,
                2 => changed.start[0] += 1,
                _ => changed.boot.push('x'),
            }
            assert!(!observed_shell(&changed, &binding));
        }
    }
    #[test]
    fn a_second_complete_exec_error_frame_cannot_replace_the_first() {
        let (mut gate, mut peer, binding) = fixture();
        let error = |errno| ExecFailure {
            owner_uuid: binding.owner_uuid,
            owner_generation: binding.owner_generation,
            binding_digest: binding.digest().unwrap(),
            errno,
        };
        send(&mut peer, &error(2)).unwrap();
        gate.poll_channel(false).unwrap();
        assert!(matches!(
            gate.observation,
            ExecutionObservation::ExecFailed { errno: 2, .. }
        ));
        send(&mut peer, &error(13)).unwrap();
        assert!(
            gate.poll_channel(false)
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
        assert!(matches!(
            gate.observation,
            ExecutionObservation::ExecFailed { errno: 2, .. }
        ));
    }
    #[test]
    fn terminal_drain_rejects_truncated_exec_error_without_sampling_a_reaped_pid() {
        let (mut gate, mut peer, _) = fixture();
        use std::io::Write;
        peer.write_all(&[10, 0, 0, 0, b'{']).unwrap();
        drop(peer);
        assert!(gate.finish().unwrap_err().to_string().contains("truncated"));
        assert_eq!(gate.observation, ExecutionObservation::NotObserved);
    }
}
