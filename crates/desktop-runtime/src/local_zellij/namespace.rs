use super::Correlation;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::UnixListener,
    },
    path::{Path, PathBuf},
};
use uuid::Uuid;
use zmux_remote_runtime::local_zellij::observer_protocol::{Descriptor, FRAME_LIMIT, runtime_base};
use zmux_session_runtime::child::ProcessIdentity;

fn fail(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}
fn inode(path: &Path) -> io::Result<(u64, u64)> {
    let m = path.symlink_metadata()?;
    Ok((m.dev(), m.ino()))
}
fn private(path: &Path, kind: u8) -> io::Result<()> {
    let m = path.symlink_metadata()?;
    let type_ok = match kind {
        0 => m.is_dir(),
        1 => m.file_type().is_socket(),
        _ => m.is_file() && m.nlink() == 1 && m.len() <= FRAME_LIMIT as u64,
    };
    if !type_ok || m.uid() != nix::unistd::geteuid().as_raw() || m.mode() & 0o077 != 0 {
        return Err(fail(
            "local Zellij observer namespace type/owner/mode changed",
        ));
    }
    Ok(())
}
#[derive(Clone)]
pub(super) struct View {
    root: PathBuf,
    socket: PathBuf,
    descriptor: PathBuf,
    root_id: (u64, u64),
    socket_id: (u64, u64),
    descriptor_id: (u64, u64),
}
impl View {
    pub fn validate(&self) -> io::Result<()> {
        private(&self.root, 0)?;
        private(&self.socket, 1)?;
        private(&self.descriptor, 2)?;
        if inode(&self.root)? != self.root_id
            || inode(&self.socket)? != self.socket_id
            || inode(&self.descriptor)? != self.descriptor_id
            || self.root.canonicalize()? != self.root
        {
            return Err(fail("local Zellij observer namespace inode changed"));
        }
        Ok(())
    }
}
pub(super) struct Namespace {
    pub view: View,
    pub nonce: String,
    cleaned: bool,
}
impl Namespace {
    pub fn create(
        context: Correlation,
        launcher: ProcessIdentity,
    ) -> io::Result<(Self, UnixListener)> {
        let root = runtime_base()?.join(format!("zo{}", Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&root)?;
        let root_id = inode(&root)?;
        let mut created_socket = None;
        let mut created_descriptor = None;
        let result = (|| {
            private(&root, 0)?;
            let socket = root.join("s");
            let descriptor = root.join("request.json");
            let listener = UnixListener::bind(&socket)?;
            created_socket = Some(inode(&socket)?);
            fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
            listener.set_nonblocking(true)?;
            let nonce = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
            let bytes = serde_json::to_vec(&Descriptor {
                version: 3,
                socket: socket.clone(),
                launcher,
                context,
                nonce: nonce.clone(),
            })
            .map_err(io::Error::other)?;
            if bytes.len() > FRAME_LIMIT {
                return Err(fail("observer descriptor exceeds bound"));
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&descriptor)?;
            let info = file.metadata()?;
            created_descriptor = Some((info.dev(), info.ino()));
            file.write_all(&bytes)?;
            file.sync_all()?;
            let view = View {
                root: root.clone(),
                root_id,
                socket_id: inode(&socket)?,
                descriptor_id: inode(&descriptor)?,
                socket,
                descriptor,
            };
            view.validate()?;
            Ok((
                Self {
                    view,
                    nonce,
                    cleaned: false,
                },
                listener,
            ))
        })();
        if result.is_err() && inode(&root).ok() == Some(root_id) {
            // Setup failures have no child. Retained inode identities still fence
            // cleanup so a replaced name never becomes deletion authority.
            for (name, expected) in [("request.json", created_descriptor), ("s", created_socket)] {
                let path = root.join(name);
                if expected.is_some()
                    && inode(&path).ok() == expected
                    && let Err(error) = fs::remove_file(&path)
                {
                    eprintln!("local Zellij observer setup cleanup: {error}");
                }
            }
            if inode(&root).ok() == Some(root_id)
                && let Err(error) = fs::remove_dir(&root)
            {
                eprintln!("local Zellij observer root setup cleanup: {error}");
            }
        }
        result
    }
    pub fn descriptor(&self) -> &Path {
        &self.view.descriptor
    }
    pub fn cleanup(&mut self) -> io::Result<()> {
        if self.cleaned {
            return Ok(());
        }
        self.view.validate()?;
        fs::remove_file(&self.view.descriptor)?;
        // Recheck each retained inode immediately before unlinking its name.
        if inode(&self.view.root)? != self.view.root_id
            || inode(&self.view.socket)? != self.view.socket_id
        {
            return Err(fail("observer cleanup namespace changed"));
        }
        fs::remove_file(&self.view.socket)?;
        if inode(&self.view.root)? != self.view.root_id {
            return Err(fail("observer cleanup root changed"));
        }
        fs::remove_dir(&self.view.root)?;
        self.cleaned = true;
        Ok(())
    }
}
impl Drop for Namespace {
    fn drop(&mut self) {
        if !self.cleaned
            && let Err(e) = self.cleanup()
        {
            eprintln!("local Zellij observer namespace retained after cleanup failure: {e}");
        }
    }
}
