use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use serde_json::from_slice;
use std::{
    fs, io,
    os::unix::fs::DirBuilderExt,
    os::unix::fs::PermissionsExt,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_remote_runtime::scheduler::{
    DeadlineInput, JobFailureCode, JobState, MAX_DISPATCH_LATENESS_SECONDS, MAX_MESSAGE_BYTES,
    MAX_PENDING_JOBS, MAX_RESPONSE_BYTES, SCHEDULER_PROTOCOL_VERSION, SchedulerAction,
    SchedulerReply, SchedulerRequest, SchedulerResponse, SchedulerResult, SchedulerTarget,
    TargetIdentity,
};

const RESTORED_MARKER: &str = "@zmux_scheduler_server_incarnation";

const SCHEDULER_BINARY: &str = env!("CARGO_BIN_EXE_zmux-scheduler");
const SERVER_WORKERS: usize = 16;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        for _ in 0..5 {
            // AF_UNIX paths are limited to 104 bytes on macOS. Keep this root
            // short even when TMPDIR points into a long /var/folders path.
            let path = PathBuf::from("/tmp").join(format!("zsched-{}", Uuid::new_v4().simple()));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create private scheduler daemon test directory: {error}"),
            }
        }
        panic!("could not allocate a unique scheduler daemon test directory")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
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
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => panic!("could not check tmux availability: {error}"),
        }
        let socket = directory.join("tmux-test.sock");
        assert!(
            socket.as_os_str().as_encoded_bytes().len() < 104,
            "tmux fixture socket must fit the macOS AF_UNIX path limit"
        );
        Some(Self { socket })
    }

    fn start(&self, capture: &Path) {
        let command = format!(
            "stty raw -echo 2>/dev/null; exec /bin/cat > {}",
            capture.display()
        );
        let output = self.run(&[
            "new-session",
            "-d",
            "-s",
            "bound",
            "-x",
            "80",
            "-y",
            "24",
            "/bin/sh",
            "-c",
            &command,
        ]);
        assert_success(&output, "start isolated tmux server");
    }

    fn set_restored_marker(&self, value: &str) {
        let output = self.run(&["set-option", "-g", RESTORED_MARKER, value]);
        assert_success(&output, "set simulated restored tmux server marker");
    }

    fn server_pid(&self) -> u32 {
        let output = self.run(&["display-message", "-p", "-F", "#{pid}"]);
        assert_success(&output, "read isolated tmux server PID");
        String::from_utf8(output.stdout)
            .expect("tmux server PID is UTF-8")
            .trim()
            .parse()
            .expect("tmux server returned a numeric PID")
    }

    fn wait_for_stop(&self, pid: u32) {
        let deadline = Instant::now() + Duration::from_secs(3);
        let process = Pid::from_raw(pid as i32);
        loop {
            match kill(process, None) {
                Ok(()) => {
                    assert!(
                        Instant::now() < deadline,
                        "tmux server process {pid} did not stop after kill-server"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(nix::errno::Errno::ESRCH) => break,
                Err(error) => panic!("could not inspect isolated tmux server process: {error}"),
            }
        }
        if self.socket.exists() {
            fs::remove_file(&self.socket).expect("remove stale isolated tmux server socket");
        }
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

struct Fixture {
    root: PathBuf,
    capture: PathBuf,
    tmux: TmuxFixture,
}

impl Fixture {
    fn new(directory: &Path) -> Option<Self> {
        let tmux = TmuxFixture::new(directory)?;
        let capture = directory.join("bound.out");
        tmux.start(&capture);
        Some(Self {
            root: directory.join("state"),
            capture,
            tmux,
        })
    }

    fn rpc(&self, request: SchedulerRequest) -> SchedulerResponse {
        self.rpc_with_binary(request, None)
    }

    fn rpc_with_binary(
        &self,
        request: SchedulerRequest,
        binary: Option<&Path>,
    ) -> SchedulerResponse {
        let output = self.rpc_output_with_binary(request, binary);
        assert_success(&output, "run scheduler rpc");
        from_slice(&output.stdout).expect("parse scheduler rpc response")
    }

    fn rpc_output_with_binary(&self, request: SchedulerRequest, binary: Option<&Path>) -> Output {
        let mut command = Command::new(SCHEDULER_BINARY);
        command
            .arg("rpc")
            .env("ZMUX_SCHEDULER_ROOT", &self.root)
            .env("ZMUX_SCHEDULER_TMUX_SOCKET", &self.tmux.socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(binary) = binary {
            command.env("ZMUX_SCHEDULER_TMUX_BIN", binary);
        }
        let mut child = command.spawn().expect("spawn scheduler rpc helper");
        let bytes = serde_json::to_vec(&request).expect("serialize scheduler request");
        let mut input = child.stdin.take().expect("rpc stdin");
        use std::io::Write;
        input.write_all(&bytes).expect("write rpc JSON");
        input.write_all(b"\n").expect("terminate rpc JSON line");
        drop(input);

        child.wait_with_output().expect("wait for scheduler rpc")
    }

    fn start_daemon(&self) -> Daemon {
        self.start_daemon_with_binary(None)
    }

    fn start_daemon_with_binary(&self, binary: Option<&Path>) -> Daemon {
        let mut command = Command::new(SCHEDULER_BINARY);
        command
            .arg("serve")
            .env("ZMUX_SCHEDULER_ROOT", &self.root)
            .env("ZMUX_SCHEDULER_TMUX_SOCKET", &self.tmux.socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(binary) = binary {
            command.env("ZMUX_SCHEDULER_TMUX_BIN", binary);
        }
        let mut child = command.spawn().expect("spawn foreground scheduler daemon");
        let socket = self.root.join("control.sock");
        assert!(
            socket.as_os_str().as_encoded_bytes().len() < 104,
            "daemon fixture socket must fit the macOS AF_UNIX path limit"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if UnixStream::connect(&socket).is_ok() {
                return Daemon { child };
            }
            if let Some(status) = child.try_wait().expect("inspect scheduler daemon") {
                panic!("scheduler daemon exited before serving requests: {status}");
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("scheduler daemon did not create its control socket");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

struct Daemon {
    child: Child,
}

impl Daemon {
    fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn continue_after_pause(&self) {
        let pid = Pid::from_raw(self.child.id() as i32);
        kill(pid, Signal::SIGCONT).expect("continue paused scheduler daemon");
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn assert_success(output: &Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn request(action: SchedulerAction) -> SchedulerRequest {
    SchedulerRequest {
        protocol_version: SCHEDULER_PROTOCOL_VERSION,
        request_id: None,
        action,
    }
}

fn create_request(target_id: String, seconds: u64, message: &str) -> SchedulerRequest {
    create_request_with_submit(target_id, seconds, message, false)
}

fn create_request_with_submit(
    target_id: String,
    seconds: u64,
    message: &str,
    submit: bool,
) -> SchedulerRequest {
    SchedulerRequest {
        protocol_version: SCHEDULER_PROTOCOL_VERSION,
        request_id: Some(Uuid::new_v4()),
        action: SchedulerAction::Create {
            target_id,
            deadline: DeadlineInput::InSeconds { seconds },
            message: message.into(),
            submit,
        },
    }
}

fn discovered_target(response: SchedulerResponse) -> String {
    discovered_target_info(response).id
}

fn discovered_target_info(response: SchedulerResponse) -> SchedulerTarget {
    let SchedulerResult::Success(SchedulerReply::Targets { targets }) = response.result else {
        panic!("target discovery failed: {response:?}");
    };
    targets
        .into_iter()
        .find(|target| target.display.session_name == "bound")
        .expect("bound tmux target")
}

fn write_pause_wrapper(directory: &Path, pause_after: &str) -> (PathBuf, PathBuf) {
    assert!(matches!(pause_after, "load-buffer" | "delete-buffer"));
    let wrapper = directory.join("tmux-wrapper");
    let ready = directory.join("pause-ready");
    let script = format!(
        "#!/bin/sh\ntmux \"$@\"\nstatus=$?\nif [ \"$status\" -eq 0 ]; then\n  case \" $* \" in\n    *' {pause_after} '*) if [ ! -e '{ready}' ]; then : > '{ready}'; kill -STOP \"$PPID\"; fi;;\n  esac\nfi\nexit \"$status\"\n",
        ready = ready.display(),
    );
    fs::write(&wrapper, script).expect("write real-tmux pause wrapper");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))
        .expect("make real-tmux wrapper executable");
    (wrapper, ready)
}

fn write_logging_wrapper(directory: &Path) -> (PathBuf, PathBuf) {
    let wrapper = directory.join("tmux-log-wrapper");
    let log = directory.join("tmux.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\nexec tmux \"$@\"\n",
        log = log.display(),
    );
    fs::write(&wrapper, script).expect("write tmux invocation logger");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))
        .expect("make tmux invocation logger executable");
    (wrapper, log)
}

fn wait_until_file_exists(path: &Path, timeout: Duration, description: &str) {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn show_job(fixture: &Fixture, id: Uuid) -> zmux_remote_runtime::scheduler::ScheduledJob {
    let response = fixture.rpc(request(SchedulerAction::Show { job_id: id }));
    let SchedulerResult::Success(SchedulerReply::Job { job }) = response.result else {
        panic!("show scheduler job failed: {response:?}");
    };
    job
}

#[test]
fn helper_binary_recovers_overdue_and_dispatches_future_jobs_without_a_client() {
    let scratch = Scratch::new();
    let Some(fixture) = Fixture::new(&scratch.0) else {
        eprintln!("skipping scheduler daemon integration test because tmux is not installed");
        return;
    };
    let daemon = fixture.start_daemon();
    let target = discovered_target(fixture.rpc(request(SchedulerAction::Targets)));
    let future_message = "future job dispatched while no client is connected";
    let future = fixture.rpc(create_request(target.clone(), 6, future_message));
    let SchedulerResult::Success(SchedulerReply::Mutation { job: future, .. }) = future.result
    else {
        panic!("future job creation through RPC failed: {future:?}");
    };
    // Create the one-second deadline last so the daemon can be stopped before
    // it becomes due. Recovery, rather than the live daemon, must mark it missed.
    let overdue = fixture.rpc(create_request(target, 1, "recover as missed"));
    let SchedulerResult::Success(SchedulerReply::Mutation { job: overdue, .. }) = overdue.result
    else {
        panic!("overdue job creation through RPC failed: {overdue:?}");
    };
    daemon.stop();

    // Let the accepted deadline pass while the service is stopped. Restart must
    // recover the overdue job as missed and leave future work pending.
    thread::sleep(Duration::from_millis(1_200));
    let restarted = fixture.start_daemon();
    let history = fixture.rpc(request(SchedulerAction::History { limit: Some(10) }));
    let SchedulerResult::Success(SchedulerReply::History { jobs, .. }) = history.result else {
        panic!("history through restarted RPC service failed: {history:?}");
    };
    assert_eq!(
        jobs.iter()
            .find(|item| item.id == overdue.id)
            .map(|item| item.state),
        Some(JobState::Missed)
    );
    let pending = fixture.rpc(request(SchedulerAction::Show { job_id: future.id }));
    let SchedulerResult::Success(SchedulerReply::Job { job: pending }) = pending.result else {
        panic!("show through restarted RPC service failed: {pending:?}");
    };
    assert_eq!(
        pending.state,
        JobState::Pending,
        "a future accepted job must remain pending through restart"
    );

    // No RPC client stays connected while the daemon waits for and dispatches
    // the future job. The owned pane's byte capture proves real tmux delivery.
    let capture_deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if fs::read(&fixture.capture).is_ok_and(|bytes| bytes == future_message.as_bytes()) {
            break;
        }
        assert!(
            Instant::now() < capture_deadline,
            "restarted daemon did not deliver the future job to its bound pane; observed {:?}",
            fs::read(&fixture.capture).ok()
        );
        thread::sleep(Duration::from_millis(20));
    }
    let shown = fixture.rpc(request(SchedulerAction::Show { job_id: future.id }));
    let SchedulerResult::Success(SchedulerReply::Job { job: future }) = shown.result else {
        panic!("show through restarted RPC service failed: {shown:?}");
    };
    assert_eq!(future.state, JobState::Sent);
    restarted.stop();
}

#[test]
fn target_discovery_does_not_write_the_legacy_server_marker() {
    let scratch = Scratch::new();
    let Some(fixture) = Fixture::new(&scratch.0) else {
        eprintln!("skipping scheduler daemon integration test because tmux is not installed");
        return;
    };
    let daemon = fixture.start_daemon();
    let before = fixture.tmux.run(&["show-option", "-gqv", RESTORED_MARKER]);
    assert_success(&before, "read absent legacy tmux server marker");
    assert!(before.stdout.is_empty());

    let _ = discovered_target_info(fixture.rpc(request(SchedulerAction::Targets)));

    let after = fixture.tmux.run(&["show-option", "-gqv", RESTORED_MARKER]);
    assert_success(
        &after,
        "confirm target discovery did not write legacy marker",
    );
    assert!(
        after.stdout.is_empty(),
        "Targets wrote a legacy marker: {after:?}"
    );
    daemon.stop();
}

#[test]
fn restored_legacy_marker_cannot_authorize_a_restarted_tmux_server() {
    let scratch = Scratch::new();
    let Some(fixture) = Fixture::new(&scratch.0) else {
        eprintln!("skipping scheduler daemon integration test because tmux is not installed");
        return;
    };
    let daemon = fixture.start_daemon();
    let restored_marker = Uuid::new_v4().to_string();
    fixture.tmux.set_restored_marker(&restored_marker);
    let original = discovered_target_info(fixture.rpc(request(SchedulerAction::Targets)));
    let original_identity = TargetIdentity::from_target_id(&original.id).unwrap();
    let message = "must not reach a recreated pane";
    let created = fixture.rpc(create_request(original.id.clone(), 8, message));
    let SchedulerResult::Success(SchedulerReply::Mutation { job, .. }) = created.result else {
        panic!("create before tmux restart failed: {created:?}");
    };

    let server_pid = fixture.tmux.server_pid();
    let stopped = fixture.tmux.run(&["kill-server"]);
    assert_success(&stopped, "stop original isolated tmux server");
    fixture.tmux.wait_for_stop(server_pid);
    fixture.tmux.start(&fixture.capture);
    fixture.tmux.set_restored_marker(&restored_marker);
    let replacement = discovered_target_info(fixture.rpc(request(SchedulerAction::Targets)));
    let replacement_identity = TargetIdentity::from_target_id(&replacement.id).unwrap();
    assert_eq!(
        replacement_identity.session_id,
        original_identity.session_id
    );
    assert_eq!(replacement_identity.pane_id, original_identity.pane_id);
    assert_ne!(replacement.id, original.id);
    assert!(
        replacement_identity.server_pid != original_identity.server_pid
            || replacement_identity.server_start_time != original_identity.server_start_time
            || replacement_identity.server_incarnation != original_identity.server_incarnation,
        "replacement server retained the complete old identity"
    );

    let rejected = fixture.rpc(create_request(
        original_identity.target_id().unwrap(),
        10,
        message,
    ));
    let SchedulerResult::Error(error) = rejected.result else {
        panic!("Create accepted a token from the prior server incarnation: {rejected:?}");
    };
    assert_eq!(
        error.code,
        zmux_remote_runtime::scheduler::SchedulerErrorCode::TargetChanged
    );

    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let current = show_job(&fixture, job.id);
        if current.state == JobState::Failed {
            assert_eq!(current.failure_code, Some(JobFailureCode::TargetChanged));
            break;
        }
        assert_ne!(
            current.state,
            JobState::Missed,
            "test deadline expired before dispatch could validate the restarted server"
        );
        assert!(
            Instant::now() < deadline,
            "restarted daemon did not reject the persisted old target; job={current:?}"
        );
        assert!(
            fs::read(&fixture.capture).map_or(true, |bytes| bytes.is_empty()),
            "restarted daemon delivered to a recreated pane: {:?}",
            fs::read(&fixture.capture).ok()
        );
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        fs::read(&fixture.capture).map_or(true, |bytes| bytes.is_empty()),
        "restarted daemon delivered to a recreated pane: {:?}",
        fs::read(&fixture.capture).ok()
    );
    daemon.stop();
}

#[test]
fn list_show_and_dispatch_follow_a_renamed_reindexed_window_for_the_same_pane() {
    let scratch = Scratch::new();
    let Some(fixture) = Fixture::new(&scratch.0) else {
        eprintln!("skipping scheduler daemon integration test because tmux is not installed");
        return;
    };
    let daemon = fixture.start_daemon();
    let target = discovered_target_info(fixture.rpc(request(SchedulerAction::Targets)));
    let identity = TargetIdentity::from_target_id(&target.id).unwrap();
    let message = "delivery follows immutable pane after window rename and reindex";
    let created = fixture.rpc(create_request(target.id, 6, message));
    let SchedulerResult::Success(SchedulerReply::Mutation { job, .. }) = created.result else {
        panic!("create before target display change failed: {created:?}");
    };

    for (args, operation) in [
        (
            &["rename-session", "-t", "bound", "renamed"][..],
            "rename tmux session",
        ),
        (
            &["rename-window", "-t", "renamed:0", "renamed-window"][..],
            "rename bound tmux window",
        ),
        (
            &["move-window", "-s", "renamed:0", "-t", "renamed:4"][..],
            "reindex bound tmux window",
        ),
    ] {
        let output = fixture.tmux.run(args);
        assert_success(&output, operation);
    }

    let shown = show_job(&fixture, job.id);
    assert_eq!(shown.target.session_id, identity.session_id);
    assert_eq!(shown.target.pane_id, identity.pane_id);
    assert_eq!(shown.display.session_name, "renamed");
    assert_eq!(shown.display.window_name, "renamed-window");
    assert_eq!(shown.display.window_index, 4);
    let listed = fixture.rpc(request(SchedulerAction::List { limit: Some(100) }));
    let SchedulerResult::Success(SchedulerReply::Jobs { jobs }) = listed.result else {
        panic!("list after target display change failed: {listed:?}");
    };
    let listed = jobs
        .iter()
        .find(|item| item.id == job.id)
        .expect("listed job");
    assert_eq!(listed.display.session_name, "renamed");
    assert_eq!(listed.display.window_name, "renamed-window");
    assert_eq!(listed.display.window_index, 4);

    let capture_deadline = Instant::now() + Duration::from_secs(9);
    loop {
        if fs::read(&fixture.capture).is_ok_and(|bytes| bytes == message.as_bytes()) {
            break;
        }
        assert!(
            Instant::now() < capture_deadline,
            "daemon did not deliver to the unchanged pane ID after move; observed {:?}",
            fs::read(&fixture.capture).ok()
        );
        thread::sleep(Duration::from_millis(20));
    }
    let delivered = Instant::now() + Duration::from_secs(2);
    while show_job(&fixture, job.id).state != JobState::Sent {
        assert!(
            Instant::now() < delivered,
            "job was not marked sent after pane delivery"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let stopped = fixture.tmux.run(&["kill-server"]);
    assert_success(&stopped, "stop tmux after pane delivery");
    let retained = show_job(&fixture, job.id);
    assert_eq!(retained.display.session_name, "renamed");
    assert_eq!(retained.display.window_name, "renamed-window");
    assert_eq!(retained.display.window_index, 4);
    daemon.stop();
}

#[test]
fn helper_list_bounds_unicode_labels_and_serializes_one_hundred_maximum_messages() {
    let scratch = Scratch::new();
    let Some(fixture) = Fixture::new(&scratch.0) else {
        eprintln!("skipping scheduler daemon integration test because tmux is not installed");
        return;
    };
    let (wrapper, log) = write_logging_wrapper(&scratch.0);
    let daemon = fixture.start_daemon_with_binary(Some(&wrapper));
    let target = discovered_target_info(fixture.rpc(request(SchedulerAction::Targets)));

    // Each label is 256 UTF-8 bytes while occupying 128 tmux display columns.
    let unicode_label = "🧪".repeat(64);
    let rename_session = fixture
        .tmux
        .run(&["rename-session", "-t", "bound", &unicode_label]);
    assert_success(&rename_session, "rename tmux session with Unicode label");
    let target_name = format!("{unicode_label}:0");
    let rename_window = fixture
        .tmux
        .run(&["rename-window", "-t", &target_name, &unicode_label]);
    assert_success(&rename_window, "rename tmux window with Unicode label");
    let set_pane_title =
        fixture
            .tmux
            .run(&["select-pane", "-t", &target_name, "-T", &unicode_label]);
    assert_success(&set_pane_title, "set tmux pane title with Unicode label");

    let message = "\n".repeat(MAX_MESSAGE_BYTES);
    for _ in 0..MAX_PENDING_JOBS {
        let created = fixture.rpc(create_request(target.id.clone(), 3_600, &message));
        let SchedulerResult::Success(SchedulerReply::Mutation { job, .. }) = created.result else {
            panic!("create maximum-size pending message failed: {created:?}");
        };
        assert_eq!(job.message.len(), MAX_MESSAGE_BYTES);
    }

    // Creation uses one target verification each. Only inspect tmux calls made
    // by the list request to verify display refresh is a single bounded snapshot.
    fs::write(&log, b"").expect("reset tmux invocation log before list");
    let output = fixture.rpc_output_with_binary(
        request(SchedulerAction::List {
            limit: Some(MAX_PENDING_JOBS as u16),
        }),
        None,
    );
    assert_success(&output, "run maximum-size scheduler list RPC");
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    let response_bytes = output.stdout.len() - 1;
    assert!(
        response_bytes <= MAX_RESPONSE_BYTES,
        "100-job list response is {response_bytes} bytes, limit is {MAX_RESPONSE_BYTES}"
    );
    let response: SchedulerResponse =
        from_slice(&output.stdout).expect("parse maximum-size scheduler list response");
    let SchedulerResult::Success(SchedulerReply::Jobs { jobs }) = response.result else {
        panic!("maximum-size scheduler list failed: {response:?}");
    };
    assert_eq!(jobs.len(), MAX_PENDING_JOBS);
    let expected_label = "🧪".repeat(32);
    for job in &jobs {
        assert_eq!(job.message, message);
        for label in [
            &job.display.session_name,
            &job.display.window_name,
            &job.display.pane_title,
        ] {
            assert_eq!(label.len(), 128);
            assert_eq!(label, &expected_label);
        }
    }

    let tmux_calls = fs::read_to_string(&log).expect("read tmux invocation log");
    let calls: Vec<_> = tmux_calls.lines().collect();
    assert_eq!(
        calls.len(),
        1,
        "list should use one tmux subprocess: {calls:?}"
    );
    assert!(calls[0].contains(" list-panes -a -F "), "{calls:?}");
    daemon.stop();
}

#[test]
fn live_daemon_marks_stale_pre_paste_jobs_missed_and_skips_late_enter() {
    run_lateness_pause_case("load-buffer", false);
    run_lateness_pause_case("delete-buffer", true);
}

fn run_lateness_pause_case(pause_after: &str, submit: bool) {
    let scratch = Scratch::new();
    let Some(fixture) = Fixture::new(&scratch.0) else {
        eprintln!("skipping scheduler daemon integration test because tmux is not installed");
        return;
    };
    let (wrapper, ready) = write_pause_wrapper(&scratch.0, pause_after);
    let daemon = fixture.start_daemon_with_binary(Some(&wrapper));
    let target = discovered_target(fixture.rpc(request(SchedulerAction::Targets)));
    let message = if submit {
        "text accepted before late Enter check"
    } else {
        "late pre-paste input must be skipped"
    };
    let created = fixture.rpc(create_request_with_submit(target, 1, message, submit));
    let SchedulerResult::Success(SchedulerReply::Mutation { job, .. }) = created.result else {
        panic!("create before daemon pause failed: {created:?}");
    };
    wait_until_file_exists(&ready, Duration::from_secs(6), "tmux pause checkpoint");
    thread::sleep(Duration::from_secs(MAX_DISPATCH_LATENESS_SECONDS + 1));
    daemon.continue_after_pause();

    let final_deadline = Instant::now() + Duration::from_secs(5);
    let current = loop {
        let current = show_job(&fixture, job.id);
        let is_final = if submit {
            current.state == JobState::OutcomeUnknown
        } else {
            current.state == JobState::Missed
        };
        if is_final {
            break current;
        }
        assert!(
            Instant::now() < final_deadline,
            "daemon did not settle job after late pause: {current:?}"
        );
        thread::sleep(Duration::from_millis(20));
    };
    if submit {
        let receipt = current
            .dispatch
            .expect("dispatch receipt after accepted text");
        assert_eq!(
            receipt.text,
            zmux_remote_runtime::scheduler::DeliveryPhase::Accepted
        );
        assert_eq!(
            receipt.submit,
            zmux_remote_runtime::scheduler::DeliveryPhase::NotStarted
        );
        assert_eq!(
            receipt.last_durable_phase,
            zmux_remote_runtime::scheduler::DispatchCheckpoint::TextAccepted
        );
        assert_eq!(
            fs::read(&fixture.capture).expect("captured accepted text"),
            message.as_bytes()
        );
    } else {
        assert!(current.dispatch.is_some_and(|receipt| {
            receipt.text == zmux_remote_runtime::scheduler::DeliveryPhase::NotStarted
        }));
        assert!(
            fs::read(&fixture.capture).map_or(true, |bytes| bytes.is_empty()),
            "pre-paste lateness check still wrote input: {:?}",
            fs::read(&fixture.capture).ok()
        );
    }
    daemon.stop();
}

#[test]
fn helper_binary_rejects_rpc_when_all_request_workers_are_busy() {
    let scratch = Scratch::new();
    let Some(fixture) = Fixture::new(&scratch.0) else {
        eprintln!("skipping scheduler daemon integration test because tmux is not installed");
        return;
    };
    let daemon = fixture.start_daemon();
    let socket = fixture.root.join("control.sock");
    let mut stalled_clients = Vec::with_capacity(SERVER_WORKERS);
    for _ in 0..SERVER_WORKERS {
        let mut stream = UnixStream::connect(&socket).expect("connect stalled scheduler client");
        use std::io::Write;
        stream.write_all(b"{\"protocol_version\":1,").unwrap();
        stalled_clients.push(stream);
    }
    // The foreground loop accepts one connection per 100 ms tick. Keep all
    // workers occupied with incomplete frames before sending the overflow RPC.
    thread::sleep(Duration::from_millis(1_800));

    let response = fixture.rpc(request(SchedulerAction::Status {
        receipt_request_id: None,
    }));
    let SchedulerResult::Error(error) = response.result else {
        panic!("overflow request should receive a bounded busy response: {response:?}");
    };
    assert_eq!(
        error.code,
        zmux_remote_runtime::scheduler::SchedulerErrorCode::Busy
    );

    drop(stalled_clients);
    daemon.stop();
}

#[test]
fn helper_binary_reports_its_scheduler_protocol_version() {
    let output = Command::new(SCHEDULER_BINARY)
        .arg("protocol-version")
        .output()
        .expect("run helper protocol-version command");
    assert_success(&output, "read scheduler protocol version");
    assert_eq!(output.stdout, b"1\n");
}
