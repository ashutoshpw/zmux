//! Original MIT fixture calling the pinned dependency's actual public stdin owner.
use nix::{
    fcntl::{fcntl, FcntlArg, OFlag},
    sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal},
};
use std::{
    io::{self, Write},
    sync::atomic::{AtomicUsize, Ordering},
};
use zellij_client::os_input_output::{get_cli_client_os_input, ClientOsApi};

static SIGNALS: AtomicUsize = AtomicUsize::new(0);
extern "C" fn interrupted(_: i32) {
    SIGNALS.fetch_add(1, Ordering::Relaxed);
}
fn main() {
    let non_eintr = std::env::args().nth(1).as_deref() == Some("would-block");
    let action = SigAction::new(
        SigHandler::Handler(interrupted),
        SaFlags::empty(),
        SigSet::empty(),
    );
    // SAFETY: this isolated single-reader fixture owns SIGUSR1; the handler uses
    // only a lock-free counter. SA_RESTART is deliberately absent for real EINTR.
    unsafe { sigaction(Signal::SIGUSR1, &action) }.unwrap();
    let mut owner = get_cli_client_os_input().unwrap();
    if non_eintr {
        fcntl(0, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).unwrap();
    }
    println!("READY");
    io::stdout().flush().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.read_from_stdin()));
    if non_eintr {
        assert!(result.is_err(), "non-EINTR read failure was swallowed");
        println!("NON_EINTR_FAILED");
        return;
    }
    let bytes = result
        .expect("actual stdin owner panicked")
        .expect("actual stdin owner failed");
    assert_eq!(bytes, b"kernel-stdin-after-eintr\n");
    assert!(
        SIGNALS.load(Ordering::Relaxed) > 0,
        "no real signal reached reader"
    );
    println!(
        "READ_EXACT_ONCE signals={}",
        SIGNALS.load(Ordering::Relaxed)
    );
}
