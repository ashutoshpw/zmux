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
