use std::env;
use zmux_remote_runtime::scheduler::{SCHEDULER_PROTOCOL_VERSION, daemon};

fn main() {
    let mut arguments = env::args_os();
    let program = arguments.next().unwrap_or_default();
    let command = arguments.next();
    if arguments.next().is_some() {
        eprintln!("unexpected arguments; see `zmux-scheduler --help`");
        std::process::exit(2);
    }
    match command.as_deref() {
        Some(value) if value == "serve" => {
            if let Err(error) = daemon::run_serve() {
                eprintln!("zmux-scheduler serve: {error}");
                std::process::exit(1);
            }
        }
        Some(value) if value == "rpc" => {
            if let Err(error) = daemon::run_rpc() {
                eprintln!("zmux-scheduler rpc: {error}");
                std::process::exit(1);
            }
        }
        Some(value) if value == "protocol-version" => {
            println!("{SCHEDULER_PROTOCOL_VERSION}");
        }
        Some(value) if value == "--help" || value == "-h" => print_help(&program),
        None => print_help(&program),
        _ => {
            eprintln!("unknown command; see `zmux-scheduler --help`");
            std::process::exit(2);
        }
    }
}

fn print_help(program: &std::ffi::OsStr) {
    println!(
        "Usage: {} <serve|rpc|protocol-version>\n\nCommands:\n  serve             Run the foreground per-user scheduler daemon\n  rpc               Send one newline-terminated JSON request from stdin to the running daemon\n  protocol-version  Print the supported scheduler JSON protocol version\n\nEnvironment:\n  ZMUX_SCHEDULER_ROOT         Private scheduler state directory\n  ZMUX_SCHEDULER_TMUX_SOCKET  Absolute path to the default tmux server socket\n  ZMUX_SCHEDULER_TMUX_BIN     Absolute tmux executable override",
        std::path::Path::new(program).display()
    );
}
