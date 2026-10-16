//! Owned plugin publication and nonblocking hook ingress through the shipped CLI.
//! Synthetic provider --version only; no Claude invocation or user configuration.
use serde_json::Value;
use std::{
    fs,
    io::Write,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::{Command, Output, Stdio},
};
// A fork in another test thread can transiently inherit a writable executable
// copy before exec closes CLOEXEC descriptors. Keep executable publication and
// child lifetimes sequential within this test process rather than retrying exec.
static PROCESS_FIXTURE: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct Directory {
    path: PathBuf,
    _process: std::sync::MutexGuard<'static, ()>,
}
impl Directory {
    fn new() -> Self {
        let guard = PROCESS_FIXTURE.lock().unwrap();
        let p = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("zmux-hooks-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            path: p,
            _process: guard,
        }
    }
    fn plugin(&self) -> PathBuf {
        self.path.join("owned-plugin")
    }
    fn provider(&self) -> PathBuf {
        let p = self.path.join("synthetic-provider");
        if p.exists() {
            return p;
        }
        fs::write(&p,"#!/bin/sh\nif [ \"$1\" = --version ]; then printf '2.1.301 (Claude Code)\\n'; else printf invoked > \"$FIXTURE_PROVIDER_CALLED\"; fi\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        p
    }
    fn command(&self, args: &[&str]) -> Command {
        let adapter = self.path.join("adapter");
        if !adapter.exists() {
            fs::copy(env!("CARGO_BIN_EXE_zmux"), &adapter).unwrap();
            fs::set_permissions(&adapter, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut c = Command::new(adapter);
        c.env("HOME", &self.path)
            .env("FIXTURE_PROVIDER_CALLED", self.path.join("provider-called"));
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("ZMUX_") {
                c.env_remove(name);
            }
        }
        c.args(args);
        c
    }
    fn install(&self) -> Value {
        decoded(
            self.command(&[
                "hooks",
                "claude",
                "install",
                "--directory",
                self.plugin().to_str().unwrap(),
                "--provider",
                self.provider().to_str().unwrap(),
            ])
            .output()
            .unwrap(),
        )
    }
    fn call(&self, verb: &str) -> Value {
        decoded(
            self.command(&[
                "hooks",
                "claude",
                verb,
                "--directory",
                self.plugin().to_str().unwrap(),
            ])
            .output()
            .unwrap(),
        )
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).unwrap();
    }
}
fn decoded(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn owned_plugin_is_idempotent_and_uninstall_preserves_unrelated_provider_settings() {
    let dir = Directory::new();
    let unrelated = dir.path.join(".claude");
    fs::create_dir(&unrelated).unwrap();
    let sentinel = b"{\"unrelated\":true,\"hooks\":{\"Stop\":[{\"command\":\"keep me\"}]}}";
    fs::write(unrelated.join("settings.json"), sentinel).unwrap();
    let first = dir.install();
    assert_eq!(first["changed"], true);
    let second = dir.install();
    assert_eq!(second["changed"], false);
    assert_eq!(first["generation"], second["generation"]);
    assert_eq!(dir.call("status")["installed"], true);
    assert_eq!(fs::read(unrelated.join("settings.json")).unwrap(), sentinel);
    let result = dir.call("uninstall");
    assert_eq!(result["disabled"], true);
    assert_eq!(result["cleanup_pending"], false);
    assert_eq!(dir.call("status")["installed"], false);
    assert_eq!(dir.call("uninstall")["cleanup_pending"], false);
    assert_eq!(fs::read(unrelated.join("settings.json")).unwrap(), sentinel);
}
#[test]
fn changed_or_symlinked_plugin_files_are_retained_after_admissions_are_disabled() {
    for substitute in [false, true] {
        let dir = Directory::new();
        let installed = dir.install();
        let config = dir
            .plugin()
            .join(installed["generation"].as_str().unwrap())
            .join("hooks/hooks.json");
        let unrelated = dir.path.join("unrelated.json");
        fs::write(&unrelated, b"unrelated sentinel").unwrap();
        if substitute {
            fs::remove_file(&config).unwrap();
            symlink(&unrelated, &config).unwrap();
        } else {
            fs::write(&config, b"changed user file").unwrap();
        }
        let before = fs::read(&config).unwrap();
        let result = dir.call("uninstall");
        assert_eq!(result["disabled"], true);
        assert_eq!(result["cleanup_pending"], true);
        assert_eq!(fs::read(&config).unwrap(), before);
        assert_eq!(fs::read(&unrelated).unwrap(), b"unrelated sentinel");
        let record: Value =
            serde_json::from_slice(&fs::read(dir.plugin().join("installation.json")).unwrap())
                .unwrap();
        assert_eq!(record["enabled"], false);
    }
}
#[test]
fn malformed_unrouted_and_oversized_hooks_are_nonblocking_noops() {
    let dir = Directory::new();
    for bytes in [b"not JSON".to_vec(), b"{}".to_vec(), vec![b'x'; 70_000]] {
        let mut child = dir
            .command(&["hooks", "claude", "event", "UserPromptSubmit"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let _ = input.write_all(&bytes);
        drop(input);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            serde_json::json!({})
        );
        assert!(output.stderr.is_empty());
    }
}
#[test]
fn headless_and_unsupported_launches_never_start_the_provider() {
    let dir = Directory::new();
    dir.install();
    for args in [
        vec![],
        vec!["--print", "prompt"],
        vec!["--settings", "{}"],
        vec!["resume"],
        vec!["--bare"],
    ] {
        let mut command = dir.command(&[
            "hooks",
            "claude",
            "run",
            "--directory",
            dir.plugin().to_str().unwrap(),
            "--surface",
            &uuid::Uuid::new_v4().to_string(),
            "--",
        ]);
        command.args(args);
        let output = command.output().unwrap();
        assert!(!output.status.success());
        assert!(!dir.path.join("provider-called").exists());
    }
}

#[test]
fn uninstall_retains_identical_bytes_at_an_unowned_replacement_inode() {
    use std::os::unix::fs::MetadataExt;
    let dir = Directory::new();
    let installed = dir.install();
    let config = dir
        .plugin()
        .join(installed["generation"].as_str().unwrap())
        .join("hooks/hooks.json");
    let inode = fs::metadata(&config).unwrap().ino();
    let replacement = config.with_extension("replacement");
    fs::write(&replacement, fs::read(&config).unwrap()).unwrap();
    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o600)).unwrap();
    fs::rename(replacement, &config).unwrap();
    assert_ne!(fs::metadata(&config).unwrap().ino(), inode);
    let result = dir.call("uninstall");
    assert_eq!(result["disabled"], true);
    assert_eq!(result["cleanup_pending"], true);
    assert!(
        config.exists(),
        "same bytes do not prove ownership of a replacement file"
    );
}
#[test]
fn interrupted_uninstall_removes_only_remaining_verified_owned_artifacts() {
    let dir = Directory::new();
    let installed = dir.install();
    let generation = dir.plugin().join(installed["generation"].as_str().unwrap());
    // A prior cleanup may have unlinked one verified file before interruption.
    fs::remove_file(generation.join("hooks/hooks.json")).unwrap();
    let orphan = dir.plugin().join("unpublished-generation");
    fs::create_dir(&orphan).unwrap();
    fs::write(orphan.join("keep"), b"unknown staging evidence").unwrap();
    let result = dir.call("uninstall");
    assert_eq!(result["cleanup_pending"], false);
    assert!(!generation.exists());
    assert_eq!(
        fs::read(orphan.join("keep")).unwrap(),
        b"unknown staging evidence"
    );
    assert_eq!(dir.call("uninstall")["cleanup_pending"], false);
}
