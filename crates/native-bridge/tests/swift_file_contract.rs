//! Drift guard between the Rust file channel and the Swift panel that consumes it.
//!
//! There is no Swift toolchain on Linux, so the native panel cannot be compiled or run here. What
//! can be checked is the contract it depends on: the Swift `Decodable` shapes must name exactly the
//! fields the real Rust replies and snapshots carry, the Swift frame limits must equal the Rust
//! ones, and the audit of every place that tests for the browser kind must stay current. Each
//! failure message says what to update. This does not prove the Swift compiles; macOS CI does.
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_native_bridge::*;

static SERIAL: Mutex<()> = Mutex::new(());

fn sources() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/macos/Sources")
}
fn swift(name: &str) -> String {
    std::fs::read_to_string(sources().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Stored `let` properties declared directly in `struct <name>`, nested types excluded.
fn swift_fields(source: &str, name: &str) -> BTreeSet<String> {
    let marker = format!("struct {name}");
    let mut from = 0;
    let start = loop {
        let at = source[from..]
            .find(&marker)
            .unwrap_or_else(|| panic!("`{marker}` is not declared"))
            + from;
        let next = source[at + marker.len()..].chars().next();
        if !next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            break at + marker.len();
        }
        from = at + marker.len();
    };
    let body_start = source[start..].find('{').expect("struct body") + start + 1;
    let mut depth = 1;
    let mut top = String::new();
    for c in source[body_start..].chars() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ if depth == 1 => top.push(c),
            _ => {}
        }
    }
    top.split(['\n', ';'])
        .filter_map(|piece| piece.trim().strip_prefix("let "))
        .filter_map(|rest| rest.split([':', ' ', '=']).next())
        .map(str::to_owned)
        .collect()
}

/// `JSONDecoder.keyDecodingStrategy = .convertFromSnakeCase`.
fn camel(key: &str) -> String {
    let mut parts = key.split('_');
    let mut out = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}
fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("not an object: {value}"))
        .keys()
        .map(|k| camel(k))
        .collect()
}
fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn bytes_of(id: u64) -> Vec<u8> {
    let length = zmux_buffer_length(id);
    assert_eq!(length.status, 0);
    let mut data = vec![0; length.value as usize];
    // SAFETY: the owned destination is writable for exactly its declared capacity.
    assert_eq!(
        unsafe { zmux_buffer_copy(id, data.as_mut_ptr(), data.len()) },
        0
    );
    assert_eq!(zmux_buffer_release(id), 0);
    data
}
fn snapshot(desktop: u64, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let result = zmux_desktop_snapshot(desktop, 0);
        if result.status == 0 && result.value != 0 {
            let value: Value = serde_json::from_slice(&bytes_of(result.value)).unwrap();
            if predicate(&value) {
                return value;
            }
        }
        assert!(Instant::now() < deadline, "snapshot never matched");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn call(desktop: u64, header: &Value, body: &[u8]) -> (Value, Vec<u8>) {
    let header = serde_json::to_vec(header).unwrap();
    let mut frame = (header.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&header);
    frame.extend_from_slice(body);
    // SAFETY: the request storage is live for the whole call.
    let result = unsafe { zmux_desktop_file(desktop, frame.as_ptr(), frame.len()) };
    assert_eq!(result.status, 0);
    let reply = bytes_of(result.value);
    let length = u32::from_be_bytes(reply[..4].try_into().unwrap()) as usize;
    (
        serde_json::from_slice(&reply[4..4 + length]).unwrap(),
        reply[4 + length..].to_vec(),
    )
}
fn find_file_surface(value: &Value) -> Option<&Value> {
    match value {
        Value::Object(map) => {
            if map.get("kind") == Some(&json!("file"))
                && map.get("file").is_some_and(|f| f.is_object())
            {
                return Some(value);
            }
            map.values().find_map(find_file_surface)
        }
        Value::Array(items) => items.iter().find_map(find_file_surface),
        _ => None,
    }
}

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("zmux-swift-contract-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(std::fs::canonicalize(dir).unwrap())
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn swift_decodables_name_exactly_the_fields_the_real_rust_wire_carries() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = TempDir::new();
    let path = dir.0.join("contract.md");
    std::fs::write(&path, "# contract\n").unwrap();
    let desktop = zmux_desktop_create();
    assert_eq!(desktop.status, 0);
    let desktop = desktop.value;
    snapshot(desktop, |v| {
        !v["model"]["workspaces"].as_array().unwrap().is_empty()
    });

    // open: one file item that succeeds and one that fails, so both item shapes are observed.
    let missing = dir.0.join("absent.txt");
    let (opened, _) = call(
        desktop,
        &json!({"op":"open","items":[{"kind":"path","value":path},{"kind":"path","value":missing}],"focus":true}),
        &[],
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let items = opened["result"]["items"].as_array().unwrap();
    let (ok_item, failed_item) = (&items[0], &items[1]);
    assert_eq!(ok_item["ok"], true);
    assert_eq!(failed_item["ok"], false);

    let state = snapshot(desktop, |v| {
        v["files"]["entries"]
            .as_array()
            .is_some_and(|e| !e.is_empty())
    });
    let (attached, body) = call(
        desktop,
        &json!({"op":"attach","epoch":state["files"]["epoch"],
                "surface_id":ok_item["surface"]["surface_id"],"host_id":Uuid::new_v4()}),
        &[],
    );
    assert_eq!(attached["ok"], true, "{attached}");
    assert_eq!(body, b"# contract\n");
    let owner = attached["owner"].clone();
    let (saved, _) = call(
        desktop,
        &json!({"op":"save","owner":owner,"base_generation":1,"force":false}),
        b"# mine\n",
    );
    assert_eq!(saved["ok"], true, "{saved}");
    let (observed, _) = call(
        desktop,
        &json!({"op":"observe","owner":owner,"dirty":false}),
        &[],
    );
    assert_eq!(observed["ok"], true, "{observed}");
    // Captured while the last operation is the completed save; the conflict below replaces it.
    let live = snapshot(desktop, |v| {
        v["files"]["entries"][0]["last_op"]["kind"] == "save"
            && v["files"]["entries"][0]["last_op"]["state"] == "completed"
    });
    std::fs::write(&path, "# external\n").unwrap();
    let (conflict, _) = call(
        desktop,
        &json!({"op":"save","owner":owner,"base_generation":2,"force":false}),
        b"# again\n",
    );
    assert_eq!(conflict["error"]["code"], "conflict", "{conflict}");
    let (plain_failure, _) = call(
        desktop,
        &json!({"op":"detach","owner":{"epoch":Uuid::nil(),"identity_namespace":Uuid::nil(),"surface_id":1,"host_id":Uuid::nil()}}),
        &[],
    );
    assert_eq!(plain_failure["ok"], false);

    // Snapshot shapes (`DesktopSnapshot.files`, `Surface.file`).
    let models = swift("Models.swift");
    assert_eq!(keys(&live["files"]), swift_fields(&models, "FilesSnapshot"));
    assert_eq!(
        keys(&live["files"]["worker"]),
        swift_fields(&models, "FileWorkerSnapshot")
    );
    assert_eq!(live["files"]["worker"]["state"], "ok");
    assert_eq!(
        keys(&live["files"]["entries"][0]),
        swift_fields(&models, "FileEntrySnapshot")
    );
    assert_eq!(
        keys(&live["files"]["entries"][0]["last_op"]),
        swift_fields(&models, "FileOperationReceipt")
    );
    let surface = find_file_surface(&live["model"]).expect("a file surface in the model");
    assert_eq!(keys(&surface["file"]), swift_fields(&models, "FileSurface"));
    assert!(
        swift_fields(&models, "Surface").contains("file"),
        "Swift Surface must decode `file`"
    );
    assert!(
        swift_fields(&models, "DesktopSnapshot").contains("files"),
        "DesktopSnapshot must decode `files`"
    );

    // Reply shapes (`FileHostReplyHeader` and the types it nests).
    let client = swift("FileHostClient.swift");
    assert_eq!(keys(&owner), swift_fields(&client, "FileOwnerToken"));
    let header_fields = swift_fields(&client, "FileHostReplyHeader");
    let mut observed_keys = BTreeSet::new();
    for reply in [
        &attached,
        &saved,
        &observed,
        &opened,
        &conflict,
        &plain_failure,
    ] {
        let reply_keys = keys(reply);
        assert!(
            reply_keys.is_subset(&header_fields),
            "FileHostReplyHeader lacks {:?} carried by {reply}",
            reply_keys.difference(&header_fields).collect::<Vec<_>>()
        );
        observed_keys.extend(reply_keys);
    }
    assert_eq!(
        header_fields, observed_keys,
        "FileHostReplyHeader declares fields no reply carries (or a reply kind is not covered here)"
    );
    assert_eq!(
        keys(&conflict["conflict"]),
        swift_fields(&client, "FileConflict")
    );
    assert_eq!(
        keys(&plain_failure["error"]),
        swift_fields(&client, "ErrorBody")
    );
    assert_eq!(
        keys(&opened["result"]),
        swift_fields(&client, "FileOpenResult")
    );
    let item_fields = swift_fields(&client, "FileOpenItem");
    let ignored = set(&["surface"]);
    for item in [ok_item, failed_item] {
        let item_keys = keys(item);
        let unread: BTreeSet<_> = item_keys.difference(&ignored).cloned().collect();
        assert!(
            unread.is_subset(&item_fields),
            "FileOpenItem lacks {unread:?}"
        );
    }
    let mut known = keys(ok_item);
    known.extend(keys(failed_item));
    known.insert("url".to_owned()); // URL items are routed by the CLI, never by this channel.
    assert!(
        item_fields.is_subset(&known),
        "FileOpenItem declares unknown fields"
    );
    assert_eq!(
        keys(&failed_item["error"]),
        swift_fields(&client, "Failure")
    );
    assert_eq!(zmux_desktop_destroy(desktop), 0);
}

#[test]
fn swift_frame_limits_and_operations_match_the_rust_channel() {
    let client = swift("FileHostClient.swift");
    let limit = |name: &str| -> usize {
        let line = client
            .lines()
            .find(|l| l.contains(&format!("static let {name} =")))
            .unwrap_or_else(|| panic!("FileHostLimits.{name} is missing"));
        let literal = line.split('=').nth(1).unwrap().trim().replace('_', "");
        if let Some((a, b)) = literal.split_once('+') {
            a.trim().parse::<usize>().unwrap() + b.trim().parse::<usize>().unwrap()
        } else {
            literal
                .parse()
                .unwrap_or_else(|_| panic!("{name}: {literal}"))
        }
    };
    assert_eq!(limit("maxHeader"), file_channel::MAX_FILE_HEADER_BYTES);
    assert_eq!(limit("maxFrame"), file_channel::MAX_FILE_HEADER_BYTES);
    assert_eq!(limit("maxSaveBody"), file_channel::MAX_FILE_SAVE_BODY_BYTES);
    assert_eq!(
        limit("maxSaveFrame"),
        file_channel::MAX_FILE_SAVE_FRAME_BYTES
    );
    assert_eq!(
        limit("maxTextBytes"),
        2_097_152,
        "MAX_TEXT_BYTES of the file service"
    );
    assert_eq!(
        limit("maxPreviewBytes"),
        7_340_032,
        "MAX_PREVIEW_BYTES of the file service"
    );
    assert_eq!(limit("maxOpenItems"), 16, "MAX_OPEN_ITEMS");
    // Every operation and every request key the Rust deserializer knows is written by Swift.
    for needle in [
        "\"op\": \"attach\"",
        "\"op\": \"reload\"",
        "\"op\": \"save\"",
        "\"op\": \"observe\"",
        "\"op\": \"detach\"",
        "\"op\": \"open\"",
        "\"epoch\"",
        "\"surface_id\"",
        "\"host_id\"",
        "\"owner\"",
        "\"discard\"",
        "\"base_generation\"",
        "\"force\"",
        "\"dirty\"",
        "\"items\"",
        "\"focus\"",
        "\"force_mode\"",
        "\"identity_namespace\"",
    ] {
        assert!(
            client.contains(needle),
            "FileHostClient.swift no longer writes {needle}"
        );
    }
    // The header and body are framed exactly as `file_channel::parse` expects.
    let header = json!({"op":"detach","owner":{"epoch":Uuid::nil(),"identity_namespace":Uuid::nil(),"surface_id":1,"host_id":Uuid::nil()}});
    let header = serde_json::to_vec(&header).unwrap();
    let mut frame = (header.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&header);
    assert!(file_channel::parse(&frame).is_ok());
    assert!(
        client.contains("UInt32(encoded.count).bigEndian"),
        "Swift must write a big-endian u32 length"
    );
}

#[test]
fn every_browser_kind_test_in_the_swift_sources_is_audited() {
    // Each entry is a place that tests `kind == "browser"` / `!= "browser"`, with how it treats
    // the file kind. A new site must be added here deliberately (see the CU-14 receipt audit).
    let audited: &[(&str, usize, &str)] = &[
        (
            "ApplicationCoordinator.swift",
            4,
            "browser view lifecycle only; file tabs are reconciled by reconcileFilePanels",
        ),
        (
            "App.swift",
            5,
            "header detail, presentation key and the paneView fallthrough are browser-only with separate file arms; confirmClose counts browser tabs apart from file tabs; one smoke wait",
        ),
        (
            "SurfaceOrderSmoke.swift",
            1,
            "smoke fixture selects the browser surface it created",
        ),
        (
            "WorkspaceNumberSmoke.swift",
            1,
            "smoke fixture selects the browser surface it created",
        ),
        (
            "WorkspaceOrderSmoke.swift",
            1,
            "smoke fixture selects the browser surface it created",
        ),
    ];
    let mut found = Vec::new();
    for entry in std::fs::read_dir(sources()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("swift") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let count = text.matches("kind == \"browser\"").count()
            + text.matches("kind != \"browser\"").count();
        if count > 0 {
            found.push((
                path.file_name().unwrap().to_string_lossy().into_owned(),
                count,
            ));
        }
    }
    found.sort();
    let mut expected: Vec<(String, usize)> = audited
        .iter()
        .map(|(f, n, _)| ((*f).to_owned(), *n))
        .collect();
    expected.sort();
    assert_eq!(
        found, expected,
        "a browser-kind test was added or removed in apps/macos/Sources: decide how it treats the \
         file kind, then update this audit and the CU-14 receipt"
    );
    // The explicit file arm precedes the old "everything else is a browser" fallthrough.
    let app = swift("App.swift");
    let file_arm = app
        .find("pane.selected.kind == \"file\" {")
        .expect("paneView file arm");
    let fallthrough = app
        .find("pane.selected.kind != \"browser\" {")
        .expect("paneView browser fallthrough");
    assert!(
        file_arm < fallthrough,
        "paneView must test the file kind before the browser fallthrough"
    );
    assert!(
        swift("WorkspaceViews.swift").contains("case \"file\":"),
        "tab icons must handle the file kind"
    );
    // The pane key must not carry dirty state: a rebuild on the first keystroke drops focus.
    let models = swift("Models.swift");
    let key = &models[models.find("var key: String").expect("PaneTree.key")..];
    let key = &key[..key.find("var geometryKey").expect("geometryKey")];
    assert!(
        !key.contains("dirty"),
        "PaneTree.key must not include dirty state"
    );
}

/// The text of `source` from the first `start` to the next `end` after it.
fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let from = source
        .find(start)
        .unwrap_or_else(|| panic!("`{start}` not found in the Swift sources"));
    let rest = &source[from..];
    let to = rest[start.len()..]
        .find(end)
        .unwrap_or_else(|| panic!("`{end}` not found after `{start}`"));
    &rest[..start.len() + to]
}

// The Swift cannot run on this machine, so these pin the structure that the native fixes depend
// on. Each assertion fails if the corresponding fix is reverted; the macOS in-app smoke
// (`FilePanelSmoke`) is the behavioural check and has not run here.

#[test]
fn close_requests_are_queued_never_dropped() {
    let app = swift("App.swift");
    assert!(
        !app.contains("closePromptActive"),
        "a single in-flight flag silently dropped every close made while another was pending"
    );
    let body = between(
        &app,
        "private func closeAfterFileCheck(",
        "private func drainFileCloses",
    );
    assert!(
        body.contains("pendingFileCloses.append"),
        "requests must be queued"
    );
    assert!(
        body.contains("maxPendingFileCloses") && body.contains("showError("),
        "the queue is bounded and refuses visibly"
    );
    // Every early return in the request path is either a synchronous proceed, a folded repeat of
    // the same gesture or the visible refusal.
    for line in body.lines().filter(|l| l.contains("return")) {
        let line = line.trim();
        assert!(
            line.starts_with(
                "guard !pendingFileCloses.contains(where: { $0.key == key }) else { return }"
            ) || line.contains("proceed(); return")
                || line == "return",
            "unexpected silent return in closeAfterFileCheck: {line}"
        );
    }
    let drain = between(
        &app,
        "private func drainFileCloses",
        "func refreshFileChrome",
    );
    assert!(
        drain.contains("while let host = self")
            && drain.contains("pendingFileCloses.removeFirst()"),
        "the queue must be drained until empty, one request at a time"
    );
    // Every call site names the gesture so a repeat folds into it and a different one queues.
    let calls = app.matches("closeAfterFileCheck(").count();
    let keyed = app.matches("key: \"").count();
    assert_eq!(calls, 6, "one definition and five call sites expected");
    assert_eq!(keyed, 5, "every call site must pass a gesture key");
    let smoke = swift("FilePanelSmoke.swift");
    assert!(
        smoke.contains("host.closeTerminalTab(markdownID)")
            && smoke.contains("host.closeTerminalTab(reopenedID)")
            && smoke.contains("hasPendingFileCloses"),
        "the smoke must close several tabs back to back and wait for the queue to settle"
    );
}

#[test]
fn keyboard_focus_follows_content_that_replaces_the_loading_state() {
    let panel = swift("FilePanelView.swift");
    let present = between(
        &panel,
        "private func present(_ view: NSView)",
        "private func showLoading",
    );
    let decided = present
        .find("ownsFocus(in:")
        .expect("present must decide focus");
    let removed = present
        .find("removeFromSuperview()")
        .expect("present swaps views");
    assert!(
        decided < removed,
        "focus ownership must be read before the outgoing view leaves the window"
    );
    assert!(
        present.contains("makeFirstResponder(preferredResponder)"),
        "focus must move to the new content"
    );
    assert!(
        swift("FilePanelSmoke.swift")
            .contains("keyboard focus moved into the rendered Markdown view"),
        "the smoke must cover a tab selected while it loads"
    );
}

/// Index of `needle` in `haystack`, or a panic naming what is missing.
fn at(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` is missing from:\n{haystack}"))
}

// The close decision is executed for real by `scripts/check-file-close-resolution` on macOS CI.
// There is no Swift toolchain here, so the tests below can only pin the exact statements the
// decision is made of. They do not run it.
#[test]
fn close_resolution_aborts_without_discarding_and_applies_dont_save_last() {
    let resolution = swift("FileCloseResolution.swift");
    let body = between(
        &resolution,
        "static func resolve<Panel: FileCloseParticipant>(",
        "\n    }\n}",
    );
    let cancel = at(body, "case .cancel: return false");
    let save = at(
        body,
        "case .save: guard await panel.performSave(force: false) else { return false }",
    );
    let discard = at(body, "case .discard: discards.append(panel)");
    let hold = at(body, "discards.forEach { $0.holdDiscard() }");
    let settle = at(
        body,
        "guard await panel.settleDirtyMirror(assumingDiscarded: discarded) else {",
    );
    let release = at(body, "discards.forEach { $0.releaseDiscard() }");
    let resume = at(body, "panels.forEach { $0.resumeDirtyMirror() }");
    let apply = at(body, "discards.forEach { $0.applyDiscard() }");
    assert!(
        cancel < hold && save < hold && discard < hold,
        "every Cancel and Save decision must be made before any panel is held"
    );
    assert!(
        hold < settle,
        "panels must be held before the first awaited settle"
    );
    assert!(
        !body[hold..settle].contains("await"),
        "nothing may be awaited between holding the discarded panels and the first settle"
    );
    assert!(
        settle < release && release < resume,
        "a failed settle must lift the holds, then resume the mirrors"
    );
    let failure_arm = &body[settle..apply];
    assert!(
        failure_arm.contains("return false"),
        "a failed settle must abort the close"
    );
    assert!(
        resume < apply && apply == body.rfind("discards.forEach").unwrap(),
        "Don't Save is applied last, after every panel settled"
    );
    assert_eq!(
        body.matches("return false").count(),
        3,
        "exactly three abort paths: Cancel, a failed Save and a failed settle"
    );
    assert_eq!(
        body.matches("holdDiscard()").count()
            + body.matches("applyDiscard()").count()
            + body.matches("releaseDiscard()").count(),
        3,
        "holds are taken, applied and released in one place each"
    );
    assert!(
        !body[..hold].contains("Discard()") && !body.contains("discardLocalChanges"),
        "the per-panel decision loop must not touch dirty tracking"
    );
    // The coordinator only asks the question; it cannot reorder the decision.
    let coordinator = swift("ApplicationCoordinator.swift");
    let asks = between(
        &coordinator,
        "func resolveFilesBeforeClose(",
        "/// Quit: resolves every dirty panel",
    );
    assert!(
        asks.contains("FileCloseResolution.resolve(filePanels(for: surfaces))")
            && !asks.contains("discardLocalChanges")
            && !asks.contains("settleDirtyMirror")
            && !asks.contains("performSave"),
        "the coordinator must delegate the decision to FileCloseResolution"
    );
    assert!(
        swift("FilePanelView.swift").contains("extension FilePanelView: FileCloseParticipant {}"),
        "the production panel must be the participant the fixture's logic runs against"
    );
    let smoke = swift("FilePanelSmoke.swift");
    assert!(
        smoke.contains("CloseScript([.discard, .cancel])")
            && smoke.contains("CloseScript([.discard, .discard])")
            && smoke.contains("both models stay clean after Don't Save"),
        "the smoke must cancel after a Don't Save and settle two Don't Saves together"
    );
}

#[test]
fn the_close_decision_fixture_is_wired_into_macos_ci() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |path: &str| {
        std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
    };
    let script = read("scripts/check-file-close-resolution");
    assert!(
        script.contains("apps/macos/Sources/FileCloseResolution.swift")
            && script.contains("tools/native-fixtures/file-close-resolution.swift")
            && script.contains("-warnings-as-errors"),
        "the script must compile the production decision with the fixture"
    );
    assert!(
        read(".github/workflows/macos-development.yml")
            .contains("scripts/check-file-close-resolution"),
        "macOS CI must run the fixture; it cannot run on this machine"
    );
    let fixture = read("tools/native-fixtures/file-close-resolution.swift");
    for scenario in [
        "two Don't Saves must close",
        "Cancel after Don't Save must leave every panel exactly as it was",
        "a failed Save must abort the close",
        "a failed settle must abort the close",
        "control: without a hold",
    ] {
        assert!(
            fixture.contains(scenario),
            "fixture scenario `{scenario}` is missing"
        );
    }
}

#[test]
fn a_panel_being_discarded_cannot_restart_the_dirty_mirror() {
    let resolution = swift("FileCloseResolution.swift");
    let gate = between(
        &resolution,
        "func observationNeeded(",
        "/// Whether the close-time sync",
    );
    assert!(
        gate.contains(
            "attached && editable && !closed && !gaveUp && !isDiscarding && isDirty != modelDirty"
        ),
        "the gate must refuse observation while a discard is held: {gate}"
    );
    let panel = swift("FilePanelView.swift");
    let needed = between(&panel, "private var observeNeeded: Bool {", "}\n");
    assert!(
        needed.contains("dirtyGate.observationNeeded("),
        "observeNeeded must be decided by the gate"
    );
    assert_eq!(
        panel.matches("observeTask = Task").count(),
        1,
        "the only place that starts an observation is reconcileDirty"
    );
    let reconcile = between(
        &panel,
        "private func reconcileDirty() {",
        "observeLoopID += 1",
    );
    assert!(
        reconcile.contains("guard observeNeeded, observeTask == nil else { return }"),
        "reconcileDirty must consult observeNeeded"
    );
    // Both the snapshot path and the loop's own tail only restart through reconcileDirty.
    let update = between(
        &panel,
        "func update(surface: Surface",
        "private func epochChanged()",
    );
    assert!(update.contains("reconcileDirty()"));
    let hold = between(&panel, "func holdDiscard() {", "}\n");
    assert!(
        hold.contains("dirtyGate.hold()")
            && hold.contains("observeTask?.cancel()")
            && !hold.contains("await"),
        "holdDiscard must hold and cancel synchronously"
    );
    let apply = between(&panel, "func applyDiscard() {", "}\n");
    assert!(
        at(apply, "discardLocalChanges()") < at(apply, "dirtyGate.release()")
            && !apply.contains("await"),
        "the hold is lifted only after the text is dropped from dirty tracking, in one synchronous step"
    );
    let release = between(&panel, "func releaseDiscard() {", "}\n");
    assert!(
        release.contains("dirtyGate.release()") && release.contains("reconcileDirty()"),
        "an aborted close must lift the hold and resume the mirror"
    );
}

#[test]
fn closing_waits_for_an_in_flight_observation_before_writing() {
    let panel = swift("FilePanelView.swift");
    let settle = between(
        &panel,
        "func settleDirtyMirror(assumingDiscarded: Bool = false) async -> Bool {",
        "/// Restarts the dirty-mirror observation",
    );
    let cancel = at(settle, "running?.cancel()");
    let wait = at(settle, "if let running { await running.value }");
    let decide = at(settle, "DirtyMirrorGate.closeWriteNeeded(");
    let write = at(settle, "send(.observe(token, dirty: wanted))");
    assert!(
        cancel < wait && wait < decide && decide < write,
        "the close must cancel, wait out the old write, then decide and send its own"
    );
    assert!(
        settle.contains("loopMayHaveWritten: running != nil || assumingDiscarded"),
        "a held panel's loop was cancelled earlier, so its write must be assumed"
    );
    let resolution = swift("FileCloseResolution.swift");
    assert!(
        resolution.contains("loopMayHaveWritten || wanted != modelDirty"),
        "a possibly landed write must force a fresh write"
    );
}

#[test]
fn dirty_detection_compares_utf8_bytes_not_canonical_equivalence() {
    let panel = swift("FilePanelView.swift");
    for forbidden in [
        "editor.string != baseline",
        "editor.string != text",
        "editor.string == baseline",
        "editor.string == text",
    ] {
        assert!(
            !panel.contains(forbidden),
            "`{forbidden}` uses Swift String equality, which ignores normalization form"
        );
    }
    let helper = between(&panel, "private static func sameBytes", "}");
    assert!(
        helper.contains("lhs.utf8.elementsEqual(rhs.utf8)"),
        "sameBytes must compare UTF-8 code units"
    );
    assert_eq!(
        panel.matches("Self.sameBytes(").count(),
        2,
        "both the keystroke path and the post-save path must use sameBytes"
    );
}

#[test]
fn the_dirty_observation_never_gives_up_after_a_count() {
    let panel = swift("FilePanelView.swift");
    let success = between(
        &panel,
        "case .success:\n                delay = 250",
        "case .failure(let failure):\n                if failure.isRetryableObservation",
    );
    assert!(
        !success.contains("observeGaveUp"),
        "an acknowledged-but-unconfirmed observation must keep retrying, not stop for good"
    );
    assert!(
        success.contains("confirmDelay = min(confirmDelay * 2, 2_000)")
            && success.contains(".dirtyUnconfirmed"),
        "retries slow to a bounded rate and say so"
    );
    let retry = between(
        &panel,
        "private func runObserveLoop",
        "// MARK: - Close support",
    );
    assert!(
        retry.contains("banner == .dirtyUnconfirmed"),
        "the notice must clear once the model agrees"
    );
}

#[test]
fn every_manual_reload_asks_before_discarding_unsaved_text() {
    let panel = swift("FilePanelView.swift");
    // Banner buttons used to start a discarding reload directly, bypassing the confirmation that
    // File > Reload from Disk shows.
    let banner = between(
        &panel,
        "private func bannerAction(primary: Bool)",
        "@discardableResult\n    func clickBannerButton",
    );
    assert!(
        !banner.contains("startReload(") && !banner.contains("performReload("),
        "a banner button must not reload without going through reloadFromDisk()"
    );
    for case in [
        "case (.changedOnDisk, true), (.unreadable, true), (.saveResolved, true), (.ownerLost, true):",
        "case (.saveUnknown(let operation), true) where operation == 0:",
    ] {
        let at = banner
            .find(case)
            .unwrap_or_else(|| panic!("banner case `{case}` is missing"));
        let arm = &banner[at + case.len()..];
        let arm = &arm[..arm.find("case (").unwrap_or(arm.len())];
        assert!(
            arm.contains("reloadFromDisk()"),
            "`{case}` must reload through reloadFromDisk(): {arm}"
        );
    }
    // The only remaining use of the unconfirmed route is the automatic reload of a clean view.
    assert_eq!(
        panel.matches("startReload(").count(),
        2,
        "one definition and one caller expected"
    );
    assert!(
        panel.contains("startReload(discard: true, automatic: true)")
            && panel.contains("if !hasUnsavedChanges {"),
        "only the automatic reload of a view without unsaved text may skip the confirmation"
    );
    let reload = between(
        &panel,
        "func reloadFromDisk() {",
        "private func confirmDiscard()",
    );
    let ask = reload
        .find("confirmDiscard()")
        .expect("reloadFromDisk must confirm");
    let run = reload
        .find("performReload(discard: true, automatic: false)")
        .expect("reloadFromDisk must reload");
    assert!(
        reload.contains("if self.hasUnsavedChanges") && ask < run,
        "unsaved text must be confirmed before the reload runs"
    );
    let smoke = swift("FilePanelSmoke.swift");
    assert!(
        smoke.contains("DiscardScript([false, true])")
            && smoke.contains("clickBannerButton(primary: true)")
            && smoke.contains("Cancel in the discard confirmation still replaced the unsaved text"),
        "the smoke must press a banner Reload on a dirty panel, cancel once, then discard"
    );
}

#[test]
fn a_lost_binding_recovers_by_reattaching_and_a_kept_one_by_reloading() {
    // Rust refuses a re-attach by the bound host while the model is dirty (see
    // `a_dirty_view_recovers_a_kept_binding_by_reload_and_a_lost_binding_by_attach` in
    // desktop-runtime), so the panel must pick the recovery by whether it still holds a token and
    // must never leave a stale loaded panel without Reload and Copy Text.
    let panel = swift("FilePanelView.swift");
    let banner = between(
        &panel,
        "private func bannerAction(primary: Bool)",
        "@discardableResult\n    func clickBannerButton",
    );
    assert!(
        !banner.contains("retry()") || banner.contains("(.loadFailed, true)"),
        "retry() is only for the load-failed banner"
    );
    assert!(
        !banner.contains("case (.ownerLost, true):\n            retry()"),
        "the owner-lost banner must not blindly re-attach with the same host"
    );
    let perform = between(
        &panel,
        "private func performReload(discard: Bool, automatic: Bool) async",
        "// MARK: - Save",
    );
    let token = between(perform, "guard let token = ownerToken else {", "}");
    assert!(
        token.contains("await reattach()") && !token.contains("staleOwner"),
        "with no binding the reload attaches again whatever the stale flag says: {token}"
    );
    let retry = between(&panel, "func retry() {", "@objc private func retryTapped");
    assert!(
        !retry.contains("staleOwner = false")
            && !retry.contains("setBanner(.none)\n        beginAttach"),
        "retry must not clear the stale state or the recovery banner before the attach succeeds"
    );
    let failed = between(
        &panel,
        "private func loadFailed(",
        "private func showFailure(",
    );
    assert!(
        failed.contains("ownerToken == nil")
            && failed.contains("hasUnsavedChanges")
            && failed.contains("staleOwner = true")
            && failed.contains(".ownerLost("),
        "a failed re-attach of a loaded panel must stay stale with the owner-lost banner (Reload and Copy Text)"
    );
    let apply = between(
        &panel,
        "private func apply(_ reply: FileHostReply)",
        "private func applyText",
    );
    assert!(
        apply.contains("if case .ownerLost = banner { setBanner(.none) }"),
        "a successful re-attach clears the owner-lost banner"
    );
    let attach = between(
        &panel,
        "private func reattach() async",
        "/// Retry from the failed-state",
    );
    assert!(
        attach.contains("prepareAttach()") && attach.contains("await finishAttach("),
        "reattach is the awaited form of the attach"
    );
}

#[test]
fn file_service_waits_have_their_own_bounded_queue_off_the_terminal_input_path() {
    let coordinator = swift("ApplicationCoordinator.swift");
    let commands = between(&coordinator, "enum UICommand: Sendable {", "\n}\n");
    assert!(
        !commands.contains("fileOpen") && !commands.contains("fileDetach"),
        "file open and detach wait on the file worker and must not ride the terminal command queue"
    );
    let terminal_loop = between(
        &coordinator,
        "commandTask = Task { [weak self, stream] in",
        "fileCommandTask = Task {",
    );
    assert!(
        !terminal_loop.contains("fileOpen(") && !terminal_loop.contains("fileDetach("),
        "the terminal input loop must not await the file service"
    );
    let file_loop = between(
        &coordinator,
        "fileCommandTask = Task {",
        "pollingTask = Task {",
    );
    assert!(
        file_loop.contains("self.bridge.fileOpen(")
            && file_loop.contains("self.bridge.fileDetach("),
        "the dedicated file task performs the waits"
    );
    assert!(
        coordinator.contains("static let fileCommandCapacity = 32")
            && coordinator.contains(".bufferingOldest(Self.fileCommandCapacity)"),
        "the file queue is bounded"
    );
    let enqueue = between(&coordinator, "func enqueueFile(", "func reportError(");
    assert!(
        enqueue.contains("case .dropped:")
            && enqueue.contains("The file queue is full")
            && enqueue.contains("case .terminated:"),
        "overload and a closed desktop are reported, never silent"
    );
    assert!(
        coordinator.contains("enqueueFile(.detach(owner))"),
        "releasing a binding uses the file queue"
    );
    assert!(
        coordinator.contains("fileContinuation.finish()")
            && coordinator.contains("fileCommandTask?.cancel()"),
        "shutdown ends the file queue and task"
    );
    assert!(
        swift("App.swift").contains("coordinator.enqueueFile(.open(paths, true, false))"),
        "File > Open... uses the file queue"
    );
}

#[test]
fn the_smoke_covers_error_state_retry_and_descriptor_release_and_names_the_restore_gap() {
    let smoke = swift("FilePanelSmoke.swift");
    for needed in [
        "presentationSummary == \"error:not_found\"",
        "broken.retry()",
        "Retry loaded the file",
        "openDescriptorPaths(under:",
        "verifyDescriptors: true",
        "descriptors still open on the fixture after every tab was closed",
    ] {
        assert!(smoke.contains(needed), "the smoke lost `{needed}`");
    }
    assert!(
        smoke.contains("Session\n/// quit/restore (the `files.epoch` rotation) is not exercised"),
        "the smoke must state that session restore is not exercised"
    );
    // The epoch rotation is still pinned structurally: a clean panel re-attaches, a dirty one keeps
    // its text and says so, and neither path discards anything.
    let panel = swift("FilePanelView.swift");
    let epoch = between(
        &panel,
        "private func epochChanged() {",
        "private func reconcileExternal",
    );
    assert!(
        epoch.contains("if isLoaded && hasUnsavedChanges")
            && epoch.contains("staleOwner = true")
            && epoch.contains(".ownerLost(")
            && epoch.contains("phase = .idle")
            && !epoch.contains("setDirty(false)")
            && !epoch.contains("editor.string"),
        "an epoch change must keep unsaved text and re-attach only clean panels"
    );
}

#[test]
fn the_buffer_is_read_only_while_a_reload_or_attach_reply_can_replace_it() {
    // Every reply that replaces the buffer (manual reload, automatic reload, attach, re-attach and
    // retry) must not race with typing: the editor is read-only from the moment the request is
    // marked busy until its reply has been handled, and only `refreshEditable` makes it editable.
    let panel = swift("FilePanelView.swift");
    assert_eq!(
        panel.matches("editor.isEditable = ").count(),
        2,
        "editor.isEditable must be assigned only by the initial setup and refreshEditable"
    );
    let gate = between(
        &panel,
        "private func refreshEditable()",
        "// MARK: - Dirty tracking",
    );
    assert!(
        gate.contains("isLoaded && writable && !editingSuspended"),
        "the editable state must include the suspension flag: {gate}"
    );
    let apply_text = between(
        &panel,
        "private func applyText(",
        "private func applyImage(",
    );
    assert!(
        !apply_text.contains("editor.isEditable") && apply_text.contains("refreshEditable()"),
        "applyText must go through refreshEditable, not assign isEditable"
    );
    let prepare = between(
        &panel,
        "private func prepareAttach()",
        "private func finishAttach(",
    );
    assert!(
        prepare.contains("busy = true\n        suspendEditing()"),
        "an attach suspends editing in the same step that marks the panel busy: {prepare}"
    );
    let finish = between(
        &panel,
        "private func finishAttach(",
        "private func beginAttach()",
    );
    let resume = finish
        .find("resumeEditing()")
        .expect("finishAttach resumes editing");
    let applied = finish
        .find("apply(reply)")
        .expect("finishAttach applies the reply");
    assert!(
        finish
            .find("busy = false")
            .is_some_and(|busy| busy < resume),
        "editing resumes after the request completed"
    );
    assert!(
        resume < applied,
        "editing resumes before apply, which then sets the final editable state itself"
    );
    let perform = between(
        &panel,
        "private func performReload(discard: Bool, automatic: Bool) async",
        "// MARK: - Save",
    );
    let suspend = perform
        .find("suspendEditing()")
        .expect("a reload suspends editing");
    let sent = perform
        .find("await send(.reload")
        .expect("a reload sends the request");
    let resumed = perform
        .find("resumeEditing()")
        .expect("a reload resumes editing");
    assert!(
        perform
            .find("busy = true")
            .is_some_and(|busy| busy < suspend)
            && suspend < sent,
        "the reload suspends editing before the request is awaited"
    );
    assert!(
        sent < resumed,
        "the reload resumes editing only after the reply"
    );
    // No suspension point between the clean check in `reloadFromDisk` and `performReload`.
    let manual = between(
        &panel,
        "func reloadFromDisk()",
        "private func confirmDiscard()",
    );
    assert!(
        manual.contains("if self.hasUnsavedChanges { guard await self.confirmDiscard() else { return } }\n            await self.performReload("),
        "a clean panel must reach performReload without another suspension point: {manual}"
    );
}
