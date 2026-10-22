//! Drift guard between the Rust file channel and the Swift panel that consumes it.
//!
//! There is no Swift toolchain on Linux, so the native panel cannot be compiled or run here. What
//! can be checked is the contract it depends on: the Swift `Decodable` shapes must name exactly the
//! fields the real Rust replies and snapshots carry, the Swift frame limits must equal the Rust
//! ones, and the audit of every place that tests for the browser kind must stay current. Each
//! failure message says what to update. This does not prove the Swift compiles; macOS CI does.
//!
//! Two kinds of test live here and they prove different things:
//! - contract tests (the decodables, limits and browser-kind audit) execute real Rust against the
//!   Swift text and are the only tests here that check anything the Swift does;
//! - tests named `pin_*` read Swift source and assert that exact statements exist and in what
//!   order. They are structure pins: they catch a reverted fix or a moved statement, not wrong
//!   behaviour, and they never run the Swift. Behaviour is checked only by the macOS fixtures
//!   (`scripts/check-file-close-resolution`, `scripts/check-file-editor-model`,
//!   `scripts/check-file-load-gate`) and the in-app smoke, none of which can run on this machine.
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
/// Like `call`, but returns the bridge status instead of asserting it is zero: the statuses
/// `ZMUX_BUSY` (-4) and `ZMUX_CAPACITY` (-5) are the ones Swift must treat as "nothing ran".
fn call_status(desktop: u64, header: &Value, body: &[u8]) -> (i32, Value) {
    let header = serde_json::to_vec(header).unwrap();
    let mut frame = (header.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&header);
    frame.extend_from_slice(body);
    // SAFETY: the request storage is live for the whole call.
    let result = unsafe { zmux_desktop_file(desktop, frame.as_ptr(), frame.len()) };
    if result.status != 0 {
        return (result.status, Value::Null);
    }
    let reply = bytes_of(result.value);
    let length = u32::from_be_bytes(reply[..4].try_into().unwrap()) as usize;
    (0, serde_json::from_slice(&reply[4..4 + length]).unwrap())
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

/// The raw reply frame of one file-channel call: `(header_len, header, body)`.
fn raw_call(desktop: u64, header: &Value) -> (usize, Value) {
    let header = serde_json::to_vec(header).unwrap();
    let mut frame = (header.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&header);
    // SAFETY: the request storage is live for the whole call.
    let result = unsafe { zmux_desktop_file(desktop, frame.as_ptr(), frame.len()) };
    assert_eq!(result.status, 0);
    let reply = bytes_of(result.value);
    let length = u32::from_be_bytes(reply[..4].try_into().unwrap()) as usize;
    (
        length,
        serde_json::from_slice(&reply[4..4 + length]).unwrap(),
    )
}

// A legal request (header within the 16 KiB request cap) can produce a reply header larger than
// that cap, because each open item echoes its path plus a surface receipt. Swift must accept every
// reply Rust can legally send, so its reply limit must exceed what the real export produces.
#[test]
fn swift_accepts_the_largest_legal_open_reply_header_the_real_rust_export_sends() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = TempDir::new();
    let mut deep = dir.0.clone();
    for _ in 0..3 {
        deep.push("d".repeat(200));
    }
    std::fs::create_dir_all(&deep).unwrap();
    let desktop = zmux_desktop_create();
    assert_eq!(desktop.status, 0);
    let desktop = desktop.value;
    snapshot(desktop, |v| {
        !v["model"]["workspaces"].as_array().unwrap().is_empty()
    });
    // Sixteen existing files at long paths; the request header stays within the request cap.
    let mut items = Vec::new();
    for i in 0..16 {
        let path = deep.join(format!("open-{i:02}-{}.txt", "x".repeat(40)));
        std::fs::write(&path, format!("file {i}\n")).unwrap();
        items.push(json!({"kind":"path","value":path}));
    }
    let request = json!({"op":"open","items":items,"focus":true});
    assert!(
        serde_json::to_vec(&request).unwrap().len() <= file_channel::MAX_FILE_HEADER_BYTES,
        "the probe request must itself be legal"
    );
    let (length, reply) = raw_call(desktop, &request);
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["result"]["items"].as_array().unwrap().len(), 16);
    assert!(
        length > file_channel::MAX_FILE_HEADER_BYTES,
        "the probe must produce a reply header past the request cap (got {length}); lengthen the paths"
    );
    let client = swift("FileHostClient.swift");
    let limit = client
        .lines()
        .find(|l| l.contains("static let maxReplyHeader ="))
        .expect("FileHostLimits.maxReplyHeader is missing")
        .split('=')
        .nth(1)
        .unwrap()
        .trim()
        .replace('_', "")
        .parse::<usize>()
        .unwrap();
    assert!(
        length <= limit,
        "Swift would reject a legal {length}-byte open reply header (limit {limit})"
    );
    let lib =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs")).unwrap();
    assert!(
        lib.contains(&format!(
            "const MAX_RESPONSE_BYTES: usize = {};",
            "8_388_608"
        )) && limit == 8_388_608,
        "the reply header limit must equal the native bridge's whole-reply cap"
    );
    // The decode guard uses the reply limit, never the request limit.
    let decode = between(
        &client,
        "static func decode(",
        "let decoder = JSONDecoder()",
    );
    assert!(
        decode.contains("headerLength <= FileHostLimits.maxReplyHeader")
            && !decode.contains("FileHostLimits.maxHeader"),
        "FileHostReply.decode must bound the reply header by maxReplyHeader: {decode}"
    );
    assert_eq!(zmux_desktop_destroy(desktop), 0);
}

