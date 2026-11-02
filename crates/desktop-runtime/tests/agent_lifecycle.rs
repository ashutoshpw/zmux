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
    agent_lifecycle::{Action, Activity, AlertKind, Connection, Event, Provider, Request, Target},
    control::{CONTROL_VERSION, ControlCommand, ControlRequest, ControlResponse, ControlServer},
};
use zmux_remote_runtime::agent_lifecycle::{
    ProviderKind, ProviderLifecycleObservation, ProviderMarker, ProviderScope, ProviderState,
    ProviderTransition,
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

fn provider_observation(
    provider: ProviderKind,
    session_id: &str,
    turn_id: Option<&str>,
    state: ProviderState,
    transition: ProviderTransition,
    marker: ProviderMarker,
) -> ProviderLifecycleObservation {
    ProviderLifecycleObservation {
        provider,
        scope: ProviderScope::Root,
        session_id: session_id.into(),
        turn_id: turn_id.map(str::to_owned),
        state,
        transition,
        marker,
    }
}

fn provider_observation_action(
    target: Target,
    runtime_id: Uuid,
    generation: Uuid,
    sequence: u64,
    observation: ProviderLifecycleObservation,
) -> Action {
    Action::ProviderObservation {
        target,
        runtime_id,
        generation,
        sequence,
        observation,
    }
}

fn provider_evidence_lost_action(
    target: Target,
    provider: ProviderKind,
    session_id: &str,
    runtime_id: Uuid,
    generation: Uuid,
    sequence: u64,
) -> Action {
    Action::ProviderEvidenceLost {
        target,
        provider,
        session_id: session_id.into(),
        runtime_id,
        generation,
        sequence,
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

#[test]
fn generic_provider_observations_are_local_control_actions_with_explicit_ends() {
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

    let codex_runtime = Uuid::new_v4();
    let codex_generation = Uuid::new_v4();
    let codex_session = "codex_session_7";
    let observation = ProviderLifecycleObservation {
        provider: ProviderKind::Codex,
        scope: ProviderScope::Root,
        session_id: codex_session.into(),
        turn_id: Some("codex_turn_8".into()),
        state: ProviderState::Working,
        transition: ProviderTransition::StateChanged,
        marker: ProviderMarker::Live,
    };
    let receipt = result(send_action(
        &socket,
        1,
        Action::ProviderObservation {
            target,
            runtime_id: codex_runtime,
            generation: codex_generation,
            sequence: 1,
            observation,
        },
    ));
    assert!(receipt.get("capability").is_none());
    let codex = live_snapshot(&desktop)
        .agent_lifecycle
        .iter()
        .find(|agent| agent.provider == Provider::CodexLocal)
        .cloned()
        .expect("the control action creates the local Codex snapshot");
    assert_eq!(codex.activity, Activity::Working);

    result(send_action(
        &socket,
        2,
        Action::ProviderEvidenceLost {
            target,
            provider: ProviderKind::Codex,
            session_id: codex_session.into(),
            runtime_id: codex_runtime,
            generation: codex_generation,
            sequence: 2,
        },
    ));
    let codex = desktop
        .snapshot()
        .unwrap()
        .agent_lifecycle
        .iter()
        .find(|agent| agent.provider == Provider::CodexLocal)
        .cloned()
        .expect("evidence loss leaves a visible Unknown record");
    assert_eq!(codex.activity, Activity::Unknown);
    assert_eq!(codex.connection, Connection::Unknown);

    let codex_recovery_runtime = Uuid::new_v4();
    let codex_recovery_generation = Uuid::new_v4();
    result(send_action(
        &socket,
        3,
        Action::ProviderObservation {
            target,
            runtime_id: codex_recovery_runtime,
            generation: codex_recovery_generation,
            sequence: 1,
            observation: ProviderLifecycleObservation {
                provider: ProviderKind::Codex,
                scope: ProviderScope::Root,
                session_id: codex_session.into(),
                turn_id: Some("codex_turn_8".into()),
                state: ProviderState::Working,
                transition: ProviderTransition::Snapshot,
                marker: ProviderMarker::Recovery,
            },
        },
    ));
    let codex = desktop
        .snapshot()
        .unwrap()
        .agent_lifecycle
        .iter()
        .find(|agent| agent.provider == Provider::CodexLocal)
        .cloned()
        .expect("recovered provider retains the exact session row");
    assert_eq!(codex.activity, Activity::Working);
    assert_eq!(codex.connection, Connection::Connected);

    let opencode_runtime = Uuid::new_v4();
    let opencode_generation = Uuid::new_v4();
    let opencode_session = "sess_opencode_9";
    result(send_action(
        &socket,
        4,
        Action::ProviderObservation {
            target,
            runtime_id: opencode_runtime,
            generation: opencode_generation,
            sequence: 1,
            observation: ProviderLifecycleObservation {
                provider: ProviderKind::OpenCode,
                scope: ProviderScope::Root,
                session_id: opencode_session.into(),
                turn_id: Some("turn_opencode_10".into()),
                state: ProviderState::Working,
                transition: ProviderTransition::StateChanged,
                marker: ProviderMarker::Live,
            },
        },
    ));
    result(send_action(
        &socket,
        5,
        Action::ProviderSessionEnded {
            target,
            provider: ProviderKind::OpenCode,
            session_id: opencode_session.into(),
            runtime_id: opencode_runtime,
            generation: opencode_generation,
            sequence: 2,
        },
    ));
    let opencode = desktop
        .snapshot()
        .unwrap()
        .agent_lifecycle
        .iter()
        .find(|agent| agent.provider == Provider::OpenCodeLocal)
        .cloned()
        .expect("provider exit leaves a visible disconnected record");
    assert_eq!(opencode.activity, Activity::Unknown);
    assert_eq!(opencode.connection, Connection::Disconnected);

    let resumed_runtime = Uuid::new_v4();
    let resumed_generation = Uuid::new_v4();
    result(send_action(
        &socket,
        51,
        provider_observation_action(
            target,
            resumed_runtime,
            resumed_generation,
            1,
            provider_observation(
                ProviderKind::OpenCode,
                opencode_session,
                Some("turn_resumed"),
                ProviderState::Working,
                ProviderTransition::Snapshot,
                ProviderMarker::Recovery,
            ),
        ),
    ));
    let resumed = live_snapshot(&desktop)
        .agent_lifecycle
        .iter()
        .find(|agent| agent.provider == Provider::OpenCodeLocal)
        .cloned()
        .unwrap();
    assert_eq!(
        (resumed.runtime_id, resumed.generation),
        (resumed_runtime, resumed_generation)
    );
    assert_eq!(
        (resumed.activity, resumed.connection),
        (Activity::Working, Connection::Connected)
    );
    let stale = send_action(
        &socket,
        52,
        provider_observation_action(
            target,
            opencode_runtime,
            opencode_generation,
            3,
            provider_observation(
                ProviderKind::OpenCode,
                opencode_session,
                Some("turn_opencode_10"),
                ProviderState::Working,
                ProviderTransition::StateChanged,
                ProviderMarker::Live,
            ),
        ),
    );
    assert!(
        !stale.ok,
        "the normally ended generation cannot displace its resumed owner"
    );

    let generic_register = send_action(
        &socket,
        6,
        Action::Register {
            target,
            provider: Provider::CodexLocal,
            runtime_id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
            provider_session_id: Uuid::new_v4(),
        },
    );
    assert!(!generic_register.ok);

    // A delayed same-generation baseline must not displace a live turn or
    // clear the completion eligibility established by its live Working event.
    let mut lifecycle_findings = Vec::new();
    result(send_action(
        &socket,
        7,
        provider_observation_action(
            target,
            codex_recovery_runtime,
            codex_recovery_generation,
            2,
            provider_observation(
                ProviderKind::Codex,
                codex_session,
                Some("turn_live_a"),
                ProviderState::Working,
                ProviderTransition::StateChanged,
                ProviderMarker::Live,
            ),
        ),
    ));
    result(send_action(
        &socket,
        8,
        provider_observation_action(
            target,
            codex_recovery_runtime,
            codex_recovery_generation,
            3,
            provider_observation(
                ProviderKind::Codex,
                codex_session,
                Some("turn_delayed_b"),
                ProviderState::Working,
                ProviderTransition::Snapshot,
                ProviderMarker::Baseline,
            ),
        ),
    ));
    result(send_action(
        &socket,
        9,
        provider_observation_action(
            target,
            codex_recovery_runtime,
            codex_recovery_generation,
            4,
            provider_observation(
                ProviderKind::Codex,
                codex_session,
                Some("turn_live_a"),
                ProviderState::Idle,
                ProviderTransition::TurnCompleted,
                ProviderMarker::Live,
            ),
        ),
    ));
    let completion_snapshot = desktop.snapshot().unwrap();
    let codex_completed: Vec<_> = completion_snapshot
        .agent_alert_events
        .iter()
        .filter(|alert| {
            alert.provider == Provider::CodexLocal && alert.kind == AlertKind::Completed
        })
        .collect();
    let recovered_codex = completion_snapshot.agent_lifecycle.iter().find(|agent| {
        agent.provider == Provider::CodexLocal && agent.runtime_id == codex_recovery_runtime
    });
    if codex_completed.len() != 1
        || recovered_codex.is_none_or(|agent| {
            agent.activity != Activity::Idle
                || agent.prompt_id != codex_completed.first().map(|alert| alert.prompt_id)
        })
    {
        lifecycle_findings.push(format!(
            "delayed baseline suppressed or displaced live completion: alerts={}, agent={recovered_codex:?}",
            codex_completed.len()
        ));
    }

    // A recovery snapshot that already reports Waiting records that turn as
    // observed, so a later live duplicate does not raise another attention alert.
    let snapshot_wait_session = "codex_snapshot_wait";
    let snapshot_wait_runtime = Uuid::new_v4();
    let snapshot_wait_generation = Uuid::new_v4();
    let attention_count_before_snapshot = desktop
        .snapshot()
        .unwrap()
        .agent_alert_events
        .iter()
        .filter(|alert| alert.kind == AlertKind::NeedsUser)
        .count();
    result(send_action(
        &socket,
        10,
        provider_observation_action(
            target,
            snapshot_wait_runtime,
            snapshot_wait_generation,
            1,
            provider_observation(
                ProviderKind::Codex,
                snapshot_wait_session,
                Some("turn_snapshot_wait"),
                ProviderState::Waiting,
                ProviderTransition::Snapshot,
                ProviderMarker::Recovery,
            ),
        ),
    ));
    result(send_action(
        &socket,
        11,
        provider_observation_action(
            target,
            snapshot_wait_runtime,
            snapshot_wait_generation,
            2,
            provider_observation(
                ProviderKind::Codex,
                snapshot_wait_session,
                Some("turn_snapshot_wait"),
                ProviderState::Waiting,
                ProviderTransition::StateChanged,
                ProviderMarker::Live,
            ),
        ),
    ));
    let attention_count_after_snapshot = desktop
        .snapshot()
        .unwrap()
        .agent_alert_events
        .iter()
        .filter(|alert| alert.kind == AlertKind::NeedsUser)
        .count();
    if attention_count_after_snapshot != attention_count_before_snapshot {
        lifecycle_findings.push(format!(
            "recovery Waiting snapshot was not deduplicated: before={attention_count_before_snapshot}, after={attention_count_after_snapshot}"
        ));
    }

    // A locally lost adapter generation retains the bounded per-session turn
    // outcomes. Recovery Working clears eligibility, while the already-alerted
    // Waiting turn stays deduplicated across the generation replacement.
    let loss_session = "codex_loss_history";
    let loss_runtime = Uuid::new_v4();
    let loss_generation = Uuid::new_v4();
    let attention_count_before_loss = desktop
        .snapshot()
        .unwrap()
        .agent_alert_events
        .iter()
        .filter(|alert| alert.kind == AlertKind::NeedsUser)
        .count();
    result(send_action(
        &socket,
        12,
        provider_observation_action(
            target,
            loss_runtime,
            loss_generation,
            1,
            provider_observation(
                ProviderKind::Codex,
                loss_session,
                Some("turn_lost_waiting"),
                ProviderState::Working,
                ProviderTransition::StateChanged,
                ProviderMarker::Live,
            ),
        ),
    ));
    result(send_action(
        &socket,
        13,
        provider_observation_action(
            target,
            loss_runtime,
            loss_generation,
            2,
            provider_observation(
                ProviderKind::Codex,
                loss_session,
                Some("turn_lost_waiting"),
                ProviderState::Waiting,
                ProviderTransition::StateChanged,
                ProviderMarker::Live,
            ),
        ),
    ));
    result(send_action(
        &socket,
        14,
        provider_evidence_lost_action(
            target,
            ProviderKind::Codex,
            loss_session,
            loss_runtime,
            loss_generation,
            3,
        ),
    ));
    let loss_recovery_runtime = Uuid::new_v4();
    let loss_recovery_generation = Uuid::new_v4();
    result(send_action(
        &socket,
        15,
        provider_observation_action(
            target,
            loss_recovery_runtime,
            loss_recovery_generation,
            1,
            provider_observation(
                ProviderKind::Codex,
                loss_session,
                Some("turn_lost_waiting"),
                ProviderState::Working,
                ProviderTransition::Snapshot,
                ProviderMarker::Recovery,
            ),
        ),
    ));
    result(send_action(
        &socket,
        16,
        provider_observation_action(
            target,
            loss_recovery_runtime,
            loss_recovery_generation,
            2,
            provider_observation(
                ProviderKind::Codex,
                loss_session,
                Some("turn_lost_waiting"),
                ProviderState::Waiting,
                ProviderTransition::StateChanged,
                ProviderMarker::Live,
            ),
        ),
    ));
    let attention_count_after_loss = desktop
        .snapshot()
        .unwrap()
        .agent_alert_events
        .iter()
        .filter(|alert| alert.kind == AlertKind::NeedsUser)
        .count();
    if attention_count_after_loss != attention_count_before_loss + 1 {
        lifecycle_findings.push(format!(
            "recoverable local loss replayed a Waiting alert: before={attention_count_before_loss}, after={attention_count_after_loss}"
        ));
    }

    // Multiple local losses must not let an older retired runtime/generation
    // become current again after a newer generation was itself lost.
    let fence_session = "codex_retired_generation";
    let fence_runtime_a = Uuid::new_v4();
    let fence_generation_a = Uuid::new_v4();
    result(send_action(
        &socket,
        17,
        provider_observation_action(
            target,
            fence_runtime_a,
            fence_generation_a,
            1,
            provider_observation(
                ProviderKind::Codex,
                fence_session,
                Some("turn_fence"),
                ProviderState::Working,
                ProviderTransition::StateChanged,
                ProviderMarker::Live,
            ),
        ),
    ));
    result(send_action(
        &socket,
        18,
        provider_evidence_lost_action(
            target,
            ProviderKind::Codex,
            fence_session,
            fence_runtime_a,
            fence_generation_a,
            2,
        ),
    ));
    let fence_runtime_b = Uuid::new_v4();
    let fence_generation_b = Uuid::new_v4();
    result(send_action(
        &socket,
        19,
        provider_observation_action(
            target,
            fence_runtime_b,
            fence_generation_b,
            1,
            provider_observation(
                ProviderKind::Codex,
                fence_session,
                Some("turn_fence"),
                ProviderState::Working,
                ProviderTransition::Snapshot,
                ProviderMarker::Recovery,
            ),
        ),
    ));
    result(send_action(
        &socket,
        20,
        provider_evidence_lost_action(
            target,
            ProviderKind::Codex,
            fence_session,
            fence_runtime_b,
            fence_generation_b,
            2,
        ),
    ));
    let delayed_old_generation = send_action(
        &socket,
        21,
        provider_observation_action(
            target,
            fence_runtime_a,
            fence_generation_a,
            1,
            provider_observation(
                ProviderKind::Codex,
                fence_session,
                Some("turn_fence"),
                ProviderState::Working,
                ProviderTransition::Snapshot,
                ProviderMarker::Recovery,
            ),
        ),
    );
    let fence_snapshot = desktop.snapshot().unwrap();
    let current_fenced_agent = fence_snapshot.agent_lifecycle.iter().find(|agent| {
        agent.provider == Provider::CodexLocal
            && agent.runtime_id == fence_runtime_b
            && agent.generation == fence_generation_b
    });
    if delayed_old_generation.ok
        || current_fenced_agent.is_none_or(|agent| {
            agent.activity != Activity::Unknown || agent.connection != Connection::Unknown
        })
    {
        lifecycle_findings.push(format!(
            "retired generation was re-admitted after repeated loss: accepted={}, current={current_fenced_agent:?}",
            delayed_old_generation.ok
        ));
    }

    assert!(
        lifecycle_findings.is_empty(),
        "provider lifecycle regressions: {}",
        lifecycle_findings.join("; ")
    );
    drop(server);
}
