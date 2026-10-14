//! Headless terminal feasibility command; not the final socket automation client.

use std::{
    error::Error,
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};
use zmux_session_runtime::{Launch, Session, SessionError, State};

const USAGE: &str = "Usage: zmux prototype [--timeout SECONDS] -- PROGRAM [ARG ...]\nRuns a local Unix PTY and prints its final visible screen as JSON. Not a native app.";

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("zmux: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<u8, Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    if command.as_deref() == Some("--help") || command.as_deref() == Some("-h") {
        println!("{USAGE}");
        return Ok(0);
    }
    if command.as_deref() != Some("prototype") {
        return Err(USAGE.into());
    }
    let mut timeout = Duration::from_secs(10);
    loop {
        match args.next().as_deref() {
            Some("--timeout") => {
                let seconds: u64 = args.next().ok_or("missing timeout")?.parse()?;
                if seconds == 0 || seconds > 3600 {
                    return Err("timeout must be 1–3600 seconds".into());
                }
                timeout = Duration::from_secs(seconds);
            }
            Some("--") => break,
            _ => return Err(USAGE.into()),
        }
    }
    let program = args.next().ok_or("missing program")?;
    let mut session = Session::spawn(Launch {
        program,
        args: args.collect(),
        cwd: None,
        size: zmux_session_runtime::default_size(),
    })?;
    let deadline = Instant::now() + timeout;
    let result = loop {
        match session.snapshot() {
            Ok(snapshot) if snapshot.state != State::Running => break Ok(snapshot),
            Ok(_) | Err(SessionError::Busy) => {}
            Err(error) => break Err(error),
        }
        if Instant::now() >= deadline {
            break Err(SessionError::Failure(
                "prototype execution exceeded its timeout".into(),
            ));
        }
        thread::sleep(Duration::from_millis(5));
    };
    session.shutdown(Duration::from_secs(3))?;
    let snapshot = result?;
    println!("{}", serde_json::to_string_pretty(&*snapshot)?);
    if let Some(failure) = &snapshot.failure {
        return Err(failure.clone().into());
    }
    Ok(snapshot.exit_code.unwrap_or(1).min(255) as u8)
}
