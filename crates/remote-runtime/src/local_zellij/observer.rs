//! Optional, one-use foreground lifecycle channel. A protocol acknowledgment is
//! a consumer assertion; only the future native persistence owner can establish
//! that its declared revision was actually committed to durable storage.
use super::{
    broker, check_deadline, fail, files,
    files::Record,
    observer_protocol::{
        Acknowledgment, Descriptor, Event, FRAME_LIMIT, Frame, FrontendReceipt, MESSAGE_LIMIT,
    },
};
use std::{
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        net::UnixStream,
    },
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_session_runtime::child::{ProcessIdentity, process_identity};

pub(super) use super::observer_protocol::ENV;

#[derive(PartialEq, Eq)]
enum State {
    New,
    Authorized,
    Bound,
    Connected,
    InputReady,
    CompletionAuthorized,
    Finished,
}

pub(super) struct Observer {
    stream: UnixStream,
    descriptor: Descriptor,
    sequence: u64,
    state: State,
}
pub(super) fn requested_deadline(default: Instant) -> io::Result<(Instant, bool)> {
    let Some(path) = std::env::var_os(ENV).map(PathBuf::from) else {
        return Ok((default, false));
    };
    let descriptor: Descriptor = serde_json::from_slice(&files::read_private(&path, FRAME_LIMIT)?)
        .map_err(io::Error::other)?;
    match (&descriptor.input_readiness, descriptor.version) {
        (Some(request), 4) => {
            request.validate()?;
            Ok((
                default.min(super::input_readiness::local_deadline(
                    request.deadline_monotonic_ms,
                )?),
                true,
            ))
        }
        (None, 1..=3) => Ok((default, false)),
        _ => Err(fail("mixed observer version and readiness capability")),
    }
}
impl Observer {
    pub(super) fn configured(
        deadline: Instant,
        mut tick: impl FnMut() -> io::Result<()>,
    ) -> io::Result<Option<Self>> {
        let Some(path) = std::env::var_os(ENV).map(PathBuf::from) else {
            return Ok(None);
        };
        tick()?;
        check_deadline(deadline, "observer setup")?;
        let descriptor: Descriptor =
            serde_json::from_slice(&files::read_private(&path, FRAME_LIMIT)?)
                .map_err(|error| fail(format!("invalid observer descriptor: {error}")))?;
        descriptor.context.validate()?;
        if !matches!(descriptor.version, 1..=4)
            || descriptor.nonce.len() != 64
            || !descriptor
                .nonce
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(fail("invalid observer version or one-use nonce"));
        }
        match (&descriptor.input_readiness, descriptor.version) {
            (Some(request), 4) => request.validate()?,
            (None, 1..=3) => (),
            _ => return Err(fail("mixed observer version and readiness capability")),
        }
        let root = descriptor
            .socket
            .parent()
            .ok_or_else(|| fail("missing observer namespace"))?;
        if !path.is_absolute()
            || path.parent() != Some(root)
            || root.parent() != Some(files::runtime_base()?.as_path())
            || root.canonicalize()? != root
            || path.canonicalize()? != path
        {
            return Err(fail(
                "observer descriptor/socket must share a canonical private runtime namespace",
            ));
        }
        files::validate_directory(root, true)?;
        files::socket_path(&descriptor.socket)?;
        let socket = descriptor.socket.symlink_metadata()?;
        if !socket.file_type().is_socket()
            || socket.uid() != nix::unistd::geteuid().as_raw()
            || socket.mode() & 0o077 != 0
            || socket.nlink() != 1
        {
            return Err(fail("unsafe observer socket type/owner/mode"));
        }
        let root_identity = files::inode(root)?;
        let socket_identity = files::inode(&descriptor.socket)?;
        let stream = broker::connect_while(&descriptor.socket, deadline, &mut tick)?;
        if broker::peer(&stream)? != descriptor.launcher
            || process_identity(std::process::id())?.parent_pid != descriptor.launcher.pid
            || files::inode(root)? != root_identity
            || files::inode(&descriptor.socket)? != socket_identity
        {
            return Err(fail(
                "observer connected peer or namespace identity rejected",
            ));
        }
        files::validate_directory(root, true)?;
        Ok(Some(Self {
            stream,
            descriptor,
            sequence: 0,
            state: State::New,
        }))
    }
    fn next(&self) -> io::Result<u64> {
        let next = self.sequence + 1;
        if next > MESSAGE_LIMIT {
            return Err(fail("observer message count exceeded"));
        }
        Ok(next)
    }
    fn emit(
        &mut self,
        event: Event,
        deadline: Option<Instant>,
        mut tick: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        if broker::peer(&self.stream)? != self.descriptor.launcher {
            return Err(fail("observer launcher identity changed"));
        }
        let sequence = self.next()?;
        let bytes = serde_json::to_vec(&Frame {
            version: self.descriptor.version,
            context: self.descriptor.context.clone(),
            nonce: self.descriptor.nonce.clone(),
            sequence,
            event,
        })
        .map_err(io::Error::other)?;
        if bytes.len() > FRAME_LIMIT {
            return Err(fail("observer frame exceeded32KiB"));
        }
        let mut frame = Vec::with_capacity(bytes.len() + 4);
        frame.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        frame.extend_from_slice(&bytes);
        transfer(&mut self.stream, &mut frame, true, deadline, &mut tick)
            .map_err(|error| fail(format!("observer lifecycle publication failed: {error}")))?;
        self.sequence = sequence;
        Ok(())
    }
    pub(super) fn reserved(
        &mut self,
        owner_uuid: Uuid,
        deadline: Instant,
        mut tick: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        if self.state != State::New {
            return Err(fail("duplicate observer reservation"));
        }
        self.emit(
            Event::OwnerReserved { owner_uuid },
            Some(deadline),
            &mut tick,
        )?;
        let mut length = [0; 4];
        transfer(
            &mut self.stream,
            &mut length,
            false,
            Some(deadline),
            &mut tick,
        )?;
        let length = u32::from_le_bytes(length) as usize;
        if !(1..=FRAME_LIMIT).contains(&length) {
            return Err(fail("observer acknowledgment frame exceeded bound"));
        }
        let mut bytes = vec![0; length];
        transfer(
            &mut self.stream,
            &mut bytes,
            false,
            Some(deadline),
            &mut tick,
        )?;
        let ack: Acknowledgment = serde_json::from_slice(&bytes).map_err(|error| {
            fail(format!(
                "invalid observer reservation acknowledgment: {error}"
            ))
        })?;
        if broker::peer(&self.stream)? != self.descriptor.launcher
            || ack.version != self.descriptor.version
            || ack.context != self.descriptor.context
            || ack.nonce != self.descriptor.nonce
            || ack.sequence != self.next()?
            || ack.event != "reservation_committed"
            || ack.owner_uuid != owner_uuid
            || ack.document_revision == 0
            || ack.owner_generation.is_some()
            || ack.receipt_digest.is_some()
        {
            return Err(fail(
                "observer reservation acknowledgment correlation rejected",
            ));
        }
        // This admits the authenticated consumer's declaration, not a claim
        // that this foreground process independently performed its durable save.
        self.sequence = ack.sequence;
        self.state = State::Authorized;
        Ok(())
    }
    pub(super) fn bound(
        &mut self,
        record: &Record,
        deadline: Instant,
        tick: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        if self.descriptor.version < 3
            && record
                .completion
                .as_ref()
                .is_some_and(|c| c.execution_contract.is_some())
        {
            return Err(fail("legacy observer cannot accept admitted V2 owner"));
        }
        if !matches!(self.state, State::New | State::Authorized) {
            return Err(fail("observer bound transition rejected"));
        }
        self.emit(
            Event::OwnerBound {
                record: record.clone(),
            },
            Some(deadline),
            tick,
        )?;
        self.state = State::Bound;
        Ok(())
    }
    pub(super) fn connected(
        &mut self,
        owner_uuid: Uuid,
        frontend: &ProcessIdentity,
        attachment_id: Option<Uuid>,
    ) -> io::Result<()> {
        if self.state != State::Bound {
            return Err(fail("observer transport transition rejected"));
        }
        // Never stall terminal forwarding or start another timeout owner. The
        // bounded frame must be accepted immediately by this nonblocking stream.
        self.emit(
            Event::TransportConnected {
                owner_uuid,
                frontend: frontend.clone(),
                attachment_id,
            },
            None,
            || Ok(()),
        )?;
        self.state = State::Connected;
        Ok(())
    }
    pub(super) fn readiness(&self) -> Option<&super::observer_protocol::ReadinessRequest> {
        self.descriptor.input_readiness.as_ref()
    }
    pub(super) fn input_ready(
        &mut self,
        owner_uuid: Uuid,
        owner_generation: Uuid,
        attachment_id: Uuid,
        frontend: &ProcessIdentity,
    ) -> io::Result<()> {
        if self.descriptor.version != 4 || self.state != State::Connected || attachment_id.is_nil()
        {
            return Err(fail("observer input readiness transition rejected"));
        }
        self.readiness()
            .ok_or_else(|| fail("input readiness capability missing"))?
            .validate()?;
        self.emit(
            Event::InputReady {
                owner_uuid,
                owner_generation,
                attachment_id,
                frontend: frontend.clone(),
            },
            None,
            || Ok(()),
        )?;
        self.state = State::InputReady;
        Ok(())
    }
    pub(super) fn version(&self) -> u8 {
        self.descriptor.version
    }
    pub(super) fn launcher(&self) -> ProcessIdentity {
        self.descriptor.launcher.clone()
    }
    pub(super) fn context(&self) -> super::observer_protocol::Context {
        self.descriptor.context.clone()
    }
    pub(super) fn completed(
        &mut self,
        receipt: &super::completion::TerminalReceipt,
        deadline: Instant,
        mut tick: impl FnMut() -> io::Result<()>,
    ) -> io::Result<u64> {
        if self.descriptor.version < 2
            || !matches!(
                self.state,
                State::Bound | State::Connected | State::InputReady
            )
        {
            return Err(fail("completion requires observer v2 bound state"));
        }
        self.emit(
            Event::WorkloadFinished {
                owner_uuid: receipt.owner_uuid(),
                receipt: receipt.clone(),
            },
            Some(deadline),
            &mut tick,
        )?;
        let mut length = [0; 4];
        transfer(
            &mut self.stream,
            &mut length,
            false,
            Some(deadline),
            &mut tick,
        )?;
        let length = u32::from_le_bytes(length) as usize;
        if !(1..=FRAME_LIMIT).contains(&length) {
            return Err(fail("completion acknowledgment exceeds bound"));
        }
        let mut bytes = vec![0; length];
        transfer(
            &mut self.stream,
            &mut bytes,
            false,
            Some(deadline),
            &mut tick,
        )?;
        let ack: Acknowledgment = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if broker::peer(&self.stream)? != self.descriptor.launcher
            || ack.version != self.descriptor.version
            || ack.context != self.descriptor.context
            || ack.nonce != self.descriptor.nonce
            || ack.sequence != self.next()?
            || ack.event != "completion_committed"
            || ack.owner_uuid != receipt.owner_uuid()
            || ack.owner_generation != Some(receipt.owner_generation())
            || ack.receipt_digest.as_ref() != Some(&receipt.digest()?)
            || ack.document_revision == 0
        {
            return Err(fail("completion acknowledgment correlation rejected"));
        }
        self.sequence = ack.sequence;
        self.state = State::CompletionAuthorized;
        Ok(ack.document_revision)
    }
    pub(super) fn poll(&mut self) -> io::Result<()> {
        let mut byte = [0];
        match self.stream.read(&mut byte) {
            Ok(0) => Err(fail("observer EOF; frontend outcome is unknown")),
            Ok(_) => Err(fail(
                "unexpected observer frame after reservation acknowledgment",
            )),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(()),
            Err(error) => Err(error),
        }
    }
    pub(super) fn finished(
        &mut self,
        owner_uuid: Uuid,
        receipt: FrontendReceipt,
    ) -> io::Result<()> {
        if !matches!(
            self.state,
            State::Bound | State::Connected | State::InputReady | State::CompletionAuthorized
        ) {
            return Err(fail("observer finished transition rejected"));
        }
        // Cleanup is already physically complete. Do not renew or add to the
        // cancellation deadline while trying to publish its final receipt.
        self.emit(
            Event::FrontendFinished {
                owner_uuid,
                receipt,
            },
            None,
            || Ok(()),
        )?;
        self.state = State::Finished;
        Ok(())
    }
}

