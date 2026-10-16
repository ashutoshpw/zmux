//! `zmux open` and `zmux markdown open`: lexical operand classification, absolute paths, the
//! request bound and per-item result reporting. The app owns everything that needs the
//! filesystem or the model (existence, type, size, content, targets, dedupe); this module only
//! decides what can be refused before any request is sent.
use super::*;
use zmux_desktop_runtime::targets::{
    OpenForceMode, OpenItem, OpenItemKind, TargetOperation, TargetRequest,
};

/// Operands per invocation. Mirrors the server's own bound.
pub(super) const MAX_OPERANDS: usize = 16;
/// Longest path or web address accepted for one operand.
const MAX_OPERAND_BYTES: usize = 4096;
/// The server probes the files for up to a second and then waits up to two seconds for the
/// workspace executor, so the generic 3 second budget would leave no headroom.
pub(super) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Builds the `open_items` operation. Any lexical refusal fails the whole invocation here, before
/// a request exists, so a partly valid command never has side effects.
pub(super) fn operation(
    markdown: bool,
    operands: &[String],
    focus: bool,
) -> Result<TargetOperation, CliError> {
    let command = if markdown { "markdown open" } else { "open" };
    if operands.is_empty() {
        return Err(CliError(if markdown {
            "markdown open requires at least one file path".into()
        } else {
            "open requires at least one file path or http(s) URL".into()
        }));
    }
    if operands.len() > MAX_OPERANDS {
        return Err(CliError(format!(
            "{command} takes at most {MAX_OPERANDS} operands; split the operands across invocations"
        )));
    }
    let mut cwd = None;
    let items = operands
        .iter()
        .enumerate()
        .map(|(index, operand)| classify(index, operand, markdown, &mut cwd))
        .collect::<Result<Vec<_>, _>>()?;
    let operation = TargetOperation::OpenItems {
        items,
        focus,
        force_mode: markdown.then_some(OpenForceMode::Markdown),
    };
    // The exact encoded size is only known with the caller's password (see `run`), but the
    // operands alone already decide the common case, so refuse it before connecting.
    let probe = encoded_len(
        &ControlCommand::Targeted {
            request: TargetRequest {
                operation: operation.clone(),
                ..Default::default()
            },
        },
        None,
    )?;
    if probe > MAX_CONTROL_REQUEST_BYTES {
        return Err(too_large(probe));
    }
    Ok(operation)
}

fn too_large(bytes: usize) -> CliError {
    CliError(format!(
        "request_too_large: the operands encode to {bytes} bytes and a request may not exceed {MAX_CONTROL_REQUEST_BYTES}; split the operands across invocations"
    ))
}

fn classify(
    index: usize,
    operand: &str,
    markdown: bool,
    cwd: &mut Option<PathBuf>,
) -> Result<OpenItem, CliError> {
    let number = index + 1;
    if operand.is_empty() {
        return Err(CliError(format!(
            "invalid_operand: operand {number} is empty"
        )));
    }
    if operand.chars().any(char::is_control) {
        return Err(CliError(format!(
            "invalid_operand: operand {number} contains a control character"
        )));
    }
    if has_scheme(operand) {
        if markdown {
            return Err(CliError(format!(
                "invalid_operand: operand {number} looks like a web address; markdown open takes file paths only"
            )));
        }
        let lower = operand.to_ascii_lowercase();
        if !(lower.starts_with("http://") || lower.starts_with("https://")) {
            return Err(CliError(format!(
                "unsupported_scheme: operand {number} is not an http:// or https:// address; name a local file by its path"
            )));
        }
        if operand.len() > MAX_OPERAND_BYTES {
            return Err(CliError(format!(
                "invalid_operand: operand {number} is longer than {MAX_OPERAND_BYTES} bytes"
            )));
        }
        return Ok(OpenItem {
            kind: OpenItemKind::Url,
            value: operand.to_owned(),
        });
    }
    let path = PathBuf::from(operand);
    let absolute = if path.is_absolute() {
        path
    } else {
        // The directory is read once and only when some operand is relative. It is joined, never
        // canonicalised, and `~` is not expanded: the app resolves symlinks and `..`.
        if cwd.is_none() {
            *cwd = Some(env::current_dir().map_err(|error| {
                CliError(format!(
                    "cannot resolve relative paths: current directory is unavailable: {error}"
                ))
            })?);
        }
        cwd.as_ref().expect("set above").join(path)
    };
    let value = absolute.to_str().ok_or_else(|| {
        CliError(format!(
            "invalid_operand: operand {number} is not valid UTF-8 once made absolute"
        ))
    })?;
    if value.len() > MAX_OPERAND_BYTES {
        return Err(CliError(format!(
            "invalid_operand: operand {number} is longer than {MAX_OPERAND_BYTES} bytes once made absolute"
        )));
    }
    Ok(OpenItem {
        kind: OpenItemKind::Path,
        value: value.to_owned(),
    })
}

