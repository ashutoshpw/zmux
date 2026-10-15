//! Bounded local contracts and explicit historical coverage. Parsing probes never execute a command.
use super::{COMMANDS, find};
use crate::{CliError, MAX_TIMEOUT_MS, MIN_TIMEOUT_MS, Parsed};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    ffi::OsString,
};
use zmux_desktop_runtime::{browser, downloads};

const INVENTORY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/baseline/command-inventory.json"
));
const RECONCILIATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/baseline/audit-reconciliation.json"
));
const COVERAGE: &str = include_str!("frozen-coverage.json");

#[derive(Clone, Copy)]
enum ValueKind {
    Text,
    Uuid,
    SignedInteger,
    Number,
    Target,
    Path,
    Flag,
    Integer(u64, u64),
    Choice(&'static [&'static str]),
}
struct Argument {
    name: &'static str,
    forms: &'static [&'static str],
    kind: ValueKind,
    required: bool,
}
impl Argument {
    fn json(&self) -> Value {
        let value = match self.kind {
            ValueKind::Text => json!({"type": "string"}),
            ValueKind::Uuid => json!({"type": "uuid"}),
            ValueKind::SignedInteger => {
                json!({"type": "integer", "minimum": i64::MIN, "maximum": i64::MAX})
            }
            ValueKind::Number => json!({"type": "number", "finite": true}),
            ValueKind::Target => json!({"type": "target", "resolution": "live_owner_checked"}),
            ValueKind::Path => json!({"type": "path"}),
            ValueKind::Flag => json!({"type": "flag"}),
            ValueKind::Integer(min, max) => {
                json!({"type": "integer", "minimum": min, "maximum": max})
            }
            ValueKind::Choice(values) => json!({"type": "enum", "values": values}),
        };
        json!({"name": self.name, "forms": self.forms, "placement": if self.forms.is_empty() {"positional"} else {"option"},
            "required": self.required, "value": value})
    }
}
fn positional(name: &'static str, kind: ValueKind, required: bool) -> Argument {
    Argument {
        name,
        forms: &[],
        kind,
        required,
    }
}
fn option(name: &'static str, forms: &'static [&'static str], kind: ValueKind) -> Argument {
    Argument {
        name,
        forms,
        kind,
        required: false,
    }
}
struct Contract {
    name: String,
    probe: Vec<String>,
    arguments: Vec<Argument>,
    expected: Vec<(&'static str, &'static str)>,
    route: &'static str,
    requirements: &'static [&'static str],
    focus: &'static str,
    output: &'static str,
    default_pointers: Vec<(&'static str, &'static str)>,
    defaults: Value,
    limitations: &'static str,
    argument_rules: Vec<Value>,
}
fn contract(
    name: &str,
    operands: &[&str],
    arguments: Vec<Argument>,
    command: &'static str,
) -> Contract {
    Contract {
        name: name.into(),
        probe: name
            .split_whitespace()
            .chain(operands.iter().copied())
            .map(String::from)
            .collect(),
        arguments,
        expected: vec![("/method", command)],
        route: "app",
        requirements: &["running_app"],
        focus: "preserve",
        output: "json_result",
        default_pointers: Vec::new(),
        defaults: json!({}),
        limitations: "Bounded local argument description; full frozen option/output/error compatibility is unresolved.",
        argument_rules: Vec::new(),
    }
}
fn target_arguments() -> Vec<Argument> {
    vec![
        option("window", &["--window"], ValueKind::Target),
        option("workspace", &["--workspace"], ValueKind::Target),
        option("pane", &["--pane"], ValueKind::Target),
        option("surface", &["--surface", "--tab"], ValueKind::Target),
        option(
            "expected_model_revision",
            &["--expect-revision"],
            ValueKind::Integer(0, u64::MAX),
        ),
    ]
}
fn focus_arguments() -> Vec<Argument> {
    vec![
        option("focus", &["--focus"], ValueKind::Choice(&["true", "false"])),
        option("no_focus", &["--no-focus"], ValueKind::Flag),
    ]
}
fn definitions() -> Vec<Contract> {
    let mut result = Vec::new();
    for (name, command) in [
        ("list-workspaces", "list_workspaces"),
        ("list-notifications", "list_notifications"),
        ("recently-closed", "recently_closed"),
        ("list-uploads", "list_uploads"),
        ("status", "status"),
        ("capabilities", "capabilities"),
    ] {
        result.push(contract(name, &[], vec![], command));
    }
    let mut new = contract(
        "new-workspace",
        &["example"],
        vec![positional("name", ValueKind::Text, true)],
        "new_workspace",
    );
    new.focus = "select_created_workspace";
    new.output = "json_admission_launch_is_asynchronous";
    result.push(new);
    for (name, operand, action) in [
        ("new-surface", "--pane", "create_surface"),
        ("select-workspace", "--workspace", "select_workspace"),
        ("focus-pane", "--pane", "focus_pane"),
        ("select-surface", "--surface", "select_surface"),
    ] {
        let mut c = contract(name, &[operand, "1"], target_arguments(), "targeted");
        c.expected
            .push(("/request/operation/action/action", action));
        c.focus = "select_target_or_created_surface";
        c.limitations = "Optional legacy positional target and leading target options are accepted; creation launches asynchronously.";
        result.push(c);
    }
    let mut split = contract(
        "new-split",
        &["--pane", "1", "right"],
        target_arguments(),
        "targeted",
    );
    split.arguments.push(positional(
        "direction",
        ValueKind::Choice(&["left", "right", "up", "down"]),
        true,
    ));
    split.arguments.extend(focus_arguments());
    split
        .argument_rules
        .push(json!({"at_most_one": ["focus", "no_focus"]}));
    split
        .expected
        .push(("/request/operation/action/action", "split_pane_directional"));
    split
        .default_pointers
        .push(("focus", "/request/operation/action/focus"));
    split.focus = "conditional_on_focus_argument";
    split.output = "json_created_ids_with_separate_focused_ids";
    split.limitations = "Options may precede or follow direction. Target indices are one-based; create launches asynchronously.";
    result.push(split);
    for (name, operation) in [
        ("read-screen", "read_screen"),
        ("list-panes", "list_panes"),
        ("list-pane-surfaces", "list_pane_surfaces"),
    ] {
        let mut c = contract(name, &["--pane", "1"], target_arguments(), "targeted");
        c.expected.push(("/request/operation/operation", operation));
        result.push(c);
    }
    let mut send = contract(
        "send-text",
        &["--pane", "1", "example"],
        target_arguments(),
        "targeted",
    );
    send.arguments.extend([
        positional("text", ValueKind::Text, false),
        option("stdin", &["--stdin"], ValueKind::Flag),
    ]);
    send.argument_rules
        .push(json!({"exactly_one": ["text", "stdin"]}));
    send.expected
        .push(("/request/operation/operation", "send_text"));
    send.limitations = "Text or --stdin, at most 4096 UTF-8 bytes. No newline is appended. -- makes --stdin literal. PTY acceptance is not program consumption.";
    result.push(send);
    for name in [
        "next-workspace",
        "previous-workspace",
        "last-workspace",
        "next-pane",
        "previous-pane",
        "last-pane",
        "next-tab",
        "previous-tab",
        "last-tab",
    ] {
        let scope = if name.ends_with("workspace") {
            "--workspace"
        } else {
            "--pane"
        };
        let arguments = if scope == "--workspace" {
            vec![
                option("window", &["--window"], ValueKind::Target),
                option("workspace", &["--workspace"], ValueKind::Target),
                option(
                    "expected_model_revision",
                    &["--expect-revision"],
                    ValueKind::Integer(0, u64::MAX),
                ),
            ]
        } else {
            target_arguments()
        };
        let mut c = contract(name, &[scope, "1"], arguments, "targeted");
        c.expected.push((
            "/request/operation/action/action",
            if name.ends_with("workspace") {
                "navigate_workspace"
            } else if name.ends_with("tab") {
                "navigate_surface"
            } else {
                "navigate_pane"
            },
        ));
        c.expected.push((
            "/request/operation/action/direction",
            if name.starts_with("next-") {
                "next"
            } else if name.starts_with("previous-") {
                "previous"
            } else {
                "last"
            },
        ));
        c.focus = "select_navigation_result";
        c.limitations = "Navigation/history is local to its owner. A missing reachable history fails. Window navigation aliases still select workspaces.";
        result.push(c);
    }
    for name in ["save-session", "restore-session"] {
        let mut c = contract(name, &[], vec![], "session");
        c.expected.push((
            "/action/action",
            if name == "save-session" {
                "save"
            } else {
                "restore"
            },
        ));
        c.output = "json_pending_operation_id_poll_session_status";
        c.requirements = &["running_app", "durable_session_storage"];
        if name == "restore-session" {
            c.arguments = vec![
                option("from", &["--from"], ValueKind::Path),
                option("export", &["--export"], ValueKind::Path),
                option("force", &["--force"], ValueKind::Flag),
            ];
            c.focus = "saved_selection_on_success";
            c.argument_rules.extend([
                json!({"at_most_one": ["from", "export"]}),
                json!({"if_present": "force", "requires": "export"}),
            ]);
            c.limitations = "--from conflicts with --export; --force requires --export. Relative paths resolve from current directory. Restore requires an empty desktop; export is a different operation.";
        }
        result.push(c);
    }
    let mut status = contract(
        "session-status",
        &[],
        vec![option("id", &["--id"], ValueKind::Integer(1, u64::MAX))],
        "session_status",
    );
    status.requirements = &["running_app", "durable_session_storage"];
    status.limitations = "Without --id returns persistence state; with --id returns the bounded operation receipt. Unknown/expired IDs fail.";
    result.push(status);
    for verb in ["open", "navigate"] {
        let mut c = contract(
            &format!("browser {verb}"),
            &["--surface", "1", "https://example.org/"],
            target_arguments(),
            "targeted",
        );
        c.arguments.push(positional("url", ValueKind::Text, true));
        if verb == "open" {
            c.arguments.extend(focus_arguments());
            c.argument_rules
                .push(json!({"at_most_one": ["focus", "no_focus"]}));
            c.arguments
                .push(option("profile", &["--profile"], ValueKind::Text));
            c.default_pointers
                .push(("focus", "/request/operation/action/focus"));
            c.focus = "conditional_on_focus_argument";
        }
        c.expected.push((
            "/request/operation/action/action",
            if verb == "open" {
                "create_browser"
            } else {
                "navigate_browser"
            },
        ));
        c.limitations = "HTTP(S) or about:blank; credentials in URLs reject. Optional legacy positional pane/open or surface/navigate. Leading selectors; browser open defaults to per-host private profile. goto aliases navigate.";
        result.push(c);
    }
    for kind in [
        "url", "title", "text", "html", "value", "attr", "count", "box", "styles",
    ] {
        let mut operands = vec!["--surface", "1"];
        if !matches!(kind, "url" | "title") {
            operands.extend(["--selector", "#example"]);
        }
        if kind == "attr" {
            operands.extend(["--attr", "title"]);
        }
        let mut c = contract(
            &format!("browser get {kind}"),
            &operands,
            target_arguments(),
            "browser",
        );
        c.route = "browser";
        c.requirements = &["running_app", "native_webkit_host"];
        c.expected.push(("/request/action/kind", kind));
        if !matches!(kind, "url" | "title") {
            c.arguments
                .push(option("selector", &["--selector"], ValueKind::Text));
            c.arguments
                .push(positional("selector_operand", ValueKind::Text, false));
            c.argument_rules
                .push(json!({"exactly_one": ["selector", "selector_operand"]}));
        }
        if kind == "attr" {
            c.arguments
                .push(option("attribute", &["--attr"], ValueKind::Text));
            c.arguments
                .push(positional("attribute_operand", ValueKind::Text, false));
            c.argument_rules
                .push(json!({"exactly_one": ["attribute", "attribute_operand"]}));
        }
        if kind == "styles" {
            c.arguments
                .push(option("property", &["--property"], ValueKind::Text));
        }
        c.limitations = "Built-in isolated-world DOM read. Main-document CSS/ref selector; bounded results, no frame/shadow traversal. Selector/attribute requirements depend on kind; full grammar remains unresolved.";
        result.push(c);
    }
    for verb in ["wait", "snapshot", "eval", "back", "forward", "reload"] {
        let mut operands = vec!["--surface", "1"];
        if verb == "wait" {
            operands.extend(["--selector", "#example"]);
        }
        if verb == "eval" {
            operands.push("document.title");
        }
        let mut c = contract(
            &format!("browser {verb}"),
            &operands,
            target_arguments(),
            "browser",
        );
        c.route = "browser";
        c.requirements = &["running_app", "native_webkit_host"];
        c.expected.push(("/request/action/action", verb));
        c.default_pointers
            .push(("operation_timeout_ms", "/request/timeout_ms"));
        match verb {
            "wait" => {
                c.arguments
                    .push(positional("selector_operand", ValueKind::Text, false));
                c.argument_rules
                    .push(json!({"exactly_one": ["selector", "selector_operand"]}));
                c.arguments.extend([
                    option("selector", &["--selector"], ValueKind::Text),
                    option(
                        "state",
                        &["--state"],
                        ValueKind::Choice(&[
                            "attached",
                            "detached",
                            "visible",
                            "hidden",
                            "enabled",
                            "disabled",
                            "checked",
                            "unchecked",
                        ]),
                    ),
                    option(
                        "timeout_ms",
                        &["--timeout-ms"],
                        ValueKind::Integer(1, browser::MAX_OPERATION_TIMEOUT_MS),
                    ),
                ]);
                c.default_pointers.push(("state", "/request/action/state"));
            }
            "snapshot" => {
                c.arguments.extend([
                    option("interactive", &["--interactive", "-i"], ValueKind::Flag),
                    option("cursor", &["--cursor"], ValueKind::Flag),
                    option("compact", &["--compact"], ValueKind::Flag),
                    option(
                        "max_depth",
                        &["--max-depth"],
                        ValueKind::Integer(0, browser::MAX_SNAPSHOT_DEPTH.into()),
                    ),
                    option("selector", &["--selector"], ValueKind::Text),
                ]);
                for (name, path) in [
                    ("interactive", "/request/action/options/interactive"),
                    ("cursor", "/request/action/options/cursor"),
                    ("compact", "/request/action/options/compact"),
                ] {
                    c.default_pointers.push((name, path));
                }
            }
            "eval" => {
                c.arguments.extend([
                    positional("script_operand", ValueKind::Text, false),
                    option("script", &["--script"], ValueKind::Text),
                ]);
                c.argument_rules
                    .push(json!({"exactly_one": ["script", "script_operand"]}));
            }
            _ => {
                c.arguments.push(option(
                    "snapshot_after",
                    &["--snapshot-after"],
                    ValueKind::Flag,
                ));
                c.default_pointers
                    .push(("snapshot_after", "/request/action/snapshot_after"));
                c.focus = "preserve_selection_navigate_document";
            }
        }
        c.limitations = "Native execution and read-only policy are checked at admission. Eval executes page content-world JavaScript and is a mutation. Wait/snapshot are bounded observations, not screenshots. Full output variants remain unresolved.";
        result.push(c);
    }
    for verb in ["list", "wait", "cancel"] {
        let mut operands = vec!["--surface", "1"];
        if verb == "cancel" {
            operands.push("00000000-0000-0000-0000-000000000001");
        }
        let mut args = target_arguments();
        args.retain(|a| a.name != "expected_model_revision");
        match verb {
            "list" => args.extend([
                option(
                    "limit",
                    &["--limit"],
                    ValueKind::Integer(1, downloads::MAX_HISTORY_PER_SURFACE as u64),
                ),
                option("json", &["--json"], ValueKind::Flag),
            ]),
            "wait" => args.extend([
                option("path", &["--path"], ValueKind::Path),
                option("timeout_ms", &["--timeout-ms"], ValueKind::SignedInteger),
                option("timeout_seconds", &["--timeout"], ValueKind::Number),
            ]),
            _ => args.push(positional("download_id", ValueKind::Uuid, true)),
        }
        let mut c = contract(
            &format!("browser download {verb}"),
            &operands,
            args,
            "downloads",
        );
        c.route = "browser_download";
        c.expected.push(("/request/action/operation", verb));
        c.defaults = if verb == "wait" {
            json!({"wait_timeout_ms": downloads::DEFAULT_WAIT_MS, "timeout_clamp_ms": [1, downloads::MAX_WAIT_MS]})
        } else {
            json!({})
        };
        c.limitations = "List is non-consuming history; no-path wait consumes a terminal event via a repeatable ticket. --path observes a nonempty file only, never download provenance or a save destination. Relative paths use current directory. Wait numbers clamp, they do not cancel work. ls aliases list; omitted verb means wait. No --expect-revision support.";
        result.push(c);
    }
    result
}

