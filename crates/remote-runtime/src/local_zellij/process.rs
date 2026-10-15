use super::{LIMIT, check_deadline, fail};
use filedescriptor::FileDescriptor;
use nix::{
    sys::{
        signal::{Signal, killpg},
        termios::{self, SetArg, Termios},
    },
    unistd::{Pid, tcgetpgrp, tcsetpgrp},
};
use signal_hook::{consts::signal::*, iterator::Signals};
use std::{
    io::{self, IsTerminal, Read},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use zmux_session_runtime::child::{ExitObservation, observe_child};

pub(super) struct OwnedChild {
    pub child: Child,
    reaped: bool,
}
impl OwnedChild {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        Ok(Self {
            child: command.process_group(0).spawn()?,
            reaped: false,
        })
    }
    pub fn observe(&self, job_control: bool) -> io::Result<Option<ExitObservation>> {
        observe_child(self.child.id(), job_control)
    }
    pub fn signal(&self, signal: Signal) -> io::Result<()> {
        if self.reaped {
            return Err(fail("cannot signal a reaped child"));
        }
        if self.observe(false)?.is_none() {
            killpg(Pid::from_raw(self.child.id() as i32), signal)?;
        }
        Ok(())
    }
    pub fn reap(&mut self) -> io::Result<ExitObservation> {
        let result = self
            .observe(false)?
            .ok_or_else(|| fail("child has not independently exited"))?;
        self.child.wait()?;
        self.reaped = true;
        Ok(result)
    }
    pub fn cancel(&mut self) -> io::Result<()> {
        self.signal(Signal::SIGTERM)?;
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.observe(false)?.is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if self.observe(false)?.is_none() {
            self.signal(Signal::SIGKILL)?;
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.observe(false)?.is_none() {
            check_deadline(deadline, "owned child KILL")?;
            thread::sleep(Duration::from_millis(10));
        }
        self.reap()?;
        Ok(())
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.reaped
            && let Err(error) = self.cancel()
        {
            eprintln!("zmux owned client cleanup: {error}");
        }
    }
}

pub(super) fn capture(
    command: &mut Command,
    deadline: Instant,
    mut check_cancel: impl FnMut() -> io::Result<()>,
) -> io::Result<String> {
    check_cancel()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = OwnedChild::spawn(command)?;
    let mut pipes = [
        FileDescriptor::dup(
            &child
                .child
                .stdout
                .take()
                .ok_or_else(|| fail("missing stdout"))?,
        )
        .map_err(io::Error::other)?,
        FileDescriptor::dup(
            &child
                .child
                .stderr
                .take()
                .ok_or_else(|| fail("missing stderr"))?,
        )
        .map_err(io::Error::other)?,
    ];
    for pipe in &mut pipes {
        pipe.set_non_blocking(true).map_err(io::Error::other)?;
    }
    let mut output = [Vec::new(), Vec::new()];
    let mut eof = [false; 2];
    while !eof.iter().all(|eof| *eof) || child.observe(false)?.is_none() {
        check_cancel()?;
        check_deadline(deadline, "bounded launcher")?;
        let mut progress = false;
        for index in 0..2 {
            if eof[index] {
                continue;
            }
            let mut bytes = [0; 4096];
            match pipes[index].read(&mut bytes) {
                Ok(0) => eof[index] = true,
                Ok(count) => {
                    progress = true;
                    if output[index].len() + count > LIMIT {
                        return Err(fail("launcher output exceeds64KiB"));
                    }
                    output[index].extend_from_slice(&bytes[..count]);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        if !progress {
            thread::sleep(Duration::from_millis(10));
        }
    }
    if child.reap()? != ExitObservation::Exited(0) {
        return Err(fail(format!(
            "launcher failed: {}",
            String::from_utf8_lossy(&output[1])
        )));
    }
    String::from_utf8(std::mem::take(&mut output[0])).map_err(io::Error::other)
}

/// Terminal state is local to the foreground CLI process, never an app-wide hook.
pub struct ForegroundTerminal {
    settings: Termios,
    group: Pid,
    signals: Signals,
    restored: bool,
}
impl ForegroundTerminal {
    pub fn acquire() -> io::Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(fail("headless zellij requires terminal stdin and stdout"));
        }
        let stdin = io::stdin();
        let group = tcgetpgrp(&stdin)?;
        if group != nix::unistd::getpgrp() {
            return Err(fail(
                "headless zellij must start in its foreground terminal group",
            ));
        }
        Ok(Self {
            settings: termios::tcgetattr(&stdin)?,
            group,
            signals: Signals::new([SIGINT, SIGTERM, SIGHUP, SIGTSTP, SIGCONT, SIGWINCH, SIGCHLD])?,
            restored: false,
        })
    }
    fn tty(&self, child: Option<u32>) -> io::Result<()> {
        use nix::sys::signal::{SigSet, SigmaskHow, pthread_sigmask};
        let mut block = SigSet::empty();
        block.add(Signal::SIGTTOU);
        let mut previous = SigSet::empty();
        pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&block), Some(&mut previous))?;
        let result = (|| {
            tcsetpgrp(
                io::stdin(),
                child
                    .map(|pid| Pid::from_raw(pid as i32))
                    .unwrap_or(self.group),
            )?;
            if child.is_none() {
                termios::tcsetattr(io::stdin(), SetArg::TCSANOW, &self.settings)?;
            }
            Ok(())
        })();
        let restored = pthread_sigmask(SigmaskHow::SIG_SETMASK, Some(&previous), None);
        result.and(restored.map_err(io::Error::from))
    }
    pub(super) fn give_to(&self, child: &OwnedChild) -> io::Result<()> {
        self.tty(Some(child.child.id()))
    }
    /// Before a workload is authorized, cancellation aborts startup. After spawn,
    /// the caller preserves the reservation rather than replaying or killing it.
    pub(super) fn startup_events(&mut self) -> io::Result<()> {
        for number in self.signals.pending() {
            match number {
                SIGINT | SIGTERM | SIGHUP => {
                    return Err(fail(format!(
                        "foreground startup canceled by signal{number}; creation must not be replayed"
                    )));
                }
                SIGTSTP => {
                    self.tty(None)?;
                    nix::sys::signal::kill(nix::unistd::getpid(), Signal::SIGSTOP)?;
                }
                _ => (),
            }
        }
        Ok(())
    }
    pub(super) fn events(
        &mut self,
        child: &OwnedChild,
        mut before_cancel: impl FnMut(),
    ) -> io::Result<bool> {
        let mut cancel = false;
        for number in self.signals.pending() {
            match number {
                SIGINT | SIGTERM | SIGHUP => {
                    before_cancel();
                    child.signal(Signal::try_from(number)?)?;
                    cancel = true;
                }
                SIGWINCH => child.signal(Signal::SIGWINCH)?,
                SIGCONT => {
                    self.tty(Some(child.child.id()))?;
                    child.signal(Signal::SIGCONT)?;
                }
                SIGTSTP => {
                    child.signal(Signal::SIGTSTP)?;
                    self.tty(None)?;
                    nix::sys::signal::kill(nix::unistd::getpid(), Signal::SIGSTOP)?;
                    self.tty(Some(child.child.id()))?;
                    child.signal(Signal::SIGCONT)?;
                }
                _ => (),
            }
        }
        if matches!(child.observe(true)?, Some(ExitObservation::Stopped(_))) {
            self.tty(None)?;
            nix::sys::signal::kill(nix::unistd::getpid(), Signal::SIGSTOP)?;
            self.tty(Some(child.child.id()))?;
            child.signal(Signal::SIGCONT)?;
        }
        Ok(cancel)
    }
    pub fn restore(&mut self) -> io::Result<()> {
        if !self.restored {
            self.tty(None)?;
            self.restored = true;
        }
        Ok(())
    }
}
impl Drop for ForegroundTerminal {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("zmux terminal restoration: {error}");
        }
        self.signals.handle().close();
    }
}
