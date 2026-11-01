use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_desktop_runtime::{
    Desktop, Shell,
    agent_lifecycle::{Action, Activity, Connection, Event, Provider, Request, Target},
    control::{CONTROL_VERSION, ControlCommand, ControlRequest, ControlResponse, ControlServer},
};
use zmux_workspace_core::targets::TargetKind;

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        // Keep the Unix socket fixture short on macOS, where TMPDIR commonly
        // includes a long /var/folders prefix and sockaddr_un.sun_path is small.
        let path = PathBuf::from("/tmp").join(format!(
            "zagent-{}-{}",
            std::process::id(),
            Uuid::new_v4().simple()
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn desktop() -> Desktop {
    Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: None,
    })
    .unwrap()
}

fn live_snapshot(desktop: &Desktop) -> Arc<zmux_desktop_runtime::Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(snapshot) = desktop.snapshot()
            && !snapshot.control_frames.is_empty()
            && snapshot.control_frames.iter().all(|frame| {
                frame
                    .snapshot
                    .as_ref()
                    .is_some_and(|terminal| terminal.state == zmux_session_runtime::State::Running)
            })
        {
            return snapshot;
        }
        assert!(Instant::now() < deadline, "terminal did not become live");
        thread::sleep(Duration::from_millis(5));
    }
}

fn call(path: &std::path::Path, request_id: u64, command: ControlCommand) -> ControlResponse {
    let request = ControlRequest {
        version: CONTROL_VERSION,
        request_id,
        password: None,
        command,
    };
    let bytes = serde_json::to_vec(&request).unwrap();
    let mut stream = UnixStream::connect(path).unwrap();
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
    let mut length = [0; 4];
    stream.read_exact(&mut length).unwrap();
    let mut response = vec![0; u32::from_be_bytes(length) as usize];
    stream.read_exact(&mut response).unwrap();
    serde_json::from_slice(&response).unwrap()
}

fn send_action(path: &std::path::Path, request_id: u64, action: Action) -> ControlResponse {
    call(
        path,
        request_id,
        ControlCommand::AgentLifecycle {
            request: Request {
                request_id: Uuid::new_v4(),
                action,
            },
        },
    )
}

fn event(
    capability: Uuid,
    runtime_id: Uuid,
    generation: Uuid,
    sequence: u64,
    event: Event,
) -> Action {
    Action::Event {
        capability,
        runtime_id,
        generation,
        sequence,
        hook_id: None,
        event,
    }
}

fn hook_event(
    capability: Uuid,
    runtime_id: Uuid,
    generation: Uuid,
    sequence: u64,
    hook_id: Uuid,
    event: Event,
) -> Action {
    Action::Event {
        capability,
        runtime_id,
        generation,
        sequence,
        hook_id: Some(hook_id),
        event,
    }
}

fn register(
    target: Target,
    runtime_id: Uuid,
    generation: Uuid,
    provider_session_id: Uuid,
) -> Action {
    Action::Register {
        target,
        provider: Provider::ClaudeLocal,
        runtime_id,
        generation,
        provider_session_id,
    }
}

fn result(response: ControlResponse) -> Value {
    assert!(response.ok, "{:?}", response.error);
    response.result.unwrap()
}