fn failure(message: impl Into<String>) -> CliError {
    CliError(format!("command contract drift: {}", message.into()))
}
fn parsed(c: &Contract) -> Result<(Value, u128), CliError> {
    let (parsed, _, timeout, _) = crate::parse(c.probe.iter().map(OsString::from).collect())
        .map_err(|error| failure(format!("{}: {}", c.name, error)))?;
    let (route, command) = match parsed {
        Parsed::App(command) => ("app", command),
        Parsed::Browser(command, _) => ("browser", command),
        Parsed::BrowserDownload(command, _) => ("browser_download", command),
        _ => {
            return Err(failure(format!(
                "{} no longer parses to its app request",
                c.name
            )));
        }
    };
    if route != c.route {
        return Err(failure(format!("{} changed its execution route", c.name)));
    }
    let request = serde_json::to_value(command).map_err(|e| failure(e.to_string()))?;
    for (path, expected) in &c.expected {
        if request.pointer(path).and_then(Value::as_str) != Some(expected) {
            return Err(failure(format!("{} expected {path}={expected}", c.name)));
        }
    }
    Ok((request, timeout.as_millis()))
}
pub(super) fn errors() -> Value {
    json!({"success_exit_status": 0, "failure_exit_status": 1, "stderr": "text prefixed with zmux:",
        "machine_readable_error_json": false, "server_error_rendering": "code: message",
        "timeout_outcome": "may_be_unknown; never automatically replay a mutation",
        "taxonomy_status": "partial; parser, transport, authorization, stale-target and server failures are not one complete stable code enum"})
}
pub(super) fn globals() -> Value {
    json!({"placement": "before_command", "parameters": [
        {"forms": ["--socket"], "type": "path", "default": "explicit flag, ZMUX_SOCKET_PATH, ZMUX_SOCKET, platform default; offline discovery does not validate/connect"},
        {"forms": ["--password"], "type": "secret", "default": "flag, ZMUX_SOCKET_PASSWORD, private credential file; never emitted by discovery"},
        {"forms": ["--timeout"], "type": "integer_milliseconds", "minimum": MIN_TIMEOUT_MS, "maximum": MAX_TIMEOUT_MS, "default": crate::DEFAULT_TIMEOUT.as_millis()},
        {"forms": ["--help", "-h"], "type": "offline_help"}, {"forms": ["--version", "-V"], "type": "offline_version"}],
        "notes": "Browser wait/download may derive a different default deadline; inspect parser_defaults on that leaf. Post-verb --timeout for download is seconds, unlike the global milliseconds option."})
}
pub(super) fn validated() -> Result<(Vec<Value>, Value), CliError> {
    let mut names = HashSet::new();
    for command in COMMANDS {
        for name in std::iter::once(&command.name).chain(command.aliases.iter()) {
            if !names.insert(*name) {
                return Err(failure(format!("duplicate dispatcher/alias {name}")));
            }
        }
    }
    let mut leaves = Vec::new();
    let mut local = HashSet::new();
    for c in definitions() {
        if !local.insert(c.name.clone()) {
            return Err(failure(format!("duplicate leaf {}", c.name)));
        }
        let root = c
            .name
            .split_whitespace()
            .next()
            .expect("nonempty definition");
        if find(root).is_none_or(|spec| spec.name != root) {
            return Err(failure(format!("{} has no live dispatcher", c.name)));
        }
        let (request, timeout) = parsed(&c)?;
        let mut defaults = c.defaults;
        defaults["client_timeout_ms"] = json!(timeout);
        for (name, path) in c.default_pointers {
            defaults[name] = request
                .pointer(path)
                .ok_or_else(|| failure(format!("{} lost default {path}", c.name)))?
                .clone();
        }
        leaves.push(json!({"name": c.name, "dispatcher": root, "availability": {"state": "implemented", "requires": c.requirements},
            "contract_status": "partial", "arguments": c.arguments.iter().map(Argument::json).collect::<Vec<_>>(),
            "argument_rules": c.argument_rules,
            "focus": c.focus, "output": {"format": c.output, "schema_status": "partial"},
            "parser_defaults": defaults, "default_source": "live parser probe or shared runtime constant; not behavioral execution",
            "parser_probe": {"argv": c.probe, "request": request}, "limitations": c.limitations}));
    }
    let inventory: Value = serde_json::from_str(INVENTORY).map_err(|e| failure(e.to_string()))?;
    let reconciliation: Value =
        serde_json::from_str(RECONCILIATION).map_err(|e| failure(e.to_string()))?;
    let coverage: Value = serde_json::from_str(COVERAGE).map_err(|e| failure(e.to_string()))?;
    if coverage["schema_version"] != 1 || coverage["behavioral_parity_verified"] != false {
        return Err(failure(
            "coverage must preserve the versioned unverified compatibility boundary",
        ));
    }
    if coverage["baseline_cmux_sha"] != inventory["baseline_cmux_sha"] {
        return Err(failure("frozen baseline changed"));
    }
    let rows = coverage["entries"]
        .as_array()
        .ok_or_else(|| failure("coverage entries missing"))?;
    let mut mapped = BTreeMap::new();
    for row in rows {
        let id = row["inventory_id"]
            .as_str()
            .ok_or_else(|| failure("coverage ID missing"))?;
        if mapped.insert(id, row).is_some() {
            return Err(failure(format!("duplicate coverage ID {id}")));
        }
        if let Some(name) = row["local_contract"].as_str() {
            if !local.contains(name) {
                return Err(failure(format!(
                    "{id} refers to stale local contract {name}"
                )));
            }
        } else if !row["local_contract"].is_null() {
            return Err(failure(format!("{id} invalid local contract")));
        }
    }
    let owners: BTreeMap<_, _> = reconciliation["commands"]
        .as_array()
        .ok_or_else(|| failure("review owner inventory missing"))?
        .iter()
        .filter_map(|row| Some((row["id"].as_str()?, row["review"].as_str()?)))
        .collect();
    let inventory_rows = inventory["commands"]
        .as_array()
        .ok_or_else(|| failure("frozen commands missing"))?;
    let mut seen = HashSet::new();
    for row in inventory_rows {
        let id = row["id"]
            .as_str()
            .ok_or_else(|| failure("frozen command ID missing"))?;
        if !seen.insert(id) {
            return Err(failure(format!("duplicate frozen ID {id}")));
        }
        let mapped = mapped
            .remove(id)
            .ok_or_else(|| failure(format!("unmapped frozen command {id}")))?;
        if mapped["reference_command"] != row["command"]
            || mapped["review_owner"].as_str() != owners.get(id).copied()
        {
            return Err(failure(format!(
                "{id} frozen spelling or review owner changed"
            )));
        }
        if mapped["local_contract"].is_null() && mapped["status"] != "unresolved" {
            return Err(failure(format!(
                "{id} missing contract is not disclosed as unresolved"
            )));
        }
        if !mapped["local_contract"].is_null()
            && mapped["status"] != "local_counterpart_compatibility_unverified"
        {
            return Err(failure(format!("{id} mapping overstates compatibility")));
        }
    }
    if !mapped.is_empty() {
        return Err(failure(
            "coverage contains IDs absent from frozen inventory",
        ));
    }
    Ok((leaves, coverage))
}
