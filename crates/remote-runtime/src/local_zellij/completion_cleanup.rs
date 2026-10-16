//! Explicit completed-owner retirement. Every process exits through its own
//! supervisor; deletion is descriptor-relative and never grants PID signal rights.
use super::{LocalZellij, Record, completion::TerminalReceipt, completion_control, fail, files};
use serde::Serialize;
use std::{
    fs::File,
    io,
    os::unix::fs::MetadataExt,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_session_runtime::child::{ProcessIdentity, process_identity};
#[derive(Serialize)]
pub struct CompletionStatus {
    #[serde(flatten)]
    pub record: Record,
    pub terminal_receipt: Option<TerminalReceipt>,
    pub completion_authenticated: bool,
    pub consumption_confirmed: bool,
    pub completion_error: Option<String>,
}
#[derive(Serialize)]
pub struct CleanupReceipt {
    pub owner_uuid: Uuid,
    pub owner_generation: Uuid,
    pub receipt_digest: String,
    pub namespace_cleaned: bool,
    pub registry_reclaimed: bool,
}
impl LocalZellij {
    pub fn inspect(&self, id: Uuid) -> io::Result<CompletionStatus> {
        let record = {
            let _lock = self.lock()?;
            self.records()?
                .into_iter()
                .find(|r| r.id == id)
                .ok_or_else(|| fail("local Zellij owner not found"))?
        };
        self.inspect_record(record)
    }
    fn inspect_record(&self, mut record: Record) -> io::Result<CompletionStatus> {
        let (terminal, consumed, error) = if record.completion.is_some() {
            match completion_control::Client::connect(
                &record,
                Instant::now() + Duration::from_secs(2),
            )
            .and_then(|mut c| c.state())
            {
                Ok((terminal, consumed)) => (terminal, consumed, None),
                Err(error) => (None, false, Some(error.to_string())),
            }
        } else {
            (None, false, None)
        };
        record.state = if terminal.is_some() {
            "completed_retained"
        } else if record.verify_live().is_ok() {
            "identity_present"
        } else {
            "unknown"
        }
        .into();
        Ok(CompletionStatus {
            record,
            completion_authenticated: terminal.is_some(),
            consumption_confirmed: consumed,
            terminal_receipt: terminal,
            completion_error: error,
        })
    }
    pub fn inspect_all(&self) -> io::Result<Vec<CompletionStatus>> {
        let records = {
            let _lock = self.lock()?;
            self.records()?
        };
        records
            .into_iter()
            .map(|r| self.inspect_record(r))
            .collect()
    }
    pub fn cleanup_completed(&self, id: Uuid, generation: Uuid) -> io::Result<CleanupReceipt> {
        if id.is_nil() || generation.is_nil() {
            return Err(fail(
                "completed cleanup requires nonnil owner and generation",
            ));
        }
        let record = {
            let _lock = self.lock()?;
            self.records()?
                .into_iter()
                .find(|r| r.id == id)
                .ok_or_else(|| fail("completed owner not found"))?
        };
        let binding = record.execution_binding()?;
        if binding.owner_generation() != generation {
            return Err(fail("completed cleanup owner generation differs"));
        }
        let mut client =
            completion_control::Client::connect(&record, Instant::now() + Duration::from_secs(3))?;
        let terminal = client
            .query()?
            .ok_or_else(|| fail("cleanup refuses running or unknown owner"))?;
        self.consume_terminal(&record, &terminal)?;
        client.retire(&terminal)?;
        let deadline = Instant::now() + Duration::from_secs(3);
        while !departed(binding.supervisor())? || !departed(binding.server())? {
            if Instant::now() >= deadline {
                return Err(fail(
                    "completed cleanup awaits original supervisor/backend departure; no signals sent",
                ));
            }
            thread::sleep(Duration::from_millis(5));
        }
        let root = record.runtime()?;
        let base = File::from(rustix::fs::open(
            files::runtime_base()?,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )?);
        let name = root
            .file_name()
            .ok_or_else(|| fail("missing completed runtime name"))?;
        let mut budget = 4096;
        remove_directory(&base, name, binding.namespace().root, 0, &mut budget)?;
        base.sync_all()?;
        let _lock = self.lock()?;
        let mut records = self.records()?;
        let current = records
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| fail("completed cleanup registry changed"))?;
        if current.execution_binding()? != binding {
            return Err(fail(
                "completed cleanup binding changed before registry reclaim",
            ));
        }
        records.retain(|r| r.id != id);
        self.save(&records)?;
        let profile = File::from(rustix::fs::open(
            &self.root,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )?);
        rustix::fs::unlinkat(
            &profile,
            format!("consumed-{id}.json"),
            rustix::fs::AtFlags::empty(),
        )?;
        profile.sync_all()?;
        Ok(CleanupReceipt {
            owner_uuid: id,
            owner_generation: generation,
            receipt_digest: terminal.digest()?,
            namespace_cleaned: true,
            registry_reclaimed: true,
        })
    }
}
fn departed(identity: &ProcessIdentity) -> io::Result<bool> {
    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(identity.pid as i32), None) {
        Err(nix::errno::Errno::ESRCH) => Ok(true),
        Err(e) => Err(e.into()),
        Ok(()) => match process_identity(identity.pid) {
            Ok(now) => Ok(now != *identity),
            Err(_) => Ok(false),
        },
    }
}
fn remove_directory(
    parent: &File,
    name: impl rustix::path::Arg,
    expected: (u64, u64),
    depth: usize,
    budget: &mut usize,
) -> io::Result<()> {
    use rustix::fs::{self as at, AtFlags, Mode, OFlags};
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    if depth > 16 || *budget == 0 {
        return Err(fail("completed runtime cleanup exceeds depth/file bound"));
    }
    let name = name.into_c_str()?.into_owned();
    let directory = File::from(at::openat(
        parent,
        &name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    let info = directory.metadata()?;
    if (info.dev(), info.ino()) != expected || info.uid() != nix::unistd::geteuid().as_raw() {
        return Err(fail("completed cleanup directory identity changed"));
    }
    for entry in at::Dir::read_from(&directory)? {
        let entry = entry?;
        let entry_name = entry.file_name();
        if entry_name == c"." || entry_name == c".." {
            continue;
        }
        *budget = budget
            .checked_sub(1)
            .ok_or_else(|| fail("completed cleanup file bound"))?;
        let current = at::statat(&directory, entry_name, AtFlags::SYMLINK_NOFOLLOW)?;
        if current.st_uid != nix::unistd::geteuid().as_raw() {
            return Err(fail("completed cleanup encountered unowned entry"));
        }
        let expected = stat_identity(&current);
        let kind = at::FileType::from_raw_mode(current.st_mode);
        if kind == at::FileType::Directory {
            remove_directory(
                &directory,
                OsString::from_vec(entry_name.to_bytes().to_vec()),
                expected,
                depth + 1,
                budget,
            )?;
        } else {
            if !matches!(kind, at::FileType::RegularFile | at::FileType::Socket) {
                return Err(fail(
                    "completed cleanup refuses symlink or unexpected entry type",
                ));
            }
            let again = at::statat(&directory, entry_name, AtFlags::SYMLINK_NOFOLLOW)?;
            if stat_identity(&again) != expected {
                return Err(fail("completed cleanup entry replaced"));
            }
            at::unlinkat(&directory, entry_name, AtFlags::empty())?;
        }
    }
    let again = at::statat(parent, &name, AtFlags::SYMLINK_NOFOLLOW)?;
    if stat_identity(&again) != expected {
        return Err(fail("completed cleanup root replaced"));
    }
    at::unlinkat(parent, &name, AtFlags::REMOVEDIR)?;
    Ok(())
}

// Match MetadataExt::dev() when comparing descriptor metadata with statat.
// Darwin exposes a signed dev_t; Linux already exposes the normalized u64.
fn stat_identity(stat: &rustix::fs::Stat) -> (u64, u64) {
    #[cfg(target_vendor = "apple")]
    let device = stat.st_dev as u64;
    #[cfg(not(target_vendor = "apple"))]
    let device = stat.st_dev;
    (device, stat.st_ino)
}