#[test]
fn local_lifecycle_requires_the_exact_running_owner_and_fences_old_generations() {
    let directory = Directory::new();
    let socket = directory.0.join("control.sock");
    let desktop = Arc::new(desktop());
    let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
    let initial = live_snapshot(&desktop);
    let model = &initial.model;
    let workspace = &model.workspaces[0];
    let pane = workspace.root.panes()[0];
    let surface = pane.selected();
    let target = Target {
        identity_namespace: model.identity_namespace,
        surface_id: surface.id,
        surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
        session_id: surface.session_id.unwrap(),
    };

    let invalid_target = Target {
        identity_namespace: Uuid::new_v4(),
        ..target
    };
    let rejected = send_action(
        &socket,
        1,
        register(
            invalid_target,
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        ),
    );
    assert!(!rejected.ok);
    assert!(desktop.snapshot().unwrap().agent_lifecycle.is_empty());

    let provider_session_id = Uuid::new_v4();
    let runtime_one = Uuid::new_v4();
    let generation_one = Uuid::new_v4();
    let registered = result(send_action(
        &socket,
        2,
        register(target, runtime_one, generation_one, provider_session_id),
    ));
    let capability_one: Uuid = registered["capability"].as_str().unwrap().parse().unwrap();
    let initial_agent = live_snapshot(&desktop).agent_lifecycle[0].clone();
    assert_eq!(initial_agent.activity, Activity::Unknown);
    assert_eq!(initial_agent.connection, Connection::Connected);
    let snapshot_json = serde_json::to_value(desktop.snapshot().unwrap().as_ref()).unwrap();
    assert!(
        snapshot_json["agent_lifecycle"][0]
            .get("capability")
            .is_none()
    );

    result(send_action(
        &socket,
        3,
        event(
            capability_one,
            runtime_one,
            generation_one,
            1,
            Event::PromptSubmitted {
                prompt_id: Uuid::new_v4(),
            },
        ),
    ));
    assert_eq!(
        live_snapshot(&desktop).agent_lifecycle[0].activity,
        Activity::Working
    );

    result(send_action(
        &socket,
        4,
        event(
            capability_one,
            runtime_one,
            generation_one,
            2,
            Event::NeedsUser,
        ),
    ));
    assert_eq!(
        live_snapshot(&desktop).agent_lifecycle[0].activity,
        Activity::Waiting
    );

    result(send_action(
        &socket,
        5,
        event(
            capability_one,
            runtime_one,
            generation_one,
            3,
            Event::TurnStopped {
                state: zmux_desktop_runtime::agent_lifecycle::TurnStopState::Idle,
            },
        ),
    ));
    assert_eq!(
        live_snapshot(&desktop).agent_lifecycle[0].activity,
        Activity::Idle
    );

    let runtime_two = Uuid::new_v4();
    let generation_two = Uuid::new_v4();
    let registered = result(send_action(
        &socket,
        6,
        register(target, runtime_two, generation_two, provider_session_id),
    ));
    let capability_two: Uuid = registered["capability"].as_str().unwrap().parse().unwrap();
    assert_ne!(capability_one, capability_two);
    let refreshed = live_snapshot(&desktop).agent_lifecycle[0].clone();
    assert_eq!(refreshed.activity, Activity::Unknown);
    assert_eq!(refreshed.generation, generation_two);

    let stale = send_action(
        &socket,
        7,
        event(
            capability_one,
            runtime_one,
            generation_one,
            4,
            Event::TurnStopped {
                state: zmux_desktop_runtime::agent_lifecycle::TurnStopState::Idle,
            },
        ),
    );
    assert!(!stale.ok);
    assert_eq!(
        live_snapshot(&desktop).agent_lifecycle[0].activity,
        Activity::Unknown
    );

    let prompt_id = Uuid::new_v4();
    result(send_action(
        &socket,
        8,
        event(
            capability_two,
            runtime_two,
            generation_two,
            1,
            Event::PromptSubmitted { prompt_id },
        ),
    ));
    let duplicate = send_action(
        &socket,
        9,
        event(
            capability_two,
            runtime_two,
            generation_two,
            1,
            Event::PromptSubmitted { prompt_id },
        ),
    );
    assert!(duplicate.ok);
    let stale_sequence = send_action(
        &socket,
        10,
        event(
            capability_two,
            runtime_two,
            generation_two,
            1,
            Event::TurnStopped {
                state: zmux_desktop_runtime::agent_lifecycle::TurnStopState::Idle,
            },
        ),
    );
    assert!(!stale_sequence.ok);
    assert_eq!(
        live_snapshot(&desktop).agent_lifecycle[0].activity,
        Activity::Working
    );

    result(send_action(
        &socket,
        11,
        event(
            capability_two,
            runtime_two,
            generation_two,
            2,
            Event::SessionEnded,
        ),
    ));
    assert_eq!(
        live_snapshot(&desktop).agent_lifecycle[0].connection,
        Connection::Disconnected
    );
    let exact_reregister = send_action(
        &socket,
        12,
        register(target, runtime_two, generation_two, provider_session_id),
    );
    assert!(!exact_reregister.ok);
    let new_generation_reregister = send_action(
        &socket,
        13,
        register(target, Uuid::new_v4(), Uuid::new_v4(), provider_session_id),
    );
    assert!(!new_generation_reregister.ok);
    assert_eq!(
        live_snapshot(&desktop).agent_lifecycle[0].connection,
        Connection::Disconnected
    );

    let runtime_three = Uuid::new_v4();
    let generation_three = Uuid::new_v4();
    let provider_session_three = Uuid::new_v4();
    let registered = result(send_action(
        &socket,
        14,
        register(
            target,
            runtime_three,
            generation_three,
            provider_session_three,
        ),
    ));
    let capability_three: Uuid = registered["capability"].as_str().unwrap().parse().unwrap();
    result(send_action(
        &socket,
        15,
        event(
            capability_three,
            runtime_three,
            generation_three,
            1,
            Event::EvidenceLost,
        ),
    ));
    assert_eq!(
        live_snapshot(&desktop).agent_lifecycle[0].connection,
        Connection::Unknown
    );
    let resurrected = send_action(
        &socket,
        16,
        event(
            capability_three,
            runtime_three,
            generation_three,
            2,
            Event::Resumed,
        ),
    );
    assert!(!resurrected.ok);
    let retired_reregister = send_action(
        &socket,
        17,
        register(
            target,
            runtime_three,
            generation_three,
            provider_session_three,
        ),
    );
    assert!(!retired_reregister.ok);
    assert_eq!(
        live_snapshot(&desktop).agent_lifecycle[0].connection,
        Connection::Unknown
    );

    let next_provider_session = Uuid::new_v4();
    result(send_action(
        &socket,
        18,
        register(
            target,
            Uuid::new_v4(),
            Uuid::new_v4(),
            next_provider_session,
        ),
    ));
    let replaced = live_snapshot(&desktop).agent_lifecycle.clone();
    assert_eq!(replaced.len(), 1);
    assert_eq!(replaced[0].provider_session_id, next_provider_session);

    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn exact_hook_receipts_cover_new_and_same_run_alerts_and_reject_reuse() {
    let directory = Directory::new();
    let socket = directory.0.join("control.sock");
    let desktop = Arc::new(desktop());
    let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
    let initial = live_snapshot(&desktop);
    let model = &initial.model;
    let surface = model.workspaces[0].root.panes()[0].selected();
    let target = Target {
        identity_namespace: model.identity_namespace,
        surface_id: surface.id,
        surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
        session_id: surface.session_id.unwrap(),
    };
    let runtime_id = Uuid::new_v4();
    let generation = Uuid::new_v4();
    let provider_session_id = Uuid::new_v4();
    let registered = result(send_action(
        &socket,
        1,
        register(target, runtime_id, generation, provider_session_id),
    ));
    let capability: Uuid = registered["capability"].as_str().unwrap().parse().unwrap();

    let prompt_id = Uuid::new_v4();
    let prompt = result(send_action(
        &socket,
        2,
        hook_event(
            capability,
            runtime_id,
            generation,
            1,
            Uuid::new_v4(),
            Event::PromptSubmitted { prompt_id },
        ),
    ));
    assert_eq!(prompt["hook_disposition"], "not_handled");
    assert!(live_snapshot(&desktop).agent_alert_events.is_empty());

    let hook_id = Uuid::new_v4();
    let needs_user = hook_event(
        capability,
        runtime_id,
        generation,
        2,
        hook_id,
        Event::NeedsUser,
    );
    let first = result(send_action(&socket, 3, needs_user.clone()));
    assert_eq!(first["hook_disposition"], "alert_covered");

    // Replaying the exact request after its response was lost returns the
    // stored disposition and cannot append a second lifecycle alert.
    let replay = result(send_action(&socket, 4, needs_user));
    assert_eq!(replay["hook_disposition"], "alert_covered");

    // A new hook invocation can still be covered by the same run's admitted
    // alert even though the provider state is already Waiting.
    let same_run = result(send_action(
        &socket,
        5,
        hook_event(
            capability,
            runtime_id,
            generation,
            3,
            Uuid::new_v4(),
            Event::NeedsUser,
        ),
    ));
    assert_eq!(same_run["hook_disposition"], "alert_covered");

    let stop = hook_event(
        capability,
        runtime_id,
        generation,
        4,
        Uuid::new_v4(),
        Event::TurnStopped {
            state: zmux_desktop_runtime::agent_lifecycle::TurnStopState::Unknown,
        },
    );
    let stop_receipt = result(send_action(&socket, 6, stop.clone()));
    assert_eq!(stop_receipt["hook_disposition"], "handled_no_alert");
    let stop_replay = result(send_action(&socket, 7, stop));
    assert_eq!(stop_replay["hook_disposition"], "handled_no_alert");

    let idle = result(send_action(
        &socket,
        8,
        hook_event(
            capability,
            runtime_id,
            generation,
            5,
            Uuid::new_v4(),
            Event::TurnStopped {
                state: zmux_desktop_runtime::agent_lifecycle::TurnStopState::Idle,
            },
        ),
    ));
    assert_eq!(idle["hook_disposition"], "alert_covered");

    // Claude may invoke Stop after idle_prompt for the same provider turn.
    // The completed run owns that late hook: acknowledge it without regressing
    // activity to Unknown or adding another notification fallback.
    let late_stop = hook_event(
        capability,
        runtime_id,
        generation,
        6,
        Uuid::new_v4(),
        Event::TurnStopped {
            state: zmux_desktop_runtime::agent_lifecycle::TurnStopState::Unknown,
        },
    );
    let late_stop_receipt = result(send_action(&socket, 9, late_stop.clone()));
    assert_eq!(late_stop_receipt["hook_disposition"], "handled_no_alert");
    let late_stop_replay = result(send_action(&socket, 10, late_stop));
    assert_eq!(late_stop_replay["hook_disposition"], "handled_no_alert");
    let after_late_stop = live_snapshot(&desktop);
    assert_eq!(after_late_stop.agent_lifecycle[0].activity, Activity::Idle);
    assert_eq!(after_late_stop.agent_alert_events.len(), 2);

    // Explicit Stop continuation evidence is stronger: accept the current
    // completed prompt's working state while still suppressing a false
    // completion fallback. Local normalization distinguishes this from other
    // generic Resumed hooks.
    let continued_stop = hook_event(
        capability,
        runtime_id,
        generation,
        7,
        Uuid::new_v4(),
        Event::StopContinued,
    );
    let continued_receipt = result(send_action(&socket, 11, continued_stop.clone()));
    assert_eq!(continued_receipt["hook_disposition"], "handled_no_alert");
    let continued_replay = result(send_action(&socket, 12, continued_stop));
    assert_eq!(continued_replay["hook_disposition"], "handled_no_alert");

    let background_stop = hook_event(
        capability,
        runtime_id,
        generation,
        8,
        Uuid::new_v4(),
        Event::TurnStopped {
            state: zmux_desktop_runtime::agent_lifecycle::TurnStopState::Working,
        },
    );
    let background_receipt = result(send_action(&socket, 13, background_stop));
    assert_eq!(background_receipt["hook_disposition"], "handled_no_alert");
    let after_continued_stop = live_snapshot(&desktop);
    assert_eq!(
        after_continued_stop.agent_lifecycle[0].activity,
        Activity::Working
    );
    assert_eq!(after_continued_stop.agent_alert_events.len(), 2);

    let reused_id = send_action(
        &socket,
        14,
        hook_event(
            capability,
            runtime_id,
            generation,
            9,
            hook_id,
            Event::Resumed,
        ),
    );
    assert!(!reused_id.ok);
    assert!(
        reused_id
            .error
            .as_ref()
            .unwrap()
            .message
            .contains("hook ID was reused for a different event")
    );

    let accepted = live_snapshot(&desktop);
    assert_eq!(accepted.agent_alert_events.len(), 2);
    assert_eq!(
        accepted
            .notifications
            .iter()
            .filter(|notification| notification.agent_alert_kind.as_deref() == Some("needs_user"))
            .count(),
        1
    );
    assert_eq!(
        accepted
            .notifications
            .iter()
            .filter(|notification| notification.agent_alert_kind.as_deref() == Some("completed"))
            .count(),
        1
    );

    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}
