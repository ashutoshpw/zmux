//! Actual desktop dispatch to a raw PTY; the child independently records input.
use serde_json::json;
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime::{Desktop, Request, Shell};

#[test]
fn semantic_pointer_reaches_raw_child_with_negotiated_protocol_bytes() {
    let directory = std::env::temp_dir().join(format!("zmux-pointer-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let script = directory.join("child.py");
    fs::write(
        &script,
        r#"import os,pathlib,select,sys,time,tty,json
root=pathlib.Path(sys.argv[1]);tty.setraw(0)
os.write(1,b'\x1b[?1002h\x1b[?1006hPOINTER_READY')
received=b''; phase='input'
while True:
    if (root/'mode').exists():
        config=json.loads((root/'mode').read_text());(root/'mode').unlink()
        received=b'';phase=config['phase']
        os.write(1,b'\x1b[?9l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l'+config['mode'].encode()+b'\r\n'+phase.encode())
    if select.select([0],[],[],.01)[0]:
        received+=os.read(0,4096)
        (root/'input.tmp').write_bytes(received)
        (root/'input.tmp').replace(root/phase)
"#,
    )
    .unwrap();
    let desktop = Desktop::new(Shell {
        program: "python3".into(),
        args: vec![
            "-u".into(),
            script.to_string_lossy().into_owned(),
            directory.to_string_lossy().into_owned(),
        ],
        cwd: None,
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let session = loop {
        if let Ok(snapshot) = desktop.snapshot()
            && let Some(frame) = snapshot.frames.iter().find(|f| {
                f.snapshot
                    .as_ref()
                    .is_some_and(|s| s.viewport.contents.contains("POINTER_READY"))
            })
        {
            break frame.session_id;
        }
        assert!(Instant::now() < deadline, "raw child not ready");
        thread::sleep(Duration::from_millis(5));
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for (i, action) in ["press", "motion", "release"].iter().enumerate() {
            let request: Request = serde_json::from_value(json!({"version":1,"request_id":i+1,"payload":{"command":"pointer","session_id":session,"event":{"action":action,"button":"left","row":2,"col":if i == 0 {4} else {5},"modifiers":0}}})).expect("public semantic pointer command must be accepted");
            desktop.submit(request).unwrap();
        }
        let expected = b"\x1b[<0;5;3M\x1b[<32;6;3M\x1b[<0;6;3m";
        loop {
            let observed = fs::read(directory.join("input")).unwrap_or_default();
            if observed.len() >= expected.len() {
                assert_eq!(observed, expected);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "child did not receive pointer reports: {observed:?}"
            );
            thread::sleep(Duration::from_millis(5));
        }
        // Mode changes originate in child output, independently of event input.
        // A following ordinary byte fences suppressed events in the same queue.
        let cases = [
            ("none", "", "left", 0, "!"),
            ("press_only", "\x1b[?9h", "right", 16, "\x1b[M\"%#!"),
            ("vt200", "\x1b[?1000h", "middle", 16, "\x1b[M1%#\x1b[M3%#!"),
            (
                "any_motion",
                "\x1b[?1003h\x1b[?1006h",
                "none",
                0,
                "\x1b[<35;5;3M!",
            ),
            (
                "utf8",
                "\x1b[?1000h\x1b[?1005h",
                "right",
                8,
                "\x1b[M*%#\x1b[M+%#!",
            ),
        ];
        let mut request_id = 10;
        for (name, mode, button, modifiers, expected) in cases {
            fs::write(
                directory.join("mode.tmp"),
                json!({"phase":name,"mode":mode}).to_string(),
            )
            .unwrap();
            fs::rename(directory.join("mode.tmp"), directory.join("mode")).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if desktop.snapshot().is_ok_and(|s| {
                    s.frames.iter().any(|f| {
                        f.snapshot
                            .as_ref()
                            .is_some_and(|v| v.viewport.contents.contains(name))
                    })
                }) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "child mode not published: {name}"
                );
                thread::sleep(Duration::from_millis(5));
            }
            for action in ["press", "motion", "release"] {
                request_id += 1;
                let request:Request=serde_json::from_value(json!({"version":1,"request_id":request_id,"payload":{"command":"pointer","session_id":session,"event":{"action":action,"button":button,"row":2,"col":4,"modifiers":modifiers}}})).unwrap();
                desktop.submit(request).unwrap();
            }
            request_id += 1;
            desktop
                .submit(Request {
                    version: 1,
                    request_id,
                    payload: zmux_desktop_runtime::Command::Input {
                        session_id: session,
                        text: "!".into(),
                    },
                })
                .unwrap();
            loop {
                let observed = fs::read(directory.join(name)).unwrap_or_default();
                if observed.ends_with(b"!") {
                    assert_eq!(observed, expected.as_bytes(), "mode {name}");
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "child input not observed: {name}: {observed:?}"
                );
                thread::sleep(Duration::from_millis(5));
            }
        }
    }));
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    fs::remove_dir_all(directory).unwrap();
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
}
