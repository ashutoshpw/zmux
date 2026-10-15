//! Real isolated tmux servers own persistence/identity; no fake tmux executable.
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt, symlink},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_remote_runtime::local_multiplexer::{
    LocalTmux, MAX_REGISTRY_BYTES, MultiplexerError, SessionSelector,
};
struct Fixture {
    root: PathBuf,
    binary: PathBuf,
    owner: LocalTmux,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from("/tmp").join(format!("zmt-{}", Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let binary = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join("tmux"))
            .find(|p| p.is_file())
            .expect("real tmux is required for local multiplexer contracts");
        let owner = LocalTmux::new(root.join("profile"), binary.clone()).unwrap();
        Self {
            root,
            binary,
            owner,
        }
    }
    fn tmux(&self, args: &[&str]) -> std::process::Output {
        Command::new(&self.binary)
            .args(["-f", "/dev/null", "-S"])
            .arg(self.owner.socket_path())
            .args(args)
            .env_remove("TMUX")
            .output()
            .unwrap()
    }
    fn run(&self, args: &[&str]) -> String {
        let output = self.tmux(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    fn start(&self, name: &str) -> zmux_remote_runtime::local_multiplexer::SessionStatus {
        self.owner
            .start(name, &self.root, Some("exec sleep 60"))
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if fs::symlink_metadata(self.owner.socket_path()).is_ok_and(|m| m.file_type().is_socket()) {
            let _ = self.tmux(&["kill-server"]);
        }
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn name(n: &str) -> SessionSelector {
    SessionSelector::name(n).unwrap()
}
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "tmux observation timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn durable_owner_tracks_rename_and_preserves_unmanaged_sessions_on_close_cleanup() {
    let f = Fixture::new();
    let cwd = f.root.join("cwd #() ' spaced");
    fs::create_dir(&cwd).unwrap();
    let output = f.root.join("literal");
    let text = "literal $(touch unselected) ' ; text";
    let command = format!(
        "printf '%s' {} > {}; exec sleep 60",
        quote(text),
        quote(output.to_str().unwrap())
    );
    let session = f.owner.start("work", &cwd, Some(&command)).unwrap();
    wait(|| fs::read_to_string(&output).is_ok_and(|value| value == text));
    assert_eq!(fs::read_to_string(output).unwrap(), text);
    assert_eq!(session.state, "live");
    assert!(session.clients.is_empty());
    assert_eq!(
        f.run(&[
            "display-message",
            "-p",
            "-t",
            &session.tmux_session_id,
            "#{pane_current_path}"
        ])
        .trim(),
        cwd.to_str().unwrap()
    );
    assert_eq!(
        f.run(&["show-options", "-g", "history-limit"]).trim(),
        "history-limit 10000"
    );
    for (path, mode) in [
        (f.root.join("profile"), 0o700),
        (f.root.join("profile/registry.lock"), 0o600),
        (f.root.join("profile/registry.json"), 0o600),
        (f.owner.socket_path(), 0o600),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            mode
        );
    }
    f.run(&["rename-session", "-t", &session.tmux_session_id, "renamed"]);
    assert_eq!(
        f.owner
            .status(&SessionSelector::Id(session.id))
            .unwrap()
            .name,
        "renamed"
    );
    f.run(&["new-session", "-d", "-s", "outside", "exec sleep 60"]);
    let list = f.owner.list().unwrap();
    assert_eq!(list.unmanaged.len(), 1);
    assert_eq!(list.unmanaged[0].name, "outside");
    assert!(matches!(
        f.owner.close(&name("outside")),
        Err(MultiplexerError::NotFound)
    ));
    assert!(f.owner.cleanup(false).unwrap().stale_ids.is_empty());
    assert!(f.owner.cleanup(true).unwrap().stale_ids.is_empty());
    f.owner.close(&SessionSelector::Id(session.id)).unwrap();
    assert!(f.owner.list().unwrap().sessions.is_empty());
    assert!(f.tmux(&["has-session", "-t", "=outside"]).status.success());
}
#[test]
fn server_restart_and_reused_session_id_cannot_retarget_saved_attach_detach_or_close() {
    let f = Fixture::new();
    let old = f.start("work");
    let attach = f.owner.attach_spec(&SessionSelector::Id(old.id)).unwrap();
    f.run(&["kill-server"]);
    wait(|| std::os::unix::net::UnixStream::connect(f.owner.socket_path()).is_err());
    f.run(&["new-session", "-d", "-s", "replacement", "exec sleep 60"]);
    fs::set_permissions(f.owner.socket_path(), fs::Permissions::from_mode(0o600)).unwrap();
    let live = f.run(&["list-sessions", "-F", "#{session_id}"]);
    assert_eq!(live.trim(), old.tmux_session_id);
    assert_eq!(
        f.owner.status(&SessionSelector::Id(old.id)).unwrap().state,
        "stale"
    );
    assert!(matches!(
        f.owner.detach(&SessionSelector::Id(old.id), true),
        Err(MultiplexerError::Stale)
    ));
    assert!(matches!(
        f.owner.close(&SessionSelector::Id(old.id)),
        Err(MultiplexerError::Stale)
    ));
    assert!(matches!(
        f.owner.attach_spec(&SessionSelector::Id(old.id)),
        Err(MultiplexerError::Stale)
    ));
    let rejected = Command::new(attach.program)
        .args(attach.args)
        .env_remove("TMUX")
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("__zmux_stale_local_tmux_binding__")
    );
    assert_eq!(f.owner.cleanup(false).unwrap().stale_ids, [old.id]);
    assert_eq!(f.owner.list().unwrap().sessions.len(), 1);
    assert_eq!(f.owner.cleanup(true).unwrap().stale_ids, [old.id]);
    let list = f.owner.list().unwrap();
    assert!(list.sessions.is_empty());
    assert_eq!(list.unmanaged[0].name, "replacement");
    assert!(
        f.tmux(&["has-session", "-t", "=replacement"])
            .status
            .success()
    );
}
struct Client(Child);
impl Client {
    fn attach(f: &Fixture, session: &str) -> Self {
        Self(
            Command::new(&f.binary)
                .args(["-f", "/dev/null", "-S"])
                .arg(f.owner.socket_path())
                .args(["-C", "attach-session", "-t", session])
                .env_remove("TMUX")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            self.0.kill().unwrap();
        }
        self.0.wait().unwrap();
    }
}
#[test]
fn detach_requires_explicit_all_for_multiple_clients_and_preserves_foreign_clients() {
    let f = Fixture::new();
    let managed = f.start("work");
    f.run(&["new-session", "-d", "-s", "outside", "exec sleep 60"]);
    let _one = Client::attach(&f, "work");
    let _two = Client::attach(&f, "work");
    let _foreign = Client::attach(&f, "outside");
    wait(|| f.owner.status(&name("work")).unwrap().clients.len() == 2);
    assert!(matches!(
        f.owner.detach(&name("work"), false),
        Err(MultiplexerError::AmbiguousClients)
    ));
    assert_eq!(f.owner.status(&name("work")).unwrap().clients.len(), 2);
    let detached = f.owner.detach(&name("work"), true).unwrap();
    assert_eq!(detached.id, managed.id);
    assert_eq!(detached.detached_clients, 2);
    wait(|| f.owner.status(&name("work")).unwrap().clients.is_empty());
    assert_eq!(
        f.run(&["list-clients", "-t", "=outside", "-F", "#{client_name}"])
            .lines()
            .count(),
        1
    );
    let _single = Client::attach(&f, "work");
    wait(|| f.owner.status(&name("work")).unwrap().clients.len() == 1);
    assert_eq!(
        f.owner
            .detach(&name("work"), false)
            .unwrap()
            .detached_clients,
        1
    );
}
#[test]
fn unsafe_profile_objects_and_malformed_bounded_registry_fail_without_touching_links() {
    let f = Fixture::new();
    f.owner.list().unwrap();
    let root = f.root.join("profile");
    let registry = root.join("registry.json");
    let victim = f.root.join("victim");
    fs::write(&victim, b"preserved").unwrap();
    symlink(&victim, &registry).unwrap();
    assert!(f.owner.list().is_err());
    assert_eq!(fs::read(&victim).unwrap(), b"preserved");
    fs::remove_file(&registry).unwrap();
    fs::write(&registry, b"malformed").unwrap();
    fs::set_permissions(&registry, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(f.owner.list().is_err());
    fs::write(&registry, vec![b'x'; MAX_REGISTRY_BYTES + 1]).unwrap();
    assert!(
        f.owner
            .list()
            .unwrap_err()
            .to_string()
            .contains("byte bound")
    );
    fs::remove_file(&registry).unwrap();
    fs::set_permissions(
        root.join("registry.lock"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(matches!(f.owner.list(), Err(MultiplexerError::Unsafe(_))));
    fs::set_permissions(
        root.join("registry.lock"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(f.owner.list(), Err(MultiplexerError::Unsafe(_))));
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&victim, f.owner.socket_path()).unwrap();
    assert!(matches!(f.owner.list(), Err(MultiplexerError::Unsafe(_))));
    assert_eq!(fs::read(victim).unwrap(), b"preserved");
}

#[test]
fn duplicate_stale_and_renamed_live_names_require_explicit_id_for_status() {
    let f = Fixture::new();
    let stale = f.start("first");
    let live = f.start("second");
    f.run(&["kill-session", "-t", &stale.tmux_session_id]);
    f.run(&["rename-session", "-t", &live.tmux_session_id, "first"]);
    let error = f.owner.status(&name("first")).unwrap_err();
    assert!(error.to_string().contains("ambiguous"), "{error}");
    assert_eq!(
        f.owner
            .status(&SessionSelector::Id(stale.id))
            .unwrap()
            .state,
        "stale"
    );
    assert_eq!(
        f.owner.status(&SessionSelector::Id(live.id)).unwrap().state,
        "live"
    );
}

#[test]
fn saved_identity_pane_routing_and_gui_hints_follow_exact_owned_objects() {
    use zmux_remote_runtime::local_multiplexer::GuiBinding;
    let f = Fixture::new();
    let first = f.start("first");
    let second = f.start("second");
    let identity = first.identity();
    let pane = f.run(&[
        "display-message",
        "-p",
        "-t",
        &first.tmux_session_id,
        "#{pane_id}",
    ]);
    assert_eq!(
        f.owner
            .pane_identity(&f.owner.socket_path(), pane.trim())
            .unwrap(),
        identity
    );
    assert!(
        f.owner
            .pane_identity(&f.root.join("foreign.sock"), pane.trim())
            .is_err()
    );
    let binding = GuiBinding {
        identity_namespace: Uuid::new_v4(),
        workspace: Uuid::new_v4(),
        surface: Uuid::new_v4(),
    };
    f.owner.remember_gui(&identity, binding.clone()).unwrap();
    assert_eq!(
        f.owner.status_identity(&identity).unwrap().gui_binding,
        Some(binding)
    );
    assert!(f.owner.attach_identity(&identity).is_ok());
    f.run(&[
        "move-pane",
        "-s",
        pane.trim(),
        "-t",
        &second.tmux_session_id,
    ]);
    assert_eq!(
        f.owner
            .pane_identity(&f.owner.socket_path(), pane.trim())
            .unwrap(),
        second.identity()
    );
    assert!(f.owner.attach_identity(&identity).is_err());
    // Logical UUID lookup alone must not accept a saved descriptor with different immutable fields.
    let mut altered = second.identity();
    altered.created_at += 1;
    assert!(f.owner.status_identity(&altered).is_err());
    assert!(f.owner.attach_identity(&altered).is_err());
    f.run(&["new-session", "-d", "-s", "outside", "exec sleep 60"]);
    let outside = f.run(&["display-message", "-p", "-t", "=outside", "#{pane_id}"]);
    assert!(
        f.owner
            .pane_identity(&f.owner.socket_path(), outside.trim())
            .is_err()
    );
}
