use super::*;
use crate::scheduler::store::MAX_RECEIPTS;
use crate::scheduler::{
    DeadlineInput, EditPatch, JobState, SCHEDULER_PROTOCOL_VERSION, SchedulerAction,
    SchedulerReply, SchedulerRequest, SchedulerResult,
};
use std::os::unix::fs::DirBuilderExt;
use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};
use time::OffsetDateTime;

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
                Err(error) => panic!("create private scheduler test directory: {error}"),
            }
        }
        panic!("could not allocate a unique scheduler test directory")
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

    fn add_capture_session(&self, name: &str, capture: &Path) {
        self.add_capture_session_with_bracketed_paste(name, capture, false, None);
    }

    fn add_bracketed_capture_session(&self, name: &str, capture: &Path, ready: &Path) {
        self.add_capture_session_with_bracketed_paste(name, capture, true, Some(ready));
    }

    fn add_capture_session_with_bracketed_paste(
        &self,
        name: &str,
        capture: &Path,
        bracketed_paste: bool,
        ready: Option<&Path>,
    ) {
        let enable_bracketed_paste = if bracketed_paste {
            format!(
                "printf '\\033[?2004h'; /bin/sleep 0.1; printf ready > {}; ",
                ready
                    .expect("bracketed capture needs a readiness path")
                    .display()
            )
        } else {
            String::new()
        };
        let command = format!(
            "stty raw -echo 2>/dev/null; {enable_bracketed_paste}exec /bin/cat > {}",
            capture.display()
        );
        let output = self.run(&[
            "new-session",
            "-d",
            "-s",
            name,
            "-x",
            "80",
            "-y",
            "24",
            "/bin/sh",
            "-c",
            &command,
        ]);
        assert_success(&output, "create tmux capture session");
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

fn make_service(root: &Path, tmux: &TmuxFixture) -> SchedulerService {
    SchedulerService::open_with_tmux(root.to_path_buf(), Some(tmux.socket.clone()), None)
        .expect("open scheduler with explicit isolated tmux configuration")
}

fn request(request_id: Option<Uuid>, action: SchedulerAction) -> SchedulerRequest {
    SchedulerRequest {
        protocol_version: SCHEDULER_PROTOCOL_VERSION,
        request_id,
        action,
    }
}

fn target_id(service: &SchedulerService, session_name: &str) -> String {
    let response = service.handle(request(None, SchedulerAction::Targets));
    let SchedulerResult::Success(SchedulerReply::Targets { targets }) = response.result else {
        panic!("target discovery failed: {response:?}");
    };
    targets
        .into_iter()
        .find(|target| target.display.session_name == session_name)
        .unwrap_or_else(|| panic!("tmux session {session_name:?} was not discovered"))
        .id
}

fn create(
    service: &SchedulerService,
    target_id: String,
    request_id: Uuid,
    seconds: u64,
    message: String,
    submit: bool,
) -> SchedulerResponse {
    service.handle(request(
        Some(request_id),
        SchedulerAction::Create {
            target_id,
            deadline: DeadlineInput::InSeconds { seconds },
            message,
            submit,
        },
    ))
}

fn mutation_job(response: SchedulerResponse) -> crate::scheduler::ScheduledJob {
    let SchedulerResult::Success(SchedulerReply::Mutation { job, .. }) = response.result else {
        panic!("mutation failed: {response:?}");
    };
    job
}

fn error(response: SchedulerResponse) -> crate::scheduler::SchedulerError {
    let SchedulerResult::Error(error) = response.result else {
        panic!("expected mutation error: {response:?}");
    };
    error
}

fn service_with_tmux(scratch: &Scratch, tmux: &TmuxFixture) -> SchedulerService {
    make_service(&scratch.0.join("state"), tmux)
}

#[test]
fn create_replays_the_original_receipt_and_rejects_request_id_reuse() {
    let scratch = Scratch::new();
    let Some(tmux) = TmuxFixture::new(&scratch.0) else {
        eprintln!("skipping scheduler integration test because tmux is not installed");
        return;
    };
    tmux.add_capture_session("bound", &scratch.0.join("bound.out"));
    let service = service_with_tmux(&scratch, &tmux);
    let target = target_id(&service, "bound");
    let request_id = Uuid::new_v4();
    let action = SchedulerAction::Create {
        target_id: target.clone(),
        deadline: DeadlineInput::InSeconds { seconds: 3_600 },
        message: "first payload".into(),
        submit: false,
    };
    let original = service.handle(request(Some(request_id), action.clone()));
    let replay = service.handle(request(Some(request_id), action));
    assert_eq!(
        replay, original,
        "exact retries must return the saved response"
    );

    let conflict = service.handle(request(
        Some(request_id),
        SchedulerAction::Create {
            target_id: target,
            deadline: DeadlineInput::InSeconds { seconds: 3_600 },
            message: "different payload".into(),
            submit: false,
        },
    ));
    assert_eq!(error(conflict).code, SchedulerErrorCode::RequestIdConflict);

    let jobs = service.store.read(|state| state.jobs.clone()).unwrap();
    assert_eq!(jobs.len(), 1, "a replay must not create a second job");
}

#[test]
fn edit_cancel_and_replay_enforce_job_versions() {
    let scratch = Scratch::new();
    let Some(tmux) = TmuxFixture::new(&scratch.0) else {
        eprintln!("skipping scheduler integration test because tmux is not installed");
        return;
    };
    tmux.add_capture_session("bound", &scratch.0.join("bound.out"));
    let service = service_with_tmux(&scratch, &tmux);
    let job = mutation_job(create(
        &service,
        target_id(&service, "bound"),
        Uuid::new_v4(),
        3_600,
        "original".into(),
        false,
    ));
    let shown = service.handle(request(None, SchedulerAction::Show { job_id: job.id }));
    let SchedulerResult::Success(SchedulerReply::Job { job: shown }) = shown.result else {
        panic!("show should return the created job: {shown:?}");
    };
    assert_eq!(shown, job);

    let edit_id = Uuid::new_v4();
    let edit_action = SchedulerAction::Edit {
        job_id: job.id,
        expected_version: 1,
        patch: EditPatch {
            deadline: None,
            message: Some("edited".into()),
        },
    };
    let edited_response = service.handle(request(Some(edit_id), edit_action.clone()));
    let edited = mutation_job(edited_response.clone());
    assert_eq!(edited.version, 2);
    assert_eq!(edited.message, "edited");
    assert_eq!(
        edited.deadline_utc, job.deadline_utc,
        "message-only edits preserve the deadline"
    );
    assert_eq!(
        service.handle(request(Some(edit_id), edit_action)),
        edited_response,
        "an edit retry must return its original receipt"
    );
    let edit_status = service.handle(request(
        None,
        SchedulerAction::Status {
            receipt_request_id: Some(edit_id),
        },
    ));
    let SchedulerResult::Success(SchedulerReply::RequestStatus { receipt }) = edit_status.result
    else {
        panic!("edit receipt status should be available: {edit_status:?}");
    };
    assert_eq!(receipt.request_id, edit_id);
    assert_eq!(receipt.job_id, Some(job.id));
    assert_eq!(receipt.job_version, Some(2));

    let stale = service.handle(request(
        Some(Uuid::new_v4()),
        SchedulerAction::Edit {
            job_id: job.id,
            expected_version: 1,
            patch: EditPatch {
                deadline: Some(DeadlineInput::InSeconds { seconds: 7_200 }),
                message: None,
            },
        },
    ));
    let stale_error = error(stale);
    assert_eq!(stale_error.code, SchedulerErrorCode::StaleVersion);
    assert_eq!(stale_error.current_version, Some(2));

    let cancel_id = Uuid::new_v4();
    let cancel_action = SchedulerAction::Cancel {
        job_id: job.id,
        expected_version: 2,
    };
    let cancelled_response = service.handle(request(Some(cancel_id), cancel_action.clone()));
    let cancelled = mutation_job(cancelled_response.clone());
    assert_eq!(cancelled.version, 3);
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert_eq!(
        service.handle(request(Some(cancel_id), cancel_action)),
        cancelled_response,
        "a cancel retry must return its original receipt"
    );
    let cancel_status = service.handle(request(
        None,
        SchedulerAction::Status {
            receipt_request_id: Some(cancel_id),
        },
    ));
    let SchedulerResult::Success(SchedulerReply::RequestStatus { receipt }) = cancel_status.result
    else {
        panic!("cancel receipt status should be available: {cancel_status:?}");
    };
    assert_eq!(receipt.request_id, cancel_id);
    assert_eq!(receipt.job_id, Some(job.id));
    assert_eq!(receipt.job_version, Some(3));
}

#[test]
fn receipt_capacity_preserves_one_cancel_slot_per_pending_job() {
    let scratch = Scratch::new();
    let Some(tmux) = TmuxFixture::new(&scratch.0) else {
        eprintln!("skipping scheduler integration test because tmux is not installed");
        return;
    };
    tmux.add_capture_session("bound", &scratch.0.join("bound.out"));
    let service = service_with_tmux(&scratch, &tmux);
    let target = target_id(&service, "bound");
    let first = mutation_job(create(
        &service,
        target.clone(),
        Uuid::new_v4(),
        3_600,
        "first".into(),
        false,
    ));
    let second = mutation_job(create(
        &service,
        target.clone(),
        Uuid::new_v4(),
        3_600,
        "second".into(),
        false,
    ));

    service
        .store
        .transact(|state| {
            let pending = state
                .jobs
                .iter()
                .filter(|job| job.state == JobState::Pending)
                .count();
            let fill = MAX_RECEIPTS - state.receipts.len() - pending;
            let expiry = receipt_expiry(OffsetDateTime::now_utc());
            for index in 0..fill {
                let request_id = Uuid::new_v4();
                let receipt = RequestReceipt {
                    request_id,
                    operation: MutationKind::Edit,
                    state: ReceiptState::Rejected,
                    job_id: None,
                    job_version: None,
                    received_at_utc: format_utc(OffsetDateTime::now_utc()),
                    completed_at_utc: Some(format_utc(OffsetDateTime::now_utc())),
                };
                let response = SchedulerResponse::error(
                    Some(request_id),
                    SchedulerError::new(
                        SchedulerErrorCode::StaleVersion,
                        "retained fixture receipt",
                    ),
                );
                state.receipts.push(StoredReceipt {
                    fingerprint: format!("{index:064x}"),
                    receipt,
                    response,
                    expires_at_unix_nanos: expiry,
                });
            }
            Ok(((), true))
        })
        .expect("fill the retained receipt ledger while preserving cancel reservations");

    let edit = service.handle(request(
        Some(Uuid::new_v4()),
        SchedulerAction::Edit {
            job_id: first.id,
            expected_version: 1,
            patch: EditPatch {
                deadline: None,
                message: Some("cannot spend cancel reserve".into()),
            },
        },
    ));
    assert_eq!(error(edit).code, SchedulerErrorCode::Capacity);
    let create = service.handle(request(
        Some(Uuid::new_v4()),
        SchedulerAction::Create {
            target_id: target,
            deadline: DeadlineInput::InSeconds { seconds: 3_600 },
            message: "cannot add another pending reservation".into(),
            submit: false,
        },
    ));
    assert_eq!(error(create).code, SchedulerErrorCode::Capacity);

    for job_id in [first.id, second.id] {
        let cancel = service.handle(request(
            Some(Uuid::new_v4()),
            SchedulerAction::Cancel {
                job_id,
                expected_version: 1,
            },
        ));
        assert_eq!(mutation_job(cancel).state, JobState::Cancelled);
    }
    let (receipt_count, pending_count) = service
        .store
        .read(|state| {
            (
                state.receipts.len(),
                state
                    .jobs
                    .iter()
                    .filter(|job| job.state == JobState::Pending)
                    .count(),
            )
        })
        .unwrap();
    assert_eq!(receipt_count, MAX_RECEIPTS);
    assert_eq!(pending_count, 0);
}

#[test]
fn due_dispatch_delivers_literal_multiline_text_to_the_bound_pane() {
    let scratch = Scratch::new();
    let Some(tmux) = TmuxFixture::new(&scratch.0) else {
        eprintln!("skipping scheduler integration test because tmux is not installed");
        return;
    };
    let bound_capture = scratch.0.join("bound.out");
    let decoy_capture = scratch.0.join("decoy.out");
    tmux.add_capture_session("bound", &bound_capture);
    tmux.add_capture_session("decoy", &decoy_capture);
    let selected = tmux.run(&["select-pane", "-t", "decoy:0.0"]);
    assert_success(&selected, "select the decoy pane");

    let service = service_with_tmux(&scratch, &tmux);
    let message = format!(
        "literal ; $(touch {})\nsecond line with \"quotes\" and `ticks`",
        scratch.0.join("shell-side-effect").display()
    );
    let job = mutation_job(create(
        &service,
        target_id(&service, "bound"),
        Uuid::new_v4(),
        1,
        message.clone(),
        false,
    ));

    let dispatch_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if service
            .dispatch_due_once()
            .expect("claim and dispatch due job")
        {
            break;
        }
        assert!(Instant::now() < dispatch_deadline, "job never became due");
        thread::sleep(Duration::from_millis(20));
    }

    let capture_deadline = Instant::now() + Duration::from_secs(2);
    let captured = loop {
        match fs::read(&bound_capture) {
            Ok(bytes) if bytes == message.as_bytes() => break bytes,
            Ok(_) | Err(_) => {}
        }
        assert!(
            Instant::now() < capture_deadline,
            "pane did not capture the scheduled text; observed {:?}",
            fs::read(&bound_capture).ok()
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        captured,
        message.as_bytes(),
        "delivery must preserve all literal bytes without adding a newline"
    );
    assert_eq!(
        fs::read(&decoy_capture).unwrap_or_default(),
        b"",
        "dispatch must not follow the selected pane"
    );
    let buffers = tmux.run(&["list-buffers", "-F", "#{buffer_name}"]);
    assert_success(&buffers, "list tmux buffers after dispatch");
    assert!(
        !String::from_utf8_lossy(&buffers.stdout)
            .lines()
            .any(|name| name == format!("zmux-schedule-{}", job.id.simple())),
        "dispatch must clean up its temporary paste buffer"
    );
    assert!(
        !scratch.0.join("shell-side-effect").exists(),
        "message content must not execute as shell syntax"
    );
    let final_job = service
        .store
        .read(|state| state.jobs.iter().find(|item| item.id == job.id).cloned())
        .unwrap()
        .unwrap();
    assert_eq!(final_job.state, JobState::Sent);
}

#[test]
fn dispatch_grace_clock_is_sampled_after_waiting_for_the_store_lock() {
    let scratch = Scratch::new();
    let Some(tmux) = TmuxFixture::new(&scratch.0) else {
        eprintln!("skipping scheduler integration test because tmux is not installed");
        return;
    };
    tmux.add_capture_session("bound", &scratch.0.join("bound.out"));
    let service = service_with_tmux(&scratch, &tmux);
    let job = mutation_job(create(
        &service,
        target_id(&service, "bound"),
        Uuid::new_v4(),
        3_600,
        "must become missed while waiting for the store lock".into(),
        false,
    ));

    // Make the deadline three seconds old before starting the dispatch call.
    // It is initially inside the five-second grace, then falls outside it while
    // the external process lock prevents the claim transaction from starting.
    let past_deadline = format_utc(OffsetDateTime::now_utc() - time::Duration::seconds(3));
    service
        .store
        .transact(|state| {
            let pending = state
                .jobs
                .iter_mut()
                .find(|pending| pending.id == job.id)
                .expect("created pending job");
            pending.deadline_utc = past_deadline.clone();
            pending.deadline_input = DeadlineInput::At {
                rfc3339: past_deadline.clone(),
            };
            Ok(((), true))
        })
        .expect("move deadline inside grace before lock contention");

    let lock_path = service.store.root().join("scheduler.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .expect("open scheduler lock for contention test");
    lock.lock().expect("hold scheduler store lock");
    let dispatch_service = service.clone();
    let dispatch = thread::spawn(move || {
        dispatch_service
            .dispatch_due_once()
            .expect("dispatch after store lock is released")
    });
    thread::sleep(Duration::from_millis(100));
    thread::sleep(Duration::from_millis(2_200));
    drop(lock);

    assert!(
        !dispatch.join().expect("join blocked dispatch attempt"),
        "a job that aged past its grace while waiting must not be claimed"
    );
    let stored = service
        .store
        .read(|state| {
            state
                .jobs
                .iter()
                .find(|item| item.id == job.id)
                .unwrap()
                .clone()
        })
        .unwrap();
    assert_eq!(stored.state, JobState::Missed);
    assert!(
        stored.dispatch.is_none(),
        "pre-claim missed jobs must not acquire a dispatch receipt"
    );
}

#[test]
fn dispatch_uses_bracketed_paste_only_when_the_application_enabled_it() {
    let scratch = Scratch::new();
    let Some(tmux) = TmuxFixture::new(&scratch.0) else {
        eprintln!("skipping scheduler integration test because tmux is not installed");
        return;
    };
    let capture = scratch.0.join("bracketed.out");
    let ready = scratch.0.join("bracketed.ready");
    tmux.add_bracketed_capture_session("bracketed", &capture, &ready);
    let mode_deadline = Instant::now() + Duration::from_secs(2);
    while !ready.exists() {
        assert!(
            Instant::now() < mode_deadline,
            "application did not enable bracketed paste"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let service = service_with_tmux(&scratch, &tmux);
    let message = "literal first line\nsecond line without a trailing newline";
    let job = mutation_job(create(
        &service,
        target_id(&service, "bracketed"),
        Uuid::new_v4(),
        1,
        message.into(),
        false,
    ));
    let dispatch_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if service
            .dispatch_due_once()
            .expect("claim and dispatch due job")
        {
            break;
        }
        assert!(Instant::now() < dispatch_deadline, "job never became due");
        thread::sleep(Duration::from_millis(20));
    }

    let expected = format!("\u{1b}[200~{message}\u{1b}[201~");
    let capture_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if fs::read(&capture).is_ok_and(|bytes| bytes == expected.as_bytes()) {
            break;
        }
        assert!(
            Instant::now() < capture_deadline,
            "bracketed-paste application did not receive the expected delimiters"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let buffers = tmux.run(&["list-buffers", "-F", "#{buffer_name}"]);
    assert_success(&buffers, "list tmux buffers after bracketed dispatch");
    assert!(
        !String::from_utf8_lossy(&buffers.stdout)
            .lines()
            .any(|name| name == format!("zmux-schedule-{}", job.id.simple())),
        "bracketed dispatch must clean up its temporary paste buffer"
    );
}

#[test]
fn restart_marks_overdue_and_interrupted_jobs_without_replaying_them() {
    let scratch = Scratch::new();
    let Some(tmux) = TmuxFixture::new(&scratch.0) else {
        eprintln!("skipping scheduler integration test because tmux is not installed");
        return;
    };
    tmux.add_capture_session("bound", &scratch.0.join("bound.out"));
    let service = service_with_tmux(&scratch, &tmux);
    let target = target_id(&service, "bound");
    let overdue = mutation_job(create(
        &service,
        target.clone(),
        Uuid::new_v4(),
        3_600,
        "miss me".into(),
        false,
    ));
    let interrupted = mutation_job(create(
        &service,
        target,
        Uuid::new_v4(),
        3_600,
        "do not replay".into(),
        true,
    ));

    let past_deadline = format_utc(OffsetDateTime::now_utc() - time::Duration::seconds(5));
    service
        .store
        .transact(|state| {
            let overdue = state
                .jobs
                .iter_mut()
                .find(|job| job.id == overdue.id)
                .unwrap();
            overdue.deadline_utc = past_deadline;
            let interrupted = state
                .jobs
                .iter_mut()
                .find(|job| job.id == interrupted.id)
                .unwrap();
            interrupted.state = JobState::Dispatching;
            interrupted.dispatch = Some(DispatchReceipt {
                text: DeliveryPhase::NotStarted,
                submit: DeliveryPhase::NotStarted,
                last_durable_phase: DispatchCheckpoint::Claimed,
            });
            Ok(((), true))
        })
        .expect("persist pre-restart states");

    // Construct a new service over the same durable store, as the daemon does
    // after a process restart, then run its startup recovery pass.
    let restarted = make_service(&scratch.0.join("state"), &tmux);
    restarted.recover_on_start().expect("recover after restart");
    let recovered = restarted.store.read(|state| state.jobs.clone()).unwrap();
    let overdue = recovered.iter().find(|job| job.id == overdue.id).unwrap();
    assert_eq!(overdue.state, JobState::Missed);
    let interrupted = recovered
        .iter()
        .find(|job| job.id == interrupted.id)
        .unwrap();
    assert_eq!(interrupted.state, JobState::OutcomeUnknown);
    assert_eq!(
        interrupted.dispatch,
        Some(DispatchReceipt {
            text: DeliveryPhase::Unknown,
            submit: DeliveryPhase::Unknown,
            last_durable_phase: DispatchCheckpoint::Claimed,
        })
    );
}