// Each file tab has its own undo history. AppKit gives every text view without a delegate answer
// the window's single manager, which lets undo reach a hidden tab and lets any load erase every
// tab's history. This pin cannot run AppKit; it keeps the per-panel manager wired.
#[test]
fn pin_every_file_editor_has_its_own_undo_history() {
    let panel = swift("FilePanelView.swift");
    for needed in [
        "private let undoHistory: UndoManager",
        "editor.delegate = self",
        "func undoManager(for view: NSTextView) -> UndoManager?",
        "view === editor ? undoHistory : nil",
        "undoHistory.removeAllActions()",
    ] {
        assert!(panel.contains(needed), "missing `{needed}`");
    }
    assert!(
        !panel.contains("editor.undoManager"),
        "reading editor.undoManager would clear or use the window's shared manager if the delegate is lost"
    );
    let apply = between(&panel, "private func applyText(", "isLoaded = true");
    assert!(
        apply.contains("undoHistory.removeAllActions()"),
        "a load clears this panel's history only: {apply}"
    );
    // Nothing else in the app may clear a shared manager from the file panel.
    assert_eq!(
        panel.matches("removeAllActions").count(),
        1,
        "only applyText clears an undo history, and only this panel's own"
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
        (
            "TerminalSelectionSmoke.swift",
            3,
            "link smoke checks only fixture-created browser surfaces for its exact URLs; file surfaces are neither counted nor rerouted",
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

// The Swift cannot run on this machine, so the `pin_*` tests below pin the structure that the
// native fixes depend on. An assertion fails if the pinned text is reverted or moved; it does not
// show that the Swift behaves. The macOS fixtures and the in-app smoke (`FilePanelSmoke`) are the
// behavioural checks and have not run here.

#[test]
fn pin_close_requests_are_queued_never_dropped() {
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
fn pin_keyboard_focus_follows_content_that_replaces_the_loading_state() {
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
// There is no Swift toolchain here, so the pins below can only check the exact statements the
// decision is made of. They do not run it.
#[test]
fn pin_close_resolution_aborts_without_discarding_and_defers_dont_save_to_the_core() {
    let resolution = swift("FileCloseResolution.swift");
    let body = between(
        &resolution,
        "static func resolve<Panel: FileCloseParticipant>(",
        "\n    }\n}",
    );
    let cancel = at(body, "case .cancel: return nil");
    let save = at(
        body,
        "case .save: guard await panel.performSave(force: false) else { return nil }",
    );
    let discard = at(body, "case .discard: discards.append(panel)");
    let hold = at(body, "discards.forEach { $0.holdDiscard() }");
    let settle = at(
        body,
        "guard await panel.settleDirtyMirror(assumingDiscarded: discarded) else {",
    );
    let release = at(body, "discards.forEach { $0.releaseDiscard() }");
    let resume = at(body, "panels.forEach { $0.resumeDirtyMirror() }");
    let settlement = at(body, "return FileCloseSettlement(discards: discards)");
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
        settle < release && release < resume && resume < settlement,
        "a failed settle must lift the holds, then resume the mirrors"
    );
    assert!(
        body[settle..settlement].contains("return nil"),
        "a failed settle must abort the close"
    );
    assert_eq!(
        body.matches("return nil").count(),
        3,
        "exactly three abort paths: Cancel, a failed Save and a failed settle"
    );
    assert!(
        !body.contains("applyDiscard") && !body.contains("discardLocalChanges"),
        "resolve must never drop the text itself: only the core closing the tab may (data loss otherwise)"
    );
    assert_eq!(
        body.matches("holdDiscard()").count() + body.matches("releaseDiscard()").count(),
        2,
        "holds are taken and released in one place each"
    );
    assert!(
        !body[..hold].contains("Discard()"),
        "the per-panel decision loop must not touch dirty tracking"
    );
    // The settlement is the only place a Don't Save is applied or given back, and only once.
    let finish = between(
        &resolution,
        "func finish(closeSucceeded: Bool) {",
        "\n    }\n}",
    );
    let guard_once = at(finish, "guard !isResolved else { return }");
    let mark = at(finish, "isResolved = true");
    let apply = at(
        finish,
        "if closeSucceeded { discards.forEach { $0.applyDiscard() } }",
    );
    let give_back = at(finish, "else { discards.forEach { $0.releaseDiscard() } }");
    assert!(
        guard_once < mark && mark < apply && apply < give_back,
        "the first outcome decides: apply on success, release on failure"
    );
    assert_eq!(
        resolution.matches("$0.applyDiscard()").count(),
        1,
        "applyDiscard is called from the settlement only"
    );
    // The coordinator only asks the question; it cannot reorder the decision.
    let coordinator = swift("ApplicationCoordinator.swift");
    let asks = between(
        &coordinator,
        "func resolveFilesBeforeClose(",
        "/// Quit: resolves every dirty panel",
    );
    assert!(
        asks.contains("-> FileCloseSettlement?")
            && asks.contains("FileCloseResolution.resolve(filePanels(for: surfaces))")
            && !asks.contains("discardLocalChanges")
            && !asks.contains("applyDiscard")
            && !asks.contains("settleDirtyMirror")
            && !asks.contains("performSave"),
        "the coordinator must delegate the decision to FileCloseResolution and hand back its pending settlement"
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
fn pin_a_close_the_core_refuses_gives_the_discard_back() {
    let app = swift("App.swift");
    let drain = between(
        &app,
        "private func drainFileCloses",
        "func refreshFileChrome",
    );
    let resolve = at(drain, "host.coordinator.resolveFilesBeforeClose(");
    let wait = at(drain, "await next.proceed().wait()");
    let finish = at(drain, "settlement.finish(closeSucceeded: closed)");
    let remove = at(drain, "pendingFileCloses.removeFirst()");
    assert!(
        resolve < wait && wait < finish && finish < remove,
        "the queue must wait for the core's answer to the close, finish the settlement with it, and only then dequeue"
    );
    assert!(
        !drain.contains("if allowed"),
        "the close must not be fire-and-forget: its outcome decides the discard"
    );
    assert!(
        app.contains("let proceed: @MainActor () -> CommandOutcome"),
        "a close request must hand the queue the core's outcome"
    );
    // Every close the queue sends is tracked; a plain enqueue would discard on an unknown result.
    let sends = app.matches("self?.sendClose(").count();
    assert_eq!(
        sends, 5,
        "all five close gestures must send a tracked close"
    );
    assert_eq!(
        app.matches("?? .failed()").count(),
        5,
        "a released host fails the close instead of dropping it"
    );
    for gesture in [
        "close_surface",
        "close_other_surfaces",
        "close_pane",
        "close_workspace",
        ".window(\"close\"",
    ] {
        let line = app
            .lines()
            .find(|l| l.contains("closeAfterFileCheck(") && l.contains(gesture))
            .unwrap_or_else(|| panic!("no closeAfterFileCheck call for {gesture}"));
        assert!(
            line.contains("self?.sendClose("),
            "{gesture} must use the tracked close: {line}"
        );
    }
    let coordinator = swift("ApplicationCoordinator.swift");
    let tracked = between(
        &coordinator,
        "func enqueueTracked(",
        "func refreshFileChrome",
    );
    assert!(
        tracked.contains("enqueue(.tracked(command, outcome))")
            && tracked.contains("outcome.finish(false)"),
        "a command the queue refuses must fail its outcome: {tracked}"
    );
    let perform = between(
        &coordinator,
        "case .tracked(let inner, let outcome):",
        "\n    }\n",
    );
    let ran = at(perform, "try await perform(inner, acknowledged: true)");
    let ok = at(perform, "outcome.finish(true)");
    let failed = at(perform, "outcome.finish(false)");
    let rethrow = at(perform, "throw error");
    assert!(
        ran < ok && ok < failed && failed < rethrow,
        "a tracked command reports success only after it ran and still surfaces its error: {perform}"
    );
    // The executor's own answer, not queue acceptance: every trackable command forwards the flag
    // to the bridge, and the tracked case refuses a command the bridge cannot acknowledge.
    let performer = between(
        &coordinator,
        "private func perform(_ command: UICommand, acknowledged: Bool = false)",
        "case .tracked(let inner, let outcome):",
    );
    for arm in [
        "try await bridge.window(operation, window: window, focus: focus, acknowledged: acknowledged)",
        "workspace: workspace, acknowledged: acknowledged)",
        "closed: closed, acknowledged: acknowledged)",
    ] {
        assert!(
            performer.contains(arm),
            "a tracked close must reach the bridge as an acknowledged send; missing `{arm}`"
        );
    }
    assert!(
        perform.contains("case .window, .action, .surface, .reconnectSSH: break")
            && perform.contains("default: throw BridgeFailure("),
        "only commands the bridge acknowledges may be tracked: {perform}"
    );
    let native_bridge = swift("NativeBridge.swift");
    let reconnect_bridge = between(
        &native_bridge,
        "func reconnectSSH(identityNamespace:",
        "/// Native-only download acknowledgements",
    );
    assert!(
        reconnect_bridge.contains("expected_session_id")
            && reconnect_bridge.contains("]], acknowledged: true)"),
        "a tracked SSH reconnect must wait for the bridge's acknowledged action result: {reconnect_bridge}"
    );
    let command_loop = between(
        &coordinator,
        "commandTask = Task { [weak self, stream] in",
        "try await self.perform(command)",
    );
    assert!(
        command_loop
            .contains("if case .tracked(_, let outcome) = command { outcome.finish(false) }"),
        "a close the application stopped before running must fail, not hang the close queue"
    );
    // A tab that the core closed is retired without an orphan prompt; any other unsaved panel is
    // never dropped silently.
    let retire = between(
        &coordinator,
        "private func retireFilePanel(",
        "private func presentNextOrphan",
    );
    let pending = at(retire, "if view.isDiscardPending {");
    let orphan = at(retire, "} else if view.hasUnsavedChanges {");
    assert!(
        pending < orphan && retire[pending..orphan].contains("view.close()"),
        "a panel whose Don't Save is in flight is closed quietly; others with text are orphaned: {retire}"
    );
    // Quit applies Don't Save only once termination is certain: the coordinator hands the
    // settlement back unfinished, and the delegate commits it in the step that starts the
    // shutdown, never before the application has decided to terminate.
    let quit = between(
        &coordinator,
        "func resolveFilesBeforeQuit() async -> FileCloseSettlement? {",
        "/// Sends a close command",
    );
    assert!(
        !quit
            .split("func commitQuit(")
            .next()
            .unwrap()
            .contains("finish(closeSucceeded"),
        "resolving a quit must not apply or release the settlement: {quit}"
    );
    let commit = between(&coordinator, "func commitQuit(", "\n    }\n");
    assert!(
        at(commit, "settlement.finish(closeSucceeded: false)")
            < at(commit, "settlement.finish(closeSucceeded: true)")
            && commit.contains("settlement.isDiscarding($0)"),
        "a quit with text nobody answered gives the discards back; otherwise it applies them: {commit}"
    );
    let delegate = between(
        &app,
        "func applicationShouldTerminate(",
        "private func beginTermination(",
    );
    assert!(
        at(delegate, "coordinator.commitQuit(settlement)")
            < at(
                delegate,
                "self.beginTermination(sender, coordinator: coordinator)"
            ),
        "the quit commits its discards and starts the shutdown in one synchronous step: {delegate}"
    );
    assert!(
        !delegate.contains("if proceed {")
            && delegate
                .matches("sender.reply(toApplicationShouldTerminate: false)")
                .count()
                == 2,
        "a refused or superseded quit replies cancel and applies nothing: {delegate}"
    );
    let begin = between(&app, "private func beginTermination(", "\n}\n\n@main");
    assert!(
        at(begin, "terminating = true") < at(begin, "coordinator.shutdown()"),
        "{begin}"
    );
    // The panel keeps its text through the pending period and only drops it on apply.
    let panel = swift("FilePanelView.swift");
    let apply = between(&panel, "func applyDiscard() {", "\n    }\n");
    assert!(
        at(apply, "guard !closed else {") < at(apply, "discardLocalChanges()"),
        "a panel retired with its tab must not touch dirty tracking again: {apply}"
    );
    assert!(
        panel.contains("var isDiscardPending: Bool { dirtyGate.isDiscarding }"),
        "the coordinator reads the pending state from the gate"
    );
    // The fixture and the smoke exercise the refused close.
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/native-fixtures/file-close-resolution.swift");
    let fixture = std::fs::read_to_string(fixture_path).unwrap();
    for scenario in [
        "the text must stay on the panel until the core has closed the tab",
        "a refused close must mark the model dirty again so a later close still asks",
        "a refused close must leave the edited text and its marker",
        "a late success must not drop text after a refusal",
        "the first refusal must not lift the second close's hold",
        "an abandoned quit leaves the text edited, the model dirty and no hold",
        "a panel the quit did not answer is not covered",
    ] {
        assert!(
            fixture.contains(scenario),
            "fixture scenario `{scenario}` is missing"
        );
    }
    let smoke = swift("FilePanelSmoke.swift");
    assert!(
        smoke.contains("pending.finish(closeSucceeded: false)")
            && smoke.contains("a refused close discarded the edits or left the discard held")
            && smoke.contains("applied.finish(closeSucceeded: true)")
            && smoke.contains("coordinator.enqueueTracked(.surface(\"close_surface\", markdownID")
            && smoke.contains("a close the core refused read as accepted"),
        "the smoke must refuse one close and accept the next, and make the real core refuse one"
    );
}

#[test]
fn pin_the_smoke_expects_a_plain_open_of_a_valid_file_to_succeed() {
    // `cli(args, expectFailure)`: only the batch that includes the missing path and the folder is
    // expected to exit non-zero. A single valid, not-yet-open file succeeds (exit 0), so passing
    // `true` there makes `nativeCLI` throw and the whole smoke fail before its later stages.
    let smoke = swift("FilePanelSmoke.swift");
    let opens: Vec<&str> = smoke
        .lines()
        .filter(|l| l.contains("cli([\"open\""))
        .collect();
    assert!(
        opens.len() >= 3,
        "the smoke opens files through the real CLI"
    );
    for line in opens {
        let expects_failure = line.contains("], true))");
        let mixed = line.contains("f.missing");
        assert_eq!(
            expects_failure, mixed,
            "only the mixed batch may expect a failing exit status: {line}"
        );
    }
}

#[test]
fn pin_the_close_decision_fixture_is_wired_into_macos_ci() {
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
fn pin_a_panel_being_discarded_cannot_restart_the_dirty_mirror() {
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
    let apply = between(&panel, "func applyDiscard() {", "\n    }\n");
    assert!(
        at(apply, "discardLocalChanges()") < apply.rfind("dirtyGate.release()").unwrap()
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
fn pin_closing_waits_for_an_in_flight_observation_before_writing() {
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
fn pin_dirty_detection_compares_utf8_bytes_not_canonical_equivalence() {
    let panel = swift("FilePanelView.swift");
    let model = swift("FileEditorModel.swift");
    for source in [&panel, &model] {
        for forbidden in [
            "editor.string != baseline",
            "editor.string != text",
            "editor.string == baseline",
            "editor.string == text",
            "text != baseline",
            "text == baseline",
            "current != written",
            "current == written",
        ] {
            assert!(
                !source.contains(forbidden),
                "`{forbidden}` uses Swift String equality, which ignores normalization form"
            );
        }
    }
    let helper = between(&model, "static func sameBytes", "}");
    assert!(
        helper.contains("lhs.utf8.elementsEqual(rhs.utf8)"),
        "sameBytes must compare UTF-8 code units"
    );
    assert_eq!(
        model.matches("Self.sameBytes(").count(),
        3,
        "the keystroke path and both post-save paths (confirmed and committed-but-unsynced) of the model must use sameBytes"
    );
    assert!(
        !panel.contains("sameBytes"),
        "the panel must ask the model, not compare text itself"
    );
}

#[test]
fn pin_the_dirty_observation_never_gives_up_after_a_count() {
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
fn pin_every_manual_reload_asks_before_discarding_unsaved_text() {
    let panel = swift("FilePanelView.swift");
    let model = swift("FileEditorModel.swift");
    // Banner buttons used to start a discarding reload directly, bypassing the confirmation that
    // File > Reload from Disk shows. The model decides which button means "reload" and the panel
    // only maps that answer to reloadFromDisk().
    let banner = between(
        &panel,
        "private func bannerAction(primary: Bool)",
        "@discardableResult\n    func clickBannerButton",
    );
    assert!(
        !banner.contains("startReload(") && !banner.contains("performReload("),
        "a banner button must not reload without going through reloadFromDisk()"
    );
    let reload_arm = between(banner, "case .reloadFromDisk:", "case .retry");
    assert!(
        reload_arm.contains("reloadFromDisk()"),
        "the model's reload answer must reload through reloadFromDisk(): {reload_arm}"
    );
    let actions = between(
        &model,
        "mutating func bannerAction(",
        "default:\n            return .nothing",
    );
    for case in [
        "case (.changedOnDisk, true), (.unreadable, true), (.saveResolved, true), (.ownerLost, true):",
        "case (.saveUnknown(let operation), true) where operation == 0:",
    ] {
        let at = actions
            .find(case)
            .unwrap_or_else(|| panic!("banner case `{case}` is missing"));
        let arm = &actions[at + case.len()..];
        let arm = &arm[..arm.find("case (").unwrap_or(arm.len())];
        assert!(
            arm.contains("return .reloadFromDisk"),
            "`{case}` must answer reloadFromDisk: {arm}"
        );
    }
    // The only remaining use of the unconfirmed route is the automatic reload of a clean view.
    assert_eq!(
        panel.matches("startReload(").count(),
        2,
        "one definition and one caller expected"
    );
    assert!(
        panel.contains("case .reloadAutomatically: startReload(discard: true, automatic: true)")
            && model.contains("if !hasUnsavedChanges {\n                return diskGeneration != autoReloadedDiskGeneration ? .reloadAutomatically : .none"),
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
fn pin_a_lost_binding_recovers_by_reattaching_and_a_kept_one_by_reloading() {
    // Rust refuses a re-attach by the bound host while the model is dirty (see
    // `a_dirty_view_recovers_a_kept_binding_by_reload_and_a_lost_binding_by_attach` in
    // desktop-runtime), so the panel must pick the recovery by whether it still holds a token and
    // must never leave a stale loaded panel without Reload and Copy Text. A lost binding is
    // re-attached because no host is bound and the entry's dirty bit was cleared with it (the
    // desktop-runtime test asserts the bit is false after a detach); it is not a recovery that
    // works over a dirty model.
    let panel = swift("FilePanelView.swift");
    let model = swift("FileEditorModel.swift");
    let banner = between(
        &panel,
        "private func bannerAction(primary: Bool)",
        "@discardableResult\n    func clickBannerButton",
    );
    assert_eq!(
        banner.matches("retry()").count(),
        1,
        "retry() has exactly one caller in the banner: the model's retry answer"
    );
    let actions = between(
        &model,
        "mutating func bannerAction(",
        "default:\n            return .nothing",
    );
    let retry_arms: Vec<&str> = actions
        .lines()
        .filter(|l| l.contains("return hasUnsavedChanges ? .dismiss : .retry"))
        .collect();
    assert_eq!(
        retry_arms.len(),
        1,
        "only the load-failed banner may answer retry, and never over unsaved text"
    );
    assert!(
        actions.contains(
            "case (.loadFailed, true):\n            return hasUnsavedChanges ? .dismiss : .retry"
        ),
        "retry is only for the load-failed banner"
    );
    assert!(
        !actions.contains("(.ownerLost, true):\n            return .retry"),
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
        apply.contains("if banner.isObsoleteAfterLoad { setBanner(.none) }")
            && between(&model, "var isObsoleteAfterLoad: Bool {", "}\n    }")
                .contains(".ownerLost"),
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
fn pin_file_service_waits_have_their_own_bounded_queue_off_the_terminal_input_path() {
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
fn pin_the_smoke_covers_error_state_retry_and_descriptor_release_and_names_the_restore_gap() {
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
            && !epoch.contains("dirtyChanged(")
            && !epoch.contains("model.adopt")
            && !epoch.contains("editor.string"),
        "an epoch change must keep unsaved text and re-attach only clean panels"
    );
}

#[test]
fn pin_the_buffer_is_read_only_while_a_reload_or_attach_reply_can_replace_it() {
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
        gate.contains("model.isEditable(loaded: isLoaded, writable: writable)"),
        "the editable state must come from the model: {gate}"
    );
    let model = swift("FileEditorModel.swift");
    assert!(
        model.contains("loaded && writable && !editingSuspended"),
        "the model's editable state must include the suspension flag"
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
        .find("await sendLoad(.reload")
        .expect("a reload sends the request through the load gate");
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

// `FileEditorModel` is the production owner of every dirty, save, reload and banner decision of
// the editor, and it is a Foundation-only type so that `scripts/check-file-editor-model` can run
// the real code on macOS. These tests cannot run it; they keep it runnable by that fixture and keep
// the decisions from drifting back into the AppKit view, where no fixture could reach them.
#[test]
fn the_file_editor_model_fixture_is_wired_into_macos_ci() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |path: &str| {
        std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
    };
    let script = read("scripts/check-file-editor-model");
    assert!(
        script.contains("apps/macos/Sources/FileEditorModel.swift")
            && script.contains("tools/native-fixtures/file-editor-model.swift")
            && script.contains("-warnings-as-errors")
            && script.contains("-swift-version 6"),
        "the script must compile the production model with the fixture, strictly"
    );
    assert!(
        !script.contains("FilePanelView.swift"),
        "the fixture must not need AppKit: it compiles the model alone"
    );
    assert!(
        read(".github/workflows/macos-development.yml").contains("scripts/check-file-editor-model"),
        "macOS CI must run the fixture; it cannot run on this machine"
    );
    let fixture = read("tools/native-fixtures/file-editor-model.swift");
    for scenario in [
        "an edit that only changes the normalization form",
        "the save payload for",
        "text typed while the save was in flight is still unsaved",
        "a reply with no generation must not reset the base generation",
        "kept edits over a newer disk version ask before saving",
        "the buffer is read-only while a reload or attach is in flight",
        "an automatic reload must not replace text typed after it was scheduled",
        "a dirty buffer is never reloaded automatically",
        "a dismissed disk generation is not announced again",
        "reloads through reloadFromDisk",
        "no retry may replace the text",
        "the next save is based on the generation the failure reported",
        "a clean buffer can be saved while the directory sync is owed",
        "a final unknown outcome stops waiting and offers Reload",
    ] {
        assert!(
            fixture.contains(scenario),
            "fixture scenario `{scenario}` is missing"
        );
    }
}

// A save whose rename landed but whose directory sync failed is a dead end unless the view rebases
// on the generation the service adopted. Rust proves the generation travels with the failure
// (`crates/desktop-runtime/tests/files.rs`, `file_channel` unit test); the model fixture proves
// what the model does with it. This pins that the panel connects the two.
#[test]
fn pin_a_save_whose_directory_sync_failed_rebases_and_offers_a_repeat_save() {
    let client = swift("FileHostClient.swift");
    assert!(
        client.contains("var committedGeneration: UInt64?")
            && client.contains("committedGeneration: header.generation"),
        "the failure must carry the generation the Rust failure header reports"
    );
    let panel = swift("FilePanelView.swift");
    let failed = between(
        &panel,
        "private func saveFailed(_ failure: FileHostFailure, written: String)",
        "if failure.isOwnerLoss",
    );
    let rebase = at(
        failed,
        "model.savedWithoutDurability(written: written, current: editor.string, generation: committed)",
    );
    let banner = at(failed, "setBanner(.savedUnsynced(failure.code))");
    let waiting = at(failed, "setBanner(.saveUnknown(failure.opId ?? 0))");
    assert!(
        at(failed, "failure.committedGeneration") < rebase && rebase < banner && banner < waiting,
        "a committed failure rebases before it shows the notice; only an uncommitted one waits: {failed}"
    );
    assert!(
        at(&panel, "case .saveAgain: save()") > at(&panel, "model.bannerAction(primary: primary"),
        "the notice's primary button must repeat the save"
    );
    assert!(
        panel.contains("primary: \"Save Again\""),
        "the unsynced notice must label its repeat-save button"
    );
    let model = swift("FileEditorModel.swift");
    assert!(
        model.contains("(isDirty || durabilityUnconfirmed)"),
        "a clean buffer must still be saveable while the directory sync is owed"
    );
    let outcome = between(
        &model,
        "func saveOutcomeBanner(",
        "mutating func bannerAction",
    );
    assert!(
        outcome.contains("case \"outcome_unknown\"")
            && outcome.contains("code == \"timeout\" ? nil : .saveUnknown(0)"),
        "a final unknown outcome must stop waiting; a timeout keeps waiting: {outcome}"
    );
    let fixture = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/native-fixtures/file-editor-model.swift"),
    )
    .unwrap();
    assert!(
        fixture.contains("        unsyncedSave()\n"),
        "the fixture must run the unsynced-save scenario"
    );
}

#[test]
fn pin_the_file_editor_model_is_appkit_free_and_the_panel_asks_it() {
    let model = swift("FileEditorModel.swift");
    let imports: Vec<&str> = model
        .lines()
        .filter(|l| l.trim_start().starts_with("import "))
        .collect();
    assert_eq!(
        imports,
        ["import Foundation"],
        "the model must import Foundation only so the macOS fixture can compile it alone"
    );
    for appkit in [
        "NSView",
        "NSTextView",
        "NSColor",
        "NSAlert",
        "NSButton",
        "NSPasteboard",
        "@MainActor",
    ] {
        assert!(
            !model.contains(appkit),
            "`{appkit}` does not belong in the Foundation-only model"
        );
    }
    let panel = swift("FilePanelView.swift");
    for moved in [
        "private var baseline",
        "var isDirty = ",
        "isDirty = value",
        "private var externalPending",
        "externalPending = ",
        "dismissedDeleted",
        "dismissedDiskGeneration",
        "autoReloadedDiskGeneration",
        "latestDiskGeneration",
        "editingSuspended",
        "Data(text.utf8)",
        "private var generation",
        "enum Banner",
    ] {
        assert!(
            !panel.contains(moved),
            "`{moved}` is editor state or a decision that belongs to FileEditorModel; the panel must ask the model"
        );
    }
    for asks in [
        "model.hasUnsavedChanges(editable:",
        "model.canSave(editable:",
        "model.savePayload(for: text)",
        "baseGeneration: model.generation",
        "model.saved(written: text, current: editor.string, generation: reply.header.generation)",
        "model.textEdited(editor.string)",
        "model.adopt(text: text)",
        "model.externalStateReported(",
        "model.saveOutcomeBanner(",
        "model.reloadReplied(",
        "model.saveNeedsConflictPrompt(force: force)",
        "model.bannerAction(primary: primary",
        "model.setBanner(value)",
        "model.isEditable(loaded: isLoaded, writable: writable)",
    ] {
        assert!(
            panel.contains(asks),
            "the panel no longer asks the model: `{asks}`"
        );
    }
    // The panel applies the model's dirty answer in exactly one place.
    assert_eq!(
        panel.matches("private func dirtyChanged(").count(),
        1,
        "one place turns the model's dirty change into chrome and mirror work"
    );
    assert!(
        panel.contains("typealias Banner = FileBanner"),
        "the smoke and the coordinator still name FilePanelView.Banner"
    );
}

// ---------------------------------------------------------------------------------------------
// The load gate. Every file tab attaches when its snapshot arrives, so sixteen tabs opened or
// restored together used to start sixteen bridge requests at once. Rust queues 8 jobs and runs 1
// and answers `busy` at once when full, so the overload came from Swift and the tabs past the limit
// ended in the error state. `FileLoadGate` keeps the number in flight below what Rust admits and
// retries the failures that mean "busy". The Swift cannot run here; these tests execute the real
// Rust worker with the numbers read from the Swift source.
// ---------------------------------------------------------------------------------------------

/// The text after `=` on the line of `static let <name>` in `FileLoadGate.swift`.
fn load_gate_value(source: &str, name: &str) -> String {
    let marker = format!("static let {name}");
    let line = source
        .lines()
        .find(|l| l.contains(&marker))
        .unwrap_or_else(|| panic!("FileLoadPolicy.{name} is missing"));
    line.split_once('=')
        .unwrap_or_else(|| panic!("FileLoadPolicy.{name} has no value: {line}"))
        .1
        .trim()
        .to_owned()
}
fn load_gate_numbers(value: &str) -> Vec<usize> {
    value
        .trim_matches(['[', ']'])
        .split(',')
        .map(|n| n.trim().replace('_', "").parse().expect("a number"))
        .collect()
}
fn load_gate_codes(value: &str) -> BTreeSet<String> {
    value
        .trim_matches(['[', ']'])
        .split(',')
        .map(|c| c.trim().trim_matches('"').to_owned())
        .collect()
}

#[test]
fn the_swift_load_gate_keeps_sixteen_tabs_inside_what_the_rust_worker_admits() {
    use std::sync::{
        Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let gate = swift("FileLoadGate.swift");
    let max_in_flight = load_gate_numbers(&load_gate_value(&gate, "defaultMaxInFlight"))[0];
    let max_attempts = load_gate_numbers(&load_gate_value(&gate, "defaultMaxAttempts"))[0];
    let backoff = load_gate_numbers(&load_gate_value(&gate, "defaultBackoffMilliseconds"));
    let transient = load_gate_codes(&load_gate_value(&gate, "defaultTransientCodes"));

    // The arithmetic: loads share the worker queue with saves, reloads of other tabs and `open`
    // probes, so they may use at most half of it, and the worker admits a job without `busy`
    // only while fewer than QUEUE_BOUND are queued.
    let queue = zmux_desktop_runtime::files::QUEUE_BOUND;
    assert!(
        (1..=queue / 2).contains(&max_in_flight),
        "FileLoadPolicy.defaultMaxInFlight ({max_in_flight}) must be at least 1 and at most half the worker queue ({queue})"
    );
    assert_eq!(
        backoff.len() + 1,
        max_attempts,
        "one back-off before each retry"
    );
    assert!(
        max_attempts >= 2 && backoff.windows(2).all(|w| w[0] <= w[1]),
        "retry at least once, with a back-off that does not shrink: {backoff:?}"
    );
    assert!(
        backoff.iter().sum::<usize>() <= 5_000,
        "the whole back-off must stay short enough to be a visible wait, not a hang"
    );
    // The most file surfaces one command may open (16) are attached through the gate's callers and
    // none of them may be refused.
    let operands = zmux_desktop_runtime::files::MAX_OPEN_ITEMS;
    assert_eq!(
        operands, 16,
        "the scenario below is the design's largest open"
    );

    let dir = TempDir::new();
    let desktop = zmux_desktop_create();
    assert_eq!(desktop.status, 0);
    let desktop = desktop.value;
    snapshot(desktop, |v| {
        !v["model"]["workspaces"].as_array().unwrap().is_empty()
    });
    let mut items = Vec::new();
    for i in 0..operands {
        let path = dir.0.join(format!("tab-{i:02}.txt"));
        // Large enough that each load occupies the worker for a while.
        std::fs::write(&path, format!("tab {i}\n").repeat(40_000)).unwrap();
        items.push(json!({"kind":"path","value":path}));
    }
    let (opened, _) = call(
        desktop,
        &json!({"op":"open","items":items,"focus":true}),
        &[],
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let surfaces: Vec<u64> = opened["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["surface"]["surface_id"].as_u64().expect("a surface"))
        .collect();
    assert_eq!(surfaces.len(), operands);
    let state = snapshot(desktop, |v| {
        v["files"]["entries"]
            .as_array()
            .is_some_and(|e| e.len() == 16)
    });
    let epoch = state["files"]["epoch"].clone();

    // The gate: `max_in_flight` callers pull tabs in order, so no more than that many requests are
    // ever outstanding. Both the attach of every tab and a reload of every tab must succeed.
    let hosts: Vec<Uuid> = surfaces.iter().map(|_| Uuid::new_v4()).collect();
    let owners = Mutex::new(vec![Value::Null; operands]);
    let run_gated = |phase: &str, request: &(dyn Fn(usize) -> Value + Sync)| {
        let next = AtomicUsize::new(0);
        let in_flight = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let start = Barrier::new(max_in_flight);
        let deadline = Instant::now() + Duration::from_secs(60);
        let failures = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for _ in 0..max_in_flight {
                scope.spawn(|| {
                    start.wait();
                    loop {
                        let index = next.fetch_add(1, Ordering::SeqCst);
                        if index >= operands {
                            break;
                        }
                        let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        // Statuses -4 and -5 are refused before the request runs (the registry was
                        // contended, or no reply buffer was free); Swift reports them as the
                        // transient `busy` and sends the request again. Any other nonzero status
                        // is a failure here.
                        let reply = loop {
                            let (status, reply) = call_status(desktop, &request(index), &[]);
                            match status {
                                0 => break reply,
                                -4 | -5 => {
                                    assert!(
                                        Instant::now() < deadline,
                                        "{phase}: still refused after a minute"
                                    );
                                    std::thread::yield_now();
                                }
                                other => panic!("{phase}: unexpected bridge status {other}"),
                            }
                        };
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        if reply["ok"] != true {
                            failures.lock().unwrap().push((index, reply));
                        } else if let Some(owner) = reply.get("owner") {
                            owners.lock().unwrap()[index] = owner.clone();
                        }
                    }
                });
            }
        });
        let failures = failures.into_inner().unwrap();
        assert!(
            failures.is_empty(),
            "{phase}: the file worker refused loads that stayed within the gate's limit of {max_in_flight}: {failures:?}"
        );
        assert!(
            peak.load(Ordering::SeqCst) <= max_in_flight,
            "{phase}: the harness exceeded the limit it models"
        );
    };
    run_gated(
        "attach",
        &|i| json!({"op":"attach","epoch":epoch,"surface_id":surfaces[i],"host_id":hosts[i]}),
    );
    let tokens = owners.lock().unwrap().clone();
    assert!(
        tokens.iter().all(Value::is_object),
        "every tab attached and got an owner"
    );
    run_gated(
        "reload",
        &|i| json!({"op":"reload","owner":tokens[i],"discard":true}),
    );

    // Failures the person must see at once are not in the transient set; the ones that mean
    // "busy" are. Real Rust supplies the non-transient codes.
    std::fs::remove_file(dir.0.join("tab-00.txt")).unwrap();
    let (missing, _) = call(
        desktop,
        &json!({"op":"reload","owner":tokens[0],"discard":true}),
        &[],
    );
    let stale_owner = {
        let mut wrong = tokens[1].clone();
        wrong["host_id"] = json!(Uuid::new_v4());
        call(
            desktop,
            &json!({"op":"reload","owner":wrong,"discard":true}),
            &[],
        )
        .0
    };
    for (what, reply) in [
        ("a vanished file", &missing),
        ("a foreign owner", &stale_owner),
    ] {
        let code = reply["error"]["code"].as_str().unwrap_or_default();
        assert!(
            !code.is_empty() && !transient.contains(code),
            "{what} fails with `{code}`, which the gate must not retry: {reply}"
        );
    }
    // Every transient code is one the worker really emits for overload.
    let files = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../desktop-runtime/src/files.rs"),
    )
    .unwrap();
    for code in &transient {
        assert!(
            files.contains(&format!("\"{code}\"")),
            "transient code `{code}` is never emitted by the Rust file worker"
        );
    }
    for code in ["busy", "file_worker_unavailable", "timeout"] {
        assert!(
            transient.contains(code),
            "the worker's overload code `{code}` must be retried"
        );
    }
    assert_eq!(zmux_desktop_destroy(desktop), 0);
}

#[test]
fn the_file_load_gate_fixture_is_wired_into_macos_ci() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |path: &str| {
        std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
    };
    let script = read("scripts/check-file-load-gate");
    assert!(
        script.contains("apps/macos/Sources/FileLoadGate.swift")
            && script.contains("tools/native-fixtures/file-load-gate.swift")
            && script.contains("-warnings-as-errors")
            && script.contains("-swift-version 6")
            && !script.contains("FilePanelView.swift"),
        "the script must compile the production gate alone with the fixture, strictly"
    );
    assert!(
        read(".github/workflows/macos-development.yml").contains("scripts/check-file-load-gate"),
        "macOS CI must run the fixture; it cannot run on this machine"
    );
    let gate = read("apps/macos/Sources/FileLoadGate.swift");
    let imports: Vec<&str> = gate
        .lines()
        .filter(|l| l.trim_start().starts_with("import "))
        .collect();
    assert_eq!(
        imports,
        ["import Foundation"],
        "the gate must be AppKit free"
    );
    let fixture = read("tools/native-fixtures/file-load-gate.swift");
    for scenario in [
        "sixteen requests: three running and thirteen waiting",
        "only three requests reached the file service",
        "all sixteen loads succeed with their own reply",
        "busy triggers a retry each time",
        "exactly four tries",
        "is not transient and must not be retried",
        "is tried once",
        "a request that is backing off holds no slot",
        "a cancelled request never reaches the file service",
        "a queued tab says it is waiting and where",
    ] {
        assert!(
            fixture.contains(scenario),
            "fixture scenario `{scenario}` is missing"
        );
    }
}

#[test]
fn pin_every_whole_file_load_goes_through_the_gate_and_the_panel_shows_waiting_and_retry() {
    let panel = swift("FilePanelView.swift");
    let coordinator = swift("ApplicationCoordinator.swift");
    // One gate, owned by the coordinator, shared by every panel.
    assert_eq!(
        coordinator
            .matches("let fileLoadGate = FileLoadGate()")
            .count(),
        1,
        "the coordinator owns exactly one load gate"
    );
    assert!(
        panel.contains("var fileLoadGate: FileLoadGate { get }"),
        "panels reach the gate through their owner"
    );
    // Attach and reload (the requests that read a whole file) never bypass it.
    let send_load = between(
        &panel,
        "private func sendLoad(",
        "private func loadProgressChanged(",
    );
    for needed in [
        "owner.fileLoadGate.run(",
        "isCancelled: { self.closed }",
        "progress: { self.loadProgressChanged($0) }",
        "operation: { await self.send(request) }",
        "loadProgress = nil",
    ] {
        assert!(
            send_load.contains(needed),
            "sendLoad lost `{needed}`: {send_load}"
        );
    }
    let uses = |needle: &str| panel.matches(needle).count();
    assert_eq!(
        uses("await sendLoad(request)") + uses("await sendLoad(.reload("),
        2,
        "the attach and the reload are the only callers of sendLoad"
    );
    assert_eq!(
        uses("await send(.save(") + uses("await send(.observe("),
        3,
        "the save and the two observation sites are sent directly, not queued behind loads"
    );
    assert!(
        !panel.contains("await send(.reload(") && !panel.contains("await send(request)"),
        "a whole-file load must not bypass the gate"
    );
    // A waiting tab says so and a retrying tab says why; both leave the state when the load ends.
    let progress = between(
        &panel,
        "private func loadProgressChanged(",
        "/// Marks the panel busy",
    );
    assert!(
        progress.contains("showLoading(progress.loadingTitle(for: title))"),
        "an unloaded tab shows the waiting or retrying state: {progress}"
    );
    assert!(
        panel.contains("loadProgress?.statusNote"),
        "the header carries the waiting or retrying note"
    );
    // The in-app smoke opens sixteen tabs in one command and checks the gate's peak.
    let smoke = swift("FilePanelSmoke.swift");
    assert!(
        smoke.contains("writeManyFiles(count: 16")
            && smoke.contains("gate.peakInFlightCount <= FileLoadPolicy.defaultMaxInFlight")
            && smoke.contains("ended in an error state instead of loading"),
        "the smoke must open sixteen file tabs at once and check none failed and the limit held"
    );
}

// The core allows `MAX_FILE_SURFACES` file tabs across the whole desktop and answers every further
// new path of an `open` with `file_capacity`. The smoke's sixteen-file step therefore needs a desktop
// with no other file tab: the image, the PDF and the reopened tab that earlier steps leave open take
// three of the sixteen places and the command exits 1 with three `file_capacity` items.
#[test]
fn the_sixteen_file_open_needs_every_earlier_file_tab_closed_and_the_smoke_closes_them() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let capacity = zmux_desktop_runtime::MAX_FILE_SURFACES;
    let operands = zmux_desktop_runtime::files::MAX_OPEN_ITEMS;
    assert_eq!(
        (capacity, operands),
        (16, 16),
        "the smoke's step 13 opens as many files as the desktop admits"
    );
    let dir = TempDir::new();
    let desktop = zmux_desktop_create();
    assert_eq!(desktop.status, 0);
    let desktop = desktop.value;
    snapshot(desktop, |v| {
        !v["model"]["workspaces"].as_array().unwrap().is_empty()
    });
    let paths = |prefix: &str, count: usize| -> Vec<Value> {
        (0..count)
            .map(|i| {
                let path = dir.0.join(format!("{prefix}-{i:02}.txt"));
                std::fs::write(&path, format!("{prefix} {i}\n")).unwrap();
                json!({"kind":"path","value":path})
            })
            .collect()
    };
    let open = |items: Vec<Value>| -> Value {
        let (reply, _) = call(
            desktop,
            &json!({"op":"open","items":items,"focus":false}),
            &[],
        );
        reply
    };
    let outcomes = |reply: &Value| -> Vec<(bool, Option<String>)> {
        reply["result"]["items"]
            .as_array()
            .unwrap_or_else(|| panic!("an open reply carries its items: {reply}"))
            .iter()
            .map(|item| {
                (
                    item["ok"] == true,
                    item["error"]["code"].as_str().map(str::to_owned),
                )
            })
            .collect()
    };

    // The smoke at step 13 as it was: three file tabs still open, then sixteen new files.
    let lingering = open(paths("lingering", 3));
    assert_eq!(lingering["ok"], true, "{lingering}");
    let held: Vec<u64> = lingering["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["surface"]["surface_id"].as_u64().expect("a surface"))
        .collect();
    let crowded = open(paths("many", operands));
    let crowded = outcomes(&crowded);
    let refused: Vec<usize> = crowded
        .iter()
        .enumerate()
        .filter(|(_, (ok, _))| !ok)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        refused,
        vec![13, 14, 15],
        "three lingering tabs leave room for thirteen of sixteen: {crowded:?}"
    );
    assert!(
        refused
            .iter()
            .all(|&i| crowded[i].1.as_deref() == Some("file_capacity")),
        "the refusals are file_capacity: {crowded:?}"
    );

    // Closing every one of them (all clean, as the smoke's are) frees the places, so sixteen new
    // files all open. Close the thirteen that did open as well, to start from an empty desktop.
    let state = snapshot(desktop, |v| {
        v["files"]["entries"]
            .as_array()
            .is_some_and(|e| e.len() == capacity)
    });
    let mut open_ids = Vec::new();
    fn collect(value: &Value, out: &mut Vec<u64>) {
        match value {
            Value::Object(map) => {
                if map.get("kind") == Some(&json!("file")) && map.get("file").is_some() {
                    out.push(map["id"].as_u64().expect("a surface id"));
                }
                map.values().for_each(|v| collect(v, out));
            }
            Value::Array(items) => items.iter().for_each(|v| collect(v, out)),
            _ => {}
        }
    }
    collect(&state["model"]["workspaces"], &mut open_ids);
    assert_eq!(open_ids.len(), capacity, "{open_ids:?}");
    assert!(held.iter().all(|id| open_ids.contains(id)));
    for (request, id) in open_ids.iter().enumerate() {
        let close = serde_json::to_vec(&json!({
            "version":1,"request_id":request as u64 + 1000,
            "payload":{"command":"action","action":{"action":"close_surface","surface_id":id}}
        }))
        .unwrap();
        // SAFETY: the encoded request storage is readable for the whole call.
        let closed = unsafe { zmux_desktop_dispatch(desktop, close.as_ptr(), close.len()) };
        assert_eq!(closed.status, 0);
        let reply: Value = serde_json::from_slice(&bytes_of(closed.value)).unwrap();
        assert_eq!(reply["ok"], true, "closing a clean file tab: {reply}");
    }
    snapshot(desktop, |v| {
        let mut remaining = Vec::new();
        // `closed_items` keeps the closed tabs for reopening; only open workspaces count.
        collect(&v["model"]["workspaces"], &mut remaining);
        remaining.is_empty()
    });
    let fresh = outcomes(&open(paths("fresh", operands)));
    assert!(
        fresh.iter().all(|(ok, code)| *ok && code.is_none()),
        "with no other file tab, sixteen new files all open: {fresh:?}"
    );
    assert_eq!(zmux_desktop_destroy(desktop), 0);

    // The smoke closes the three tabs it keeps (the image, the PDF and the reopened error tab) and
    // waits until no file tab remains before it opens the sixteen files.
    let smoke = swift("FilePanelSmoke.swift");
    let journey = between(&smoke, "// 13. Sixteen file tabs", "let manyPaths");
    for needed in [
        "for keptID in [imageID, pdfID, brokenID] { host.closeTerminalTab(keptID) }",
        "fileSurfaces($0).isEmpty",
        "coordinator.files.isEmpty",
    ] {
        assert!(
            journey.contains(needed),
            "the smoke must clear the desktop of file tabs before step 13; missing `{needed}`: {journey}"
        );
    }
    let before_step = &smoke[..at(&smoke, "// 13. Sixteen file tabs")];
    for opened in ["imageID", "pdfID", "brokenID"] {
        assert!(
            before_step.contains(&format!("let {opened}"))
                || before_step.contains(&format!("{opened} = ")),
            "{opened} is a tab the smoke opened earlier"
        );
    }
}

// `ZMUX_BUSY` (the bridge registry was contended) and `ZMUX_CAPACITY` (all sixteen reply buffers
// are reserved) are answered before a file request reaches the worker. Swift used to report every
// status but -2 as a `transport` failure with an unknown outcome, which is never retried, so a
// momentary collision between two bridge calls turned into an error tab. The Swift mapping
// (`FileBridgeRefusal`) must name the statuses Rust really uses, and -5 must really mean that
// nothing ran.
#[test]
fn swift_treats_the_bridge_statuses_that_ran_nothing_as_busy_and_rust_agrees_nothing_ran() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let lib =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs")).unwrap();
    let rust_status = |name: &str| -> i32 {
        lib.lines()
            .find_map(|l| l.strip_prefix(&format!("const {name}: i32 = ")))
            .unwrap_or_else(|| panic!("const {name} is missing from lib.rs"))
            .trim_end_matches(';')
            .parse()
            .unwrap()
    };
    let gate = swift("FileLoadGate.swift");
    let swift_status = |name: &str| -> i32 {
        gate.lines()
            .find_map(|l| {
                l.trim()
                    .strip_prefix(&format!("static let {name}: Int32 = "))
            })
            .unwrap_or_else(|| panic!("FileBridgeRefusal.{name} is missing"))
            .trim()
            .parse()
            .unwrap()
    };
    assert_eq!(swift_status("busyStatus"), rust_status("BUSY"));
    assert_eq!(swift_status("capacityStatus"), rust_status("CAPACITY"));
    assert_eq!(
        swift_status("invalidArgumentStatus"),
        rust_status("INVALID_ARGUMENT")
    );
    let header =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("include/zmux.h"))
            .unwrap();
    assert!(
        header.contains("ZMUX_BUSY = -4") && header.contains("ZMUX_CAPACITY = -5"),
        "the header and the Swift mapping must agree on the refusal statuses"
    );
    let bridge = swift("NativeBridge.swift");
    assert!(
        between(
            &bridge,
            "func file(_ request: FileHostRequest",
            "func pastePreparedImage"
        )
        .contains("FileBridgeRefusal(status: result.status)"),
        "NativeBridge.file must classify a nonzero status with FileBridgeRefusal"
    );
    let refusal = between(&gate, "struct FileBridgeRefusal", "/// What the gate needs");
    assert!(
        refusal.contains("case Self.busyStatus:")
            && refusal.contains("case Self.capacityStatus:")
            && refusal.matches("code: \"busy\"").count() == 2
            && refusal.matches("outcomeUnknown: false").count() == 3
            && refusal.matches("outcomeUnknown: true").count() == 1,
        "-4 and -5 are busy and ran nothing; -2 ran nothing; every other status is unknown: {refusal}"
    );

    // -5 really means nothing ran: reserve every reply buffer, send a request that changes the
    // model, observe the refusal, release, and see that the model is unchanged.
    let dir = TempDir::new();
    let path = dir.0.join("capacity.txt");
    std::fs::write(&path, "x\n").unwrap();
    let desktop = zmux_desktop_create();
    assert_eq!(desktop.status, 0);
    let desktop = desktop.value;
    snapshot(desktop, |v| {
        !v["model"]["workspaces"].as_array().unwrap().is_empty()
    });
    let (opened, _) = call(
        desktop,
        &json!({"op":"open","items":[{"kind":"path","value":path}],"focus":true}),
        &[],
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let state = snapshot(desktop, |v| {
        v["files"]["entries"]
            .as_array()
            .is_some_and(|e| !e.is_empty())
    });
    let (attached, _) = call(
        desktop,
        &json!({"op":"attach","epoch":state["files"]["epoch"],
                "surface_id":opened["result"]["items"][0]["surface"]["surface_id"],
                "host_id":Uuid::new_v4()}),
        &[],
    );
    assert_eq!(attached["ok"], true, "{attached}");
    let dirty = |model: &Value| find_file_surface(model).map(|s| s["file"]["dirty"].clone());
    assert_eq!(
        dirty(&snapshot(desktop, |_| true)["model"]),
        Some(json!(false))
    );

    let mut held = Vec::new();
    let refused = loop {
        let taken = zmux_desktop_snapshot(desktop, 0);
        if taken.status != 0 {
            assert_eq!(taken.status, rust_status("CAPACITY"));
            break taken.status;
        }
        held.push(taken.value);
        assert!(held.len() <= 16, "the reply buffer pool is larger than 16");
    };
    assert_eq!(refused, rust_status("CAPACITY"));
    let observe = json!({"op":"observe","owner":attached["owner"],"dirty":true});
    let (status, _) = call_status(desktop, &observe, &[]);
    assert_eq!(
        status,
        rust_status("CAPACITY"),
        "a file request with no free reply buffer is refused with ZMUX_CAPACITY"
    );
    for id in held {
        assert_eq!(zmux_buffer_release(id), 0);
    }
    // The refused observation changed nothing; sending it again (what Swift does) works.
    assert_eq!(
        dirty(&snapshot(desktop, |_| true)["model"]),
        Some(json!(false)),
        "a request refused with ZMUX_CAPACITY must not have run"
    );
    let (status, reply) = call_status(desktop, &observe, &[]);
    assert_eq!((status, &reply["ok"]), (0, &json!(true)), "{reply}");
    snapshot(desktop, |v| {
        find_file_surface(&v["model"]).is_some_and(|s| s["file"]["dirty"] == true)
    });
    assert_eq!(zmux_desktop_destroy(desktop), 0);
}

/// One JSON command through `zmux_desktop_dispatch` (queue acceptance) or
/// `zmux_desktop_dispatch_acknowledged` (the executor's own result).
fn dispatch_json(desktop: u64, payload: &Value, acknowledged: bool) -> (i32, Value) {
    let request =
        serde_json::to_vec(&json!({"version":1,"request_id":7,"payload":payload})).unwrap();
    // SAFETY: the encoded request storage is readable for the whole call.
    let result = unsafe {
        if acknowledged {
            zmux_desktop_dispatch_acknowledged(desktop, request.as_ptr(), request.len())
        } else {
            zmux_desktop_dispatch(desktop, request.as_ptr(), request.len())
        }
    };
    if result.status != 0 {
        return (result.status, Value::Null);
    }
    (0, serde_json::from_slice(&bytes_of(result.value)).unwrap())
}

/// The close queue used to finish a close as "succeeded" when `zmux_desktop_dispatch` answered
/// `ok:true`, which only means the command was queued. A close the core then refuses (here a file
/// tab the model holds as dirty; a terminal restoration in progress is refused the same way) left
/// the tab open with its discarded text shown as clean. This drives the real exports: plain
/// dispatch says `ok:true` for a close that is refused, and the acknowledged export that the
/// tracked closes now use reports the refusal and later the success.
#[test]
fn an_acknowledged_close_reports_the_cores_refusal_where_plain_dispatch_reports_only_queueing() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = TempDir::new();
    let path = dir.0.join("dirty.txt");
    std::fs::write(&path, "on disk\n").unwrap();
    let desktop = zmux_desktop_create();
    assert_eq!(desktop.status, 0);
    let desktop = desktop.value;
    snapshot(desktop, |v| {
        !v["model"]["workspaces"].as_array().unwrap().is_empty()
    });
    let (opened, _) = call(
        desktop,
        &json!({"op":"open","items":[{"kind":"path","value":path}],"focus":false}),
        &[],
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let surface = opened["result"]["items"][0]["surface"]["surface_id"]
        .as_u64()
        .expect("a file surface");
    let state = snapshot(desktop, |v| {
        v["files"]["entries"]
            .as_array()
            .is_some_and(|e| e.len() == 1)
    });
    let (attached, _) = call(
        desktop,
        &json!({"op":"attach","epoch":state["files"]["epoch"],"surface_id":surface,"host_id":Uuid::new_v4()}),
        &[],
    );
    assert_eq!(attached["ok"], true, "{attached}");
    // `observe` returns after the executor applied it, so the model is dirty from here on.
    let (observed, _) = call(
        desktop,
        &json!({"op":"observe","owner":attached["owner"],"dirty":true}),
        &[],
    );
    assert_eq!(observed["ok"], true, "{observed}");
    let present = |expected: bool| {
        let result = zmux_desktop_snapshot(desktop, 0);
        assert_eq!((result.status, result.value != 0), (0, true));
        let value: Value = serde_json::from_slice(&bytes_of(result.value)).unwrap();
        let mut ids = Vec::new();
        fn collect(value: &Value, out: &mut Vec<u64>) {
            match value {
                Value::Object(map) => {
                    if map.get("kind") == Some(&json!("file")) && map.get("file").is_some() {
                        out.push(map["id"].as_u64().expect("a surface id"));
                    }
                    map.values().for_each(|v| collect(v, out));
                }
                Value::Array(items) => items.iter().for_each(|v| collect(v, out)),
                _ => {}
            }
        }
        collect(&value["model"]["workspaces"], &mut ids);
        assert_eq!(ids.contains(&surface), expected, "{ids:?}");
        value
    };
    let close =
        json!({"command":"action","action":{"action":"close_surface","surface_id":surface}});

    // Plain dispatch: queued, so `ok:true`. The close itself is refused by the dirty guard, and
    // the acknowledged call below runs behind it in the same executor queue, so by the time it
    // answers the refused close has been executed and the tab is verifiably still there.
    let (status, queued) = dispatch_json(desktop, &close, false);
    assert_eq!((status, &queued["ok"]), (0, &json!(true)), "{queued}");

    let (status, refused) = dispatch_json(desktop, &close, true);
    assert_eq!(status, 0);
    assert_eq!(
        refused["ok"], false,
        "the core refused the close: {refused}"
    );
    assert_eq!(refused["outcome_unknown"], false, "{refused}");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unsaved changes"),
        "{refused}"
    );
    present(true);

    // Once the model is clean the same close runs, and the reply carries the executor's revision
    // of the snapshot that already shows the tab gone: no polling is needed. The revision counts
    // every executor publish, including unrelated asynchronous ones (the default terminal's output
    // frames), so a later snapshot may be newer than the reply; the deterministic claims are that
    // the reply is newer than the snapshot taken before the close and no newer than the one taken
    // after it, and that the later snapshot, which cannot predate the reply, shows the tab gone.
    let (observed, _) = call(
        desktop,
        &json!({"op":"observe","owner":attached["owner"],"dirty":false}),
        &[],
    );
    assert_eq!(observed["ok"], true, "{observed}");
    let before = present(true)["revision"].as_u64().expect("a revision");
    let (status, closed) = dispatch_json(desktop, &close, true);
    assert_eq!((status, &closed["ok"]), (0, &json!(true)), "{closed}");
    let after = present(false)["revision"].as_u64().expect("a revision");
    let replied = closed["revision"]
        .as_u64()
        .expect("the reply names a revision");
    assert!(
        before < replied && replied <= after,
        "the reply names the revision that shows the close: before {before}, reply {replied}, \
         after {after}"
    );

    // Only action and windows commands are acknowledged this way; anything else is refused before
    // it is queued.
    let (status, other) = dispatch_json(
        desktop,
        &json!({"command":"input","session_id":1,"text":"x"}),
        true,
    );
    assert_eq!((status, &other["ok"]), (0, &json!(false)), "{other}");
    assert_eq!(other["outcome_unknown"], false);
    assert_eq!(zmux_desktop_destroy(desktop), 0);

    // Swift drift guard: the tracked closes use the acknowledged export and a failed or unknown
    // acknowledgement is an error, so a refused close can never read as a success.
    let bridge = swift("NativeBridge.swift");
    let send = between(
        &bridge,
        "private func send(_ payload: [String: Any], acknowledged: Bool = false)",
        "nonisolated static func consume",
    );
    assert!(
        send.contains("zmux_desktop_dispatch_acknowledged(")
            && send.contains("guard response?[\"ok\"] as? Bool == true else")
            && send.contains("throw BridgeFailure(message: message)")
            && send.contains("outcome_unknown"),
        "the acknowledged send must surface a refusal as an error: {send}"
    );
    let header =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("include/zmux.h"))
            .unwrap();
    assert!(header.contains("zmux_desktop_dispatch_acknowledged("));
}

/// Reads a task-evidence file that sits next to the tracker the closure work maintains.
fn task_reference(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.agents/tasks/2026-10-14/05-command-ux-parity/references")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Contract test between the Swift panel and the closure tracker: the tracker copied the design's
/// "word wrap deferred" line after S4 delivered the toggle, so a later agent would have rebuilt a
/// finished feature. While the Swift defines the toggle, the CV-06 gates must say it is delivered
/// (with its real scope) and must not list it among the undelivered editor settings, and the
/// receipt's closing limitations paragraph must not list it either.
#[test]
fn the_tracker_and_receipt_say_the_word_wrap_toggle_is_delivered_while_the_swift_defines_it() {
    let panel = swift("FilePanelView.swift");
    let app = swift("App.swift");
    assert!(
        panel.contains("private static let wrapKey = \"filePanel.wordWrap\"")
            && panel.contains("func toggleWordWrap()")
            && app.contains("(\"Toggle Word Wrap\", #selector(toggleFileWordWrap(_:))"),
        "the Swift no longer defines the word wrap toggle (key, toggle and File menu item); \
         if it was removed on purpose, restore the CV-06 gate in closure-tracker.json to list \
         word wrap as not delivered and update this test"
    );

    let tracker: Value = serde_json::from_str(&task_reference("closure-tracker.json"))
        .expect("closure-tracker.json is valid JSON");
    let cv06 = tracker["features"]
        .as_array()
        .expect("features array")
        .iter()
        .find(|feature| feature["feature_id"] == "CV-06")
        .expect("CV-06 is in the tracker");
    let gates: Vec<&str> = cv06["remaining_gates"]
        .as_array()
        .expect("CV-06 remaining_gates")
        .iter()
        .map(|gate| gate.as_str().expect("gate is a string"))
        .collect();
    let mentioning: Vec<&str> = gates
        .iter()
        .copied()
        .filter(|gate| {
            let lower = gate.to_lowercase();
            lower.contains("word-wrap") || lower.contains("word wrap")
        })
        .collect();
    assert_eq!(
        mentioning.len(),
        1,
        "exactly one CV-06 gate should describe word wrap, found {mentioning:?}"
    );
    let gate = mentioning[0];
    assert!(
        gate.starts_with("Delivered in S4:") && gate.contains("filePanel.wordWrap"),
        "the CV-06 word wrap gate must say it was delivered and name its global default: {gate}"
    );
    let undelivered = gate
        .split_once("Not delivered:")
        .expect("the gate separates what was delivered from what was not")
        .1
        .to_lowercase();
    assert!(
        !undelivered.contains("word-wrap") && !undelivered.contains("word wrap"),
        "word wrap is listed as not delivered although the Swift defines it: {gate}"
    );

    let receipt = task_reference("cu14-file-panel.md");
    let section = receipt
        .split("### Remaining gates and limitations (not hidden)")
        .nth(1)
        .expect("the S5 receipt keeps its remaining gates section")
        .split("\n### ")
        .next()
        .unwrap()
        .replace('\n', " ");
    let not_delivered = section
        .split_once("editor settings")
        .expect("the receipt names the editor settings that remain")
        .1
        .split_once(')')
        .expect("the editor settings list is parenthesised")
        .0
        .to_lowercase();
    assert!(
        !not_delivered.contains("word wrap") && !not_delivered.contains("word-wrap"),
        "the receipt lists word wrap as undelivered although the Swift defines it: {not_delivered}"
    );
}

/// Collapses runs of whitespace (including the line breaks of wrapped Markdown) to single spaces.
fn flatten(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Contract test for the rollback consequence of session format 11. `session_document()` writes
/// `SESSION_DOCUMENT_VERSION` for every session (see the workspace-core test
/// `a_session_with_no_file_surface_is_still_saved_as_version_11`), so the bridge doc must not say
/// only sessions that carry a Schedules surface are version 11. Fails if the doc goes back to that
/// claim, drops the "every session" statement, or the constant moves without the doc.
#[test]
fn the_bridge_doc_states_that_every_saved_session_is_version_11() {
    assert_eq!(
        zmux_workspace_core::SESSION_DOCUMENT_VERSION,
        11,
        "the session format moved; update the version in docs/native-bridge.md and its rollback sentence"
    );
    let doc = flatten(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/native-bridge.md"),
        )
        .unwrap(),
    );
    assert!(
        !doc.contains("carrying a Schedules surface are session format version 11"),
        "docs/native-bridge.md again claims only sessions with Schedules surfaces are version 11"
    );
    for required in [
        "Every session this build saves is session format version 11, whether or not it has a Schedules surface.",
        "A build that predates version 11 refuses all of those documents, not only the ones with Schedules tabs",
        "Documents written by earlier builds (versions 1 to 10) restore unchanged.",
    ] {
        assert!(
            doc.contains(required),
            "docs/native-bridge.md lost: {required}"
        );
    }
}

/// Contract test between design section 7 and the tracker. The design says every deferred item
/// "stays a `remaining_gates` entry"; each item the design lists must be named by a gate of the
/// feature that owns it, so the tracker cannot under-report undelivered work. The check is on the
/// stated limitation text, not on any behaviour.
#[test]
fn the_tracker_names_every_deferred_item_of_design_section_7() {
    let tracker: Value = serde_json::from_str(&task_reference("closure-tracker.json"))
        .expect("closure-tracker.json is valid JSON");
    let gates_of = |id: &str| -> String {
        tracker["features"]
            .as_array()
            .expect("features array")
            .iter()
            .find(|feature| feature["feature_id"] == id)
            .unwrap_or_else(|| panic!("{id} is in the tracker"))["remaining_gates"]
            .as_array()
            .expect("remaining_gates")
            .iter()
            .map(|gate| gate.as_str().expect("gate is a string"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase()
    };
    // (feature, phrase that must appear in one of its gates, the design item it records)
    let required = [
        (
            "CU-14",
            "case-variant alias dedupe on case-insensitive volumes",
            "case-variant alias dedupe",
        ),
        ("CU-14", "real file watching", "real file watching"),
        (
            "CU-14",
            "persisted viewport and markdown view mode",
            "persisted viewport / Markdown view mode",
        ),
        ("CV-01", "real file watching", "real file watching"),
        (
            "CV-01",
            "persisted viewport and markdown view mode",
            "persisted viewport / Markdown view mode",
        ),
        ("CV-05", "real file watching", "real file watching"),
        (
            "CV-05",
            "persisted viewport of an image or pdf",
            "persisted viewport",
        ),
        (
            "CV-06",
            "in-place fallback writes",
            "in-place fallback writes",
        ),
        ("CV-06", "persisted editor viewport", "persisted viewport"),
        ("CV-06", "save as", "Save As"),
        (
            "CV-06",
            "autosave and crash recovery",
            "autosave and crash recovery",
        ),
        ("CU-14", "non-utf-8 paths", "non-UTF-8 paths"),
        (
            "CU-14",
            "remote (ssh) workspace files",
            "remote (SSH) files",
        ),
        ("CV-01", "relative file links", "relative links"),
        ("CV-05", "quick look", "Quick Look"),
    ];
    for (feature, phrase, item) in required {
        assert!(
            gates_of(feature).contains(phrase),
            "{feature} remaining_gates in closure-tracker.json no longer record the deferred item \
             `{item}` (expected a gate containing `{phrase}`)"
        );
    }
    // Section 7 itself must still list the items, otherwise this test checks a stale list.
    let design = flatten(&task_reference("cu14-file-panel-design.md")).to_lowercase();
    for item in [
        "case-variant alias dedupe on case-insensitive volumes",
        "in-place fallback writes",
        "persisted viewport/markdown view mode",
        "real file watching",
    ] {
        assert!(
            design.contains(item),
            "design section 7 no longer lists `{item}`; update this test"
        );
    }
    // The receipt's closing paragraph is the human summary of the same list.
    let receipt = flatten(&task_reference("cu14-file-panel.md")).to_lowercase();
    let section = receipt
        .split("### remaining gates and limitations (not hidden)")
        .nth(1)
        .expect("the S5 receipt keeps its remaining gates section")
        .split(" ### ")
        .next()
        .unwrap()
        .to_string();
    for item in [
        "case-variant alias dedupe",
        "in-place fallback writes",
        "persisted viewport",
        "markdown view mode",
        "real file watching",
    ] {
        assert!(
            section.contains(item),
            "the receipt's remaining-gates paragraph omits `{item}`"
        );
    }
}

/// Source pins for the banner ranking. The behaviour itself is exercised by the macOS fixture
/// (`tools/native-fixtures/file-editor-model.swift`, UNAVAILABLE on this machine).
#[test]
fn a_disk_state_report_never_replaces_an_error_or_recovery_banner() {
    let model = swift("FileEditorModel.swift");
    let yields = between(
        &model,
        "var yieldsToExternal: Bool {",
        "var isObsoleteAfterLoad",
    );
    assert!(
        yields.contains("case .none, .dirtyUnconfirmed, .saveResolved: return true")
            && yields.contains("default: return isExternal")
            && !yields.contains(".message")
            && !yields.contains(".saveUnknown")
            && !yields.contains(".ownerLost")
            && !yields.contains(".loadFailed"),
        "only an empty, informational or disk-state banner may yield to a disk-state banner"
    );
    let reported = between(
        &model,
        "mutating func externalStateReported(",
        "private mutating func externalReaction(",
    );
    assert!(
        reported.contains("if case .show = reaction, !banner.yieldsToExternal { return .none }"),
        "externalStateReported must drop a .show reaction while an error or recovery banner is showing"
    );
    let fixture = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/native-fixtures/file-editor-model.swift"),
    )
    .unwrap();
    for needed in [
        "does not replace \\(banner)",
        ".saveUnknown(7), .saveUnknown(0)",
        "a pending-save notice survives disk reports so its outcome can still resolve it",
    ] {
        assert!(
            fixture.contains(needed),
            "the macOS fixture lost its banner-ranking case `{needed}`"
        );
    }
    let panel = swift("FilePanelView.swift");
    let reason = between(
        &panel,
        "private var saveUnavailableReason: String {",
        "/// Saves the buffer.",
    );
    assert!(
        reason.contains("ownerToken == nil || staleOwner"),
        "a stale owner must not be reported as 'no changes to save'"
    );
}
