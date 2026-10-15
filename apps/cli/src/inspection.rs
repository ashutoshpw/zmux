//! App-wide cached inspection; no ambient caller or target resolution occurs.
use super::*;
use zmux_desktop_runtime::inspection::{DEFAULT_CURRENT_LIMIT, MAX_CURRENT_LIMIT};

pub(super) fn parse(tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some("current".into()))));
    }
    let args = utf8_args(tail)?;
    let mut limit = None;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--limit" if limit.is_none() => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| CliError("--limit requires 1–200".into()))?;
                let value = value
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=MAX_CURRENT_LIMIT).contains(n))
                    .ok_or_else(|| CliError("current limit must be 1–200".into()))?;
                limit = Some(value);
            }
            "--json" if !json => json = true,
            arg => {
                return Err(CliError(format!(
                    "unsupported or repeated current argument `{arg}`"
                )));
            }
        }
        index += 1;
    }
    Ok(Parsed::App(ControlCommand::Current {
        limit: limit.unwrap_or(DEFAULT_CURRENT_LIMIT),
    }))
}
