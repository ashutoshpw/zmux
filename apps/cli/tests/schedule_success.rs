//! Successful CLI scheduling flow through the real scheduler store and tmux.
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
};
use uuid::Uuid;
use zmux_remote_runtime::scheduler::{SchedulerRequest, SchedulerService};

const SURFACE: &str = "00000000-0000-0000-0000-000000000010";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = PathBuf::from("/tmp").join(format!(
            "zsched-ok-{}-{}",
            std::process::id(),
            Uuid::new_v4().simple()
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }

    fn command(&self, socket: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zmux"));
        command
            .arg("--socket")
            .arg(socket)
            .args(["--timeout", "10000"])
            .args(args)
            .env("HOME", &self.0);
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("ZMUX_") {
                command.env_remove(name);
            }
        }
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

struct TmuxFixture {
    socket: PathBuf,
}

impl TmuxFixture {
    fn new(directory: &Path) -> Option<Self> {
        match Command::new("tmux").arg("-V").output() {
            Ok(output) if output.status.success() => {}
            Ok(output) => panic!(
                "tmux -V failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => panic!("could not check tmux availability: {error}"),
        }
        let fixture = Self {
            socket: directory.join("tmux.sock"),
        };
        let output = fixture.run(&[
            "new-session",
            "-d",
            "-s",
            "bound",
            "-x",
            "80",
            "-y",
            "24",
            "/bin/cat",
        ]);
        assert_success(&output, "create isolated tmux session");
        Some(fixture)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new("tmux")
            .args(["-f", "/dev/null", "-S"])
            .arg(&self.socket)
            .arg("-u")
            .args(args)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .output()
            .expect("run isolated tmux command")
    }
}

impl Drop for TmuxFixture {
    fn drop(&mut self) {
        let _ = self.run(&["kill-server"]);
    }
}

fn assert_success(output: &Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_frame(stream: &mut UnixStream) -> Value {
    let mut length = [0; 4];
    stream.read_exact(&mut length).unwrap();
    let mut bytes = vec![0; u32::from_be_bytes(length) as usize];
    stream.read_exact(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn write_frame(stream: &mut UnixStream, response: &Value) {
    let bytes = serde_json::to_vec(response).unwrap();
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
}

fn scheduler_control_server(
    path: &Path,
    service: SchedulerService,
    request_count: usize,
) -> thread::JoinHandle<()> {
    let listener = UnixListener::bind(path).unwrap();
    thread::spawn(move || {
        for _ in 0..request_count {
            let (mut stream, _) = listener.accept().unwrap();
            let local_request = read_frame(&mut stream);
            assert_eq!(local_request["version"], 2);
            let local_request_id = local_request["request_id"].clone();
            let command = &local_request["command"];
            assert_eq!(command["method"], "schedule");
            assert_eq!(command["surface"], SURFACE);
            let scheduler_request: SchedulerRequest =
                serde_json::from_value(command["request"].clone()).unwrap();
            let reply = service.handle(scheduler_request);
            write_frame(
                &mut stream,
                &json!({
                    "version": 2,
                    "request_id": local_request_id,
                    "ok": true,
                    "result": {
                        "ok": true,
                        "reply": reply,
                        "outcome_unknown": false
                    }
                }),
            );
        }
    })
}

fn run_cli(fixture: &Fixture, socket: &Path, args: &[&str]) -> (Value, String) {
    let output = fixture.command(socket, args).output().unwrap();
    assert_success(&output, &format!("zmux {}", args.join(" ")));
    (
        serde_json::from_slice(&output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

#[test]
fn cli_create_show_combined_edit_cancel_and_receipt_status_use_real_store() {
    let fixture = Fixture::new();
    let Some(tmux) = TmuxFixture::new(&fixture.0) else {
        eprintln!("skipping CLI scheduler integration because tmux is not installed");
        return;
    };
    let service = SchedulerService::open_with_tmux(
        fixture.0.join("scheduler-state"),
        Some(tmux.socket.clone()),
        None,
    )
    .expect("open real scheduler service against isolated tmux");
    service
        .recover_on_start()
        .expect("recover isolated service state");

    let control_directory = fixture.0.join("control");
    fs::create_dir(&control_directory).unwrap();
    fs::set_permissions(&control_directory, fs::Permissions::from_mode(0o700)).unwrap();
    let control_socket = control_directory.join("c");
    let server = scheduler_control_server(&control_socket, service, 11);

    let (targets, _) = run_cli(
        &fixture,
        &control_socket,
        &["schedule", "targets", "--surface", SURFACE],
    );
    assert_eq!(targets["response"]["result"]["data"]["kind"], "targets");
    let token = targets["response"]["result"]["data"]["targets"][0]["id"]
        .as_str()
        .expect("real tmux target token");

    let (created, create_stderr) = run_cli(
        &fixture,
        &control_socket,
        &[
            "schedule",
            "create",
            "--surface",
            SURFACE,
            "--remote-pane",
            token,
            "--in",
            "1h",
            "--message",
            "original schedule message",
        ],
    );
    let create_request_id = created["request_id"]
        .as_str()
        .expect("CLI-generated create request UUID");
    assert!(create_stderr.contains(&format!("schedule request_id={create_request_id}")));
    assert_eq!(created["response"]["result"]["data"]["kind"], "mutation");
    let create_job = &created["response"]["result"]["data"]["job"];
    assert_eq!(create_job["version"], 1);
    assert_eq!(create_job["message"], "original schedule message");
    let task_id = create_job["id"].as_str().unwrap();
    let original_deadline = create_job["deadline_utc"].as_str().unwrap().to_owned();

    let (shown, _) = run_cli(
        &fixture,
        &control_socket,
        &["schedule", "show", task_id, "--surface", SURFACE],
    );
    assert_eq!(shown["response"]["result"]["data"]["kind"], "job");
    assert_eq!(shown["response"]["result"]["data"]["job"]["version"], 1);

    let edit_request_id = Uuid::from_u128(0x1234).to_string();
    let (edited, _) = run_cli(
        &fixture,
        &control_socket,
        &[
            "schedule",
            "edit",
            task_id,
            "--surface",
            SURFACE,
            "--expect-version",
            "1",
            "--in",
            "2h",
            "--message",
            "combined patch schedule message",
            "--request-id",
            &edit_request_id,
        ],
    );
    assert_eq!(edited["request_id"], edit_request_id);
    assert_eq!(edited["response"]["result"]["data"]["kind"], "mutation");
    let edited_job = &edited["response"]["result"]["data"]["job"];
    assert_eq!(edited_job["version"], 2);
    assert_eq!(edited_job["message"], "combined patch schedule message");
    assert_ne!(edited_job["deadline_utc"], original_deadline);

    let (shown_again, _) = run_cli(
        &fixture,
        &control_socket,
        &["schedule", "show", task_id, "--surface", SURFACE],
    );
    assert_eq!(
        shown_again["response"]["result"]["data"]["job"]["version"],
        2
    );

    let (listed, _) = run_cli(
        &fixture,
        &control_socket,
        &["schedule", "list", "--surface", SURFACE],
    );
    assert_eq!(
        listed["response"]["result"]["data"]["jobs"][0]["id"],
        task_id
    );
    let relative = fixture
        .command(
            &control_socket,
            &["schedule", "list", "--surface", SURFACE, "--relative"],
        )
        .output()
        .unwrap();
    assert_success(&relative, "relative schedule list");
    let text = String::from_utf8(relative.stdout).unwrap();
    assert!(
        text.contains(task_id) && text.contains("in 01:") && text.contains("pending"),
        "{text}"
    );
    assert!(text.contains("snapshot"));

    let (cancelled, cancel_stderr) = run_cli(
        &fixture,
        &control_socket,
        &[
            "schedule",
            "cancel",
            task_id,
            "--surface",
            SURFACE,
            "--expect-version",
            "2",
        ],
    );
    let cancel_request_id = cancelled["request_id"].as_str().unwrap();
    assert!(cancel_stderr.contains(&format!("schedule request_id={cancel_request_id}")));
    let cancelled_job = &cancelled["response"]["result"]["data"]["job"];
    assert_eq!(cancelled_job["state"], "cancelled");
    assert_eq!(cancelled_job["version"], 3);

    for (request_id, version) in [
        (create_request_id, 1),
        (&edit_request_id, 2),
        (cancel_request_id, 3),
    ] {
        let (status, _) = run_cli(
            &fixture,
            &control_socket,
            &[
                "schedule",
                "status",
                "--surface",
                SURFACE,
                "--request-id",
                request_id,
            ],
        );
        assert_eq!(
            status["response"]["result"]["data"]["kind"],
            "request_status"
        );
        assert_eq!(
            status["response"]["result"]["data"]["receipt"]["request_id"],
            request_id
        );
        assert_eq!(
            status["response"]["result"]["data"]["receipt"]["job_version"],
            version
        );
    }
    server.join().unwrap();

    let journal_path = fixture
        .0
        .join(".config/zmux/schedule-requests/requests.json");
    let journal: Value = serde_json::from_slice(&fs::read(journal_path).unwrap()).unwrap();
    let requests = journal["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 3);
    for (id, operation) in [
        (create_request_id, "create"),
        (&edit_request_id, "edit"),
        (cancel_request_id, "cancel"),
    ] {
        let saved = requests
            .iter()
            .find(|request| request["request_id"] == id)
            .expect("local journal retains mutation request");
        assert_eq!(saved["operation"], operation);
        assert_eq!(saved["state"], "resolved");
    }
}
