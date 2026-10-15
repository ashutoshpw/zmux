use super::*;
use zmux_remote_runtime::local_zellij::{ForegroundTerminal, LocalZellij};

pub(super) enum Invocation {
    List,
    Status(uuid::Uuid),
    Start {
        name: String,
        cwd: PathBuf,
        command: Option<String>,
    },
    Attach(uuid::Uuid),
    Supervisor(PathBuf),
}

pub(super) fn parse(tail: &[OsString]) -> Result<Parsed, CliError> {
    let args = utf8_args(tail)?;
    if args.is_empty() || args.as_slice() == ["--help"] || args.as_slice() == ["help"] {
        return Ok(Parsed::Offline(Offline::Help(Some("local-zellij".into()))));
    }
    let invocation = match args[0].as_str() {
        "list" if args.len() == 1 => Invocation::List,
        "status" | "attach" if args.len() == 3 && args[1] == "--id" => {
            if args[0] == "status" {
                Invocation::Status(
                    args[2]
                        .parse()
                        .map_err(|_| CliError("--id requires a UUID".into()))?,
                )
            } else {
                return Err(CliError(
                    "attach requires --headless; GUI attachment is unavailable".into(),
                ));
            }
        }
        "attach" if args.len() == 4 && args[1] == "--headless" && args[2] == "--id" => {
            Invocation::Attach(
                args[3]
                    .parse()
                    .map_err(|_| CliError("--id requires a UUID".into()))?,
            )
        }
        "start" => {
            let name = args
                .get(1)
                .ok_or_else(|| CliError("start requires NAME --headless".into()))?
                .clone();
            let mut cwd = env::current_dir().map_err(|e| CliError(e.to_string()))?;
            let mut command = None;
            let mut headless = false;
            let mut cwd_set = false;
            let mut index = 2;
            while index < args.len() {
                match args[index].as_str() {
                    "--headless" if !headless => { headless = true; index += 1; }
                    "--cwd" if !cwd_set => { index += 1; cwd = PathBuf::from(args.get(index).ok_or_else(|| CliError("--cwd requires a path".into()))?); cwd_set = true; index += 1; }
                    "--command" if command.is_none() => { index += 1; command = Some(args.get(index).ok_or_else(|| CliError("--command requires shell text".into()))?.clone()); index += 1; }
                    _ => return Err(CliError("unknown/repeated local-zellij start option; only foreground --headless is implemented".into())),
                }
            }
            if !headless {
                return Err(CliError(
                    "start requires --headless; detached/GUI creation is unavailable".into(),
                ));
            }
            if !cwd.is_absolute() {
                cwd = env::current_dir()
                    .map_err(|e| CliError(e.to_string()))?
                    .join(cwd);
            }
            Invocation::Start { name, cwd, command }
        }
        _ => {
            return Err(CliError(
                "local-zellij supports foreground start/attach and identity list/status only"
                    .into(),
            ));
        }
    };
    Ok(Parsed::LocalZellij(invocation))
}
pub(super) fn run(invocation: Invocation) -> Result<u8, Box<dyn Error>> {
    if let Invocation::Supervisor(path) = invocation {
        return Ok(
            u8::try_from(zmux_remote_runtime::local_zellij::run_supervisor(&path)?).unwrap_or(1),
        );
    }
    let mut terminal = if matches!(invocation, Invocation::Start { .. } | Invocation::Attach(_)) {
        Some(ForegroundTerminal::acquire()?)
    } else {
        None
    };
    let owner = LocalZellij::from_environment(terminal.as_mut())?;
    let code = match invocation {
        Invocation::List => {
            println!("{}", serde_json::to_string_pretty(&owner.list()?)?);
            0
        }
        Invocation::Status(id) => {
            println!("{}", serde_json::to_string_pretty(&owner.status(id)?)?);
            0
        }
        Invocation::Start { name, cwd, command } => owner.start(
            &name,
            &cwd,
            command.as_deref(),
            terminal.as_mut().expect("foreground terminal"),
        )?,
        Invocation::Attach(id) => {
            owner.attach(id, terminal.as_mut().expect("foreground terminal"))?
        }
        Invocation::Supervisor(_) => unreachable!(),
    };
    Ok(u8::try_from(code).unwrap_or(1))
}
