//! Exact process-bound, foreground-only local Zellij ownership.
//! No arbitrary socket adoption, automatic owner close, or GUI readiness receipt.
mod broker;
mod files;
mod observer;
pub mod observer_protocol;
mod process;
mod supervisor;

pub use files::{LocalZellij, Record};
pub use process::ForegroundTerminal;
pub use supervisor::run_supervisor;

use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};
use zmux_session_runtime::child::{ProcessIdentity, process_identity};

const LIMIT: usize = 65536;
const OWNED_CONFIG: &[u8] = b"session_serialization false\non_force_close \"detach\"\nshow_release_notes false\nshow_startup_tips false\n";
const START_TIMEOUT: Duration = Duration::from_secs(10);

fn fail(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}
fn check_deadline(deadline: Instant, context: &str) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(fail(format!(
            "{context} deadline; outcome is unknown, do not replay start"
        )))
    } else {
        Ok(())
    }
}
fn same(identity: &ProcessIdentity) -> io::Result<()> {
    if process_identity(identity.pid)? == *identity {
        Ok(())
    } else {
        Err(fail("reserved process identity changed"))
    }
}
fn executable(path: &Path) -> io::Result<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = path.canonicalize()?;
    let metadata = path.metadata()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(fail(
            "executable must be an absolute executable regular file",
        ));
    }
    Ok(path)
}
