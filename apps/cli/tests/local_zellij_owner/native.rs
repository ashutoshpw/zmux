//! Actual native coordinator, S01 filesystem and retained foreground CLI Session.
use super::*;
use std::sync::Arc;
use zmux_desktop_runtime::{
    Command as DesktopCommand, Desktop, DesktopOptions, Request, Shell, Snapshot,
    local_zellij::{Configuration, Installation, Profile},
    zellij_coordinator::Request as ZellijRequest,
};
use zmux_remote_runtime::local_zellij::Record;
use zmux_workspace_core::{Action, SurfaceId};

struct NativeDesktop(Desktop);
impl std::ops::Deref for NativeDesktop {
    type Target = Desktop;
    fn deref(&self) -> &Desktop {
        &self.0
    }
}
impl Drop for NativeDesktop {
    fn drop(&mut self) {
        if let Err(error) = self.0.shutdown(Duration::from_secs(5)) {
            eprintln!("native fixture owned Desktop cleanup: {error}");
        }
    }
}
fn send(desktop: &Desktop, id: u64, payload: DesktopCommand) {
    desktop
        .submit(Request {
            version: 1,
            request_id: id,
            payload,
        })
        .unwrap();
}
#[track_caller]
fn wait(desktop: &Desktop, mut predicate: impl FnMut(&Snapshot) -> bool) -> Arc<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match desktop.snapshot() {
            Ok(snapshot) if predicate(&snapshot) => return snapshot,
            Ok(snapshot) => assert!(
                Instant::now() < deadline,
                "native foreground path timed out: {}",
                serde_json::json!({"model":snapshot.model,"operations":snapshot.local_zellij_operations,"persistence":snapshot.persistence,"pending_launches":snapshot.pending_launches,"last_error":snapshot.last_error,"cleanup_error":snapshot.cleanup_error})
            ),
            Err(zmux_desktop_runtime::DesktopError::Busy) => assert!(Instant::now() < deadline),
            Err(e) => panic!("{e}"),
        }
        thread::sleep(Duration::from_millis(5));
    }
}
fn departed(identity: &zmux_session_runtime::child::ProcessIdentity) -> bool {
    matches!(
        kill(Pid::from_raw(identity.pid as i32), None),
        Err(nix::errno::Errno::ESRCH)
    ) || zmux_session_runtime::child::process_lifetime(identity.pid)
        .is_ok_and(|current| current != identity.into())
}
fn terminal(snapshot: &Snapshot, owner: uuid::Uuid) -> (SurfaceId, zmux_workspace_core::SessionId) {
    snapshot
        .model
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .flat_map(|p| &p.surfaces)
        .find(|s| {
            s.local_zellij
                .as_ref()
                .is_some_and(|i| i.owner_uuid == owner)
        })
        .map(|s| (s.id, s.session_id.unwrap()))
        .expect("actual owner has a canonical surface and Session")
}

fn native_desktop(
    fixture: &Fixture,
    profile: uuid::Uuid,
    installation: uuid::Uuid,
    state: Option<PathBuf>,
) -> NativeDesktop {
    NativeDesktop(
        Desktop::with_options(
            Shell {
                program: "/bin/sh".into(),
                args: vec!["-i".into()],
                cwd: Some(fixture.root.clone()),
            },
            DesktopOptions {
                state_directory: state,
                local_zellij: Some(Configuration {
                    installation: Installation {
                        epoch: installation,
                        cli: PathBuf::from(env!("CARGO_BIN_EXE_zmux")),
                        zellij: fixture.binary.clone(),
                    },
                    profiles: vec![Profile {
                        id: profile,
                        root: fixture.root.join("profile"),
                    }],
                }),
                ..Default::default()
            },
        )
        .unwrap(),
    )
}