fn transfer(
    stream: &mut UnixStream,
    bytes: &mut [u8],
    writing: bool,
    deadline: Option<Instant>,
    tick: &mut impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        tick()?;
        if let Some(deadline) = deadline {
            check_deadline(deadline, "observer frame")?;
        }
        let result = if writing {
            stream.write(&bytes[offset..])
        } else {
            stream.read(&mut bytes[offset..])
        };
        match result {
            Ok(0) => return Err(fail("observer EOF or zero-byte write")),
            Ok(count) => offset += count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let Some(deadline) = deadline else {
                    return Err(fail(
                        "observer lifecycle frame would block; outcome unknown",
                    ));
                };
                check_deadline(deadline, "observer frame")?;
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted && deadline.is_some() => (),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_lifecycle_frame_fails_without_waiting_or_replaying_after_socket_backpressure() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        writer.set_nonblocking(true).unwrap();
        nix::sys::socket::setsockopt(&writer, nix::sys::socket::sockopt::SndBuf, &1024).unwrap();
        let mut frame = (FRAME_LIMIT as u32).to_le_bytes().to_vec();
        frame.extend(std::iter::repeat_n(b'x', FRAME_LIMIT));
        let expected = frame.clone();
        let started = Instant::now();
        let error = transfer(&mut writer, &mut frame, true, None, &mut || Ok(())).unwrap_err();
        assert!(error.to_string().contains("lifecycle frame would block"));
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(writer);
        let mut actual = Vec::new();
        reader.read_to_end(&mut actual).unwrap();
        assert!(!actual.is_empty() && actual.len() < expected.len());
        assert_eq!(actual, expected[..actual.len()]);
        assert_eq!(
            u32::from_le_bytes(actual[..4].try_into().unwrap()) as usize,
            FRAME_LIMIT
        );
    }
}
