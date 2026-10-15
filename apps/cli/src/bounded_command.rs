//! Finite external lookups using the existing nonreaping physical-child owner.
//! Zellij's private launcher also owns foreground job control, so it cannot be
//! used directly by noninteractive Git/GitHub lookups.
use super::*;
use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    sys::signal::{Signal, kill, killpg},
    unistd::Pid,
};
use std::{
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use zmux_session_runtime::child::{ExitObservation, observe_child};

const STREAM_BYTES: usize = 65_536;
pub(super) struct Output {
    pub status: ExitObservation,
    pub stdout: String,
    /// Bounded in-memory classification only. Never emit tool diagnostics: they
    /// can contain authentication data or private URLs.
    pub stderr: String,
}
pub(super) struct Cancellation {
    flag: Arc<AtomicBool>,
    signals: Vec<signal_hook::SigId>,
}
impl Cancellation {
    pub fn new() -> io::Result<Self> {
        let mut value = Self {
            flag: Arc::new(AtomicBool::new(false)),
            signals: Vec::new(),
        };
        for number in [
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGHUP,
        ] {
            value.signals.push(signal_hook::flag::register(
                number,
                Arc::clone(&value.flag),
            )?);
        }
        Ok(value)
    }
    pub fn check(&self, deadline: Instant) -> Result<(), CliError> {
        if self.flag.load(Ordering::Acquire) {
            return Err(CliError(
                "PR lookup cancelled; metadata was not changed".into(),
            ));
        }
        if Instant::now() >= deadline {
            return Err(CliError(
                "PR lookup deadline; metadata was not changed".into(),
            ));
        }
        Ok(())
    }
}
impl Drop for Cancellation {
    fn drop(&mut self) {
        for signal in &self.signals {
            signal_hook::low_level::unregister(*signal);
        }
    }
}
struct Owned {
    child: Child,
    reaped: bool,
    cleanup_deadline: Option<Instant>,
    signal_group: bool,
}
impl Owned {
    fn finish(&mut self) -> io::Result<()> {
        if self.reaped {
            return Ok(());
        }
        // Physical cleanup has one reserve independent of the lookup deadline.
        // A Drop retry must never renew it after an explicit finish failed.
        let deadline = *self
            .cleanup_deadline
            .get_or_insert_with(|| Instant::now() + Duration::from_secs(2));
        // WNOWAIT reserves the exact leader's PID/PGID through the final signal,
        // including descendants holding pipes after their leader has exited.
        let observed = observe_child(self.child.id(), false)?;
        let signalled = if self.signal_group {
            stop_group(self.child.id(), deadline)
        } else if observed.is_none() {
            // Only the trusted /bin/ps probe uses direct-child cleanup; it never
            // creates a process group or grants authority over adopted PIDs.
            signal_result(kill(Pid::from_raw(self.child.id() as i32), Signal::SIGKILL))
        } else {
            Ok(())
        };
        while observe_child(self.child.id(), false)?.is_none() {
            if Instant::now() >= deadline {
                return Err(io::Error::other(
                    "owned lookup child did not exit after cleanup",
                ));
            }
            thread::sleep(Duration::from_millis(5));
        }
        self.child.wait()?;
        self.reaped = true;
        signalled
    }
}
fn signal_result(result: Result<(), nix::errno::Errno>) -> io::Result<()> {
    match result {
        Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
        Err(error) => Err(error.into()),
    }
}
fn stop_group(pid: u32, _deadline: Instant) -> io::Result<()> {
    match killpg(Pid::from_raw(pid as i32), Signal::SIGKILL) {
        Err(nix::errno::Errno::EPERM) if cfg!(target_os = "macos") => {
            confirm_zombie_group(pid, _deadline)
        }
        result => signal_result(result),
    }
}
fn confirm_zombie_group(pid: u32, deadline: Instant) -> io::Result<()> {
    // Darwin may reject signalling a group made entirely of zombies. The exact
    // leader stays unreaped throughout this public, bounded group snapshot.
    if observe_child(pid, false)?.is_none() {
        return Err(io::Error::other(
            "group signal denied while lookup leader may still execute",
        ));
    }
    let mut probe = Owned {
        child: Command::new("/bin/ps")
            .args(["-A", "-o", "pid=,pgid=,stat="])
            .env_clear()
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
        reaped: false,
        cleanup_deadline: Some(deadline),
        signal_group: false,
    };
    let mut pipe = probe
        .child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("process snapshot stdout unavailable"))?;
    nonblocking(pipe.as_raw_fd())?;
    let (mut output, mut eof) = (Vec::new(), false);
    let status = loop {
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "process group inspection exceeded cleanup deadline",
            ));
        }
        if !eof {
            read(&mut pipe, &mut output, &mut eof)?;
        }
        if let Some(status) = observe_child(probe.child.id(), false)?
            && eof
        {
            break status;
        }
        thread::sleep(Duration::from_millis(5));
    };
    probe.finish()?;
    if status != ExitObservation::Exited(0) {
        return Err(io::Error::other("public process group inspection failed"));
    }
    let text = std::str::from_utf8(&output)
        .map_err(|_| io::Error::other("process group snapshot is not UTF-8"))?;
    let mut leader = false;
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3 {
            return Err(io::Error::other("malformed public process group snapshot"));
        }
        let member: u32 = fields[0]
            .parse()
            .map_err(|_| io::Error::other("invalid snapshot PID"))?;
        let group: u32 = fields[1]
            .parse()
            .map_err(|_| io::Error::other("invalid snapshot group"))?;
        if group == pid {
            if !fields[2].starts_with('Z') {
                return Err(io::Error::other(
                    "group signal denied with live lookup descendants",
                ));
            }
            leader |= member == pid;
        }
    }
    if !leader {
        return Err(io::Error::other(
            "public process snapshot omitted reserved lookup leader",
        ));
    }
    Ok(())
}
fn nonblocking(descriptor: i32) -> io::Result<()> {
    let flags = fcntl(descriptor, FcntlArg::F_GETFL)?;
    fcntl(
        descriptor,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )?;
    Ok(())
}
impl Drop for Owned {
    fn drop(&mut self) {
        if let Err(error) = self.finish() {
            eprintln!("zmux lookup cleanup failed: {error}");
        }
    }
}
fn read(pipe: &mut impl Read, output: &mut Vec<u8>, eof: &mut bool) -> io::Result<()> {
    for _ in 0..4 {
        let mut bytes = [0; 4096];
        match pipe.read(&mut bytes) {
            Ok(0) => {
                *eof = true;
                break;
            }
            Ok(count) => {
                if output.len() + count > STREAM_BYTES {
                    return Err(io::Error::other("lookup stream exceeded64KiB"));
                }
                output.extend_from_slice(&bytes[..count]);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
pub(super) fn capture(
    command: &mut Command,
    deadline: Instant,
    cancellation: &Cancellation,
) -> Result<Output, CliError> {
    cancellation.check(deadline)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut owned = Owned {
        child: command.spawn()?,
        reaped: false,
        cleanup_deadline: None,
        signal_group: true,
    };
    let mut stdout = owned
        .child
        .stdout
        .take()
        .ok_or_else(|| CliError("lookup stdout unavailable".into()))?;
    let mut stderr = owned
        .child
        .stderr
        .take()
        .ok_or_else(|| CliError("lookup stderr unavailable".into()))?;
    for descriptor in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
        nonblocking(descriptor)?;
    }
    let (mut out, mut err, mut out_eof, mut err_eof) = (Vec::new(), Vec::new(), false, false);
    let status = loop {
        cancellation.check(deadline)?;
        if !out_eof {
            read(&mut stdout, &mut out, &mut out_eof)?;
        }
        if !err_eof {
            read(&mut stderr, &mut err, &mut err_eof)?;
        }
        if let Some(status) = observe_child(owned.child.id(), false)?
            && out_eof
            && err_eof
        {
            break status;
        }
        thread::sleep(Duration::from_millis(5));
    };
    owned.finish()?;
    Ok(Output {
        status,
        stdout: String::from_utf8(out)
            .map_err(|_| CliError("lookup returned invalid UTF-8".into()))?,
        stderr: String::from_utf8(err)
            .map_err(|_| CliError("lookup diagnostics returned invalid UTF-8".into()))?,
    })
}
