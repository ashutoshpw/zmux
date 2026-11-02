//! Bounded reader for provider lifecycle frames carried by a managed SSH master.
//!
//! Frames are treated as unauthenticated provider claims from the selected SSH
//! account. The nonce correlates the exact terminal bootstrap; it is not provider
//! process authentication or a replay cursor.

use crate::agent_lifecycle::Target;
use filedescriptor::{AsRawFileDescriptor, FileDescriptor, POLLIN, POLLOUT, poll, pollfd};
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_remote_runtime::{
    agent_lifecycle::{
        AGENT_LIFECYCLE_PROTOCOL_VERSION, ClientFrame, Frame, HookDisposition,
        MAX_AGENT_LIFECYCLE_FRAME_BYTES, MAX_PENDING_HOOK_RECEIPTS,
    },
    connection::ManagedConnection,
};
use zmux_session_runtime::shell_integration::OpenRequestNonce;

const MAX_PARTIAL_FRAME_BYTES: usize = MAX_AGENT_LIFECYCLE_FRAME_BYTES;
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);
const CLIENT_FRAME_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const CHILD_REAP_SYNC_TIMEOUT: Duration = Duration::from_millis(250);
const CHILD_REAP_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_RECONNECT_ATTEMPTS: usize = 5;
const MAX_RECONNECT_GENERATIONS: usize = 8;
const MAX_PENDING_RECEIPTS: usize = MAX_PENDING_HOOK_RECEIPTS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub target: Target,
    pub launch_id: u64,
    pub runtime_id: Uuid,
    pub generation: Uuid,
    pub nonce: Uuid,
}

impl Binding {
    /// Stable owner identity shared by every generation of one launched stream.
    /// Control transitions use this so a queued reconnect is not discarded just
    /// because a later generation has already become the stream's latest binding.
    pub fn same_owner(self, other: Self) -> bool {
        self.target == other.target
            && self.launch_id == other.launch_id
            && self.runtime_id == other.runtime_id
            && self.nonce == other.nonce
    }

    /// Frame claims are admitted only from the latest handshake generation.
    pub fn is_current_generation(self, other: Self) -> bool {
        self == other
    }
}

pub enum Message {
    Connected {
        binding: Binding,
        previous_generation: Option<Uuid>,
    },
    Frame {
        binding: Binding,
        frame: Frame,
    },
    Lost {
        binding: Binding,
    },
}

#[derive(Clone, Copy)]
struct ReceiptCommand {
    binding: Binding,
    event_id: Uuid,
    disposition: HookDisposition,
}

/// Lifecycle side-channel ownership attached to one exact terminal launch.
/// Dropping the owner cancels its reader without waiting on the desktop worker.
pub struct Stream {
    binding: Arc<Mutex<Binding>>,
    cancellation: Arc<AtomicBool>,
    receipt_sender: SyncSender<ReceiptCommand>,
}

impl Stream {
    pub fn start(
        connection: Arc<ManagedConnection>,
        nonce: OpenRequestNonce,
        target: Target,
        launch_id: u64,
        sender: SyncSender<Message>,
    ) -> io::Result<Self> {
        let binding = Binding {
            target,
            launch_id,
            runtime_id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
            nonce: Uuid::parse_str(&nonce.remote_lifecycle_token())
                .expect("generated SSH lifecycle token is a UUID"),
        };
        let cancellation = Arc::new(AtomicBool::new(false));
        let binding_state = Arc::new(Mutex::new(binding));
        let (receipt_sender, receipt_receiver) = sync_channel(MAX_PENDING_RECEIPTS);
        let thread_cancellation = Arc::clone(&cancellation);
        let thread_binding_state = Arc::clone(&binding_state);
        thread::Builder::new()
            .name(format!("zmux-agent-ssh-{}", target.session_id.0))
            .spawn(move || {
                if let Err(error) = supervise(
                    connection,
                    nonce,
                    binding,
                    thread_binding_state,
                    sender.clone(),
                    receipt_receiver,
                    &thread_cancellation,
                ) && !thread_cancellation.load(Ordering::Acquire)
                {
                    eprintln!("zmux remote agent lifecycle supervisor ended: {error}");
                }
            })?;
        Ok(Self {
            binding: binding_state,
            cancellation,
            receipt_sender,
        })
    }

    pub fn binding(&self) -> Binding {
        *self
            .binding
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Queue an event-bound reply without blocking the desktop executor. A full
    /// or stale queue deliberately leaves the remote hook to emit its silent
    /// visual fallback when its bounded receipt deadline expires.
    pub fn send_receipt(
        &self,
        binding: Binding,
        event_id: Uuid,
        disposition: HookDisposition,
    ) -> bool {
        if event_id.is_nil() || self.binding() != binding {
            return false;
        }
        self.receipt_sender
            .try_send(ReceiptCommand {
                binding,
                event_id,
                disposition,
            })
            .is_ok()
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.cancellation.store(true, Ordering::Release);
    }
}

struct ReapJob {
    child: Child,
    _slot: ReapSlot,
}

struct ReapState {
    reserved: usize,
    pending: VecDeque<ReapJob>,
}

struct ReapManager {
    state: Mutex<ReapState>,
    ready: Condvar,
}

impl ReapManager {
    fn start() -> io::Result<Arc<Self>> {
        static MANAGER: OnceLock<Result<Arc<ReapManager>, String>> = OnceLock::new();
        match MANAGER.get_or_init(|| {
            let manager = Arc::new(Self {
                state: Mutex::new(ReapState {
                    reserved: 0,
                    pending: VecDeque::new(),
                }),
                ready: Condvar::new(),
            });
            let worker = Arc::clone(&manager);
            thread::Builder::new()
                .name("zmux-agent-child-reaper".into())
                .spawn(move || worker.run())
                .map_err(|error| format!("start managed SSH child reaper: {error}"))?;
            Ok(manager)
        }) {
            Ok(manager) => Ok(Arc::clone(manager)),
            Err(error) => Err(io::Error::other(error.clone())),
        }
    }

    fn reserve(self: &Arc<Self>) -> io::Result<ReapSlot> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.reserved >= crate::MAX_TERMINALS {
            return Err(io::Error::other(
                "managed SSH child reaper capacity is exhausted",
            ));
        }
        state.reserved += 1;
        Ok(ReapSlot {
            manager: Arc::clone(self),
        })
    }

    fn enqueue(&self, job: ReapJob) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        // Every queued job keeps one of the MAX_TERMINALS reservations. The
        // worker owns at most one more, so this queue cannot grow without bound.
        debug_assert!(state.pending.len() < crate::MAX_TERMINALS);
        state.pending.push_back(job);
        self.ready.notify_one();
    }

    fn run(self: Arc<Self>) -> ! {
        loop {
            let job = {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                while state.pending.is_empty() {
                    state = self
                        .ready
                        .wait(state)
                        .unwrap_or_else(|poison| poison.into_inner());
                }
                state.pending.pop_front().expect("pending reaper job")
            };
            let ReapJob { mut child, _slot } = job;
            if let Err(error) = child.wait() {
                // `wait` can fail only after the process is already unavailable
                // to this owner (for example, an external reaper consumed it).
                eprintln!("zmux remote lifecycle background reap: {error}");
            }
            drop(_slot);
        }
    }
}

struct ReapSlot {
    manager: Arc<ReapManager>,
}

impl Drop for ReapSlot {
    fn drop(&mut self) {
        let mut state = self
            .manager
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        debug_assert!(state.reserved > 0);
        state.reserved = state.reserved.saturating_sub(1);
    }
}

struct ChildOwner {
    child: Option<Child>,
    reaped: bool,
    slot: Option<ReapSlot>,
    deferred_reap: Arc<AtomicBool>,
}

impl ChildOwner {
    fn spawn(command: &mut Command, deferred_reap: Arc<AtomicBool>) -> io::Result<Self> {
        // Reserve before spawn so even a child that cannot be reaped promptly
        // has a guaranteed place in the bounded background reaper.
        let slot = ReapManager::start()?.reserve()?;
        let child = command.spawn()?;
        Ok(Self {
            child: Some(child),
            reaped: false,
            slot: Some(slot),
            deferred_reap,
        })
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("owned managed SSH child")
    }

    #[cfg(test)]
    fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        let status = self.child_mut().try_wait()?;
        self.reaped |= status.is_some();
        Ok(status)
    }

    fn terminate_group(&mut self) -> bool {
        // `try_wait` releases the PID once the child exits. Since this PID is
        // also the process-group ID, it may already belong to an unrelated
        // process group and must never be signaled after reaping.
        if self.reaped {
            return false;
        }
        let Some(child) = &self.child else {
            return false;
        };
        if let Err(error) = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL)
            && error != nix::errno::Errno::ESRCH
        {
            eprintln!("zmux remote lifecycle process cancellation: {error}");
        }
        true
    }
}

impl Drop for ChildOwner {
    fn drop(&mut self) {
        self.terminate_group();
        if !self.reaped {
            let deadline = Instant::now() + CHILD_REAP_SYNC_TIMEOUT;
            loop {
                match self.child_mut().try_wait() {
                    Ok(Some(_)) => {
                        self.reaped = true;
                        break;
                    }
                    Ok(None) if Instant::now() < deadline => {
                        thread::sleep(
                            CHILD_REAP_POLL_INTERVAL
                                .min(deadline.saturating_duration_since(Instant::now())),
                        );
                    }
                    Ok(None) => break,
                    Err(error) => {
                        eprintln!("zmux remote lifecycle process reap poll: {error}");
                        break;
                    }
                }
            }
        }

        let Some(child) = self.child.take() else {
            return;
        };
        if self.reaped {
            self.slot.take();
            return;
        }

        let Some(slot) = self.slot.take() else {
            eprintln!("managed SSH child reached teardown without a reap reservation");
            return;
        };
        self.deferred_reap.store(true, Ordering::Release);
        Arc::clone(&slot.manager).enqueue(ReapJob { child, _slot: slot });
    }
}