/// `scheme://`: an ASCII letter, then letters, digits, `+`, `-` or `.`, then `://`. `foo:bar.txt`
/// and `dir/a://b` are paths.
fn has_scheme(operand: &str) -> bool {
    let mut characters = operand.char_indices();
    match characters.next() {
        Some((_, first)) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for (index, character) in characters {
        if character == ':' {
            return operand[index..].starts_with("://");
        }
        if !(character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')) {
            return false;
        }
    }
    false
}

fn encoded_len(
    command: &ControlCommand,
    password: Option<&SocketPassword>,
) -> Result<usize, CliError> {
    serde_json::to_vec(&ControlRequest {
        version: CONTROL_VERSION,
        request_id: u64::MAX,
        password: password.cloned(),
        command: command.clone(),
    })
    .map(|bytes| bytes.len())
    .map_err(|error| CliError(format!("encode local-control request: {error}")))
}

/// Sends the request once, prints the server's full result as JSON and returns the exit status:
/// 0 when every item succeeded (a deduplicated item is a success), 1 when any item failed.
/// A whole-request failure (stale revision, unknown target, policy, transport) is an `Err`.
pub(super) fn run(options: &ClientOptions, command: ControlCommand) -> Result<u8, Box<dyn Error>> {
    let size = encoded_len(&command, options.password.as_ref())?;
    if size > MAX_CONTROL_REQUEST_BYTES {
        return Err(too_large(size).into());
    }
    let result = response_result(request(options, command)?)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    let items = result
        .get("items")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            CliError(
                "running app returned an open result without per-item results; the command was not retried"
                    .into(),
            )
        })?;
    let failed = items
        .iter()
        .filter(|item| item.get("ok").and_then(serde_json::Value::as_bool) != Some(true))
        .count();
    if failed == 0 && result.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(0);
    }
    eprintln!("zmux: {failed} of {} items failed", items.len());
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(operation: TargetOperation) -> Vec<(OpenItemKind, String)> {
        match operation {
            TargetOperation::OpenItems { items, .. } => {
                items.into_iter().map(|i| (i.kind, i.value)).collect()
            }
            other => panic!("not an open operation: {other:?}"),
        }
    }

    #[test]
    fn only_scheme_slash_slash_is_a_scheme() {
        for scheme in [
            "http://x",
            "HTTPS://x",
            "file:///etc",
            "ftp://h",
            "a+b-c.d://x",
        ] {
            assert!(has_scheme(scheme), "{scheme}");
        }
        for path in [
            "foo:bar.txt",
            "about:blank",
            "dir/a://b",
            "./http://x",
            "1http://x",
            "://x",
            "http:/x",
            "http",
            "",
            "c:\\x",
        ] {
            assert!(!has_scheme(path), "{path}");
        }
    }

    #[test]
    fn classification_orders_and_kinds_are_preserved() {
        let operation = operation(
            false,
            &["/abs/a.rs".into(), "https://example.org/".into()],
            true,
        )
        .unwrap();
        assert_eq!(
            items(operation),
            vec![
                (OpenItemKind::Path, "/abs/a.rs".to_owned()),
                (OpenItemKind::Url, "https://example.org/".to_owned()),
            ]
        );
    }

    #[test]
    fn markdown_forces_the_mode_and_refuses_urls() {
        let forced = operation(true, &["/a/README".into()], false).unwrap();
        assert!(matches!(
            forced,
            TargetOperation::OpenItems {
                force_mode: Some(OpenForceMode::Markdown),
                focus: false,
                ..
            }
        ));
        for url in ["https://example.org/", "file:///a.md", "ftp://x/a.md"] {
            let error = operation(true, &[url.into()], true).unwrap_err().0;
            assert!(error.starts_with("invalid_operand"), "{url}: {error}");
        }
    }

    #[test]
    fn limits_are_enforced_before_a_request_exists() {
        let many: Vec<String> = (0..=MAX_OPERANDS).map(|n| format!("/f{n}")).collect();
        assert!(
            operation(false, &many, true)
                .unwrap_err()
                .0
                .contains("at most 16")
        );
        assert!(operation(false, &many[..MAX_OPERANDS], true).is_ok());
        let long = format!("/{}", "a".repeat(MAX_OPERAND_BYTES));
        assert!(
            operation(false, &[long], true)
                .unwrap_err()
                .0
                .starts_with("invalid_operand")
        );
        let exact = format!("/{}", "a".repeat(MAX_OPERAND_BYTES - 1));
        assert!(operation(false, &[exact], true).is_ok());
        // Four maximum-length paths already exceed the control request bound.
        let heavy: Vec<String> = (0..4)
            .map(|n| format!("/{n}{}", "b".repeat(MAX_OPERAND_BYTES - 2)))
            .collect();
        assert!(
            operation(false, &heavy, true)
                .unwrap_err()
                .0
                .contains("split the operands across invocations")
        );
    }

    #[test]
    fn control_characters_and_empty_operands_are_invalid() {
        for operand in ["", "a\nb", "tab\t", "nul\u{0}", "bell\u{7}"] {
            let error = operation(false, &[operand.into()], true).unwrap_err().0;
            assert!(error.starts_with("invalid_operand"), "{operand:?}: {error}");
        }
    }

    #[test]
    fn open_gets_a_five_second_budget_unless_a_global_timeout_is_given() {
        let parse = |arguments: &[&str]| {
            crate::parse(arguments.iter().map(OsString::from).collect())
                .map(|(_, _, timeout, _)| timeout)
        };
        assert_eq!(parse(&["open", "/a"]).unwrap(), DEFAULT_TIMEOUT);
        assert_eq!(parse(&["markdown", "open", "/a"]).unwrap(), DEFAULT_TIMEOUT);
        assert_eq!(
            parse(&["--timeout", "700", "open", "/a"]).unwrap(),
            Duration::from_millis(700)
        );
        // Other targeted commands keep the global default.
        assert_eq!(parse(&["list-panes"]).unwrap(), crate::DEFAULT_TIMEOUT);
    }
}
