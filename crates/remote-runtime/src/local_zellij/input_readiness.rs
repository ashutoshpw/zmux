//! Optional per-attachment readiness proof from the retained configured frontend.
//! This channel carries no user input and grants no workload creation authority.
use super::{broker, fail, files, observer_protocol::ReadinessRequest};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_session_runtime::child::{ProcessIdentity, process_identity};
pub const ENV: &str = "ZMUX_ZELLIJ_FRONTEND_READY";
pub const CAPABILITY: &str = "zmux.zellij.frontend.input_ready";
const FRAME_LIMIT: usize = 1024;

pub fn monotonic_ms() -> io::Result<u64> {
    let now =
        nix::time::clock_gettime(nix::time::ClockId::CLOCK_MONOTONIC).map_err(io::Error::from)?;
    let seconds = u64::try_from(now.tv_sec()).map_err(|_| fail("negative monotonic clock"))?;
    let nanos = u64::try_from(now.tv_nsec()).map_err(|_| fail("negative monotonic nanoseconds"))?;
    seconds
        .checked_mul(1000)
        .and_then(|v| v.checked_add(nanos / 1_000_000))
        .ok_or_else(|| fail("monotonic clock overflow"))
}
pub fn monotonic_deadline(deadline: Instant) -> io::Result<u64> {
    monotonic_ms()?
        .checked_add(
            u64::try_from(
                deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis(),
            )
            .map_err(|_| fail("startup deadline overflow"))?,
        )
        .ok_or_else(|| fail("startup deadline overflow"))
}
pub fn local_deadline(deadline_ms: u64) -> io::Result<Instant> {
    let remaining = deadline_ms
        .checked_sub(monotonic_ms()?)
        .filter(|v| *v > 0 && *v <= 10000)
        .ok_or_else(|| fail("input readiness startup deadline expired or exceeds10s"))?;
    Ok(Instant::now() + Duration::from_millis(remaining))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capability {
    capability: String,
    version: u8,
    transport: String,
    max_frame_bytes: usize,
}
pub(super) fn verify_capability(output: &str) -> io::Result<()> {
    if output.len() > FRAME_LIMIT {
        return Err(fail("readiness capability exceeds bound"));
    }
    let value: Capability = serde_json::from_str(output).map_err(io::Error::other)?;
    if value.capability != CAPABILITY
        || value.version != 1
        || value.transport != "unix"
        || value.max_frame_bytes != FRAME_LIMIT
    {
        return Err(fail(
            "selected Zellij lacks required input readiness capability1",
        ));
    }
    Ok(())
}
#[derive(Serialize)]
struct Descriptor {
    version: u8,
    attachment_id: Uuid,
    owner_uuid: Uuid,
    owner_generation: Uuid,
    parent_pid: u32,
    parent_uid: u32,
    socket: PathBuf,
    namespace_identity: (u64, u64),
    socket_identity: (u64, u64),
    nonce: String,
    deadline_monotonic_ms: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    version: u8,
    event: String,
    attachment_id: Uuid,
    nonce: String,
    sequence: u8,
}
fn encode(frame: &Frame) -> io::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(frame).map_err(io::Error::other)?;
    if bytes.is_empty() || bytes.len() > FRAME_LIMIT {
        return Err(fail("readiness frame exceeds1KiB"));
    }
    let mut out = (bytes.len() as u32).to_le_bytes().to_vec();
    out.extend(bytes);
    Ok(out)
}
#[derive(PartialEq, Eq)]
enum State {
    AwaitPeer,
    Challenge,
    Ready,
    Accepted,
    Eof,
    Admitted,
}
pub(super) struct Gate {
    listener: UnixListener,
    stream: Option<UnixStream>,
    root: PathBuf,
    path: PathBuf,
    descriptor: PathBuf,
    root_id: (u64, u64),
    socket_id: (u64, u64),
    descriptor_id: (u64, u64),
    nonce: String,
    pub attachment_id: Uuid,
    deadline_ms: u64,
    state: State,
    input: Vec<u8>,
    output: Vec<u8>,
    offset: usize,
}
impl Gate {
    pub fn create(
        root: &Path,
        record: &super::Record,
        request: &ReadinessRequest,
    ) -> io::Result<Self> {
        request.validate()?;
        files::validate_directory(root, true)?;
        let attachment_id = root
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix('a'))
            .and_then(|n| Uuid::parse_str(n).ok())
            .ok_or_else(|| fail("invalid readiness attachment namespace"))?;
        let path = root.join("r");
        let descriptor = root.join("r.json");
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let root_id = files::inode(root)?;
        let socket_id = files::inode(&path)?;
        let nonce = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let parent = process_identity(std::process::id())?;
        let bytes = serde_json::to_vec(&Descriptor {
            version: 1,
            attachment_id,
            owner_uuid: record.id,
            owner_generation: record.execution_binding()?.owner_generation(),
            parent_pid: parent.pid,
            parent_uid: parent.uid,
            socket: path.clone(),
            namespace_identity: root_id,
            socket_identity: socket_id,
            nonce: nonce.clone(),
            deadline_monotonic_ms: request.deadline_monotonic_ms,
        })
        .map_err(io::Error::other)?;
        if bytes.len() > 8192 {
            return Err(fail("readiness descriptor exceeds8KiB"));
        }
        files::write_private(&descriptor, &bytes)?;
        let descriptor_id = files::inode(&descriptor)?;
        Ok(Self {
            listener,
            stream: None,
            root: root.into(),
            path,
            descriptor,
            root_id,
            socket_id,
            descriptor_id,
            nonce,
            attachment_id,
            deadline_ms: request.deadline_monotonic_ms,
            state: State::AwaitPeer,
            input: Vec::new(),
            output: Vec::new(),
            offset: 0,
        })
    }
    pub fn descriptor(&self) -> &Path {
        &self.descriptor
    }
    fn validate(&self, expected: &ProcessIdentity) -> io::Result<()> {
        if monotonic_ms()? >= self.deadline_ms {
            return Err(fail("frontend input readiness deadline; input unavailable"));
        }
        files::validate_directory(&self.root, true)?;
        let socket = fs::symlink_metadata(&self.path)?;
        let descriptor = fs::symlink_metadata(&self.descriptor)?;
        if files::inode(&self.root)? != self.root_id
            || files::inode(&self.path)? != self.socket_id
            || files::inode(&self.descriptor)? != self.descriptor_id
            || !socket.file_type().is_socket()
            || socket.uid() != expected.uid
            || socket.mode() & 0o077 != 0
            || !descriptor.is_file()
            || descriptor.uid() != expected.uid
            || descriptor.mode() & 0o077 != 0
            || descriptor.nlink() != 1
            || descriptor.len() > 8192
            || expected.parent_pid != std::process::id()
            || expected.uid != nix::unistd::geteuid().as_raw()
            || process_identity(expected.pid)? != *expected
        {
            return Err(fail(
                "readiness namespace or exact retained frontend identity changed",
            ));
        }
        Ok(())
    }
    fn outgoing(&self, event: &str, sequence: u8) -> io::Result<Vec<u8>> {
        encode(&Frame {
            version: 1,
            event: event.into(),
            attachment_id: self.attachment_id,
            nonce: self.nonce.clone(),
            sequence,
        })
    }
    pub fn poll(&mut self, expected: &ProcessIdentity) -> io::Result<bool> {
        if self.state == State::Admitted {
            return Ok(true);
        }
        self.validate(expected)?;
        if self.state == State::AwaitPeer {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if broker::peer(&stream)? != *expected {
                        return Err(fail(
                            "readiness peer is not the exact retained configured frontend",
                        ));
                    }
                    stream.set_nonblocking(true)?;
                    self.stream = Some(stream);
                    self.output = self.outgoing("challenge", 1)?;
                    self.state = State::Challenge;
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    return Ok(false);
                }
                Err(e) => return Err(e),
            }
        }
        let stream = self.stream.as_mut().expect("accepted retained peer");
        if self.state == State::Challenge || self.state == State::Accepted {
            match stream.write(&self.output[self.offset..]) {
                Ok(0) => return Err(fail("readiness zero write")),
                Ok(n) => self.offset += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    return Ok(false);
                }
                Err(e) => return Err(e),
            }
            if self.offset < self.output.len() {
                return Ok(false);
            }
            self.output.clear();
            self.offset = 0;
            self.state = if self.state == State::Challenge {
                State::Ready
            } else {
                State::Eof
            };
        }
        let mut bytes = [0; FRAME_LIMIT + 5];
        let eof = match stream.read(&mut bytes) {
            Ok(0) => true,
            Ok(n) => {
                self.input.extend_from_slice(&bytes[..n]);
                false
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                return Ok(false);
            }
            Err(e) => return Err(e),
        };
        if self.state == State::Eof {
            if !self.input.is_empty() {
                return Err(fail("unexpected readiness bytes after accepted"));
            }
            if eof {
                self.validate(expected)?;
                self.state = State::Admitted;
                return Ok(true);
            }
            return Ok(false);
        }
        if eof {
            return Err(fail("frontend readiness EOF before complete proof"));
        }
        if self.input.len() > FRAME_LIMIT + 4 {
            return Err(fail("readiness frame exceeds1KiB"));
        }
        if self.input.len() >= 4 {
            let size = u32::from_le_bytes(self.input[..4].try_into().expect("four bytes")) as usize;
            if size == 0 || size > FRAME_LIMIT || self.input.len() > size + 4 {
                return Err(fail("invalid or trailing readiness frame"));
            }
            if self.input.len() == size + 4 {
                let frame: Frame =
                    serde_json::from_slice(&self.input[4..]).map_err(io::Error::other)?;
                if frame.version != 1
                    || frame.event != "input_ready"
                    || frame.sequence != 2
                    || frame.attachment_id != self.attachment_id
                    || frame.nonce != self.nonce
                {
                    return Err(fail("frontend readiness correlation rejected"));
                }
                self.input.clear();
                self.output = self.outgoing("accepted", 3)?;
                self.state = State::Accepted;
            }
        }
        Ok(false)
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        self.stream.take();
        for (path, id) in [
            (&self.descriptor, self.descriptor_id),
            (&self.path, self.socket_id),
        ] {
            if files::inode(&self.root).ok() == Some(self.root_id)
                && files::inode(path).ok() == Some(id)
                && let Err(error) = fs::remove_file(path)
            {
                eprintln!("readiness private channel cleanup: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Actual retained process/Unix channel controls. Ready is synthetic here;
    //! only real patched-Zellij acceptance can establish its semantic trigger.
    use super::*;
    use crate::local_zellij::{
        NamespaceIdentity, Record,
        completion::{Capability as OwnerCapability, CommittedBinding},
    };
    use serde_json::{Value, json};
    use std::{
        os::unix::fs::DirBuilderExt,
        process::{Child, Command, Stdio},
        thread,
    };
    const HELPER: &str = "local_zellij::input_readiness::tests::retained_readiness_peer";
    fn exact(stream: &mut UnixStream) -> io::Result<Value> {
        let mut size = [0; 4];
        stream.read_exact(&mut size)?;
        let size = u32::from_le_bytes(size) as usize;
        if size > 1024 {
            return Err(fail("fixture frame bound"));
        }
        let mut bytes = vec![0; size];
        stream.read_exact(&mut bytes)?;
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    }
    fn wait(path: &Path) {
        let until = Instant::now() + Duration::from_secs(3);
        while !path.exists() {
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    #[ignore = "owned subprocess protocol fixture"]
    fn retained_readiness_peer() {
        let root = PathBuf::from(std::env::var_os("ZMUX_READY_FIXTURE_ROOT").unwrap());
        let mode = std::env::var("ZMUX_READY_FIXTURE_MODE").unwrap();
        if mode == "hold" {
            wait(&root.join("release"));
            return;
        }
        let Ok(bytes) = fs::read(root.join("r.json")) else {
            return;
        };
        let descriptor: Value = serde_json::from_slice(&bytes).unwrap();
        let mut stream = UnixStream::connect(descriptor["socket"].as_str().unwrap()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let Ok(challenge) = exact(&mut stream) else {
            return;
        };
        assert_eq!(challenge["event"], "challenge");
        assert_eq!(challenge["sequence"], 1);
        let ready = json!({"version":1,"event":"input_ready","attachment_id":descriptor["attachment_id"],"nonce":if mode=="wrong_nonce" {json!("0".repeat(64))} else {descriptor["nonce"].clone()},"sequence":2});
        let bytes = serde_json::to_vec(&ready).unwrap();
        let mut out = (bytes.len() as u32).to_le_bytes().to_vec();
        out.extend(bytes);
        if mode == "trailing" {
            out.push(b'x');
        }
        for bytes in out.chunks(3) {
            if stream.write_all(bytes).is_err() {
                return;
            }
        }
        let Ok(accepted) = exact(&mut stream) else {
            return;
        };
        assert_eq!(accepted["event"], "accepted");
        assert_eq!(accepted["sequence"], 3);
        fs::write(root.join("accepted"), b"reporter parsed accepted").unwrap();
        wait(&root.join("close-reporter"));
        drop(stream);
        wait(&root.join("release"));
    }
    struct Fixture {
        gate: Option<Gate>,
        child: Child,
        root: PathBuf,
        identity: ProcessIdentity,
    }
    impl Fixture {
        fn new(mode: &str) -> Self {
            let root = files::runtime_base()
                .unwrap()
                .join(format!("a{}", Uuid::new_v4().simple()));
            fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
            let own = process_identity(std::process::id()).unwrap();
            let mut server = own.clone();
            server.pid = own.parent_pid;
            let mut workload = own.clone();
            workload.pid += 1;
            workload.parent_pid = own.pid;
            let owner = Uuid::new_v4();
            let binding = CommittedBinding {
                version: 1,
                owner_uuid: owner,
                owner_generation: Uuid::new_v4(),
                namespace: NamespaceIdentity {
                    root: (1, 2),
                    socket: (1, 3),
                },
                completion_socket: (1, 4),
                server: server.clone(),
                supervisor: own.clone(),
                workload: workload.clone(),
            };
            let record = Record {
                id: owner,
                name: "channel-only".into(),
                state: "bound".into(),
                server: Some(server),
                supervisor: Some(own),
                workload: Some(workload),
                namespace: Some(binding.namespace.clone()),
                completion: Some(OwnerCapability {
                    execution_contract: None,
                    admission: None,
                    owner_generation: binding.owner_generation,
                    completion_socket: binding.completion_socket,
                    binding_digest: binding.digest().unwrap(),
                }),
            };
            let request = ReadinessRequest {
                capability: CAPABILITY.into(),
                version: 1,
                deadline_monotonic_ms: monotonic_ms().unwrap() + 3000,
            };
            let gate = Gate::create(&root, &record, &request).unwrap();
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", HELPER, "--ignored", "--nocapture"])
                .env("ZMUX_READY_FIXTURE_ROOT", &root)
                .env("ZMUX_READY_FIXTURE_MODE", mode)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            let identity = process_identity(child.id()).unwrap();
            Self {
                gate: Some(gate),
                child,
                root,
                identity,
            }
        }
        fn poll(&mut self) -> io::Result<bool> {
            self.gate.as_mut().unwrap().poll(&self.identity)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.gate.take();
            fs::write(self.root.join("close-reporter"), b"cleanup").unwrap();
            fs::write(self.root.join("release"), b"cleanup").unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            while self.child.try_wait().unwrap().is_none() {
                if Instant::now() >= deadline {
                    self.child.kill().unwrap();
                    self.child.wait().unwrap();
                    panic!("owned channel peer cleanup expired");
                }
                thread::sleep(Duration::from_millis(2));
            }
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    #[test]
    fn actual_retained_peer_requires_accepted_eof_before_admission() {
        let mut fixture = Fixture::new("valid");
        let deadline = Instant::now() + Duration::from_secs(2);
        while !fixture.root.join("accepted").exists() {
            assert!(!fixture.poll().unwrap());
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
        assert!(
            !fixture.poll().unwrap(),
            "physical accepted write alone is not reporter teardown"
        );
        fs::write(
            fixture.root.join("close-reporter"),
            b"valid reporter teardown",
        )
        .unwrap();
        while !fixture.poll().unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            process_identity(fixture.child.id()).unwrap(),
            fixture.identity
        );
    }
    #[test]
    fn actual_retained_peer_wrong_nonce_and_trailing_frame_never_admit() {
        for mode in ["wrong_nonce", "trailing"] {
            let mut fixture = Fixture::new(mode);
            let deadline = Instant::now() + Duration::from_secs(2);
            let error = loop {
                match fixture.poll() {
                    Err(error) => break error,
                    Ok(false) => (),
                    Ok(true) => panic!("invalid control admitted"),
                };
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(2));
            };
            assert!(
                error.to_string().contains(if mode == "wrong_nonce" {
                    "correlation"
                } else {
                    "trailing"
                }),
                "{error}"
            );
        }
    }
    #[test]
    fn a_different_actual_direct_child_cannot_assert_readiness_for_the_retained_frontend() {
        let held = Fixture::new("hold");
        let mut fixture = Fixture::new("valid");
        fixture.identity = held.identity.clone();
        let deadline = Instant::now() + Duration::from_secs(2);
        let error = loop {
            match fixture.poll() {
                Err(error) => break error,
                Ok(false) => (),
                Ok(true) => panic!("wrong actual Child admitted"),
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        };
        assert!(
            error
                .to_string()
                .contains("peer is not the exact retained configured frontend"),
            "{error}"
        );
    }
    #[test]
    fn replacing_the_actual_readiness_socket_cannot_admit_a_frontend() {
        let mut fixture = Fixture::new("hold");
        fs::rename(fixture.root.join("r"), fixture.root.join("original-r")).unwrap();
        let _replacement = UnixListener::bind(fixture.root.join("r")).unwrap();
        assert!(
            fixture
                .poll()
                .unwrap_err()
                .to_string()
                .contains("namespace")
        );
    }
    #[test]
    fn fixed_monotonic_budget_and_changed_retained_identity_fail_closed() {
        assert!(local_deadline(monotonic_ms().unwrap()).is_err());
        assert!(local_deadline(monotonic_ms().unwrap() + 11000).is_err());
        let mut fixture = Fixture::new("valid");
        fixture.identity.start[0] += 1;
        assert!(
            fixture
                .poll()
                .unwrap_err()
                .to_string()
                .contains("identity changed")
        );
    }
}
