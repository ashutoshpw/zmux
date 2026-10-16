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
    ) || process_identity(identity.pid).is_ok_and(|current| current != *identity)
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
            .any(|r| r.state == "transport_connected")
            && checkpoint.exists()
    });
    let first = connected.local_zellij_operations.last().unwrap().clone();
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
        s.local_zellij_operations
            .iter()
            .any(|r| r.operation_id != first.operation_id && r.state == "transport_connected")
    });
    let (_, session) = terminal(&attached, owner);
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