fn supervise(
    connection: Arc<ManagedConnection>,
    nonce: OpenRequestNonce,
    mut binding: Binding,
    shared_binding: Arc<Mutex<Binding>>,
    sender: SyncSender<Message>,
    receipt_receiver: Receiver<ReceiptCommand>,
    cancellation: &AtomicBool,
) -> Result<(), String> {
    let mut connected = false;
    let mut reconnect_generations = 0;
    let mut failures = 0;
    let reap_deferred = Arc::new(AtomicBool::new(false));
    let first_handshake_deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Ok(());
        }
        let previous_generation = binding.generation;
        let mut handshake_generation = None;
        let mut reconnect_previous_generation = None;
        let attempt_handshake_deadline = (!connected).then_some(first_handshake_deadline);
        let mut attempt_state = AttemptState {
            binding: &mut binding,
            shared_binding: &shared_binding,
            connected: &mut connected,
            handshake_generation: &mut handshake_generation,
            reconnect_previous_generation: &mut reconnect_previous_generation,
            reap_deferred: &reap_deferred,
        };
        let result = run_attempt(
            Arc::clone(&connection),
            nonce,
            &mut attempt_state,
            sender.clone(),
            &receipt_receiver,
            cancellation,
            attempt_handshake_deadline,
        );
        if cancellation.load(Ordering::Acquire) {
            return Ok(());
        }
        if reap_deferred.load(Ordering::Acquire) {
            if handshake_generation.is_some() {
                send_cancellable(&sender, Message::Lost { binding }, cancellation)?;
            }
            return result.and(Err(
                "managed SSH lifecycle child was handed to the bounded reaper".into(),
            ));
        }
        if !connected {
            // The login shell may still be writing its nonce-scoped integration
            // files when the ControlMaster socket becomes usable. Retry only
            // this startup race, on the same managed route and within one
            // bounded first-handshake deadline. Until Active is validated, the
            // remote notice hook stays unlocked and emits its generic fallback.
            failures += 1;
            if failures >= MAX_RECONNECT_ATTEMPTS || Instant::now() >= first_handshake_deadline {
                return result.and(Err(
                    "managed SSH lifecycle stream ended before handshake deadline".into(),
                ));
            }
            if let Err(error) = &result {
                eprintln!("zmux remote agent lifecycle startup retry: {error}");
            }
            let delay = reconnect_delay(failures)
                .min(first_handshake_deadline.saturating_duration_since(Instant::now()));
            if !sleep_cancellable(delay, cancellation) {
                return Ok(());
            }
            continue;
        }
        if let Some(handshake_generation) = handshake_generation {
            if handshake_generation != previous_generation {
                reconnect_generations += 1;
            }
            send_cancellable(&sender, Message::Lost { binding }, cancellation)?;
        }
        if handshake_generation.is_some() {
            failures = 0;
        }
        failures += 1;
        if failures >= MAX_RECONNECT_ATTEMPTS || reconnect_generations >= MAX_RECONNECT_GENERATIONS
        {
            return result.and(Err("managed SSH lifecycle reconnect limit reached".into()));
        }
        if !sleep_cancellable(reconnect_delay(failures), cancellation) {
            return Ok(());
        }
        if let Err(error) = result {
            eprintln!("zmux remote agent lifecycle stream retry: {error}");
        }
    }
}

fn reconnect_delay(failures: usize) -> Duration {
    match failures {
        0 | 1 => Duration::from_millis(250),
        2 => Duration::from_millis(500),
        3 => Duration::from_secs(1),
        _ => Duration::from_secs(2),
    }
}

/// Preserve transition order across bounded-channel pressure. The supervisor
/// waits until a loss or a reconnect handshake has entered the same queue before
/// it proceeds to the next generation.
pub(crate) fn send_cancellable<T>(
    sender: &SyncSender<T>,
    mut message: T,
    cancellation: &AtomicBool,
) -> Result<(), &'static str> {
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Err("managed SSH lifecycle message cancelled");
        }
        match sender.try_send(message) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Full(returned)) => message = returned,
            Err(TrySendError::Disconnected(_)) => {
                return Err("managed SSH lifecycle receiver closed");
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn sleep_cancellable(duration: Duration, cancellation: &AtomicBool) -> bool {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if cancellation.load(Ordering::Acquire) {
            return false;
        }
        thread::sleep(
            Duration::from_millis(25).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
    !cancellation.load(Ordering::Acquire)
}

struct AttemptState<'a> {
    binding: &'a mut Binding,
    shared_binding: &'a Mutex<Binding>,
    connected: &'a mut bool,
    handshake_generation: &'a mut Option<Uuid>,
    reconnect_previous_generation: &'a mut Option<Uuid>,
    reap_deferred: &'a Arc<AtomicBool>,
}

fn run_attempt(
    connection: Arc<ManagedConnection>,
    nonce: OpenRequestNonce,
    state: &mut AttemptState<'_>,
    sender: SyncSender<Message>,
    receipt_receiver: &Receiver<ReceiptCommand>,
    cancellation: &AtomicBool,
    first_handshake_deadline: Option<Instant>,
) -> Result<(), String> {
    let spec = connection
        .agent_lifecycle_spec(&nonce)
        .map_err(|error| error.to_string())?;
    let mut command = Command::new(spec.program);
    command
        .args(spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = ChildOwner::spawn(&mut command, Arc::clone(state.reap_deferred))
        .map_err(|error| format!("start managed SSH stream: {error}"))?;
    let stdin = child
        .child_mut()
        .stdin
        .take()
        .ok_or_else(|| "managed SSH stream has no stdin pipe".to_owned())?;
    let mut input = FileDescriptor::dup(&stdin)
        .map_err(|error| format!("open managed SSH stream input: {error}"))?;
    input
        .set_non_blocking(true)
        .map_err(|error| format!("configure managed SSH stream input: {error}"))?;
    drop(stdin);
    let stdout = child
        .child_mut()
        .stdout
        .take()
        .ok_or_else(|| "managed SSH stream has no stdout pipe".to_owned())?;
    let mut output = FileDescriptor::dup(&stdout)
        .map_err(|error| format!("open managed SSH stream output: {error}"))?;
    output
        .set_non_blocking(true)
        .map_err(|error| format!("configure managed SSH stream output: {error}"))?;
    drop(stdout);

    let started_at = Instant::now();
    let handshake_deadline =
        first_handshake_deadline.unwrap_or_else(|| started_at + HANDSHAKE_TIMEOUT);
    let mut last_frame_at = started_at;
    let mut output_pending = Vec::new();
    let mut pending_write = None;
    let mut hello_seen = false;
    let mut active_seen = false;
    let mut activation_sent = false;
    let mut last_sequence = 0;
    let mut output_eof = false;
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Ok(());
        }
        if active_seen && pending_write.is_none() {
            pending_write = take_next_receipt(receipt_receiver, *state.binding)?;
        }
        let now = Instant::now();
        if (!active_seen && now >= handshake_deadline)
            || (active_seen && now.duration_since(last_frame_at) >= HEARTBEAT_TIMEOUT)
        {
            return Err("managed SSH lifecycle heartbeat expired".into());
        }
        if pending_write
            .as_ref()
            .is_some_and(|pending: &PendingClientWrite| Instant::now() >= pending.deadline)
        {
            return Err("managed SSH lifecycle client frame write timed out".into());
        }
        let mut descriptors = [
            pollfd {
                fd: output.as_raw_file_descriptor(),
                events: POLLIN,
                revents: 0,
            },
            pollfd {
                fd: input.as_raw_file_descriptor(),
                events: if pending_write.is_some() { POLLOUT } else { 0 },
                revents: 0,
            },
        ];
        let descriptor_count = if pending_write.is_some() { 2 } else { 1 };
        let poll_timeout = pending_write
            .as_ref()
            .map(|pending: &PendingClientWrite| {
                POLL_INTERVAL.min(pending.deadline.saturating_duration_since(Instant::now()))
            })
            .unwrap_or(POLL_INTERVAL);
        if let Err(error) = poll(&mut descriptors[..descriptor_count], Some(poll_timeout))
            && !matches!(&error, filedescriptor::Error::Poll(error) if error.kind() == io::ErrorKind::Interrupted)
        {
            return Err(format!("poll managed SSH lifecycle pipes: {error}"));
        }
        if descriptor_count == 2 && descriptors[1].revents != 0 {
            write_pending_client_frame(&mut input, &mut pending_write, &mut activation_sent)?;
        }
        if descriptors[0].revents == 0 {
            continue;
        }
        for _ in 0..4 {
            let mut bytes = [0; 4_096];
            match output.read(&mut bytes) {
                Ok(0) => {
                    output_eof = true;
                    break;
                }
                Ok(length) => {
                    output_pending.extend_from_slice(&bytes[..length]);
                    while let Some(newline) = output_pending.iter().position(|byte| *byte == b'\n')
                    {
                        if newline + 1 > MAX_PARTIAL_FRAME_BYTES {
                            return Err(
                                "managed SSH lifecycle frame exceeded its byte bound".into()
                            );
                        }
                        let line: Vec<_> = output_pending.drain(..=newline).collect();
                        if line[..newline].is_empty() {
                            continue;
                        }
                        let frame: Frame =
                            serde_json::from_slice(&line[..newline]).map_err(|error| {
                                format!("decode managed SSH lifecycle frame: {error}")
                            })?;
                        if frame.version() != AGENT_LIFECYCLE_PROTOCOL_VERSION
                            || frame.nonce() != state.binding.nonce
                        {
                            return Err("managed SSH lifecycle handshake identity mismatch".into());
                        }
                        if frame
                            .generation()
                            .is_some_and(|generation| generation != state.binding.generation)
                        {
                            return Err("managed SSH lifecycle generation mismatch".into());
                        }
                        last_frame_at = Instant::now();
                        match &frame {
                            Frame::Hello { .. } if !hello_seen && last_sequence == 0 => {
                                hello_seen = true;
                                if *state.connected {
                                    *state.reconnect_previous_generation =
                                        Some(state.binding.generation);
                                    state.binding.generation = Uuid::new_v4();
                                    *state
                                        .shared_binding
                                        .lock()
                                        .unwrap_or_else(|poison| poison.into_inner()) =
                                        *state.binding;
                                }
                                pending_write = Some(PendingClientWrite::new(
                                    &ClientFrame::Activate {
                                        version: AGENT_LIFECYCLE_PROTOCOL_VERSION,
                                        nonce: state.binding.nonce,
                                        generation: state.binding.generation,
                                    },
                                    ClientFrameKind::Activate,
                                )?);
                            }
                            Frame::Active { .. }
                                if hello_seen
                                    && activation_sent
                                    && !active_seen
                                    && last_sequence == 0 =>
                            {
                                active_seen = true;
                                *state.connected = true;
                                *state.handshake_generation = Some(state.binding.generation);
                                send_cancellable(
                                    &sender,
                                    Message::Connected {
                                        binding: *state.binding,
                                        previous_generation: *state.reconnect_previous_generation,
                                    },
                                    cancellation,
                                )?;
                            }
                            Frame::Hello { .. } | Frame::Active { .. } => {
                                return Err(
                                    "duplicate or out-of-order managed SSH lifecycle handshake"
                                        .into(),
                                );
                            }
                            Frame::Gap { sequence, .. } => {
                                if !active_seen || *sequence == 0 || *sequence <= last_sequence {
                                    return Err(
                                        "managed SSH lifecycle gap is stale or precedes handshake"
                                            .into(),
                                    );
                                }
                                return Err(
                                    "remote Claude lifecycle hook ingress reported lost evidence"
                                        .into(),
                                );
                            }
                            Frame::Heartbeat { sequence, .. }
                            | Frame::Hook { sequence, .. }
                            | Frame::ProviderState { sequence, .. } => {
                                if !active_seen
                                    || *sequence == 0
                                    || last_sequence.checked_add(1) != Some(*sequence)
                                {
                                    return Err("managed SSH lifecycle sequence is stale, gapped, or precedes handshake".into());
                                }
                                last_sequence = *sequence;
                                if sender
                                    .try_send(Message::Frame {
                                        binding: *state.binding,
                                        frame,
                                    })
                                    .is_err()
                                {
                                    return Err(
                                        "managed SSH lifecycle event queue is full or closed"
                                            .into(),
                                    );
                                }
                            }
                        }
                    }
                    if output_pending.len() > MAX_PARTIAL_FRAME_BYTES {
                        return Err("managed SSH lifecycle frame exceeded its byte bound".into());
                    }
                }
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock
                        || error.kind() == io::ErrorKind::Interrupted =>
                {
                    break;
                }
                Err(error) => return Err(format!("read managed SSH lifecycle output: {error}")),
            }
        }
        if output_eof {
            return Err(format!(
                "managed SSH lifecycle stream closed (hello={hello_seen}, active={active_seen})"
            ));
        }
    }
}