#[test]
fn actual_native_saved_checkpoint_precedes_workload_and_attach_reuses_original_owner() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let state = fixture.root.join("desktop");
    fs::DirBuilder::new().mode(0o700).create(&state).unwrap();
    let profile = uuid::Uuid::new_v4();
    let installation = uuid::Uuid::new_v4();
    let saved = state.join("session.json");
    let count = fixture.root.join("native-start-count");
    let checkpoint = fixture.root.join("workload-checkpoint.json");
    let input = fixture.root.join("native-input");
    let script = fixture.root.join("native-workload.py");
    let script_text = format!(
        r#"import json,os,select,sys,time,uuid
with open({saved:?},encoding='utf8') as f: document=json.load(f)
found=[]
def visit(value):
 if isinstance(value,dict):
  if value.get('kind')=='local_zellij_terminal' and value.get('profile_id')=={profile:?}: found.append(value)
  for item in value.values(): visit(item)
 elif isinstance(value,list):
  for item in value: visit(item)
visit(document)
assert document['version']>=6 and len(found)==1 and uuid.UUID(found[0]['owner_uuid']).int!=0
with open({checkpoint:?},'w',encoding='utf8') as f: json.dump(found[0],f)
with open({count:?},'a',encoding='ascii') as f: f.write('x')
os.write(1,b'R03_NATIVE_INPUT_READY\r\n')
end=time.monotonic()+30
while time.monotonic()<end:
 if not select.select([0],[],[],max(0,end-time.monotonic()))[0]: break
 line=sys.stdin.readline()
 if not line or line.strip()=='exit-workload': break
 if line.startswith('proof-'):
  with open({input:?},'w',encoding='utf8') as f: f.write(line.strip())
"#,
        saved = saved.to_str().unwrap(),
        profile = profile.to_string(),
        checkpoint = checkpoint.to_str().unwrap(),
        count = count.to_str().unwrap(),
        input = input.to_str().unwrap()
    );
    fs::write(&script, script_text).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o600)).unwrap();
    let desktop = native_desktop(&fixture, profile, installation, Some(state));
    let initial = wait(&desktop, |s| {
        !s.persistence.startup_pending && s.pending_launches == 0 && !s.model.workspaces.is_empty()
    });
    let pane = initial.model.workspaces[0].focused_pane;
    send(
        &desktop,
        1,
        DesktopCommand::LocalZellij {
            request: ZellijRequest::Start {
                profile_id: profile,
                pane_id: pane,
                name: "native-checkpoint".into(),
                cwd: fixture.root.clone(),
                command: Some(format!(
                    "python3 '{}'; native_result=$?; exit \"$native_result\"",
                    script.display()
                )),
                focus: true,
            },
        },
    );
    let connected = wait(&desktop, |s| {
        s.local_zellij_operations
            .iter()
            .any(|r| r.state == "input_ready" && r.input_ready)
            && checkpoint.exists()
    });
    let first = connected.local_zellij_operations.last().unwrap().clone();
    assert_eq!(first.execution_contract_version, Some(2));
    let owner = first.owner_uuid.unwrap();
    assert!(
        first
            .checkpoint_revision
            .is_some_and(|revision| revision > 0)
    );
    let inspected: Value = serde_json::from_slice(&fs::read(&checkpoint).unwrap()).unwrap();
    assert_eq!(
        inspected,
        serde_json::json!({"kind":"local_zellij_terminal","profile_id":profile,"owner_uuid":owner})
    );
    assert_eq!(fs::read(&count).unwrap(), b"x");
    let (surface, session) = terminal(&connected, owner);
    let cli = process_identity(
        connected
            .control_frames
            .iter()
            .find(|f| f.session_id == session)
            .unwrap()
            .process_id
            .unwrap(),
    )
    .unwrap();
    assert_eq!(cli.parent_pid, std::process::id());
    let record: Record = serde_json::from_value(fixture.records().unwrap()[0].clone()).unwrap();
    assert_eq!(record.execution_binding().unwrap().contract_version(), 2);
    record.verify_live().unwrap();
    // A broker connection is not semantic application readiness. Zellij drops
    // non-query input during its startup query window; actual pane rendering is
    // buffered until that window ends. Wait for the real rendered workload.
    wait(&desktop, |s| {
        s.control_frames
            .iter()
            .find(|f| f.session_id == session)
            .is_some_and(|f| {
                f.snapshot
                    .as_ref()
                    .is_some_and(|s| s.viewport.contents.contains("R03_NATIVE_INPUT_READY"))
            })
    });
    let proof = format!("proof-{}", uuid::Uuid::new_v4());
    send(
        &desktop,
        2,
        DesktopCommand::Input {
            session_id: session,
            text: format!("{proof}\r"),
        },
    );
    wait(&desktop, |_| {
        fs::read_to_string(&input).is_ok_and(|s| s == proof)
    });
    send(
        &desktop,
        3,
        DesktopCommand::Action {
            action: Action::CloseSurface {
                surface_id: surface,
            },
        },
    );
    wait(&desktop, |s| {
        !s.model
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .any(|s| s.id == surface)
            && departed(&cli)
            && s.local_zellij_operations
                .iter()
                .find(|r| r.operation_id == first.operation_id)
                .is_some_and(|r| {
                    r.session_joined
                        && r.final_frame_received
                        && r.monitor_joined
                        && r.namespace_cleaned
                        && r.frontend_exit_code == Some(0)
                        && r.error.is_none()
                })
    });
    record.verify_live().unwrap();
    assert_eq!(fs::read(&count).unwrap(), b"x");
    fs::remove_file(&input).unwrap();
    send(
        &desktop,
        4,
        DesktopCommand::LocalZellij {
            request: ZellijRequest::Attach {
                profile_id: profile,
                owner_uuid: owner,
                pane_id: pane,
                focus: true,
            },
        },
    );
    let attached = wait(&desktop, |s| {
        s.local_zellij_operations.iter().any(|r| {
            r.operation_id != first.operation_id && r.state == "input_ready" && r.input_ready
        })
    });
    let (_, session) = terminal(&attached, owner);
    assert_eq!(
        attached
            .local_zellij_operations
            .last()
            .unwrap()
            .execution_contract_version,
        Some(2)
    );
    let current: Record = serde_json::from_value(fixture.records().unwrap()[0].clone()).unwrap();
    assert_eq!(current.id, record.id);
    assert_eq!(current.server, record.server);
    assert_eq!(current.supervisor, record.supervisor);
    assert_eq!(current.workload, record.workload);
    current.verify_live().unwrap();
    wait(&desktop, |s| {
        s.control_frames
            .iter()
            .find(|f| f.session_id == session)
            .is_some_and(|f| {
                f.snapshot
                    .as_ref()
                    .is_some_and(|s| s.viewport.contents.contains("R03_NATIVE_INPUT_READY"))
            })
    });
    send(
        &desktop,
        5,
        DesktopCommand::Input {
            session_id: session,
            text: format!("{proof}\r"),
        },
    );
    wait(&desktop, |_| {
        fs::read_to_string(&input).is_ok_and(|s| s == proof)
    });
    assert_eq!(fs::read(&count).unwrap(), b"x");
    // The current observer proves preservation for detach, not expected finite
    // completion. Close this second frontend while the original owners are live.
    let (surface, _) = terminal(&attached, owner);
    let second = attached
        .local_zellij_operations
        .last()
        .unwrap()
        .operation_id;
    send(
        &desktop,
        6,
        DesktopCommand::Action {
            action: Action::CloseSurface {
                surface_id: surface,
            },
        },
    );
    wait(&desktop, |s| {
        s.local_zellij_operations
            .iter()
            .find(|r| r.operation_id == second)
            .is_some_and(|r| {
                r.state == "frontend_joined"
                    && r.execution_contract_version == Some(2)
                    && r.session_joined
                    && r.final_frame_received
                    && r.monitor_joined
                    && r.namespace_cleaned
                    && r.frontend_exit_code == Some(0)
                    && r.error.is_none()
            })
    });
    record.verify_live().unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    // Independently retained CLI fixture ends only this test's finite workload
    // through real stdin after native preservation acceptance has completed.
    let mut cleanup = Client::new(
        &fixture,
        &[
            "local-zellij",
            "attach",
            "--headless",
            "--id",
            &owner.to_string(),
        ],
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    while !String::from_utf8_lossy(&cleanup.output).contains("R03_NATIVE_INPUT_READY") {
        cleanup.pump(true);
        assert!(
            Instant::now() < deadline,
            "independent cleanup frontend did not render workload"
        );
        thread::sleep(Duration::from_millis(5));
    }
    cleanup.send(b"exit-workload\r");
    assert_eq!(cleanup.finish(), 0);
    let cleaned = fixture.output(&[
        "local-zellij",
        "cleanup",
        "--id",
        &owner.to_string(),
        "--generation",
        &record
            .completion
            .as_ref()
            .unwrap()
            .owner_generation
            .to_string(),
    ]);
    assert!(
        cleaned.status.success(),
        "{}",
        String::from_utf8_lossy(&cleaned.stderr)
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while [&record.server, &record.supervisor, &record.workload]
        .iter()
        .any(|i| !departed(i.as_ref().unwrap()))
    {
        assert!(
            Instant::now() < deadline,
            "finite test owner still live after independent stdin exit"
        );
        thread::sleep(Duration::from_millis(5));
    }
    cleanup_runtime(&record);
    assert_eq!(fs::read(&count).unwrap(), b"x");
    eprintln!(
        "R03 native checkpoint receipt: {}",
        serde_json::json!({"installation_epoch":installation,"profile_id":profile,"owner_uuid":owner,"checkpoint_revision":first.checkpoint_revision,"workload_inspected_actual_saved_descriptor":true,"startup_count":1,"actual_session_input":true,"frontend_close_preserved_original_owner":true,"attach_only_input":true,"outer_session_joined":true,"complete_final_frame":true,"observer_monitor_joined":true,"private_namespace_cleaned":true,"shared_shutdown":true,"native_appkit_start_attach":false})
    );
}

fn cleanup_runtime(record: &Record) {
    use std::os::unix::fs::MetadataExt;
    assert!(
        [&record.server, &record.supervisor, &record.workload]
            .iter()
            .all(|i| i.as_ref().is_none_or(departed))
    );
    let root = zmux_remote_runtime::local_zellij::observer_protocol::runtime_base()
        .unwrap()
        .join(format!("z{}", record.id.simple()));
    match root.symlink_metadata() {
        Ok(info) => {
            assert!(
                info.is_dir()
                    && info.uid() == nix::unistd::geteuid().as_raw()
                    && info.mode() & 0o077 == 0
            );
            assert_eq!(
                (info.dev(), info.ino()),
                record.namespace.as_ref().unwrap().root
            );
            fs::remove_dir_all(root).unwrap();
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn actual_native_start_refuses_without_persistence_and_writer_failure_never_starts_workload() {
    use std::os::unix::fs::symlink;
    // These are different production failure boundaries: admission must create
    // nothing; an actual failed checkpoint has already reserved an owner.
    for writer_failure in [false, true] {
        let fixture = Fixture::new();
        let profile = uuid::Uuid::new_v4();
        let installation = uuid::Uuid::new_v4();
        let state = fixture.root.join("desktop");
        if writer_failure {
            fs::DirBuilder::new().mode(0o700).create(&state).unwrap();
        }
        let desktop = native_desktop(
            &fixture,
            profile,
            installation,
            writer_failure.then_some(state.clone()),
        );
        let initial = wait(&desktop, |s| {
            !s.persistence.startup_pending
                && s.pending_launches == 0
                && !s.model.workspaces.is_empty()
                && (!writer_failure || s.persistence.last_saved_revision.is_some())
        });
        let sentinel = fixture.root.join("untouched-sentinel");
        if writer_failure {
            fs::write(&sentinel, b"must remain untouched").unwrap();
            fs::remove_file(state.join("session.json")).unwrap();
            symlink(&sentinel, state.join("session.json")).unwrap();
        }
        let marker = fixture.root.join("forbidden-workload");
        send(
            &desktop,
            10,
            DesktopCommand::LocalZellij {
                request: ZellijRequest::Start {
                    profile_id: profile,
                    pane_id: initial.model.workspaces[0].focused_pane,
                    name: "must-not-start".into(),
                    cwd: fixture.root.clone(),
                    command: Some(format!("printf forbidden > '{}'", marker.display())),
                    focus: true,
                },
            },
        );
        let result = wait(&desktop, |s| {
            if writer_failure {
                s.pending_launches == 0
                    && s.local_zellij_operations
                        .iter()
                        .any(|r| r.session_joined && r.monitor_joined && r.namespace_cleaned)
                    && s.persistence.operations.iter().any(|r| {
                        r.kind == "local_zellij_checkpoint"
                            && serde_json::to_value(&r.state).unwrap() == "failed"
                    })
            } else {
                s.last_error
                    .as_ref()
                    .is_some_and(|e| e.contains("requires session persistence"))
            }
        });
        assert!(!marker.exists());
        if writer_failure {
            assert_eq!(fs::read(&sentinel).unwrap(), b"must remain untouched");
            assert!(!result.persistence.autosave_enabled);
            let records = fixture
                .records()
                .expect("actual CLI reservation reached writer failure");
            assert_eq!(records.as_array().unwrap().len(), 1);
            let record: Record = serde_json::from_value(records[0].clone()).unwrap();
            assert!(
                record.server.is_none() && record.supervisor.is_none() && record.workload.is_none()
            );
            let receipt = result.local_zellij_operations.last().unwrap();
            assert_eq!(receipt.owner_uuid, Some(record.id));
            assert!(receipt.checkpoint_revision.is_none() && !receipt.final_frame_received);
            assert!(result.persistence.operations.iter().any(|r| {
                r.kind == "local_zellij_checkpoint"
                    && r.error
                        .as_ref()
                        .is_some_and(|e| e.contains("open private session file"))
            }));
            // Restore only the owned test path for shutdown; failure remains latched.
            fs::remove_file(state.join("session.json")).unwrap();
        } else {
            assert!(result.local_zellij_operations.is_empty());
            assert!(fixture.records().is_none());
            assert!(!fixture.root.join("profile").exists());
        }
        let ticket = desktop
            .begin_shutdown(Duration::from_secs(5), false)
            .unwrap()
            .ticket;
        let report = desktop
            .wait_shutdown(ticket, Duration::from_secs(5))
            .unwrap();
        assert!(report.physical_cleanup_complete, "{report:?}");
        eprintln!(
            "R03 actual negative receipt: {}",
            serde_json::json!({"writer_failure":writer_failure,"actual_reservation":writer_failure,"workload_started":false,"physical_cleanup_complete":report.physical_cleanup_complete,"issues":report.issues})
        );
    }
}

#[test]
fn actual_bound_finite_completion_saves_exact_workload_status_and_joins_frontend() {
    // The actual selected shell remains live until one real input line arrives,
    // so this protects bound finite completion, not the separate fast-exit race.
    for (finish, expected, code) in [
        ("exit 0", serde_json::json!({"kind":"exited","value":0}), 0),
        ("exit 7", serde_json::json!({"kind":"exited","value":7}), 7),
        (
            r#"kill -TERM "$$""#,
            serde_json::json!({"kind":"signaled","value":15}),
            143,
        ),
    ] {
        let fixture = Fixture::new();
        let state = fixture.root.join("desktop");
        fs::DirBuilder::new().mode(0o700).create(&state).unwrap();
        let profile = uuid::Uuid::new_v4();
        let desktop = native_desktop(&fixture, profile, uuid::Uuid::new_v4(), Some(state.clone()));
        let initial = wait(&desktop, |s| {
            !s.persistence.startup_pending
                && s.pending_launches == 0
                && !s.model.workspaces.is_empty()
        });
        send(
            &desktop,
            20,
            DesktopCommand::LocalZellij {
                request: ZellijRequest::Start {
                    profile_id: profile,
                    pane_id: initial.model.workspaces[0].focused_pane,
                    name: "bound-finite".into(),
                    cwd: fixture.root.clone(),
                    command: Some(format!(
                        r#"printf 'R03_FINITE_READY\n'; IFS= read -r finite_input; [ "$finite_input" = 'finish-once' ] || exit 99; {finish}"#
                    )),
                    focus: true,
                },
            },
        );
        let running = wait(&desktop, |s| {
            s.local_zellij_operations
                .iter()
                .any(|r| r.state == "input_ready" && r.input_ready)
                && s.control_frames.iter().any(|f| {
                    f.snapshot
                        .as_ref()
                        .is_some_and(|s| s.viewport.contents.contains("R03_FINITE_READY"))
                })
        });
        let owner = running
            .local_zellij_operations
            .last()
            .unwrap()
            .owner_uuid
            .unwrap();
        let (_, session) = terminal(&running, owner);
        let record: Record = serde_json::from_value(fixture.records().unwrap()[0].clone()).unwrap();
        assert_eq!(record.execution_binding().unwrap().contract_version(), 2);
        assert_eq!(
            running
                .local_zellij_operations
                .last()
                .unwrap()
                .execution_contract_version,
            Some(2)
        );
        record.verify_live().unwrap();
        send(
            &desktop,
            21,
            DesktopCommand::Input {
                session_id: session,
                text: "finish-once\r".into(),
            },
        );
        let completed = wait(&desktop, |s| {
            s.local_zellij_operations.iter().any(|r| r.session_joined)
        });
        let operation = completed.local_zellij_operations.last().unwrap();
        let value = serde_json::to_value(operation).unwrap();
        assert_eq!(value["execution_contract_version"], 2);
        assert_eq!(
            value.get("workload_exit"),
            Some(&expected),
            "actual bound finite workload lacks its authenticated terminal receipt: {value}"
        );
        assert!(
            value
                .get("completion_saved_revision")
                .and_then(Value::as_u64)
                .is_some_and(|r| r > 0),
            "finite completion was not actually Saved: {value}"
        );
        assert!(
            operation.final_frame_received
                && operation.monitor_joined
                && operation.namespace_cleaned
                && operation.error.is_none(),
            "{value}"
        );
        desktop.shutdown(Duration::from_secs(5)).unwrap();
        let saved: Value =
            serde_json::from_slice(&fs::read(state.join("session.json")).unwrap()).unwrap();
        fn completed_descriptors(value: &Value, out: &mut Vec<Value>) {
            match value {
                Value::Object(object) => {
                    if object.get("kind").and_then(Value::as_str)
                        == Some("completed_local_zellij_terminal")
                    {
                        out.push(value.clone());
                    }
                    for v in object.values() {
                        completed_descriptors(v, out);
                    }
                }
                Value::Array(array) => {
                    for v in array {
                        completed_descriptors(v, out);
                    }
                }
                _ => (),
            }
        }
        let mut descriptors = Vec::new();
        completed_descriptors(&saved, &mut descriptors);
        assert_eq!(descriptors.len(), 1, "{saved}");
        let visible = descriptors[0]["history"]["viewport"]["contents"]
            .as_str()
            .unwrap();
        assert!(
            visible.contains("R03_FINITE_READY"),
            "completed history lost its last visible workload frame: {visible}"
        );
        assert!(
            !visible.contains("zmux local-zellij attachment receipt:"),
            "internal lifecycle JSON must not overwrite workload history"
        );
        assert_eq!(
            descriptors[0]["history"]["receipt"]["receipt"]["workload_exit"],
            expected
        );
        assert_eq!(
            descriptors[0]["history"]["receipt"]["execution_contract"],
            "admitted_lifetime_v2"
        );
        assert_eq!(saved["version"], 8);
        // A fresh explicit Attach to an already completed retained owner uses
        // a real CLI Session but never launches a Zellij frontend or workload.
        let attach_state = fixture.root.join("completed-attach-desktop");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&attach_state)
            .unwrap();
        let attach = native_desktop(&fixture, profile, uuid::Uuid::new_v4(), Some(attach_state));
        let initial_attach = wait(&attach, |s| {
            !s.persistence.startup_pending
                && s.pending_launches == 0
                && !s.model.workspaces.is_empty()
        });
        send(
            &attach,
            22,
            DesktopCommand::LocalZellij {
                request: ZellijRequest::Attach {
                    profile_id: profile,
                    owner_uuid: owner,
                    pane_id: initial_attach.model.workspaces[0].focused_pane,
                    focus: true,
                },
            },
        );
        let attached = wait(&attach, |s| {
            s.local_zellij_operations.iter().any(|r| r.session_joined)
        });
        let attached_receipt = attached.local_zellij_operations.last().unwrap();
        let attached_value = serde_json::to_value(attached_receipt).unwrap();
        assert_eq!(attached_value["execution_contract_version"], 2);
        assert_eq!(
            attached_value["workload_exit"], expected,
            "{attached_value}"
        );
        assert!(
            attached_receipt.frontend_pid.is_none() && attached_receipt.error.is_none(),
            "{attached_value}"
        );
        assert!(
            attached_value["completion_saved_revision"]
                .as_u64()
                .is_some_and(|r| r > 0)
        );
        attach.shutdown(Duration::from_secs(5)).unwrap();
        let restored = native_desktop(&fixture, profile, uuid::Uuid::new_v4(), Some(state));
        let history = wait(&restored, |s| {
            !s.persistence.startup_pending && s.pending_launches == 0
        });
        let surface = history
            .model
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .find(|s| {
                s.local_zellij
                    .as_ref()
                    .is_some_and(|i| i.owner_uuid == owner)
            })
            .unwrap();
        assert!(surface.session_id.is_none() && surface.local_zellij_completion.is_some());
        assert!(
            history.local_zellij_operations.is_empty(),
            "restored history must not Attach or Start"
        );
        restored.shutdown(Duration::from_secs(5)).unwrap();
        let authenticated = zmux_remote_runtime::local_zellij::completion::authenticate(
            &record,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap()
        .unwrap();
        assert_eq!(authenticated.workload_exit().mapped().unwrap(), code);
        // A plausible disk receipt cannot replace the original live endpoint.
        // Restore only the exact retained socket inode after this denial.
        let runtime = zmux_remote_runtime::local_zellij::observer_protocol::runtime_base()
            .unwrap()
            .join(format!("z{}", owner.simple()));
        let endpoint = runtime.join("completion.sock");
        let retained_endpoint = runtime.join("completion.sock.retained-test");
        fs::rename(&endpoint, &retained_endpoint).unwrap();
        std::os::unix::fs::symlink(&retained_endpoint, &endpoint).unwrap();
        assert!(
            zmux_remote_runtime::local_zellij::completion::authenticate(
                &record,
                Instant::now() + Duration::from_secs(1)
            )
            .is_err()
        );
        fs::remove_file(&endpoint).unwrap();
        fs::rename(&retained_endpoint, &endpoint).unwrap();
        let wrong_generation = fixture.output(&[
            "local-zellij",
            "cleanup",
            "--id",
            &owner.to_string(),
            "--generation",
            &uuid::Uuid::new_v4().to_string(),
        ]);
        assert!(!wrong_generation.status.success());
        assert!(
            zmux_remote_runtime::local_zellij::completion::authenticate(
                &record,
                Instant::now() + Duration::from_secs(1)
            )
            .unwrap()
            .is_some()
        );
        let cleanup = fixture.output(&[
            "local-zellij",
            "cleanup",
            "--id",
            &owner.to_string(),
            "--generation",
            &authenticated.owner_generation().to_string(),
        ]);
        assert!(
            cleanup.status.success(),
            "{}",
            String::from_utf8_lossy(&cleanup.stderr)
        );
        assert!(fixture.records().unwrap().as_array().unwrap().is_empty());
        assert!(
            [&record.server, &record.supervisor, &record.workload]
                .iter()
                .all(|i| departed(i.as_ref().unwrap()))
        );
        eprintln!(
            "R03 bound finite receipt: {}",
            serde_json::json!({"workload_exit":code,"owner_uuid":owner,"actual_input_once":true,"actual_saved_completion":true,"physical_frontend_joined":true,"fast_prebound_exit_covered":false})
        );
    }
}

#[test]
fn actual_completion_writer_failure_retains_live_unconsumed_publisher() {
    let fixture = Fixture::new();
    let state = fixture.root.join("desktop");
    fs::DirBuilder::new().mode(0o700).create(&state).unwrap();
    let profile = uuid::Uuid::new_v4();
    let desktop = native_desktop(&fixture, profile, uuid::Uuid::new_v4(), Some(state.clone()));
    let initial = wait(&desktop, |s| {
        !s.persistence.startup_pending && s.pending_launches == 0 && !s.model.workspaces.is_empty()
    });
    send(&desktop,30,DesktopCommand::LocalZellij{request:ZellijRequest::Start{profile_id:profile,pane_id:initial.model.workspaces[0].focused_pane,name:"failed-finite-save".into(),cwd:fixture.root.clone(),command:Some(r#"printf 'R03_FAILED_COMPLETION_READY\n'; IFS= read -r finish; [ "$finish" = 'finish-once' ] || exit 99; exit 0"#.into()),focus:true}});
    let running = wait(&desktop, |s| {
        s.local_zellij_operations
            .iter()
            .any(|r| r.state == "input_ready" && r.input_ready)
            && s.control_frames.iter().any(|f| {
                f.snapshot
                    .as_ref()
                    .is_some_and(|s| s.viewport.contents.contains("R03_FAILED_COMPLETION_READY"))
            })
    });
    let owner = running
        .local_zellij_operations
        .last()
        .unwrap()
        .owner_uuid
        .unwrap();
    let (_, session) = terminal(&running, owner);
    let record: Record = serde_json::from_value(fixture.records().unwrap()[0].clone()).unwrap();
    record.verify_live().unwrap();
    let sentinel = fixture.root.join("completion-save-sentinel");
    fs::write(&sentinel, b"completion save must not modify me").unwrap();
    fs::remove_file(state.join("session.json")).unwrap();
    std::os::unix::fs::symlink(&sentinel, state.join("session.json")).unwrap();
    send(
        &desktop,
        31,
        DesktopCommand::Input {
            session_id: session,
            text: "finish-once\r".into(),
        },
    );
    let failed = wait(&desktop, |s| {
        s.local_zellij_operations.iter().any(|r| r.session_joined)
            && s.persistence.operations.iter().any(|op| {
                op.kind == "local_zellij_completion"
                    && op
                        .error
                        .as_ref()
                        .is_some_and(|e| e.contains("open private session file"))
            })
    });
    assert!(!failed.persistence.autosave_enabled);
    let receipt = serde_json::to_value(failed.local_zellij_operations.last().unwrap()).unwrap();
    assert_eq!(
        receipt["workload_exit"],
        serde_json::json!({"kind":"exited","value":0})
    );
    assert!(receipt["completion_saved_revision"].is_null());
    assert_eq!(
        fs::read(&sentinel).unwrap(),
        b"completion save must not modify me"
    );
    let status = fixture.output(&["local-zellij", "status", "--id", &owner.to_string()]);
    assert!(status.status.success());
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["completion_authenticated"], true);
    assert_eq!(
        status["consumption_confirmed"], false,
        "failed native save must not ACK: {status}"
    );
    assert!(
        !fixture
            .root
            .join("profile")
            .join(format!("consumed-{owner}.json"))
            .exists()
    );
    assert_eq!(
        process_identity(record.supervisor.as_ref().unwrap().pid).unwrap(),
        *record.supervisor.as_ref().unwrap()
    );
    fs::remove_file(state.join("session.json")).unwrap();
    let ticket = desktop
        .begin_shutdown(Duration::from_secs(5), false)
        .unwrap()
        .ticket;
    let shutdown = desktop
        .wait_shutdown(ticket, Duration::from_secs(5))
        .unwrap();
    assert!(
        shutdown.physical_cleanup_complete && !shutdown.issues.is_empty(),
        "{shutdown:?}"
    );
    let cleaned = fixture.output(&[
        "local-zellij",
        "cleanup",
        "--id",
        &owner.to_string(),
        "--generation",
        &record
            .completion
            .as_ref()
            .unwrap()
            .owner_generation
            .to_string(),
    ]);
    assert!(
        cleaned.status.success(),
        "{}",
        String::from_utf8_lossy(&cleaned.stderr)
    );
    eprintln!(
        "R03 actual failed completion save: {}",
        serde_json::json!({"owner_uuid":owner,"workload_exit":0,"real_S01_writer_failed":true,"sentinel_unchanged":true,"native_ack_sent":false,"original_publisher_retained_until_explicit_cleanup":true,"physical_session_cleanup_complete":true})
    );
}

#[test]
fn actual_typed_input_ready_delivers_one_silent_first_line_per_attachment() {
    let fixture = Fixture::new();
    let state = fixture.root.join("readiness-desktop");
    fs::DirBuilder::new().mode(0o700).create(&state).unwrap();
    let count = fixture.root.join("silent-start-count");
    let lines = fixture.root.join("silent-lines");
    let script = fixture.root.join("silent.py");
    fs::write(&script,format!("import pathlib,select,sys,time\nend=time.monotonic()+30\nwith open({count:?},'ab') as f:f.write(b'x')\nfor i in range(2):\n if not select.select([0],[],[],max(0,end-time.monotonic()))[0]:sys.exit(98)\n line=sys.stdin.readline()\n if not line:sys.exit(99)\n with open({lines:?},'a') as f:f.write(line)\nsys.exit(7)\n",count=count.to_str().unwrap(),lines=lines.to_str().unwrap())).unwrap();
    let profile = uuid::Uuid::new_v4();
    let desktop = native_desktop(&fixture, profile, uuid::Uuid::new_v4(), Some(state));
    let initial = wait(&desktop, |s| {
        !s.persistence.startup_pending && s.pending_launches == 0 && !s.model.workspaces.is_empty()
    });
    let pane = initial.model.workspaces[0].focused_pane;
    send(
        &desktop,
        100,
        DesktopCommand::LocalZellij {
            request: ZellijRequest::Start {
                profile_id: profile,
                pane_id: pane,
                name: "silent-ready".into(),
                cwd: fixture.root.clone(),
                command: Some(format!("exec python3 '{}'", script.display())),
                focus: true,
            },
        },
    );
    let first = wait(&desktop, |s| {
        s.local_zellij_operations.iter().any(|r| r.input_ready)
    });
    let operation = first.local_zellij_operations.last().unwrap();
    assert_eq!(operation.state, "input_ready");
    assert_eq!(operation.execution_contract_version, Some(2));
    let owner = operation.owner_uuid.unwrap();
    let first_operation = operation.operation_id;
    let first_attachment = operation.attachment_id.unwrap();
    let (surface, session) = terminal(&first, owner);
    let record: Record = serde_json::from_value(fixture.records().unwrap()[0].clone()).unwrap();
    record.verify_live().unwrap();
    // No workload text, file, sleep, extra key, or viewport marker admits input.
    let first_line = format!("first-{}", uuid::Uuid::new_v4());
    send(
        &desktop,
        101,
        DesktopCommand::Input {
            session_id: session,
            text: format!("{first_line}\r"),
        },
    );
    wait(&desktop, |_| {
        fs::read_to_string(&lines).is_ok_and(|text| text == format!("{first_line}\n"))
    });
    assert_eq!(fs::read(&count).unwrap(), b"x");
    send(
        &desktop,
        102,
        DesktopCommand::Action {
            action: Action::CloseSurface {
                surface_id: surface,
            },
        },
    );
    wait(&desktop, |s| {
        s.local_zellij_operations
            .iter()
            .find(|r| r.operation_id == first_operation)
            .is_some_and(|r| {
                r.state == "frontend_joined"
                    && r.session_joined
                    && r.monitor_joined
                    && r.namespace_cleaned
                    && r.error.is_none()
            })
    });
    record.verify_live().unwrap();
    send(
        &desktop,
        103,
        DesktopCommand::LocalZellij {
            request: ZellijRequest::Attach {
                profile_id: profile,
                owner_uuid: owner,
                pane_id: pane,
                focus: true,
            },
        },
    );
    let second = wait(&desktop, |s| {
        s.local_zellij_operations
            .iter()
            .any(|r| r.operation_id != first_operation && r.input_ready)
    });
    let operation = second.local_zellij_operations.last().unwrap();
    let second_attachment = operation.attachment_id.unwrap();
    assert_ne!(second_attachment, first_attachment);
    assert_eq!(operation.execution_contract_version, Some(2));
    let (_, session) = terminal(&second, owner);
    let current: Record = serde_json::from_value(fixture.records().unwrap()[0].clone()).unwrap();
    assert_eq!(current.id, record.id);
    assert_eq!(current.server, record.server);
    assert_eq!(current.supervisor, record.supervisor);
    assert_eq!(current.workload, record.workload);
    current.verify_live().unwrap();
    let second_line = format!("second-{}", uuid::Uuid::new_v4());
    send(
        &desktop,
        104,
        DesktopCommand::Input {
            session_id: session,
            text: format!("{second_line}\r"),
        },
    );
    let completed = wait(&desktop, |s| {
        s.local_zellij_operations
            .last()
            .is_some_and(|r| r.session_joined)
    });
    let operation = completed.local_zellij_operations.last().unwrap();
    assert_eq!(
        operation.workload_exit,
        Some(zmux_remote_runtime::local_zellij::completion::WorkloadExit::Exited(7))
    );
    assert!(
        operation.final_frame_received
            && operation.monitor_joined
            && operation.namespace_cleaned
            && operation.error.is_none(),
        "{}",
        serde_json::to_value(operation).unwrap()
    );
    assert!(operation.completion_saved_revision.is_some());
    assert_eq!(
        fs::read_to_string(&lines).unwrap(),
        format!("{first_line}\n{second_line}\n")
    );
    assert_eq!(fs::read(&count).unwrap(), b"x");
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let cleanup = fixture.output(&[
        "local-zellij",
        "cleanup",
        "--id",
        &owner.to_string(),
        "--generation",
        &record
            .completion
            .as_ref()
            .unwrap()
            .owner_generation
            .to_string(),
    ]);
    assert!(
        cleanup.status.success(),
        "{}",
        String::from_utf8_lossy(&cleanup.stderr)
    );
    eprintln!(
        "R03 typed input readiness: {}",
        serde_json::json!({"owner_uuid":owner,"first_attachment":first_attachment,"second_attachment":second_attachment,"silent_workload":true,"input_admission":"typed_input_ready","first_line_once":true,"second_line_once":true,"startup_count":1,"actual_saved_completion":true,"actual_frontend_session_joined":true,"workload_exit":7})
    );
}

#[test]
fn actual_terminal_completion_before_input_ready_saves_and_joins_without_interactive_admission() {
    for code in [0, 7] {
        let fixture = Fixture::new();
        let state = fixture.root.join("terminal-before-ready");
        fs::DirBuilder::new().mode(0o700).create(&state).unwrap();
        let count = fixture.root.join("finite-once");
        let profile = uuid::Uuid::new_v4();
        let desktop = native_desktop(&fixture, profile, uuid::Uuid::new_v4(), Some(state));
        let initial = wait(&desktop, |s| {
            !s.persistence.startup_pending
                && s.pending_launches == 0
                && !s.model.workspaces.is_empty()
        });
        send(
            &desktop,
            200,
            DesktopCommand::LocalZellij {
                request: ZellijRequest::Start {
                    profile_id: profile,
                    pane_id: initial.model.workspaces[0].focused_pane,
                    name: "terminal-before-ready".into(),
                    cwd: fixture.root.clone(),
                    command: Some(format!("printf x >> '{}'; exit {code}", count.display())),
                    focus: true,
                },
            },
        );
        let completed = wait(&desktop, |s| {
            s.local_zellij_operations
                .last()
                .is_some_and(|r| r.session_joined)
        });
        let operation = completed.local_zellij_operations.last().unwrap();
        assert!(
            !operation.input_ready,
            "finite completion must not manufacture input admission"
        );
        assert_eq!(operation.execution_contract_version, Some(2));
        assert_eq!(
            operation.workload_exit,
            Some(zmux_remote_runtime::local_zellij::completion::WorkloadExit::Exited(code))
        );
        assert!(
            operation.final_frame_received
                && operation.monitor_joined
                && operation.namespace_cleaned
                && operation.error.is_none(),
            "{}",
            serde_json::to_value(operation).unwrap()
        );
        assert!(operation.completion_saved_revision.is_some());
        assert_eq!(fs::read(&count).unwrap(), b"x");
        let owner = operation.owner_uuid.unwrap();
        let record: Record = serde_json::from_value(fixture.records().unwrap()[0].clone()).unwrap();
        desktop.shutdown(Duration::from_secs(5)).unwrap();
        let cleanup = fixture.output(&[
            "local-zellij",
            "cleanup",
            "--id",
            &owner.to_string(),
            "--generation",
            &record
                .completion
                .as_ref()
                .unwrap()
                .owner_generation
                .to_string(),
        ]);
        assert!(
            cleanup.status.success(),
            "{}",
            String::from_utf8_lossy(&cleanup.stderr)
        );
        eprintln!(
            "R03 completion before input readiness: {}",
            serde_json::json!({"owner_uuid":owner,"workload_exit":code,"input_ready":false,"actual_saved_completion":true,"actual_frontend_session_joined":true,"startup_count":1})
        );
    }
}

#[test]
fn unsupported_readiness_capability_refuses_before_any_owner_reservation() {
    // A probe-only configured program supplies an unsupported capability. It is
    // never a fake owner/frontend or semantic-ready producer.
    use std::os::unix::fs::PermissionsExt;
    let mut fixture = Fixture::new();
    let probe = fixture.root.join("unsupported-probe");
    let queried = fixture.root.join("capability-queried");
    let forbidden = fixture.root.join("backend-must-not-launch");
    fs::write(&probe,format!("#!/bin/sh\ncase \"$1\" in\n --version) printf 'zellij 0.43.0\\n';;\n --zmux-input-readiness-protocol) printf x > '{}'; printf 'capability unavailable\\n' >&2; exit 2;;\n *) printf x > '{}'; exit 99;;\nesac\n",queried.display(),forbidden.display())).unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o700)).unwrap();
    fixture.binary = probe;
    let state = fixture.root.join("unsupported-desktop");
    fs::DirBuilder::new().mode(0o700).create(&state).unwrap();
    let profile = uuid::Uuid::new_v4();
    let desktop = native_desktop(&fixture, profile, uuid::Uuid::new_v4(), Some(state));
    let initial = wait(&desktop, |s| {
        !s.persistence.startup_pending && s.pending_launches == 0 && !s.model.workspaces.is_empty()
    });
    send(
        &desktop,
        300,
        DesktopCommand::LocalZellij {
            request: ZellijRequest::Start {
                profile_id: profile,
                pane_id: initial.model.workspaces[0].focused_pane,
                name: "unsupported-ready".into(),
                cwd: fixture.root.clone(),
                command: Some("exit 99".into()),
                focus: true,
            },
        },
    );
    let failed = wait(&desktop, |s| {
        s.local_zellij_operations
            .last()
            .is_some_and(|r| r.session_joined)
    });
    let operation = failed.local_zellij_operations.last().unwrap();
    assert!(
        queried.exists(),
        "refusal must reach the required capability probe"
    );
    assert!(!forbidden.exists());
    assert!(fixture.records().is_none());
    assert!(
        !operation.input_ready
            && operation.owner_uuid.is_none()
            && operation.checkpoint_revision.is_none()
    );
    assert!(operation.error.is_some() && operation.monitor_joined && operation.namespace_cleaned);
    let _ = desktop.shutdown(Duration::from_secs(5));
}
