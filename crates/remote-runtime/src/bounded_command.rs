//! Bounded process I/O shared by local multiplexer and SSH configuration workers.
//! Callers provide piped output, null stdin and a private process group.
use filedescriptor::{AsRawFileDescriptor, FileDescriptor, POLLIN, poll, pollfd};
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use std::{
    io::{self, Read},
    process::{Child, Command},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use thiserror::Error;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(5);
#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("worker command timed out")]
    Timeout,
    #[error("{0}")]
    Failure(String),
}
fn failure(context: &str, error: impl std::fmt::Display) -> Error {
    Error::Failure(format!("{context}: {error}"))
}
pub(crate) struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}
struct Running {
    child: Child,
}
impl Drop for Running {
    fn drop(&mut self) {
        // A daemon that creates its own session is independent. Every remaining process
        // in the launcher's group belongs to this bounded invocation, even after
        // the leader exits or descendants close their inherited output pipes.
        if let Err(e) = killpg(Pid::from_raw(self.child.id() as i32), Signal::SIGKILL)
            && e != nix::errno::Errno::ESRCH
        {
            eprintln!("zmux worker command process cancellation: {e}");
        }
        if let Err(e) = self.child.wait() {
            eprintln!("zmux worker command process reap: {e}");
        }
    }
}
pub(crate) fn run(
    mut command: Command,
    cancellation: Option<&AtomicBool>,
) -> Result<Output, Error> {
    let mut child = Running {
        child: command
            .spawn()
            .map_err(|e| failure("launch worker command", e))?,
    };
    let mut stdout = FileDescriptor::dup(&child.child.stdout.take().expect("piped stdout"))
        .map_err(|e| failure("open worker command output", e))?;
    let mut stderr = FileDescriptor::dup(&child.child.stderr.take().expect("piped stderr"))
        .map_err(|e| failure("open worker command errors", e))?;
    stdout
        .set_non_blocking(true)
        .map_err(|e| failure("configure worker command output", e))?;
    stderr
        .set_non_blocking(true)
        .map_err(|e| failure("configure worker command errors", e))?;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut out_eof = false;
    let mut err_eof = false;
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    let mut status = None;
    loop {
        if cancellation.is_some_and(|c| c.load(Ordering::Acquire)) {
            return Err(Error::Failure(
                "worker command cancelled; inspect state if creation was pending".into(),
            ));
        }
        for (pipe, bytes, eof) in [
            (&mut stdout, &mut out, &mut out_eof),
            (&mut stderr, &mut err, &mut err_eof),
        ] {
            if *eof {
                continue;
            }
            for _ in 0..4 {
                let mut buffer = [0; 4096];
                match pipe.read(&mut buffer) {
                    Ok(0) => {
                        *eof = true;
                        break;
                    }
                    Ok(n) => {
                        if bytes.len() + n > MAX_OUTPUT_BYTES {
                            return Err(Error::Failure(
                                "worker command response exceeded the 64 KiB stream bound".into(),
                            ));
                        }
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    Err(e)
                        if e.kind() == io::ErrorKind::WouldBlock
                            || e.kind() == io::ErrorKind::Interrupted =>
                    {
                        break;
                    }
                    Err(e) => return Err(failure("read worker command response", e)),
                }
            }
        }
        if status.is_none() {
            status = child
                .child
                .try_wait()
                .map_err(|e| failure("poll worker command process", e))?;
        }
        if let Some(status) = status
            && out_eof
            && err_eof
        {
            return Ok(Output {
                success: status.success(),
                stdout: String::from_utf8(out)
                    .map_err(|e| failure("decode worker command output", e))?,
                stderr: String::from_utf8(err)
                    .map_err(|e| failure("decode worker command errors", e))?,
            });
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        let mut fds = Vec::new();
        if !out_eof {
            fds.push(pollfd {
                fd: stdout.as_raw_file_descriptor(),
                events: POLLIN,
                revents: 0,
            });
        }
        if !err_eof {
            fds.push(pollfd {
                fd: stderr.as_raw_file_descriptor(),
                events: POLLIN,
                revents: 0,
            });
        }
        if let Err(e) = poll(&mut fds, Some(Duration::from_millis(10)))
            && !matches!(&e,filedescriptor::Error::Poll(e) if e.kind()==io::ErrorKind::Interrupted)
        {
            return Err(failure("poll worker command output", e));
        }
    }
}
