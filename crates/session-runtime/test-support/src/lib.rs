//! OS controls used only by an explicitly isolated Session regression process.

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

/// Install and check actual kernel auto-reap after the fixture's exec boundary.
/// This is a test-only process policy, never a production Session injection seam.
#[allow(unsafe_code)]
pub fn install_no_child_wait_in_isolated_fixture() -> std::io::Result<()> {
    const TEST: &str =
        "actual_cleanup_failure_remains_an_error_after_join_for_both_shutdown_policies";
    if std::env::var_os("ZMUX_CLEANUP_OUTCOME_CHILD").as_deref() != Some(std::ffi::OsStr::new("1"))
        || !std::env::args().any(|argument| argument == "--exact")
        || !std::env::args().any(|argument| argument == TEST)
    {
        return Err(std::io::Error::other(
            "no-child-wait installation requires the exact isolated cleanup fixture",
        ));
    }
    let action = SigAction::new(SigHandler::SigIgn, SaFlags::SA_NOCLDWAIT, SigSet::empty());
    // SAFETY: this exact test runs in its own post-exec process. SIG_IGN installs
    // no Rust handler or callback. No parent/other test process policy changes.
    // Reinstalling the same action returns the kernel's prior installed policy,
    // which is checked rather than assuming SIG_IGN implies SA_NOCLDWAIT.
    let installed = unsafe {
        sigaction(Signal::SIGCHLD, &action)?;
        sigaction(Signal::SIGCHLD, &action)?
    };
    if !installed.flags().contains(SaFlags::SA_NOCLDWAIT)
        || !matches!(installed.handler(), SigHandler::SigIgn)
    {
        return Err(std::io::Error::other(
            "kernel did not retain the isolated SA_NOCLDWAIT policy",
        ));
    }
    Ok(())
}

/// Reproduce portable-pty's pre-exec descriptor closure in a directly owned
/// Child. The extra bounded gate permits an independent inherited-image sample.
/// This does not exercise portable-pty's complete PTY spawn implementation.
#[allow(unsafe_code)]
pub fn spawn_with_closed_spawn_pipe_gate(
    program: &std::path::Path,
    release: &std::path::Path,
) -> std::io::Result<std::process::Child> {
    use std::{
        ffi::CString,
        os::unix::process::CommandExt,
        process::{Command, Stdio},
    };
    let release = CString::new(release.as_os_str().as_encoded_bytes())
        .map_err(|_| std::io::Error::other("pre-exec gate path contains NUL"))?;
    let mut command = Command::new(program);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: paths and command are allocated before fork. The operation under
    // investigation is the actual library function called by portable-pty's
    // own pre_exec. Our added gate only calls async-signal-safe libc functions
    // on preallocated data, then exec proceeds or the child exits after 5s.
    unsafe {
        command.pre_exec(move || {
            portable_pty::unix::close_random_fds();
            let pause = nix::libc::timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000,
            };
            for _ in 0..5000 {
                if nix::libc::access(release.as_ptr(), nix::libc::F_OK) == 0 {
                    return Ok(());
                }
                nix::libc::nanosleep(&pause, std::ptr::null_mut());
            }
            nix::libc::_exit(124);
        });
    }
    command.spawn()
}
