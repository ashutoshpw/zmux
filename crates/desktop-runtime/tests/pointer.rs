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
        r#"import os,pathlib,select,sys,time,tty
root=pathlib.Path(sys.argv[1]);tty.setraw(0)
os.write(1,b'\x1b[?1002h\x1b[?1006hPOINTER_READY')
received=b''
while True:
    if select.select([0],[],[],5)[0]:
        received+=os.read(0,4096)
        (root/'input.tmp').write_bytes(received)
        (root/'input.tmp').replace(root/'input')
    else: break
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
        if let Ok(snapshot) = desktop.snapshot() {
            if let Some(frame) = snapshot.frames.iter().find(|f| {
                f.snapshot
                    .as_ref()
                    .is_some_and(|s| s.viewport.contents.contains("POINTER_READY"))
            }) {
                break frame.session_id;
            }
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
    }));
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    fs::remove_dir_all(directory).unwrap();
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
}
