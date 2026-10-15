//! Private credential-file loading shared by native server and CLI. No credentials are written.
use crate::control_policy::{ControlAccess, ControlPolicy, SocketPassword};
use nix::{libc, unistd::geteuid};
use std::{
    env,
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

/// Explicit password, then environment, then a private saved file. Offline CLI paths do not call it.
pub fn password(explicit: Option<SocketPassword>) -> Result<Option<SocketPassword>, String> {
    if let Some(value) = explicit {
        return Ok(Some(value));
    }
    if let Some(value) = env::var_os("ZMUX_SOCKET_PASSWORD") {
        return value
            .into_string()
            .map_err(|_| "ZMUX_SOCKET_PASSWORD must be valid UTF-8".to_owned())
            .and_then(|value| {
                SocketPassword::new(value)
                    .map(Some)
                    .map_err(|e| e.to_string())
            });
    }
    let (path, required) = match env::var_os("ZMUX_SOCKET_PASSWORD_FILE") {
        Some(path) => (PathBuf::from(path), true),
        None => {
            let Some(home) = env::var_os("HOME") else {
                return Ok(None);
            };
            (
                PathBuf::from(home).join(".config/zmux/socket-password"),
                false,
            )
        }
    };
    load_password(&path, required)
}

fn load_password(path: &Path, required: bool) -> Result<Option<SocketPassword>, String> {
    if !path.is_absolute() {
        return Err("socket password file path must be absolute".into());
    }
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if !required && error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("unable to open private socket password file".into()),
    };
    let metadata = file
        .metadata()
        .map_err(|_| "unable to inspect socket password file")?;
    if !metadata.is_file()
        || metadata.uid() != geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
        || metadata.len() > 1025
    {
        return Err("socket password file must be a regular file owned by this user, private (0600 or stricter), and at most 1025 bytes".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(1026)
        .read_to_end(&mut bytes)
        .map_err(|_| "unable to read socket password file")?;
    if bytes.len() > 1025 {
        return Err("socket password file exceeds its size limit".into());
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    let value = String::from_utf8(bytes).map_err(|_| "socket password file must contain UTF-8")?;
    SocketPassword::new(value)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Native server configuration. Unknown modes fail startup; credentials never enable access alone.
pub fn server_policy() -> Result<ControlPolicy, String> {
    let mode = match env::var_os("ZMUX_SOCKET_MODE") {
        Some(value) => value
            .into_string()
            .map_err(|_| "ZMUX_SOCKET_MODE must be valid UTF-8")?,
        None => "same_user".into(),
    };
    let (access, needs_password) = match mode.as_str() {
        "same_user" => (ControlAccess::Full, false),
        "read_only" => (ControlAccess::ReadOnly, false),
        "password" => (ControlAccess::Full, true),
        "password_read_only" => (ControlAccess::ReadOnly, true),
        "disabled" => (ControlAccess::Disabled, false),
        _ => return Err("invalid ZMUX_SOCKET_MODE; use same_user, read_only, password, password_read_only or disabled".into()),
    };
    if !needs_password {
        return Ok(ControlPolicy::same_user(access));
    }
    let password = password(None)?
        .ok_or("password socket mode requires a private saved password or ZMUX_SOCKET_PASSWORD")?;
    Ok(ControlPolicy::password(access, password))
}
