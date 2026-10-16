//! Process identity and non-reaping exit observation shared by terminal owners.
//! An observation never grants signal authority over an adopted process.
use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::io::Read;
use std::{io, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub uid: u32,
    pub parent_pid: u32,
    pub start: [u64; 2],
    pub boot: String,
    pub executable: PathBuf,
}

/// One retained direct-Child lifetime across legitimate image/UID changes.
/// This does not authenticate an executable or grant process signal authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessLifetime {
    pub pid: u32,
    pub parent_pid: u32,
    pub start: [u64; 2],
    pub boot: String,
}

impl From<&ProcessIdentity> for ProcessLifetime {
    fn from(identity: &ProcessIdentity) -> Self {
        Self {
            pid: identity.pid,
            parent_pid: identity.parent_pid,
            start: identity.start,
            boot: identity.boot.clone(),
        }
    }
}

/// Double-samples only PID/parent/start/boot, without executable or UID lookup.
/// Missing or mixed samples remain errors; callers must retain their Child handle.
pub fn process_lifetime(pid: u32) -> io::Result<ProcessLifetime> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(invalid("lifetime requires a positive platform PID"));
    }
    let first = lifetime_sample(pid)?;
    let second = lifetime_sample(pid)?;
    if first != second || first.start == [0, 0] || !valid_boot(&first.boot) {
        return Err(invalid("process lifetime changed or is malformed"));
    }
    Ok(first)
}

#[cfg(target_os = "linux")]
fn lifetime_sample(pid: u32) -> io::Result<ProcessLifetime> {
    let stat = bounded_file(format!("/proc/{pid}/stat"), 16384)?;
    let end = stat
        .rfind(')')
        .ok_or_else(|| invalid("invalid process stat"))?;
    let fields: Vec<_> = stat[end + 1..].split_whitespace().collect();
    let parent_pid = fields
        .get(1)
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| invalid("missing parent identity"))?;
    let start = fields
        .get(19)
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| invalid("missing process start identity"))?;
    let boot = bounded_file("/proc/sys/kernel/random/boot_id", 128)?;
    Ok(ProcessLifetime {
        pid,
        parent_pid,
        start: [start, 0],
        boot: boot.trim().into(),
    })
}

#[cfg(target_os = "macos")]
fn lifetime_sample(pid: u32) -> io::Result<ProcessLifetime> {
    use sysctl::Sysctl;
    let info = darwin_libproc::task_all_info(pid as i32)?.pbsd;
    if info.pbi_pid != pid {
        return Err(invalid("process lifetime PID changed"));
    }
    let boot = sysctl::Ctl::new("kern.bootsessionuuid")
        .and_then(|ctl| ctl.value())
        .map_err(|e| io::Error::other(e.to_string()))?;
    let sysctl::CtlValue::String(boot) = boot else {
        return Err(invalid("boot identity has an unexpected kernel type"));
    };
    Ok(ProcessLifetime {
        pid,
        parent_pid: info.pbi_ppid,
        start: [info.pbi_start_tvsec, info.pbi_start_tvusec],
        boot: boot.to_ascii_lowercase(),
    })
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}

#[cfg(target_os = "linux")]
fn bounded_file(path: impl AsRef<std::path::Path>, limit: usize) -> io::Result<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid("process identity file exceeded its bound"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("process identity is not UTF-8"))
}

#[cfg(target_os = "linux")]
fn sample(pid: u32) -> io::Result<(u32, u32, [u64; 2], String)> {
    use std::os::unix::fs::MetadataExt;
    let root = PathBuf::from(format!("/proc/{pid}"));
    let stat = bounded_file(root.join("stat"), 16384)?;
    let end = stat
        .rfind(')')
        .ok_or_else(|| invalid("invalid process stat"))?;
    let fields: Vec<_> = stat[end + 1..].split_whitespace().collect();
    let parent = fields
        .get(1)
        .and_then(|field| field.parse().ok())
        .ok_or_else(|| invalid("missing parent identity"))?;
    let start = fields
        .get(19)
        .and_then(|field| field.parse().ok())
        .ok_or_else(|| invalid("missing process start identity"))?;
    let boot = bounded_file("/proc/sys/kernel/random/boot_id", 128)?;
    Ok((
        std::fs::metadata(root)?.uid(),
        parent,
        [start, 0],
        boot.trim().into(),
    ))
}