fn _protocol_type_anchor(_: Frame) {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClientFrameKind {
    Activate,
    Receipt,
}

struct PendingClientWrite {
    bytes: Vec<u8>,
    offset: usize,
    deadline: Instant,
    kind: ClientFrameKind,
}

impl PendingClientWrite {
    fn new(frame: &ClientFrame, kind: ClientFrameKind) -> Result<Self, String> {
        let mut bytes = serde_json::to_vec(frame)
            .map_err(|error| format!("encode managed SSH lifecycle client frame: {error}"))?;
        if bytes.len() + 1 > MAX_AGENT_LIFECYCLE_FRAME_BYTES {
            return Err("managed SSH lifecycle client frame exceeded its byte bound".into());
        }
        bytes.push(b'\n');
        Ok(Self {
            bytes,
            offset: 0,
            deadline: Instant::now() + CLIENT_FRAME_WRITE_TIMEOUT,
            kind,
        })
    }
}

fn write_pending_client_frame(
    input: &mut impl Write,
    pending: &mut Option<PendingClientWrite>,
    activation_sent: &mut bool,
) -> Result<(), String> {
    let Some(frame) = pending.as_mut() else {
        return Ok(());
    };
    match input.write(&frame.bytes[frame.offset..]) {
        Ok(0) => Err("managed SSH lifecycle client frame write returned zero bytes".into()),
        Ok(length) => {
            frame.offset += length;
            if frame.offset == frame.bytes.len() {
                let kind = frame.kind;
                *pending = None;
                if kind == ClientFrameKind::Activate {
                    *activation_sent = true;
                }
            }
            Ok(())
        }
        Err(error)
            if error.kind() == io::ErrorKind::WouldBlock
                || error.kind() == io::ErrorKind::Interrupted =>
        {
            Ok(())
        }
        Err(error) => Err(format!("write managed SSH lifecycle client frame: {error}")),
    }
}

fn take_next_receipt(
    receiver: &Receiver<ReceiptCommand>,
    binding: Binding,
) -> Result<Option<PendingClientWrite>, String> {
    loop {
        let command = match receiver.try_recv() {
            Ok(command) => command,
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(None),
        };
        if command.binding != binding || command.event_id.is_nil() {
            continue;
        }
        return PendingClientWrite::new(
            &ClientFrame::Receipt {
                version: AGENT_LIFECYCLE_PROTOCOL_VERSION,
                nonce: binding.nonce,
                generation: binding.generation,
                event_id: command.event_id,
                disposition: command.disposition,
            },
            ClientFrameKind::Receipt,
        )
        .map(Some);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        sync::atomic::AtomicUsize,
        sync::mpsc::TryRecvError,
    };
    use zmux_remote_runtime::SshDescriptor;
    use zmux_remote_runtime::agent_lifecycle::ProviderKind;
    use zmux_session_runtime::{
        Launch, Session, ShutdownPolicy,
        shell_integration::{OpenRequestNonce, OpenRequestRegistration, OpenRequestSource},
    };
    use zmux_workspace_core::{
        Action as WorkspaceAction, SessionId, SurfaceId, WorkspaceStore, targets::TargetKind,
    };

    const FIXTURE_DRIVER: &str = r#"
import importlib.util
import json
from pathlib import Path
import sys
import time

configuration = Path(sys.argv[1])
diagnostic = Path(sys.argv[2])
fixture_path = Path(sys.argv[3])
spec = importlib.util.spec_from_file_location("native_ssh_fixture", fixture_path)
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)
with fixture.native_ssh_fixture(configuration, diagnostic):
    root = Path(json.loads(configuration.read_text())["root"])
    while not (root / ".fixture-controller-stop").exists():
        time.sleep(0.02)
"#;

    const STALLED_LIFECYCLE_PEER: &str = r#"
import json
import sys
import time
import uuid

nonce = sys.argv[1]
def send(frame):
    sys.stdout.write(json.dumps(frame, separators=(",", ":")) + "\n")
    sys.stdout.flush()

send({"kind": "hello", "version": 3, "nonce": nonce})
activation = json.loads(sys.stdin.buffer.readline(4097))
if activation.get("kind") != "activate" or activation.get("version") != 3 or uuid.UUID(activation.get("nonce", "")).hex != nonce:
    sys.exit(70)
generation = activation["generation"]
send({"kind": "active", "version": 3, "nonce": nonce, "generation": generation})

# Never read receipts. Keep the independent stdout path alive so the local
# owner can distinguish stdin backpressure from peer exit or heartbeat expiry.
sequence = 0
try:
    while True:
        sequence += 1
        send({"kind": "heartbeat", "version": 3, "nonce": nonce, "generation": generation, "sequence": sequence})
        time.sleep(0.1)
except BrokenPipeError:
    pass
"#;

    const PROVIDER_SEQUENCE_GAP_PEER: &str = r#"
import json
import sys
import uuid

nonce = uuid.UUID(sys.argv[1]).hex
nonce_text = str(uuid.UUID(hex=nonce))
def send(frame):
    sys.stdout.write(json.dumps(frame, separators=(",", ":")) + "\n")
    sys.stdout.flush()

send({"kind": "hello", "version": 3, "nonce": nonce_text})
activation = json.loads(sys.stdin.buffer.readline(4097))
if activation.get("kind") != "activate" or activation.get("version") != 3 or uuid.UUID(activation.get("nonce", "")).hex != nonce:
    sys.exit(70)
generation = activation["generation"]
send({"kind": "active", "version": 3, "nonce": nonce_text, "generation": generation})

observation = {
    "provider": "codex",
    "scope": "root",
    "session_id": "thread_transport_7",
    "turn_id": "turn_transport_8",
    "state": "working",
    "transition": "state_changed",
    "marker": "live",
}
for sequence in (1, 3):
    send({
        "kind": "provider_state",
        "version": 3,
        "nonce": nonce_text,
        "generation": generation,
        "sequence": sequence,
        "observation": observation,
    })
"#;

    const INSTALLED_LIFECYCLE_SCRIPT: &str =
        include_str!("../../session-runtime/src/shell/claude-lifecycle-stream.py");

    struct SshFixture {
        local_root: PathBuf,
        remote_root: PathBuf,
        configuration: serde_json::Value,
        controller: Option<Child>,
    }

    impl SshFixture {
        fn start() -> Self {
            let local_root = std::env::temp_dir().join(format!(
                "zmux-remote-agent-fixture-{}-{}",
                std::process::id(),
                Uuid::new_v4()
            ));
            fs::create_dir(&local_root).expect("create private fixture controller directory");
            fs::set_permissions(&local_root, fs::Permissions::from_mode(0o700))
                .expect("secure private fixture controller directory");
            let config = local_root.join("fixture.json");
            let diagnostic = local_root.join("sshd.log");
            let fixture_script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../scripts/native-ssh-fixture.py");
            let controller = Command::new("python3")
                .arg("-c")
                .arg(FIXTURE_DRIVER)
                .arg(&config)
                .arg(&diagnostic)
                .arg(fixture_script)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("start private loopback SSH fixture controller");
            let mut fixture = Self {
                local_root,
                remote_root: PathBuf::new(),
                configuration: serde_json::Value::Null,
                controller: Some(controller),
            };
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if config.is_file() {
                    fixture.configuration = serde_json::from_slice(
                        &fs::read(&config).expect("read private loopback SSH fixture config"),
                    )
                    .expect("decode private loopback SSH fixture config");
                    fixture.remote_root = PathBuf::from(
                        fixture.configuration["root"]
                            .as_str()
                            .expect("fixture remote root"),
                    );
                    break;
                }
                if let Some(status) = fixture
                    .controller
                    .as_mut()
                    .expect("fixture controller")
                    .try_wait()
                    .expect("inspect fixture controller")
                {
                    panic!(
                        "private loopback SSH fixture exited ({status}): {}",
                        fs::read_to_string(&diagnostic).unwrap_or_default()
                    );
                }
                assert!(
                    Instant::now() < deadline,
                    "private loopback SSH fixture did not become ready: {}",
                    fs::read_to_string(&diagnostic).unwrap_or_default()
                );
                thread::sleep(Duration::from_millis(25));
            }
            fixture.install_bootstrap_gate();
            fixture
        }

        fn install_bootstrap_gate(&self) {
            let root = shell_quote(self.remote_root.to_str().expect("fixture path is UTF-8"));
            let shell = format!(
                "#!/bin/sh\ncase \"$SSH_ORIGINAL_COMMAND\" in\n  *ZMUX_REMOTE_LIFECYCLE=1*)\n    : > {root}/.bootstrap-started\n    while [ ! -e {root}/.release-bootstrap ]; do /bin/sleep 0.01; done\n    ;;\n  *claude-lifecycle-stream.py*)\n    printf 'attempt\\n' >> {root}/.stream-attempts\n    ;;\nesac\nif [ -n \"$SSH_ORIGINAL_COMMAND\" ]; then exec /bin/sh -c \"$SSH_ORIGINAL_COMMAND\"; else exec /bin/sh -i; fi\n"
            );
            fs::write(self.remote_root.join("shell"), shell)
                .expect("install deterministic remote bootstrap gate");
            fs::set_permissions(
                self.remote_root.join("shell"),
                fs::Permissions::from_mode(0o700),
            )
            .expect("secure deterministic remote bootstrap gate");
        }

        fn connection(&self) -> Arc<ManagedConnection> {
            Arc::new(
                ManagedConnection::create(
                    SshDescriptor::builder("fixture")
                        .expect("fixture destination")
                        .config_file(
                            self.configuration["clientConfig"]
                                .as_str()
                                .expect("fixture client config"),
                        )
                        .identity(
                            self.configuration["identity"]
                                .as_str()
                                .expect("fixture identity"),
                        )
                        .build()
                        .expect("build private loopback SSH descriptor"),
                )
                .expect("create managed SSH connection"),
            )
        }
    }

    impl Drop for SshFixture {
        fn drop(&mut self) {
            let _ = fs::write(self.remote_root.join(".fixture-controller-stop"), b"stop");
            if let Some(controller) = &mut self.controller {
                let deadline = Instant::now() + Duration::from_secs(8);
                loop {
                    if controller.try_wait().ok().flatten().is_some() {
                        break;
                    }
                    if Instant::now() >= deadline {
                        let _ = controller.kill();
                        let _ = controller.wait();
                        break;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
            }
            let _ = fs::remove_dir_all(&self.local_root);
        }
    }

    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    fn private_ssh_prerequisites_available() -> bool {
        Command::new("python3")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
            && ["/usr/sbin/sshd", "/usr/bin/ssh-keygen", "/usr/bin/ssh"]
                .iter()
                .all(|path| Path::new(path).is_file())
    }

    fn push_remote_hook(
        connection: &ManagedConnection,
        nonce: OpenRequestNonce,
        input: serde_json::Value,
        receiver: &std::sync::mpsc::Receiver<Message>,
        last_stream_sequence: &mut u64,
    ) -> (
        Child,
        Binding,
        u64,
        Uuid,
        bool,
        zmux_remote_runtime::agent_lifecycle::ClaudeHook,
    ) {
        const FRAME_DEADLINE: Duration = Duration::from_secs(15);

        let root = nonce.remote_integration_directory();
        let command = format!(
            "printf '%s\\n' {} | ZMUX_AGENT_LIFECYCLE_NONCE={} ZMUX_SHELL_INTEGRATION_DIRECTORY={} python3 {root}/claude-lifecycle-hook.py",
            shell_quote(&input.to_string()),
            nonce.remote_lifecycle_token(),
            root
        );
        let spec = connection
            .operation_spec(&command)
            .expect("build the exact-session remote hook command");
        let child = Command::new(&spec.program)
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("run the remote lifecycle hook command");
        let deadline = Instant::now() + FRAME_DEADLINE;
        let mut last_heartbeat_sequence = None;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "remote lifecycle Hook frame did not arrive within {FRAME_DEADLINE:?}; last heartbeat sequence: {last_heartbeat_sequence:?}"
            );
            let message = receiver.recv_timeout(remaining).unwrap_or_else(|error| {
                panic!(
                    "remote lifecycle Hook frame did not arrive within {FRAME_DEADLINE:?}; last heartbeat sequence: {last_heartbeat_sequence:?}; receive error: {error}"
                )
            });
            match message {
                Message::Frame {
                    frame:
                        zmux_remote_runtime::agent_lifecycle::Frame::Hook {
                            sequence,
                            event_id,
                            receipt_requested,
                            event,
                            ..
                        },
                    binding,
                } => {
                    assert!(
                        sequence > *last_stream_sequence,
                        "remote lifecycle Hook sequence did not advance: previous={last_stream_sequence}, current={sequence}"
                    );
                    *last_stream_sequence = sequence;
                    return (
                        child,
                        binding,
                        sequence,
                        event_id,
                        receipt_requested,
                        *event,
                    );
                }
                Message::Frame {
                    frame: zmux_remote_runtime::agent_lifecycle::Frame::Heartbeat { sequence, .. },
                    ..
                } => {
                    assert!(
                        sequence > *last_stream_sequence,
                        "remote lifecycle Heartbeat sequence did not advance: previous={last_stream_sequence}, current={sequence}"
                    );
                    *last_stream_sequence = sequence;
                    last_heartbeat_sequence = Some(sequence);
                }
                Message::Frame { frame, .. } => {
                    panic!("unexpected remote lifecycle frame before hook admission: {frame:?}")
                }
                Message::Lost { .. } => panic!("remote lifecycle stream was lost before hook"),
                Message::Connected { .. } => {
                    panic!("unexpected reconnect during hook interleaving")
                }
            }
        }
    }

    fn wait_for_remote_heartbeat(
        receiver: &std::sync::mpsc::Receiver<Message>,
        last_stream_sequence: &mut u64,
    ) {
        const HEARTBEAT_DEADLINE: Duration = Duration::from_secs(8);

        let deadline = Instant::now() + HEARTBEAT_DEADLINE;
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "managed lifecycle heartbeat did not arrive within {HEARTBEAT_DEADLINE:?}; last stream sequence: {last_stream_sequence}"
        );
        let message = receiver.recv_timeout(remaining).unwrap_or_else(|error| {
            panic!(
                "managed lifecycle heartbeat did not arrive within {HEARTBEAT_DEADLINE:?}; last stream sequence: {last_stream_sequence}; receive error: {error}"
            )
        });
        match message {
            Message::Frame {
                frame: zmux_remote_runtime::agent_lifecycle::Frame::Heartbeat { sequence, .. },
                ..
            } => {
                assert!(
                    sequence > *last_stream_sequence,
                    "remote lifecycle Heartbeat sequence did not advance: previous={last_stream_sequence}, current={sequence}"
                );
                *last_stream_sequence = sequence;
            }
            Message::Frame { frame, .. } => {
                panic!("unexpected lifecycle frame while awaiting heartbeat: {frame:?}")
            }
            Message::Lost { .. } => panic!("remote lifecycle stream was lost before heartbeat"),
            Message::Connected { .. } => {
                panic!("unexpected reconnect while awaiting heartbeat")
            }
        }
    }

    fn wait_remote_hook(mut child: Child) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().expect("poll remote hook process") {
                assert!(status.success(), "remote hook process failed: {status}");
                return;
            }
            assert!(
                Instant::now() < deadline,
                "remote hook did not finish within its provider hook deadline"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn session_snapshot_before(
        session: &Session,
        deadline: Instant,
        operation: &str,
    ) -> Arc<zmux_session_runtime::Snapshot> {
        loop {
            assert!(
                Instant::now() < deadline,
                "{operation}: session snapshot stayed busy until its deadline"
            );
            match session.snapshot() {
                Ok(snapshot) => return snapshot,
                Err(zmux_session_runtime::SessionError::Busy) => thread::yield_now(),
                Err(error) => panic!("{operation}: {error}"),
            }
        }
    }

    fn current_notices(session: &Session) -> Vec<zmux_terminal_adapter::TerminalNotice> {
        session_snapshot_before(
            session,
            Instant::now() + Duration::from_secs(2),
            "inspect SSH terminal notices",
        )
        .viewport
        .notices
        .clone()
    }

    #[test]
    fn remote_owner_keeps_completed_idle_after_late_stop() {
        use crate::agent_lifecycle::{Activity, HookDisposition, Runtime};

        let namespace = Uuid::new_v4();
        let mut store = WorkspaceStore::with_identity_namespace(namespace);
        store
            .apply(WorkspaceAction::CreateSshWorkspace {
                name: "remote lifecycle".into(),
                connection: zmux_remote_runtime::SshConnectionConfig {
                    destination: "fixture@example.test".into(),
                    config_file: None,
                    port: None,
                    identity: None,
                    forward_agent: None,
                    ssh_options: Vec::new(),
                    jump_hosts: Vec::new(),
                    command: None,
                },
                focus: true,
            })
            .expect("create an exact workspace owner for the remote lifecycle");
        let model = store.snapshot();
        let surface = model.workspaces[0].root.panes()[0].selected();
        let target = Target {
            identity_namespace: namespace,
            surface_id: surface.id,
            surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
            session_id: surface.session_id.expect("workspace surface session"),
        };
        let runtime_id = Uuid::new_v4();
        let generation = Uuid::new_v4();
        let provider_session = Uuid::new_v4();
        let prompt_id = Uuid::new_v4();
        let now = Instant::now();
        let mut lifecycle = Runtime::default();
        let apply_hook = |lifecycle: &mut Runtime, sequence, value| {
            lifecycle
                .apply_remote_hook(
                    target,
                    runtime_id,
                    generation,
                    crate::agent_lifecycle::RemoteHookInvocation {
                        sequence,
                        hook_id: Uuid::new_v4(),
                    },
                    serde_json::from_value(value).expect("valid sanitized Claude hook"),
                    now,
                )
                .expect("admit a current remote lifecycle hook")
        };

        assert_eq!(
            apply_hook(
                &mut lifecycle,
                1,
                serde_json::json!({
                    "name":"SessionStart",
                    "session_id":provider_session,
                    "main_session":true
                }),
            ),
            HookDisposition::NotHandled
        );
        assert_eq!(
            apply_hook(
                &mut lifecycle,
                2,
                serde_json::json!({
                    "name":"UserPromptSubmit",
                    "session_id":provider_session,
                    "main_session":true,
                    "prompt_id":prompt_id
                }),
            ),
            HookDisposition::NotHandled
        );
        assert_eq!(lifecycle.snapshot(&model)[0].activity, Activity::Working);
        assert_eq!(lifecycle.alert_cursor(), 0);

        // A stream Gap is surfaced as Message::Lost. The owner fences the old
        // Working claim as Unknown without admitting an event or alert.
        assert!(lifecycle.remote_transport_lost(target, runtime_id, generation));
        let snapshot = lifecycle.snapshot(&model);
        let lost = &snapshot[0];
        assert_eq!(lost.activity, Activity::Unknown);
        assert_eq!(lost.connection, crate::agent_lifecycle::Connection::Unknown);
        assert_eq!(lifecycle.alert_cursor(), 0);
        assert!(lifecycle.alert_events(&model).is_empty());

        let reconnected_generation = Uuid::new_v4();
        assert!(lifecycle.remote_transport_reconnected(
            target,
            runtime_id,
            generation,
            reconnected_generation,
            now + Duration::from_millis(1),
        ));
        let apply_reconnected_hook = |lifecycle: &mut Runtime, sequence, value| {
            lifecycle
                .apply_remote_hook(
                    target,
                    runtime_id,
                    reconnected_generation,
                    crate::agent_lifecycle::RemoteHookInvocation {
                        sequence,
                        hook_id: Uuid::new_v4(),
                    },
                    serde_json::from_value(value).expect("valid sanitized Claude hook"),
                    now + Duration::from_millis(1),
                )
                .expect("admit a current hook after the new stream handshake")
        };
        let reconnected_prompt = Uuid::new_v4();
        assert_eq!(
            apply_reconnected_hook(
                &mut lifecycle,
                1,
                serde_json::json!({
                    "name":"UserPromptSubmit",
                    "session_id":provider_session,
                    "main_session":true,
                    "prompt_id":reconnected_prompt
                }),
            ),
            HookDisposition::NotHandled
        );
        assert_eq!(
            apply_reconnected_hook(
                &mut lifecycle,
                2,
                serde_json::json!({
                    "name":"Stop",
                    "session_id":provider_session,
                    "main_session":true
                }),
            ),
            HookDisposition::HandledNoAlert
        );
        assert_eq!(
            apply_reconnected_hook(
                &mut lifecycle,
                3,
                serde_json::json!({
                    "name":"Notification",
                    "session_id":provider_session,
                    "main_session":true,
                    "notification_type":"idle_prompt"
                }),
            ),
            HookDisposition::AlertCovered
        );
        assert_eq!(lifecycle.snapshot(&model)[0].activity, Activity::Idle);
        assert_eq!(lifecycle.alert_cursor(), 1);

        // A late Stop with only Unknown evidence cannot undo completion and
        // is acknowledged without another visual fallback. Explicit resumed
        // or background-task evidence remains admitted as Working.
        assert_eq!(
            apply_reconnected_hook(
                &mut lifecycle,
                4,
                serde_json::json!({
                    "name":"Stop",
                    "session_id":provider_session,
                    "main_session":true
                }),
            ),
            HookDisposition::HandledNoAlert
        );
        assert_eq!(lifecycle.snapshot(&model)[0].activity, Activity::Idle);
        assert_eq!(lifecycle.alert_cursor(), 1);
        assert_eq!(
            apply_reconnected_hook(
                &mut lifecycle,
                5,
                serde_json::json!({
                    "name":"Stop",
                    "session_id":provider_session,
                    "main_session":true,
                    "stop_hook_active":true
                }),
            ),
            HookDisposition::HandledNoAlert
        );
        assert_eq!(lifecycle.snapshot(&model)[0].activity, Activity::Working);
        assert_eq!(lifecycle.alert_cursor(), 1);
        assert_eq!(
            apply_reconnected_hook(
                &mut lifecycle,
                6,
                serde_json::json!({
                    "name":"Stop",
                    "session_id":provider_session,
                    "main_session":true,
                    "has_background_tasks":true
                }),
            ),
            HookDisposition::HandledNoAlert
        );
        assert_eq!(lifecycle.snapshot(&model)[0].activity, Activity::Working);
        assert_eq!(lifecycle.alert_cursor(), 1);
        let alerts = lifecycle.alert_events(&model);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, crate::agent_lifecycle::AlertKind::Completed);
    }

    fn wait_for_remote_notice(
        session: &Session,
        source: &str,
        body: &str,
    ) -> zmux_terminal_adapter::TerminalNotice {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(notice) = current_notices(session)
                .into_iter()
                .find(|notice| notice.source == source && notice.body == body)
            {
                return notice;
            }
            assert!(
                Instant::now() < deadline,
                "expected remote terminal notice was not relayed"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn control_sender_waits_for_capacity_and_preserves_call_order() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        sender.try_send(0).unwrap();
        let cancellation = Arc::new(AtomicBool::new(false));
        let thread_sender = sender.clone();
        let thread_cancellation = Arc::clone(&cancellation);
        let thread = thread::spawn(move || {
            send_cancellable(&thread_sender, 1, &thread_cancellation).unwrap();
            send_cancellable(&thread_sender, 2, &thread_cancellation).unwrap();
        });

        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)).unwrap(), 0);
        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)).unwrap(), 2);
        thread.join().unwrap();
    }

    #[test]
    fn child_owner_signals_live_group_then_skips_the_group_after_reap() {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "/bin/sleep 30 & wait"])
            .stdout(Stdio::piped())
            .process_group(0);
        let mut owner = ChildOwner::spawn(&mut command, Arc::new(AtomicBool::new(false)))
            .expect("start a private process group with a stdout-holding descendant");
        let child_stdout = owner
            .child_mut()
            .stdout
            .take()
            .expect("capture child stdout");
        let mut stdout = FileDescriptor::dup(&child_stdout).expect("duplicate child stdout");
        stdout
            .set_non_blocking(true)
            .expect("make child stdout nonblocking");
        assert!(
            owner.terminate_group(),
            "a live child group must be signaled"
        );

        let deadline = Instant::now() + Duration::from_secs(2);
        let status = loop {
            if let Some(status) = owner.try_wait().expect("reap signaled child") {
                break status;
            }
            assert!(Instant::now() < deadline, "signaled child did not exit");
            thread::sleep(Duration::from_millis(5));
        };
        assert!(
            !status.success(),
            "SIGKILL should stop the process-group leader"
        );
        let mut byte = [0; 1];
        let stdout_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match stdout.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error)
                    if error.kind() == io::ErrorKind::Interrupted
                        || error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("read process-group stdout after teardown: {error}"),
            }
            assert!(
                Instant::now() < stdout_deadline,
                "the process-group descendant retained stdout after teardown"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert!(owner.reaped);
        assert!(
            !owner.terminate_group(),
            "a reaped process-group ID may have been reused and must not be signaled"
        );
    }

    fn wait_stream_connected(receiver: &std::sync::mpsc::Receiver<Message>) -> Binding {
        let deadline = Instant::now() + Duration::from_secs(8);
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "managed lifecycle Active handshake deadline elapsed"
        );
        match receiver.recv_timeout(remaining) {
            Ok(Message::Connected { binding, .. }) => binding,
            Ok(Message::Lost { .. }) => panic!("managed lifecycle stream was lost before Active"),
            Ok(Message::Frame { frame, .. }) => {
                panic!("unexpected frame before stalled lifecycle peer: {frame:?}")
            }
            Err(error) => panic!("managed lifecycle Active handshake failed: {error}"),
        }
    }

    #[test]
    fn managed_ssh_forwards_provider_state_and_loses_gapped_streams() {
        if !private_ssh_prerequisites_available() {
            eprintln!(
                "skipping provider frame transport test: private SSH prerequisites unavailable"
            );
            return;
        }

        let fixture = SshFixture::start();
        let connection = fixture.connection();
        let nonce = OpenRequestNonce::generate();
        let target = Target {
            identity_namespace: Uuid::new_v4(),
            surface_id: SurfaceId(17),
            surface_uuid: Uuid::new_v4(),
            session_id: SessionId(23),
        };
        let registration = OpenRequestRegistration::new(nonce, OpenRequestSource::ManagedSsh);
        let terminal = connection.terminal_spec(&nonce);
        let session = Session::spawn_with_open_requests(
            Launch {
                program: terminal.program,
                args: terminal.args,
                cwd: None,
                size: zmux_session_runtime::default_size(),
            },
            &[],
            &[],
            ShutdownPolicy::Kill,
            None,
            Some(registration),
            Arc::clone(&connection),
        )
        .expect("spawn exact managed SSH terminal for provider frame test");

        let bootstrap_started = fixture.remote_root.join(".bootstrap-started");
        let bootstrap_deadline = Instant::now() + Duration::from_secs(10);
        while !bootstrap_started.exists() {
            let snapshot = session_snapshot_before(
                &session,
                bootstrap_deadline,
                "inspect provider frame SSH bootstrap",
            );
            assert_ne!(
                snapshot.state,
                zmux_session_runtime::State::Failed,
                "managed SSH bootstrap failed before provider frame setup: {:?}",
                snapshot.failure
            );
            assert!(
                Instant::now() < bootstrap_deadline,
                "managed SSH terminal did not enter its provider frame bootstrap gate"
            );
            thread::sleep(Duration::from_millis(10));
        }
        fs::write(fixture.remote_root.join(".release-bootstrap"), b"go")
            .expect("release the provider frame test bootstrap");

        let integration_root = PathBuf::from(nonce.remote_integration_directory());
        let lifecycle_script = integration_root.join("claude-lifecycle-stream.py");
        let expected_script = format!("{INSTALLED_LIFECYCLE_SCRIPT}\n");
        let install_deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if fs::read_to_string(&lifecycle_script).ok().as_deref()
                == Some(expected_script.as_str())
            {
                break;
            }
            let snapshot = session_snapshot_before(
                &session,
                install_deadline,
                "inspect provider frame lifecycle helper install",
            );
            assert_ne!(
                snapshot.state,
                zmux_session_runtime::State::Failed,
                "managed SSH bootstrap failed before provider frame helper install: {:?}",
                snapshot.failure
            );
            assert!(
                Instant::now() < install_deadline,
                "managed SSH bootstrap did not install the provider frame lifecycle helper"
            );
            thread::sleep(Duration::from_millis(10));
        }
        fs::write(&lifecycle_script, PROVIDER_SEQUENCE_GAP_PEER)
            .expect("install protocol peer that emits provider state then a skipped sequence");

        let (sender, receiver) = std::sync::mpsc::sync_channel(16);
        let stream = Stream::start(Arc::clone(&connection), nonce, target, 1, sender)
            .expect("start managed provider lifecycle stream through loopback SSH");
        let binding = wait_stream_connected(&receiver);
        let first = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("valid provider_state frame should reach the stream receiver");
        match first {
            Message::Frame {
                binding: frame_binding,
                frame:
                    zmux_remote_runtime::agent_lifecycle::Frame::ProviderState {
                        sequence,
                        observation,
                        ..
                    },
            } => {
                assert_eq!(frame_binding, binding);
                assert_eq!(sequence, 1);
                assert_eq!(observation.provider, ProviderKind::Codex);
                assert_eq!(observation.session_id, "thread_transport_7");
                assert_eq!(observation.turn_id.as_deref(), Some("turn_transport_8"));
            }
            _ => panic!("expected the sanitized provider_state frame"),
        }

        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(Message::Lost {
                binding: lost_binding,
            }) => assert_eq!(lost_binding, binding),
            Ok(_) => panic!("expected skipped provider sequence to lose stream"),
            Err(error) => panic!("skipped provider sequence did not lose stream: {error}"),
        }

        drop(stream);
        drop(session);
        drop(fixture);
    }

    struct ReceiptPump {
        stop: Arc<AtomicBool>,
        accepted: Arc<AtomicUsize>,
        rejected: Arc<AtomicUsize>,
        no_progress_since: Arc<Mutex<Option<Instant>>>,
        worker: Option<thread::JoinHandle<()>>,
    }

    #[derive(Clone, Copy, Debug)]
    struct ReceiptPumpSnapshot {
        accepted: usize,
        rejected: usize,
        no_progress_since: Option<Instant>,
    }

    impl ReceiptPump {
        fn start(stream: Arc<Stream>, binding: Binding) -> Self {
            let stop = Arc::new(AtomicBool::new(false));
            let accepted = Arc::new(AtomicUsize::new(0));
            let rejected = Arc::new(AtomicUsize::new(0));
            let no_progress_since = Arc::new(Mutex::new(None));
            let worker_stop = Arc::clone(&stop);
            let worker_accepted = Arc::clone(&accepted);
            let worker_rejected = Arc::clone(&rejected);
            let worker_no_progress_since = Arc::clone(&no_progress_since);
            let worker = thread::spawn(move || {
                let mut event_id = 1_u128;
                let mut rejection_run_started = None;
                while !worker_stop.load(Ordering::Acquire) {
                    if stream.send_receipt(
                        binding,
                        Uuid::from_u128(event_id),
                        HookDisposition::NotHandled,
                    ) {
                        worker_accepted.fetch_add(1, Ordering::Relaxed);
                        if rejection_run_started.take().is_some() {
                            *worker_no_progress_since
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner()) = None;
                        }
                        event_id = event_id.wrapping_add(1).max(1);
                    } else {
                        worker_rejected.fetch_add(1, Ordering::Relaxed);
                        if rejection_run_started.is_none() {
                            let started = Instant::now();
                            rejection_run_started = Some(started);
                            *worker_no_progress_since
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner()) = Some(started);
                        }
                        thread::yield_now();
                    }
                }
            });
            Self {
                stop,
                accepted,
                rejected,
                no_progress_since,
                worker: Some(worker),
            }
        }

        fn stats(&self) -> (usize, usize) {
            (
                self.accepted.load(Ordering::Acquire),
                self.rejected.load(Ordering::Acquire),
            )
        }

        fn no_progress_since(&self) -> Option<Instant> {
            *self
                .no_progress_since
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
        }

        fn snapshot_after_stop(&self) -> ReceiptPumpSnapshot {
            assert!(
                self.worker.is_none(),
                "snapshot managed lifecycle receipt pump only after joining it"
            );
            ReceiptPumpSnapshot {
                accepted: self.accepted.load(Ordering::Acquire),
                rejected: self.rejected.load(Ordering::Acquire),
                no_progress_since: self.no_progress_since(),
            }
        }

        fn stop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(worker) = self.worker.take() {
                worker.join().expect("join managed lifecycle receipt pump");
            }
        }
    }

    impl Drop for ReceiptPump {
        fn drop(&mut self) {
            self.stop();
        }
    }

    // The stalled peer emits a Heartbeat every 100 ms. Pressure requires a
    // 400 ms producer-side rejection epoch plus a stable joined snapshot.
    const MIN_SUSTAINED_BACKPRESSURE: Duration = Duration::from_millis(400);

    struct HeartbeatBackpressure {
        last_sequence: u64,
        last_accepted: usize,
        last_rejected: usize,
        last_observed_rejection_since: Option<Instant>,
        sustained_since: Option<Instant>,
        sustained_rejection_since: Option<Instant>,
    }

    impl HeartbeatBackpressure {
        fn new(pump: &ReceiptPump) -> Self {
            let (last_accepted, last_rejected) = pump.stats();
            Self {
                last_sequence: 0,
                last_accepted,
                last_rejected,
                last_observed_rejection_since: None,
                sustained_since: None,
                sustained_rejection_since: None,
            }
        }

        fn observe(&mut self, sequence: u64, pump: &ReceiptPump) -> (usize, usize) {
            assert!(
                sequence > self.last_sequence,
                "stalled peer Heartbeat sequence did not advance: previous={}, current={sequence}",
                self.last_sequence
            );
            self.last_sequence = sequence;

            let (accepted, rejected) = pump.stats();
            let observed_at = Instant::now();
            let no_progress_since = pump.no_progress_since();
            self.last_observed_rejection_since = if accepted == self.last_accepted {
                no_progress_since
            } else {
                None
            };
            if accepted == self.last_accepted
                && rejected > self.last_rejected
                && no_progress_since.is_some_and(|started| {
                    observed_at.saturating_duration_since(started) >= MIN_SUSTAINED_BACKPRESSURE
                })
            {
                self.sustained_since.get_or_insert(observed_at);
                self.sustained_rejection_since = no_progress_since;
            } else {
                self.sustained_since = None;
                self.sustained_rejection_since = None;
            }
            self.last_accepted = accepted;
            self.last_rejected = rejected;
            (accepted, rejected)
        }

        fn confirmed_pressure_at(
            &self,
            snapshot: ReceiptPumpSnapshot,
            observed_at: Instant,
        ) -> Option<Instant> {
            let rejection_started = self.last_observed_rejection_since?;
            let pressure_confirmed_at = rejection_started + MIN_SUSTAINED_BACKPRESSURE;
            (self.last_sequence > 0
                && snapshot.accepted == self.last_accepted
                && snapshot.rejected > self.last_rejected
                && snapshot.no_progress_since == Some(rejection_started)
                && observed_at >= pressure_confirmed_at)
                .then_some(pressure_confirmed_at)
        }
    }

    #[test]
    fn real_managed_ssh_stalled_stdin_has_bounded_loss_and_cancellation() {
        if !private_ssh_prerequisites_available() {
            eprintln!(
                "skipping remote lifecycle stalled-stdin test: private SSH prerequisites unavailable"
            );
            return;
        }

        let fixture = SshFixture::start();
        let connection = fixture.connection();
        let nonce = OpenRequestNonce::generate();
        let registration = OpenRequestRegistration::new(nonce, OpenRequestSource::ManagedSsh);
        let terminal = connection.terminal_spec(&nonce);
        let session = Session::spawn_with_open_requests(
            Launch {
                program: terminal.program,
                args: terminal.args,
                cwd: None,
                size: zmux_session_runtime::default_size(),
            },
            &[],
            &[],
            ShutdownPolicy::Kill,
            None,
            Some(registration),
            Arc::clone(&connection),
        )
        .expect("spawn exact managed SSH terminal for stalled lifecycle peer");

        let bootstrap_started = fixture.remote_root.join(".bootstrap-started");
        let bootstrap_deadline = Instant::now() + Duration::from_secs(10);
        while !bootstrap_started.exists() {
            let snapshot = session_snapshot_before(
                &session,
                bootstrap_deadline,
                "inspect managed SSH bootstrap",
            );
            assert_ne!(
                snapshot.state,
                zmux_session_runtime::State::Failed,
                "managed SSH bootstrap failed before stalled peer setup: {:?}",
                snapshot.failure
            );
            assert!(
                Instant::now() < bootstrap_deadline,
                "managed SSH terminal did not enter its bootstrap gate"
            );
            thread::sleep(Duration::from_millis(10));
        }
        fs::write(fixture.remote_root.join(".release-bootstrap"), b"go")
            .expect("release the managed SSH shell bootstrap");

        let integration_root = PathBuf::from(nonce.remote_integration_directory());
        let lifecycle_script = integration_root.join("claude-lifecycle-stream.py");
        let expected_script = format!("{INSTALLED_LIFECYCLE_SCRIPT}\n");
        let install_deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if fs::read_to_string(&lifecycle_script).ok().as_deref()
                == Some(expected_script.as_str())
            {
                break;
            }
            let snapshot = session_snapshot_before(
                &session,
                install_deadline,
                "inspect lifecycle helper install",
            );
            assert_ne!(
                snapshot.state,
                zmux_session_runtime::State::Failed,
                "managed SSH bootstrap failed before lifecycle helper install: {:?}",
                snapshot.failure
            );
            assert!(
                Instant::now() < install_deadline,
                "managed SSH bootstrap did not install its lifecycle helper"
            );
            thread::sleep(Duration::from_millis(10));
        }
        fs::write(&lifecycle_script, STALLED_LIFECYCLE_PEER)
            .expect("install protocol-valid peer that stalls on the real child stdin pipe");

        let (sender, receiver) = std::sync::mpsc::sync_channel(16);
        let stream = Stream::start(
            Arc::clone(&connection),
            nonce,
            Target {
                identity_namespace: Uuid::new_v4(),
                surface_id: SurfaceId(41),
                surface_uuid: Uuid::new_v4(),
                session_id: SessionId(53),
            },
            1,
            sender.clone(),
        )
        .expect("start managed lifecycle stream through loopback SSH");
        drop(sender);
        let binding = wait_stream_connected(&receiver);
        let stream = Arc::new(stream);
        let mut pump = ReceiptPump::start(Arc::clone(&stream), binding);
        let stalled_started = Instant::now();
        let loss_deadline = stalled_started + HANDSHAKE_TIMEOUT;
        let mut pressure = HeartbeatBackpressure::new(&pump);
        let (lost, lost_at, loss_snapshot, sustained_pressure_at) = loop {
            let remaining = loss_deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "stalled SSH stdin did not report bounded loss within {HANDSHAKE_TIMEOUT:?}; accepted={}, rejected={}, last heartbeat={}",
                pump.stats().0,
                pump.stats().1,
                pressure.last_sequence
            );
            let message = receiver.recv_timeout(remaining).unwrap_or_else(|error| {
                let (accepted, rejected) = pump.stats();
                panic!(
                    "stalled SSH stdin did not report bounded loss within {HANDSHAKE_TIMEOUT:?}: {error}; accepted={accepted}, rejected={rejected}, last heartbeat={}",
                    pressure.last_sequence
                )
            });
            match message {
                Message::Frame {
                    binding: frame_binding,
                    frame: zmux_remote_runtime::agent_lifecycle::Frame::Heartbeat { sequence, .. },
                } => {
                    assert_eq!(
                        frame_binding, binding,
                        "stalled peer Heartbeat binding changed while stdin was pressured"
                    );
                    pressure.observe(sequence, &pump);
                }
                Message::Lost {
                    binding: lost_binding,
                } => {
                    let lost_at = Instant::now();
                    assert_eq!(lost_binding, binding);
                    pump.stop();
                    let snapshot = pump.snapshot_after_stop();
                    let sustained_pressure_at = pressure
                        .confirmed_pressure_at(snapshot, lost_at)
                        .unwrap_or_else(|| {
                            let rejection_age = snapshot.no_progress_since.map(|started| {
                                lost_at.saturating_duration_since(started)
                            });
                            panic!(
                                "managed SSH reported loss before the joined receipt snapshot proved sustained pressure; snapshot={snapshot:?}, observed accepted={}, rejected={}, observed rejection epoch={:?}, sustained heartbeat epoch={:?}, rejection age={rejection_age:?}, required={MIN_SUSTAINED_BACKPRESSURE:?}, last heartbeat={}",
                                pressure.last_accepted,
                                pressure.last_rejected,
                                pressure.last_observed_rejection_since,
                                pressure.sustained_rejection_since,
                                pressure.last_sequence
                            )
                        });
                    break (lost_binding, lost_at, snapshot, sustained_pressure_at);
                }
                Message::Connected { .. } => {
                    panic!("unexpected reconnect after stalled stdin")
                }
                Message::Frame { frame, .. } => {
                    panic!("unexpected lifecycle frame from stalled peer: {frame:?}")
                }
            }
        };
        pump.stop();
        let ReceiptPumpSnapshot {
            accepted, rejected, ..
        } = loss_snapshot;
        assert!(
            accepted > MAX_PENDING_RECEIPTS,
            "only {accepted} receipts were admitted before managed SSH loss"
        );
        assert!(
            rejected > 0,
            "receipt admission never observed backpressure"
        );
        assert_eq!(lost, binding);
        let loss_after_start = lost_at.saturating_duration_since(stalled_started);
        let loss_after_sustained_pressure =
            lost_at.saturating_duration_since(sustained_pressure_at);
        let scheduling_allowance = POLL_INTERVAL + POLL_INTERVAL;
        let bounded_loss_allowance =
            CLIENT_FRAME_WRITE_TIMEOUT + CHILD_REAP_SYNC_TIMEOUT + scheduling_allowance;
        eprintln!(
            "managed lifecycle stalled stdin: accepted={accepted}, rejected={rejected}, sustained_pressure_after={:?}, loss_after_start={loss_after_start:?}, loss_after_pressure={loss_after_sustained_pressure:?}, allowance={bounded_loss_allowance:?}, last_heartbeat={}",
            sustained_pressure_at.saturating_duration_since(stalled_started),
            pressure.last_sequence
        );
        assert!(
            loss_after_sustained_pressure < bounded_loss_allowance,
            "stalled SSH stdin loss exceeded the frame deadline and bounded cleanup allowance: elapsed={loss_after_sustained_pressure:?}, frame_timeout={CLIENT_FRAME_WRITE_TIMEOUT:?}, cleanup_timeout={CHILD_REAP_SYNC_TIMEOUT:?}, scheduling_allowance={:?}, accepted={accepted}, rejected={rejected}, last heartbeat={}",
            scheduling_allowance,
            pressure.last_sequence
        );
        drop(stream);
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(2)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        ));

        // Exercise cancellation while the second real SSH child is blocked on
        // the same stalled pipe. Dropping Stream is immediate, and the reader
        // thread must notice cancellation without waiting for the peer to read.
        let (sender, receiver) = std::sync::mpsc::sync_channel(16);
        let stream = Stream::start(
            Arc::clone(&connection),
            nonce,
            Target {
                identity_namespace: Uuid::new_v4(),
                surface_id: SurfaceId(42),
                surface_uuid: Uuid::new_v4(),
                session_id: SessionId(54),
            },
            2,
            sender.clone(),
        )
        .expect("start second managed lifecycle stream for cancellation");
        drop(sender);
        let binding = wait_stream_connected(&receiver);
        let stream = Arc::new(stream);
        let mut pump = ReceiptPump::start(Arc::clone(&stream), binding);
        let cancellation_deadline = Instant::now() + HANDSHAKE_TIMEOUT;
        let mut pressure = HeartbeatBackpressure::new(&pump);
        loop {
            let remaining = cancellation_deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "cancellation phase did not reach sustained receipt backpressure within {HANDSHAKE_TIMEOUT:?}; accepted={}, rejected={}, last heartbeat={}",
                pump.stats().0,
                pump.stats().1,
                pressure.last_sequence
            );
            let message = receiver.recv_timeout(remaining).unwrap_or_else(|error| {
                let (accepted, rejected) = pump.stats();
                panic!(
                    "cancellation phase did not reach sustained receipt backpressure within {HANDSHAKE_TIMEOUT:?}: {error}; accepted={accepted}, rejected={rejected}, last heartbeat={}",
                    pressure.last_sequence
                )
            });
            match message {
                Message::Frame {
                    binding: frame_binding,
                    frame: zmux_remote_runtime::agent_lifecycle::Frame::Heartbeat { sequence, .. },
                } => {
                    assert_eq!(
                        frame_binding, binding,
                        "cancellation peer Heartbeat binding changed under receipt pressure"
                    );
                    pressure.observe(sequence, &pump);
                    if pressure.sustained_since.is_some() {
                        break;
                    }
                }
                Message::Lost { .. } => {
                    panic!("managed lifecycle was lost before cancellation under backpressure")
                }
                Message::Connected { .. } => {
                    panic!("unexpected reconnect during cancellation pressure phase")
                }
                Message::Frame { frame, .. } => {
                    panic!("unexpected lifecycle frame during cancellation: {frame:?}")
                }
            }
        }
        pump.stop();
        let cancellation_snapshot = pump.snapshot_after_stop();
        assert!(
            cancellation_snapshot.accepted > MAX_PENDING_RECEIPTS,
            "only {} receipts were admitted before cancellation",
            cancellation_snapshot.accepted
        );
        assert!(
            cancellation_snapshot.accepted == pressure.last_accepted
                && cancellation_snapshot.no_progress_since == pressure.sustained_rejection_since
                && cancellation_snapshot
                    .no_progress_since
                    .is_some_and(|started| {
                        Instant::now().saturating_duration_since(started)
                            >= MIN_SUSTAINED_BACKPRESSURE
                    }),
            "cancellation producer left the sustained rejection epoch: snapshot={cancellation_snapshot:?}, observed accepted={}, pressure epoch={:?}",
            pressure.last_accepted,
            pressure.sustained_rejection_since
        );
        let ReceiptPumpSnapshot {
            accepted, rejected, ..
        } = cancellation_snapshot;
        let mut last_sequence = pressure.last_sequence;
        drop(pump);
        let drop_started = Instant::now();
        drop(stream);
        assert!(
            drop_started.elapsed() < Duration::from_millis(100),
            "dropping a stalled managed lifecycle stream blocked the caller"
        );
        let shutdown_deadline = drop_started + Duration::from_secs(2);
        let mut drained_heartbeats = 0;
        loop {
            let remaining = shutdown_deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "cancellation did not disconnect the lifecycle sender promptly; drained {drained_heartbeats} Heartbeats, accepted={accepted}, rejected={rejected}"
            );
            match receiver.recv_timeout(remaining) {
                Ok(Message::Frame {
                    binding: frame_binding,
                    frame: zmux_remote_runtime::agent_lifecycle::Frame::Heartbeat { sequence, .. },
                }) => {
                    assert_eq!(
                        frame_binding, binding,
                        "cancellation drain received a Heartbeat from a different binding"
                    );
                    assert!(
                        sequence > last_sequence,
                        "cancellation drain Heartbeat sequence did not advance: previous={last_sequence}, current={sequence}"
                    );
                    last_sequence = sequence;
                    drained_heartbeats += 1;
                }
                Ok(Message::Lost { .. }) => {
                    panic!("cancellation reported lifecycle loss instead of stopping the sender")
                }
                Ok(Message::Connected { .. }) => {
                    panic!("cancellation unexpectedly reconnected the lifecycle stream")
                }
                Ok(Message::Frame { frame, .. }) => {
                    panic!("unexpected lifecycle frame while draining cancellation: {frame:?}")
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    panic!(
                        "cancellation did not disconnect the lifecycle sender within two seconds; drained {drained_heartbeats} Heartbeats, accepted={accepted}, rejected={rejected}"
                    )
                }
            }
        }
        eprintln!(
            "managed lifecycle cancellation: accepted={accepted}, rejected={rejected}, drained_heartbeats={drained_heartbeats}, shutdown_after={:?}",
            drop_started.elapsed()
        );

        drop(session);
        drop(fixture);
    }

    #[test]
    fn retries_a_real_managed_ssh_bootstrap_race_then_reports_session_loss() {
        if !private_ssh_prerequisites_available() {
            eprintln!("skipping remote lifecycle race test: private SSH prerequisites unavailable");
            return;
        }

        let fixture = SshFixture::start();
        let connection = fixture.connection();
        let nonce = OpenRequestNonce::generate();
        let target = Target {
            identity_namespace: Uuid::new_v4(),
            surface_id: SurfaceId(17),
            surface_uuid: Uuid::new_v4(),
            session_id: SessionId(23),
        };
        let registration = OpenRequestRegistration::new(nonce, OpenRequestSource::ManagedSsh);
        let terminal = connection.terminal_spec(&nonce);
        let mut session = Session::spawn_with_open_requests(
            Launch {
                program: terminal.program,
                args: terminal.args,
                cwd: None,
                size: zmux_session_runtime::default_size(),
            },
            &[],
            &[],
            ShutdownPolicy::Kill,
            None,
            Some(registration),
            Arc::clone(&connection),
        )
        .expect("spawn exact managed SSH terminal");
        let bootstrap_started = fixture.remote_root.join(".bootstrap-started");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !bootstrap_started.exists() {
            let snapshot =
                session_snapshot_before(&session, deadline, "inspect SSH terminal session");
            assert_ne!(
                snapshot.state,
                zmux_session_runtime::State::Failed,
                "managed SSH bootstrap failed before the race was staged: {:?}",
                snapshot.failure
            );
            assert!(
                Instant::now() < deadline,
                "managed SSH terminal did not enter the gated bootstrap"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let spec = connection
            .agent_lifecycle_spec(&nonce)
            .expect("build exact managed lifecycle operation");
        assert_eq!(spec.args[5], "ControlMaster=no");
        assert_eq!(spec.args[11], "ProxyCommand=/usr/bin/false");

        let (sender, receiver) = std::sync::mpsc::sync_channel(16);
        let stream = Stream::start(Arc::clone(&connection), nonce, target, 1, sender)
            .expect("start managed lifecycle supervisor");

        let attempts = fixture.remote_root.join(".stream-attempts");
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let count = fs::read_to_string(&attempts)
                .unwrap_or_default()
                .lines()
                .count();
            if count >= 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the real managed SSH stream did not retry before bootstrap readiness"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
        fs::write(fixture.remote_root.join(".release-bootstrap"), b"go")
            .expect("release the remote bootstrap after pre-Hello retries");

        let connected = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("stream should complete its nonce handshake after bootstrap readiness");
        let binding = match connected {
            Message::Connected { binding, .. } => binding,
            _ => panic!("expected Connected after validated Hello/Active"),
        };
        assert_eq!(binding.target, target);
        assert_eq!(
            binding.nonce,
            nonce.remote_lifecycle_token().parse::<Uuid>().unwrap()
        );
        // Heartbeats and Hooks share one transport sequence, so validate strict
        // ordering across every received frame instead of requiring adjacency.
        let mut last_stream_sequence = 0;

        // A Zmux-generated hook ID correlates this exact invocation; no-prompt
        // attention is rejected and gets a silent generic terminal fallback.
        let provider_session = Uuid::new_v4();
        let prompt_id = Uuid::new_v4();
        let mut lifecycle = crate::agent_lifecycle::Runtime::default();
        let (child, hook_binding, sequence, hook_id, receipt_requested, no_prompt) =
            push_remote_hook(
                &connection,
                nonce,
                serde_json::json!({
                    "hook_event_name":"Notification",
                    "session_id":provider_session,
                    "notification_type":"permission_prompt"
                }),
                &receiver,
                &mut last_stream_sequence,
            );
        assert_eq!(hook_binding, binding);
        assert!(receipt_requested);
        assert!(
            lifecycle
                .apply_remote_hook(
                    target,
                    binding.runtime_id,
                    binding.generation,
                    crate::agent_lifecycle::RemoteHookInvocation { sequence, hook_id },
                    no_prompt,
                    Instant::now(),
                )
                .is_err()
        );
        assert!(stream.send_receipt(
            binding,
            hook_id,
            crate::agent_lifecycle::HookDisposition::NotHandled,
        ));
        wait_remote_hook(child);
        let attention = wait_for_remote_notice(&session, "osc777-inbox", "Claude needs attention");
        assert_eq!(attention.title, "Claude");

        let (child, hook_binding, sequence, hook_id, receipt_requested, untracked_stop) =
            push_remote_hook(
                &connection,
                nonce,
                serde_json::json!({
                    "hook_event_name":"Stop",
                    "session_id":provider_session
                }),
                &receiver,
                &mut last_stream_sequence,
            );
        assert_eq!(hook_binding, binding);
        assert!(receipt_requested);
        assert!(
            lifecycle
                .apply_remote_hook(
                    target,
                    binding.runtime_id,
                    binding.generation,
                    crate::agent_lifecycle::RemoteHookInvocation { sequence, hook_id },
                    untracked_stop,
                    Instant::now(),
                )
                .is_err()
        );
        assert!(stream.send_receipt(
            binding,
            hook_id,
            crate::agent_lifecycle::HookDisposition::NotHandled,
        ));
        wait_remote_hook(child);
        let stopped = wait_for_remote_notice(&session, "osc777-inbox", "Claude finished its turn");
        assert_eq!(stopped.title, "Claude");
        let fallback_count = current_notices(&session).len();

        let (child, hook_binding, sequence, hook_id, receipt_requested, start) = push_remote_hook(
            &connection,
            nonce,
            serde_json::json!({
                "hook_event_name":"SessionStart",
                "session_id":provider_session,
                "source":"startup"
            }),
            &receiver,
            &mut last_stream_sequence,
        );
        assert_eq!(hook_binding, binding);
        assert!(!receipt_requested);
        assert_eq!(
            lifecycle
                .apply_remote_hook(
                    target,
                    binding.runtime_id,
                    binding.generation,
                    crate::agent_lifecycle::RemoteHookInvocation { sequence, hook_id },
                    start,
                    Instant::now(),
                )
                .unwrap(),
            crate::agent_lifecycle::HookDisposition::NotHandled
        );
        wait_remote_hook(child);

        let (child, hook_binding, sequence, hook_id, receipt_requested, prompt) = push_remote_hook(
            &connection,
            nonce,
            serde_json::json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":provider_session,
                "prompt_id":prompt_id,
                "permission_mode":"default"
            }),
            &receiver,
            &mut last_stream_sequence,
        );
        assert_eq!(hook_binding, binding);
        assert!(!receipt_requested);
        assert_eq!(
            lifecycle
                .apply_remote_hook(
                    target,
                    binding.runtime_id,
                    binding.generation,
                    crate::agent_lifecycle::RemoteHookInvocation { sequence, hook_id },
                    prompt,
                    Instant::now(),
                )
                .unwrap(),
            crate::agent_lifecycle::HookDisposition::NotHandled
        );
        wait_remote_hook(child);

        let (child, hook_binding, sequence, hook_id, receipt_requested, permission) =
            push_remote_hook(
                &connection,
                nonce,
                serde_json::json!({
                    "hook_event_name":"Notification",
                    "session_id":provider_session,
                    "notification_type":"permission_prompt"
                }),
                &receiver,
                &mut last_stream_sequence,
            );
        assert_eq!(hook_binding, binding);
        assert!(receipt_requested);
        let disposition = lifecycle
            .apply_remote_hook(
                target,
                binding.runtime_id,
                binding.generation,
                crate::agent_lifecycle::RemoteHookInvocation { sequence, hook_id },
                permission,
                Instant::now(),
            )
            .unwrap();
        assert_eq!(
            disposition,
            crate::agent_lifecycle::HookDisposition::AlertCovered
        );
        assert_eq!(lifecycle.alert_cursor(), 1);
        let mut stale_binding = binding;
        stale_binding.generation = Uuid::new_v4();
        assert!(!stream.send_receipt(
            stale_binding,
            hook_id,
            crate::agent_lifecycle::HookDisposition::AlertCovered,
        ));
        // Simulate a lost exact receipt after the owner already admitted the
        // alert. The hook's bounded wait expires and preserves an at-least-once
        // visual fallback; a later receipt cannot retract it.
        wait_remote_hook(child);
        let lost_receipt_deadline = Instant::now() + Duration::from_secs(3);
        let lost_receipt_notice_count = loop {
            let notices = current_notices(&session);
            if notices.len() > fallback_count {
                break notices.len();
            }
            assert!(
                Instant::now() < lost_receipt_deadline,
                "lost receipt fallback did not reach the terminal: {notices:?}"
            );
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(lost_receipt_notice_count, fallback_count + 1);
        assert!(stream.send_receipt(binding, hook_id, disposition));
        assert_eq!(current_notices(&session).len(), lost_receipt_notice_count);

        let (child, hook_binding, sequence, hook_id, receipt_requested, repeated) =
            push_remote_hook(
                &connection,
                nonce,
                serde_json::json!({
                    "hook_event_name":"Notification",
                    "session_id":provider_session,
                    "notification_type":"permission_prompt"
                }),
                &receiver,
                &mut last_stream_sequence,
            );
        assert_eq!(hook_binding, binding);
        assert!(receipt_requested);
        let disposition = lifecycle
            .apply_remote_hook(
                target,
                binding.runtime_id,
                binding.generation,
                crate::agent_lifecycle::RemoteHookInvocation { sequence, hook_id },
                repeated,
                Instant::now(),
            )
            .unwrap();
        assert_eq!(
            disposition,
            crate::agent_lifecycle::HookDisposition::AlertCovered
        );
        assert_eq!(lifecycle.alert_cursor(), 1);
        assert!(stream.send_receipt(binding, hook_id, disposition));
        wait_remote_hook(child);
        assert_eq!(current_notices(&session).len(), lost_receipt_notice_count);

        let (child, hook_binding, sequence, hook_id, receipt_requested, stop) = push_remote_hook(
            &connection,
            nonce,
            serde_json::json!({
                "hook_event_name":"Stop",
                "session_id":provider_session
            }),
            &receiver,
            &mut last_stream_sequence,
        );
        assert_eq!(hook_binding, binding);
        assert!(receipt_requested);
        let disposition = lifecycle
            .apply_remote_hook(
                target,
                binding.runtime_id,
                binding.generation,
                crate::agent_lifecycle::RemoteHookInvocation { sequence, hook_id },
                stop,
                Instant::now(),
            )
            .unwrap();
        assert_eq!(
            disposition,
            crate::agent_lifecycle::HookDisposition::HandledNoAlert
        );
        // The lost Stop receipt can produce a silent visual fallback. The later
        // idle_prompt still admits the one owner completion alert, so the two
        // visual paths may both render at-least-once without an extra Stop sound.
        assert_eq!(lifecycle.alert_cursor(), 1);
        wait_remote_hook(child);
        let stop_fallback_deadline = Instant::now() + Duration::from_secs(5);
        let stop_fallback_notice_count = loop {
            let notices = current_notices(&session);
            if notices.len() > lost_receipt_notice_count {
                assert!(notices.iter().any(|notice| {
                    notice.source == "osc777-inbox" && notice.body == "Claude finished its turn"
                }));
                break notices.len();
            }
            assert!(
                Instant::now() < stop_fallback_deadline,
                "lost Stop receipt did not produce its silent visual fallback: {notices:?}"
            );
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(stop_fallback_notice_count, lost_receipt_notice_count + 1);

        // Hold the same live stream through one real heartbeat before the next
        // Hook. This covers the shared heartbeat/Hook sequence interleaving
        // instead of assuming consecutive Hook frame numbers.
        wait_for_remote_heartbeat(&receiver, &mut last_stream_sequence);
        let (child, hook_binding, sequence, hook_id, receipt_requested, idle) = push_remote_hook(
            &connection,
            nonce,
            serde_json::json!({
                "hook_event_name":"Notification",
                "session_id":provider_session,
                "notification_type":"idle_prompt"
            }),
            &receiver,
            &mut last_stream_sequence,
        );
        assert_eq!(hook_binding, binding);
        assert!(!receipt_requested);
        assert_eq!(
            lifecycle
                .apply_remote_hook(
                    target,
                    binding.runtime_id,
                    binding.generation,
                    crate::agent_lifecycle::RemoteHookInvocation { sequence, hook_id },
                    idle,
                    Instant::now(),
                )
                .unwrap(),
            crate::agent_lifecycle::HookDisposition::AlertCovered
        );
        wait_remote_hook(child);
        assert_eq!(lifecycle.alert_cursor(), 2);
        assert_eq!(current_notices(&session).len(), stop_fallback_notice_count);

        let (child, hook_binding, sequence, hook_id, receipt_requested, late_stop) =
            push_remote_hook(
                &connection,
                nonce,
                serde_json::json!({
                    "hook_event_name":"Stop",
                    "session_id":provider_session
                }),
                &receiver,
                &mut last_stream_sequence,
            );
        assert_eq!(hook_binding, binding);
        assert!(receipt_requested);
        let disposition = lifecycle
            .apply_remote_hook(
                target,
                binding.runtime_id,
                binding.generation,
                crate::agent_lifecycle::RemoteHookInvocation { sequence, hook_id },
                late_stop,
                Instant::now(),
            )
            .unwrap();
        assert_eq!(
            disposition,
            crate::agent_lifecycle::HookDisposition::HandledNoAlert
        );
        assert!(stream.send_receipt(binding, hook_id, disposition));
        wait_remote_hook(child);
        assert_eq!(lifecycle.alert_cursor(), 2);
        assert_eq!(current_notices(&session).len(), stop_fallback_notice_count);

        let (child, hook_binding, sequence, hook_id, receipt_requested, continued_stop) =
            push_remote_hook(
                &connection,
                nonce,
                serde_json::json!({
                    "hook_event_name":"Stop",
                    "session_id":provider_session,
                    "stop_hook_active":true
                }),
                &receiver,
                &mut last_stream_sequence,
            );
        assert_eq!(hook_binding, binding);
        assert!(receipt_requested);
        let disposition = lifecycle
            .apply_remote_hook(
                target,
                binding.runtime_id,
                binding.generation,
                crate::agent_lifecycle::RemoteHookInvocation { sequence, hook_id },
                continued_stop,
                Instant::now(),
            )
            .unwrap();
        assert_eq!(
            disposition,
            crate::agent_lifecycle::HookDisposition::HandledNoAlert
        );
        assert!(stream.send_receipt(binding, hook_id, disposition));
        wait_remote_hook(child);
        assert_eq!(lifecycle.alert_cursor(), 2);
        assert_eq!(current_notices(&session).len(), stop_fallback_notice_count);

        session
            .shutdown(Duration::from_secs(5))
            .expect("tear down exact SSH terminal session");
        loop {
            match receiver.recv_timeout(Duration::from_secs(10)) {
                Ok(Message::Lost { binding: lost }) => {
                    assert_eq!(lost, binding);
                    break;
                }
                Ok(Message::Frame { .. }) => {}
                Ok(Message::Connected { .. }) => panic!("unexpected reconnect after session loss"),
                Err(error) => panic!("managed lifecycle did not publish explicit loss: {error}"),
            }
        }

        drop(stream);
        drop(session);
        drop(connection);
        zmux_remote_runtime::connection::request_cleanup_barrier()
            .expect("queue SSH cleanup barrier")
            .recv_timeout(Duration::from_secs(3))
            .expect("SSH cleanup barrier response");
    }
}
