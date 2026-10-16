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
    for (name, input) in [("pr", Some("17")), ("pr clear", None)] {
        let mut operands = vec![];
        if let Some(input) = input {
            operands.push(input);
        }
        operands.extend(["--workspace", "1"]);
        let mut arguments = target_arguments();
        arguments.retain(|a| !matches!(a.name, "pane" | "surface"));
        if input.is_some() {
            arguments.insert(0, positional("url_or_number", ValueKind::Text, true));
        }
        let mut c = contract(name, &operands, arguments, "pull_request");
        c.route = "pull_request";
        c.requirements = if input.is_some() {
            &["running_app", "local_git_worktree", "gh_authentication"]
        } else {
            &["running_app"]
        };
        c.defaults = json!({"implicit_focus_target":false,"lookup_stream_bytes":65536,"state":"transient_manual_link"});
        c.limitations = "Explicit workspace or proven caller required; pin workspace/window UUIDs across lookup. Set verifies canonical GitHub PR against local Git origin or its gh-verified fork parent and a local terminal worktree. Bare numbers use the parent for forks. Clear only removes the manual link, without Git/gh. Parser probe describes preflight, not tool execution. TTY/unique-worktree target inference, auto-refresh and full historical compatibility remain unresolved.";
        result.push(c);
    }
    for (name, operand, action) in [
        ("todo add", Some("example"), "add_todo"),
        ("todo list", None, "list_todos"),
        ("todo check", Some("1"), "set_todo_state"),
        ("todo uncheck", Some("1"), "set_todo_state"),
        ("todo start", Some("1"), "set_todo_state"),
        ("todo rm", Some("1"), "remove_todo"),
        ("todo clear", None, "clear_todos"),
    ] {
        let mut operands = Vec::new();
        let mut arguments = target_arguments();
        arguments.retain(|argument| !matches!(argument.name, "pane" | "surface"));
        if let Some(operand) = operand {
            operands.push(operand);
            arguments.insert(
                0,
                positional(
                    if name == "todo add" {
                        "text"
                    } else {
                        "item_uuid_or_one_based_position"
                    },
                    ValueKind::Text,
                    true,
                ),
            );
        }
        arguments.push(option("json", &["--json"], ValueKind::Flag));
        if name == "todo add" {
            arguments.push(option(
                "state",
                &["--state"],
                ValueKind::Choice(&["pending", "in-progress", "completed"]),
            ));
            arguments.push(option(
                "origin",
                &["--origin"],
                ValueKind::Choice(&["user", "agent"]),
            ));
        }
        operands.extend(["--workspace", "1"]);
        let mut c = contract(name, &operands, arguments, "targeted");
        c.expected
            .push(("/request/operation/operation", "metadata"));
        c.expected
            .push(("/request/operation/action/action", action));
        c.defaults =
            json!({"implicit_focus_target": false, "max_items": 50, "lifetime": "transient"});
        if name == "todo add" {
            c.default_pointers
                .push(("state", "/request/operation/action/state"));
            c.default_pointers
                .push(("origin", "/request/operation/action/origin"));
        }
        c.limitations = "Ordered shared metadata records; item UUID or current one-based position resolved within the target workspace. Explicit workspace/proven caller required; no focus fallback. List is read-only. Mutations preserve other metadata. Public caller default does not explicitly specify absent-caller behavior, so local rejection remains a compatibility gap. Persistence, edit/set/reorder/open and historical text/schema/default compatibility remain unresolved.";
        result.push(c);
    }
    let mut current = contract(
        "current",
        &[],
        vec![
            option("limit", &["--limit"], ValueKind::Integer(1, 200)),
            option("json", &["--json"], ValueKind::Flag),
        ],
        "current",
    );
    current.defaults = json!({"limit":100});
    current.default_pointers.push(("limit", "/limit"));
    current.limitations = "One cached app-wide publication; no target/caller filtering or mutation. Canonical surfaces bounded1..200; metadata and attention are counts, active tasks and browser activity unknown. Local child PID/lifecycle never proves remote work or liveness. Cloud/resources/transcripts/external repositories and historical text/schema compatibility remain unresolved.";
    result.push(current);
    let mut surface_order = contract(
        "reorder-surface",
        &["surface:1", "--index", "0"],
        {
            let mut args = target_arguments();
            args.extend(focus_arguments());
            args.extend([
                positional("subject", ValueKind::Target, false),
                option("index", &["--index"], ValueKind::SignedInteger),
                option(
                    "before",
                    &["--before", "--before-surface"],
                    ValueKind::Target,
                ),
                option("after", &["--after", "--after-surface"], ValueKind::Target),
            ]);
            args
        },
        "surface_order",
    );
    surface_order.route = "surface_order";
    surface_order.focus = "conditional_on_focus_argument";
    surface_order.output = "json_order_committed_with_separate_native_focus";
    surface_order.defaults = json!({"focus":false});
    surface_order
        .default_pointers
        .push(("focus", "/request/focus"));
    surface_order.argument_rules.push(json!({"exactly_one":["index","before","after"], "subject":"one positional or --surface; legacy [SURFACE] INDEX retained", "mutually_exclusive":["focus","no_focus"]}));
    surface_order.limitations = "Same pane only. Signed final index clamps; both self anchors are no-ops. Default focus false also applies to legacy [SURFACE] INDEX. Native focus failure leaves committed order; never replay after uncertainty. Full frozen text/error/protocol compatibility remains unverified.";
    result.push(surface_order);
    let mut single = contract(
        "reorder-workspace",
        &["workspace:1", "--index", "0"],
        vec![
            positional("workspace", ValueKind::Target, false),
            option("workspace_option", &["--workspace"], ValueKind::Target),
            option("index", &["--index"], ValueKind::SignedInteger),
            option(
                "before",
                &["--before", "--before-workspace"],
                ValueKind::Target,
            ),
            option(
                "after",
                &["--after", "--after-workspace"],
                ValueKind::Target,
            ),
            option("window", &["--window"], ValueKind::Target),
            option("dry_run", &["--dry-run"], ValueKind::Flag),
            option(
                "expected_model_revision",
                &["--expect-revision"],
                ValueKind::Integer(0, u64::MAX),
            ),
        ],
        "workspace_order",
    );
    single.argument_rules.push(json!({"exactly_one":["index","before","after"], "workspace":"one positional or --workspace", "index":"signed i64; final zero-based index clamps"}));
    single
        .default_pointers
        .push(("dry_run", "/request/dry_run"));
    single.defaults = json!({"dry_run":false});
    single.expected.push(("/request/order/operation", "single"));
    single.limitations = "Ungrouped canonical window order only. index:N targets are one-based within the selected window; final --index is zero-based and clamps. After-self advances one place, clamped. Full text/error/protocol and pin/group parity unresolved.";
    result.push(single);
    let mut bulk = contract(
        "reorder-workspaces",
        &["--order", "workspace:1,workspace:2"],
        vec![
            option("order", &["--order"], ValueKind::Text),
            option("window", &["--window"], ValueKind::Target),
            option("dry_run", &["--dry-run"], ValueKind::Flag),
            option(
                "expected_model_revision",
                &["--expect-revision"],
                ValueKind::Integer(0, u64::MAX),
            ),
        ],
        "workspace_order",
    );
    bulk.arguments[0].required = true;
    bulk.default_pointers.push(("dry_run", "/request/dry_run"));
    bulk.defaults = json!({"dry_run":false});
    bulk.expected.push(("/request/order/operation", "bulk"));
    bulk.limitations = "Stable prefix order in one ungrouped window; no pin/group model. Empty/duplicate/cross-window entries reject atomically. Full output/error/protocol parity unresolved.";
    result.push(bulk);
    let mut movement = contract(
        "move-workspace-to-window",
        &["--workspace", "workspace:1", "--window", "window:2"],
        vec![
            option("workspace", &["--workspace"], ValueKind::Target),
            option("destination_window", &["--window"], ValueKind::Target),
            option("no_focus", &["--no-focus"], ValueKind::Flag),
            option("focus", &["--focus"], ValueKind::Choice(&["true", "false"])),
            option(
                "expected_model_revision",
                &["--expect-revision"],
                ValueKind::Integer(0, u64::MAX),
            ),
        ],
        "targeted",
    );
    movement.arguments[0].required = true;
    movement.arguments[1].required = true;
    movement.defaults = json!({"focus":false});
    movement
        .default_pointers
        .push(("focus", "/request/operation/action/focus"));
    movement
        .argument_rules
        .push(json!({"mutually_exclusive":["focus", "no_focus"]}));
    movement.route = "workspace_move";
    movement.expected.push((
        "/request/operation/action/action",
        "move_workspace_to_window",
    ));
    movement.focus = "conditional_on_focus_argument";
    movement.output = "json_topology_committed_with_separate_native_focus";
    movement.limitations = "Two existing main windows. Required source and destination; no groups/canvas. Same-window no-op parity remains unresolved. Native focus failure does not undo committed topology; never replay a move after uncertainty. Split ratios are native preferences, not session-export geometry.";
    result.push(movement);
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
    for operation in ["in", "out", "reset", "<factor>"] {
        let mut c = contract(
            &format!("browser zoom {operation}"),
            &["--surface", "1"],
            target_arguments(),
            "browser",
        );
        if operation == "<factor>" {
            c.probe = ["browser", "zoom", "1.25", "--surface", "1"]
                .into_iter()
                .map(String::from)
                .collect();
            c.arguments
                .push(positional("factor", ValueKind::Number, true));
        }
        c.route = "browser";
        c.requirements = &["running_app", "native_webkit_host"];
        c.expected.push(("/request/action/action", "zoom"));
        c.expected.push((
            "/request/action/operation/kind",
            if operation == "<factor>" {
                "set"
            } else {
                operation
            },
        ));
        c.default_pointers
            .push(("operation_timeout_ms", "/request/timeout_ms"));
        c.argument_rules
            .push(json!({"finite_factor_clamp": [browser::MIN_PAGE_ZOOM, browser::MAX_PAGE_ZOOM]}));
        c.limitations = "Live public WKWebView page zoom mutation; read-only clients reject. In/out step 0.1; explicit finite factors and step results clamp to 0.25..5. Reset selects 1; reset to a configured default is unresolved. Completion reports the actual native factor and exact host/document identity. Preserves selection; no persistence, viewport emulation or replay after uncertainty. Historical compatibility remains unverified.";
        result.push(c);
    }
    let mut click = contract(
        "browser click",
        &["--surface", "1", "--selector", "#example"],
        target_arguments(),
        "browser",
    );
    click.route = "browser";
    click.requirements = &["running_app", "native_webkit_host"];
    click.expected.push(("/request/action/action", "click"));
    click.arguments.extend([
        option("selector", &["--selector"], ValueKind::Text),
        positional("selector_operand", ValueKind::Text, false),
        option("snapshot_after", &["--snapshot-after"], ValueKind::Flag),
    ]);
    click
        .argument_rules
        .push(json!({"exactly_one": ["selector", "selector_operand"]}));
    click.default_pointers.extend([
        ("snapshot_after", "/request/action/snapshot_after"),
        ("operation_timeout_ms", "/request/timeout_ms"),
    ]);
    click.limitations = "Synthetic DOM pointer/mouse sequence followed by one DOM click activation; not a trusted OS gesture. First CSS match or exact document ref. Native disabled property rejects before events. Scrolls nearest, focuses the DOM element without selecting its app tab; down-event cancellation does not suppress the later activation, while click cancellation controls the native default. Completion establishes dispatch only, not navigation readiness or successful default action. Unknown navigation/deadline outcomes never replay. Optional snapshot-after reads once after activation; read-only sockets deny. Full historical compatibility remains unverified.";
    result.push(click);
    for (name, operation, semantics) in [
        (
            "browser dblclick",
            "dblclick",
            "Two complete click attempts followed by a synthetic detail-2 dblclick. Disabled targets reject; nearest scroll. Completion proves dispatch, not a default action or navigation readiness.",
        ),
        (
            "browser hover",
            "hover",
            "Nearest scroll plus pointer/mouse over, enter and move only; no activation or DOM focus. Disabled targets are accepted. Synthetic events do not establish CSS :hover or trusted pointer input.",
        ),
        (
            "browser focus",
            "focus",
            "Ordinary element.focus when available, including its default scrolling. No disabled guard; completion does not promise the element was focusable. This is DOM focus, distinct from native tab/webview focus.",
        ),
        (
            "browser check",
            "check",
            "Requires a checked property and non-disabled target. Nearest scroll and DOM focus precede one DOM click activation if state differs; same state emits no activation/input/change. Native checkbox/radio defaults apply. Final state is verified; canceled activation fails not_changed without forced assignment or replay.",
        ),
        (
            "browser uncheck",
            "uncheck",
            "Requires a checked property and non-disabled target. Nearest scroll and DOM focus precede mutation only if state differs. Checkbox uses one DOM click; radio uses the inherited native checked setter and input/change without click. Final state is verified; cancellation fails not_changed without replay.",
        ),
        (
            "browser scroll-into-view",
            "scroll_into_view",
            "Centers both axes with instant scrollIntoView; no synthetic focus or activation. Aliases scrollintoview and scrollinto use the same operation.",
        ),
        (
            "browser scrollintoview",
            "scroll_into_view",
            "Alias of browser scroll-into-view: center both axes with instant scrolling.",
        ),
        (
            "browser scrollinto",
            "scroll_into_view",
            "Alias of browser scroll-into-view: center both axes with instant scrolling.",
        ),
    ] {
        let mut c = contract(
            name,
            &["--surface", "1", "--selector", "#example"],
            target_arguments(),
            "browser",
        );
        c.route = "browser";
        c.requirements = &["running_app", "native_webkit_host"];
        c.expected.extend([
            ("/request/action/action", "interact"),
            ("/request/action/operation", operation),
        ]);
        c.arguments.extend([
            option("selector", &["--selector"], ValueKind::Text),
            positional("selector_operand", ValueKind::Text, false),
            option("snapshot_after", &["--snapshot-after"], ValueKind::Flag),
        ]);
        c.argument_rules
            .push(json!({"exactly_one": ["selector", "selector_operand"]}));
        c.default_pointers.extend([
            ("snapshot_after", "/request/action/snapshot_after"),
            ("operation_timeout_ms", "/request/timeout_ms"),
        ]);
        c.limitations = semantics;
        result.push(c);
    }
    for (name, action, field, semantics) in [
        (
            "browser type",
            "type",
            "text",
            "Append one admitted trimmed nonblank chunk to the entire current value/textContent, ignoring caret selection. Ordinary DOM focus may scroll. Cancelable beforeinput insertText precedes inherited native setter and input/change for value fields; other elements receive input only. No disabled/readonly guard and no keyboard or trusted input claim. Explicit text overrides positional words. Read-only sockets deny; uncertain outcomes never replay.",
        ),
        (
            "browser select",
            "select",
            "value",
            "Trimmed nonblank value is assigned through the inherited native setter to any value-bearing element, then input/change always fire, even unchanged. No focus, disabled guard, beforeinput or final option-match validation; an absent option may leave an empty value. Read-only sockets deny; uncertain outcomes never replay.",
        ),
    ] {
        let flag = if field == "text" { "--text" } else { "--value" };
        let mut c = contract(
            name,
            &["--surface", "1", "--selector", "#example", flag, "example"],
            target_arguments(),
            "browser",
        );
        c.route = "browser";
        c.requirements = &["running_app", "native_webkit_host"];
        c.expected.push(("/request/action/action", action));
        c.arguments.extend([
            option("selector", &["--selector"], ValueKind::Text),
            positional("selector_operand", ValueKind::Text, false),
            option(
                field,
                if field == "text" {
                    &["--text"]
                } else {
                    &["--value"]
                },
                ValueKind::Text,
            ),
            positional("text_or_value_operand", ValueKind::Text, false),
            option("snapshot_after", &["--snapshot-after"], ValueKind::Flag),
        ]);
        c.argument_rules.extend([
            json!({"exactly_one":["selector","selector_operand"]}),
            json!({"required_nonblank":field,"trim_at_admission":true,"max_utf8_bytes":browser::MAX_INPUT_TEXT_BYTES}),
        ]);
        c.default_pointers.extend([
            ("snapshot_after", "/request/action/snapshot_after"),
            ("operation_timeout_ms", "/request/timeout_ms"),
        ]);
        c.limitations = semantics;
        result.push(c);
    }
    for (name, operation) in [
        ("browser press", "press"),
        ("browser key", "press"),
        ("browser keydown", "down"),
        ("browser keyup", "up"),
    ] {
        let mut c = contract(
            name,
            &["--surface", "1", "--key", "Enter"],
            target_arguments(),
            "browser",
        );
        c.route = "browser";
        c.requirements = &["running_app", "native_webkit_host"];
        c.expected.extend([
            ("/request/action/action", "keyboard"),
            ("/request/action/operation", operation),
        ]);
        c.arguments.extend([
            option("key", &["--key"], ValueKind::Text),
            positional("key_operand", ValueKind::Text, false),
            option("snapshot_after", &["--snapshot-after"], ValueKind::Flag),
        ]);
        c.argument_rules.extend([
            json!({"exactly_one":["key","key_operand"]}),
            json!({"max_utf8_bytes":browser::MAX_KEY_TOKEN_BYTES,"trim":false,"nul":false}),
        ]);
        c.default_pointers.extend([
            ("snapshot_after", "/request/action/snapshot_after"),
            ("operation_timeout_ms", "/request/timeout_ms"),
        ]);
        c.limitations = "Supported keys use direct native WebKit input with exact host/document ownership and no app focus change. Down/up retain bounded host-local state. Unknown opaque tokens use explicitly synthetic page events; a failed native key never downgrades. Completion establishes dispatch, not a successful page default; timeout uncertainty never authorizes replay. Full mapping, IME and native held-state lifecycle compatibility remain unverified.";
        result.push(c);
    }
    let mut scroll = contract(
        "browser scroll",
        &["--surface", "1", "--dx", "12", "--dy", "24"],
        target_arguments(),
        "browser",
    );
    scroll.route = "browser";
    scroll.requirements = &["running_app", "native_webkit_host"];
    scroll.expected.push(("/request/action/action", "scroll"));
    scroll.arguments.extend([
        option("selector", &["--selector"], ValueKind::Text),
        option("dx", &["--dx"], ValueKind::SignedInteger),
        option("dy", &["--dy"], ValueKind::SignedInteger),
        positional("dy_operand", ValueKind::SignedInteger, false),
        option("snapshot_after", &["--snapshot-after"], ValueKind::Flag),
    ]);
    scroll.argument_rules.extend([
        json!({"at_most_one":["dy","dy_operand"]}),
        json!({"exact_signed_pixel_limit":browser::MAX_SCROLL_DELTA}),
    ]);
    scroll.default_pointers.extend([
        ("snapshot_after", "/request/action/snapshot_after"),
        ("operation_timeout_ms", "/request/timeout_ms"),
    ]);
    scroll.defaults = json!({"dx":0,"dy":0,"selector":null});
    scroll.limitations = "Instant signed pixel scroll; omitted selector targets window, explicit CSS first match/exact ref targets element.scrollBy or native scrollLeft/Top fallback. Deltas default0 and must be exact integers within 9007199254740991; geometry may clamp. Does not focus or synthesize wheel events. Optional snapshot observes the same mutation. Read-only sockets deny and uncertain outcomes never replay.";
    result.push(scroll);
    let mut fill = contract(
        "browser fill",
        &["--surface", "1", "--selector", "#example"],
        target_arguments(),
        "browser",
    );
    fill.route = "browser";
    fill.requirements = &["running_app", "native_webkit_host"];
    fill.expected.push(("/request/action/action", "fill"));
    fill.arguments.extend([
        option("selector", &["--selector"], ValueKind::Text),
        positional("selector_operand", ValueKind::Text, false),
        option("text", &["--text"], ValueKind::Text),
        positional("text_operands", ValueKind::Text, false),
        option("snapshot_after", &["--snapshot-after"], ValueKind::Flag),
    ]);
    fill.argument_rules.extend([
        json!({"exactly_one": ["selector", "selector_operand"]}),
        json!({"text_max_utf8_bytes": browser::MAX_INPUT_TEXT_BYTES, "empty_text_clears": true, "explicit_text_overrides_operands": true}),
    ]);
    fill.default_pointers.extend([
        ("text", "/request/action/text"),
        ("snapshot_after", "/request/action/snapshot_after"),
        ("operation_timeout_ms", "/request/timeout_ms"),
    ]);
    fill.limitations = "Synthetic isolated-world DOM replacement, not trusted keyboard input. CSS selects its first match; document refs select their exact element. Focuses the DOM element without selecting its app tab. Cancelable beforeinput precedes native value/text replacement and input/change events; cancellation reports input_rejected. Omitted/empty text clears; positional text joins with spaces. No frame/shadow traversal or automatic replay; read-only sockets deny. Full historical compatibility remains unverified.";
    result.push(fill);
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
    let value = |command| serde_json::to_value(command).map_err(|e| failure(e.to_string()));
    let (route, request) = match parsed {
        Parsed::PullRequest(invocation) => ("pull_request", invocation.discovery()),
        Parsed::App(command) => ("app", value(command)?),
        Parsed::WorkspaceMove(command) => ("workspace_move", value(command)?),
        Parsed::SurfaceOrder(command) => ("surface_order", value(command)?),
        Parsed::Browser(command, _) => ("browser", value(command)?),
        Parsed::BrowserDownload(command, _) => ("browser_download", value(command)?),
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