#[cfg(target_os = "macos")]
fn sample(pid: u32) -> io::Result<(u32, u32, [u64; 2], String)> {
    use sysctl::Sysctl;
    let info = darwin_libproc::task_all_info(pid as i32)?.pbsd;
    if info.pbi_pid != pid {
        return Err(invalid("process identity PID changed"));
    }
    let boot = sysctl::Ctl::new("kern.bootsessionuuid")
        .and_then(|ctl| ctl.value())
        .map_err(|error| io::Error::other(error.to_string()))?;
    let sysctl::CtlValue::String(boot) = boot else {
        return Err(invalid("boot identity has an unexpected kernel type"));
    };
    if boot.len() != 36 || !valid_boot(&boot) {
        return Err(invalid("boot identity is not a nonnil UUID"));
    }
    Ok((
        info.pbi_uid,
        info.pbi_ppid,
        [info.pbi_start_tvsec, info.pbi_start_tvusec],
        boot.to_ascii_lowercase(),
    ))
}

fn valid_boot(boot: &str) -> bool {
    boot.len() == 36
        && boot.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
        && boot.bytes().any(|c| c != b'0' && c != b'-')
}

/// Reads a live process through public OS APIs, rejecting mixed identity samples.
/// Any OS error, including a zombie/uninspectable process, remains an error.
pub fn process_identity(pid: u32) -> io::Result<ProcessIdentity> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(invalid("process identity requires a positive platform PID"));
    }
    let first = sample(pid)?;
    #[cfg(target_os = "linux")]
    let executable = std::fs::read_link(format!("/proc/{pid}/exe"))?;
    #[cfg(target_os = "macos")]
    let executable = darwin_libproc::pid_path(pid as i32)?;
    let second = sample(pid)?;
    use std::os::unix::ffi::OsStrExt;
    if first != second
        || first.2[0] == 0
        || !valid_boot(&first.3)
        || !executable.is_absolute()
        || executable.as_os_str().len() > 4096
        || executable.as_os_str().as_bytes().contains(&0)
    {
        return Err(invalid("process identity changed or is malformed"));
    }
    Ok(ProcessIdentity {
        pid,
        uid: first.0,
        parent_pid: first.1,
        start: first.2,
        boot: first.3,
        executable,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ExitObservation {
    Exited(i32),
    Signaled(i32),
    Stopped(i32),
    Continued,
}

/// Caller must retain its physical child handle; this function does not reap.
/// ECHILD is an error, never a successful exit or permission to signal a PID.
/// With job_control=false, Some contains only Exited or Signaled.
pub fn observe_child(pid: u32, job_control: bool) -> io::Result<Option<ExitObservation>> {
    use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
    let pid = Pid::from_raw(i32::try_from(pid).map_err(|_| invalid("invalid child PID"))?)
        .ok_or_else(|| invalid("invalid child PID"))?;
    let mut flags = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
    if job_control {
        flags |= WaitIdOptions::STOPPED | WaitIdOptions::CONTINUED;
    }
    let Some(status) = waitid(WaitId::Pid(pid), flags)? else {
        return Ok(None);
    };
    // rustix exposes the status, not siginfo.si_pid. P_PID restricts this
    // observation to the physically retained child; never use P_ALL here.
    if status.exited() {
        Ok(Some(ExitObservation::Exited(
            status
                .exit_status()
                .ok_or_else(|| invalid("missing exit status"))?,
        )))
    } else if status.killed() || status.dumped() {
        Ok(Some(ExitObservation::Signaled(
            status
                .terminating_signal()
                .ok_or_else(|| invalid("missing terminating signal"))?,
        )))
    } else if status.stopped() {
        // Darwin can return this observation even for an EXITED-only request
        // (hosted macOS 15 regression). Keep the exit-only API terminal-only so
        // callers never treat suspension as exit or enter a blocking final wait.
        if !job_control {
            return Ok(None);
        }
        Ok(Some(ExitObservation::Stopped(
            status
                .stopping_signal()
                .ok_or_else(|| invalid("missing stop signal"))?,
        )))
    } else if status.continued() {
        Ok(job_control.then_some(ExitObservation::Continued))
    } else {
        Err(invalid("unknown child exit observation"))
    }
}
