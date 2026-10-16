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
