//! Original C ABI feasibility bridge for the Rust workspace model.
//! It does not yet execute session effects or implement native async rendering callbacks.

use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, TryLockError},
};
use zmux_desktop_runtime::{
    Command as DesktopCommand, Desktop, DesktopError, Request as DesktopRequest, Shell,
    control::ControlServer, transfers::TransferCommand,
};
use zmux_workspace_core::{Action, WorkspaceStore};

pub const ABI_VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 16_384;
const MAX_RESPONSE_BYTES: usize = 8_388_608;
const MAX_CORES: usize = 32;
const MAX_BUFFERS: usize = 16;

const OK: i32 = 0;
const INVALID_HANDLE: i32 = -1;
const INVALID_ARGUMENT: i32 = -2;
const BUFFER_TOO_SMALL: i32 = -3;
const BUSY: i32 = -4;
const CAPACITY: i32 = -5;
const INTERNAL: i32 = -6;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ZmuxResult {
    pub status: i32,
    pub value: u64,
}
impl ZmuxResult {
    fn ok(value: u64) -> Self {
        Self { status: OK, value }
    }
    fn error(status: i32) -> Self {
        Self { status, value: 0 }
    }
}

enum Entry {
    Desktop(Arc<NativeDesktop>),
    Core(Arc<Mutex<WorkspaceStore>>),
    Buffer(Option<Arc<Vec<u8>>>),
}
struct NativeDesktop {
    runtime: Arc<Desktop>,
    control: Option<ControlServer>,
}
struct Registry {
    next: u64,
    entries: HashMap<u64, Entry>,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            next: 1,
            entries: HashMap::new(),
        }
    }
}
impl Registry {
    fn insert(&mut self, entry: Entry) -> Result<u64, i32> {
        let id = self.next;
        self.next = self.next.checked_add(1).ok_or(CAPACITY)?;
        self.entries.insert(id, entry);
        Ok(id)
    }
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

// Reserve response storage before applying an action, so capacity rejection cannot mutate state.
struct Reservation {
    id: u64,
    published: bool,
}
impl Reservation {
    fn new() -> Result<Self, i32> {
        let mut registry = registry().try_lock().map_err(lock_status)?;
        if registry
            .entries
            .values()
            .filter(|e| matches!(e, Entry::Buffer(_)))
            .count()
            >= MAX_BUFFERS
        {
            return Err(CAPACITY);
        }
        Ok(Self {
            id: registry.insert(Entry::Buffer(None))?,
            published: false,
        })
    }
    fn publish(mut self, bytes: Vec<u8>) -> Result<u64, i32> {
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(INTERNAL);
        }
        let mut registry = registry().lock().map_err(|_| INTERNAL)?;
        let slot = registry.entries.get_mut(&self.id).ok_or(INTERNAL)?;
        *slot = Entry::Buffer(Some(Arc::new(bytes)));
        self.published = true;
        Ok(self.id)
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.published {
            match registry().lock() {
                Ok(mut registry) => {
                    registry.entries.remove(&self.id);
                }
                Err(error) => eprintln!("zmux bridge reservation cleanup failed: {error}"),
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u32,
    command: Command,
}
#[derive(Deserialize)]
#[serde(
    tag = "method",
    content = "params",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Command {
    Snapshot,
    Apply(Action),
}

fn core(id: u64) -> Result<Arc<Mutex<WorkspaceStore>>, i32> {
    let registry = registry().try_lock().map_err(lock_status)?;
    match registry.entries.get(&id) {
        Some(Entry::Core(core)) => Ok(Arc::clone(core)),
        _ => Err(INVALID_HANDLE),
    }
}
fn buffer(id: u64) -> Result<Arc<Vec<u8>>, i32> {
    let registry = registry().try_lock().map_err(lock_status)?;
    match registry.entries.get(&id) {
        Some(Entry::Buffer(Some(bytes))) => Ok(Arc::clone(bytes)),
        _ => Err(INVALID_HANDLE),
    }
}
fn lock_status<T>(error: TryLockError<T>) -> i32 {
    match error {
        TryLockError::WouldBlock => BUSY,
        TryLockError::Poisoned(_) => INTERNAL,
    }
}
fn guard(f: impl FnOnce() -> Result<u64, i32>) -> ZmuxResult {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(value)) => ZmuxResult::ok(value),
        Ok(Err(status)) => ZmuxResult::error(status),
        Err(_) => ZmuxResult::error(INTERNAL),
    }
}
fn error_reply(code: &str, message: String) -> serde_json::Value {
    json!({"version": ABI_VERSION, "ok": false, "error": {"code": code, "message": message.chars().take(256).collect::<String>()}})
}

fn transfer_error_reply(message: String, outcome_unknown: bool) -> serde_json::Value {
    json!({
        "ok": false,
        "error": {"message": message.chars().take(256).collect::<String>()},
        "outcome_unknown": outcome_unknown,
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn zmux_abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn zmux_core_create() -> ZmuxResult {
    guard(|| {
        let mut registry = registry().try_lock().map_err(lock_status)?;
        if registry
            .entries
            .values()
            .filter(|e| matches!(e, Entry::Core(_)))
            .count()
            >= MAX_CORES
        {
            return Err(CAPACITY);
        }
        registry.insert(Entry::Core(Arc::new(Mutex::new(
            WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4()),
        ))))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn zmux_core_destroy(id: u64) -> i32 {
    guard(|| {
        let mut registry = registry().try_lock().map_err(lock_status)?;
        if !matches!(registry.entries.get(&id), Some(Entry::Core(_))) {
            return Err(INVALID_HANDLE);
        }
        registry.entries.remove(&id);
        Ok(0)
    })
    .status
}

/// # Safety
/// `request` must reference `length` readable bytes for the duration of this call.
/// It may be null only when length is zero. Non-null dangling pointers cannot be validated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_core_dispatch(
    id: u64,
    request: *const u8,
    length: usize,
) -> ZmuxResult {
    guard(|| {
        if length > MAX_REQUEST_BYTES || (request.is_null() && length != 0) {
            return Err(INVALID_ARGUMENT);
        }
        let core = core(id)?;
        let reservation = Reservation::new()?;
        // SAFETY: the caller guarantees readable storage; null/length bounds were checked above.
        let bytes = if length == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(request, length) }
        };
        let reply = match serde_json::from_slice::<Request>(bytes) {
            Err(error) => error_reply("invalid_request", error.to_string()),
            Ok(request) if request.version != ABI_VERSION => {
                error_reply("unsupported_version", "expected protocol version 1".into())
            }
            Ok(request) => {
                let mut store = core.try_lock().map_err(lock_status)?;
                match request.command {
                    Command::Snapshot => {
                        json!({"version": ABI_VERSION, "ok": true, "snapshot": store.snapshot(), "effects": []})
                    }
                    Command::Apply(action) => match store.apply(action) {
                        Ok(transition) => {
                            json!({"version": ABI_VERSION, "ok": true, "snapshot": store.snapshot(), "effects": transition.effects})
                        }
                        Err(error) => error_reply("action_rejected", error.to_string()),
                    },
                }
            }
        };
        reservation.publish(serde_json::to_vec(&reply).map_err(|_| INTERNAL)?)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn zmux_buffer_length(id: u64) -> ZmuxResult {
    guard(|| Ok(buffer(id)?.len() as u64))
}

/// # Safety
/// `destination` must reference `capacity` writable bytes and may not overlap any input.
/// No write occurs on invalid handle, insufficient capacity, or null destination.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_buffer_copy(id: u64, destination: *mut u8, capacity: usize) -> i32 {
    guard(|| {
        let bytes = buffer(id)?;
        if capacity < bytes.len() {
            return Err(BUFFER_TOO_SMALL);
        }
        if destination.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        // SAFETY: the destination is caller-owned writable storage; source is an immutable Arc.
        // Borrowing the source Arc keeps it alive even if another caller releases its token.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
        }
        Ok(0)
    })
    .status
}

#[unsafe(no_mangle)]
pub extern "C" fn zmux_buffer_release(id: u64) -> i32 {
    guard(|| {
        let mut registry = registry().try_lock().map_err(lock_status)?;
        if !matches!(registry.entries.get(&id), Some(Entry::Buffer(Some(_)))) {
            return Err(INVALID_HANDLE);
        }
        registry.entries.remove(&id);
        Ok(0)
    })
    .status
}

fn desktop(id: u64) -> Result<Arc<Desktop>, i32> {
    let registry = registry().try_lock().map_err(lock_status)?;
    match registry.entries.get(&id) {
        Some(Entry::Desktop(desktop)) => Ok(Arc::clone(&desktop.runtime)),
        _ => Err(INVALID_HANDLE),
    }
}
fn native_desktop(id: u64) -> Result<Arc<NativeDesktop>, i32> {
    let registry = registry().try_lock().map_err(lock_status)?;
    match registry.entries.get(&id) {
        Some(Entry::Desktop(desktop)) => Ok(Arc::clone(desktop)),
        _ => Err(INVALID_HANDLE),
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn zmux_desktop_create() -> ZmuxResult {
    guard(|| {
        let mut registry = registry().try_lock().map_err(lock_status)?;
        if registry
            .entries
            .values()
            .filter(|e| matches!(e, Entry::Desktop(_)))
            .count()
            >= 4
        {
            return Err(CAPACITY);
        }
        let runtime = Arc::new(Desktop::new(Shell::default()).map_err(|error| {
            eprintln!("zmux desktop startup failed: {error}");
            INTERNAL
        })?);
        // Native tests create multiple contexts, so local control is explicitly opt-in.
        // The app supplies one absolute, owner-private path before creating its desktop.
        let control = std::env::var_os("ZMUX_SOCKET")
            .map(PathBuf::from)
            .map(|path| {
                let policy = zmux_desktop_runtime::control_credentials::server_policy().map_err(
                    |error| {
                        eprintln!("zmux local control policy configuration failed: {error}");
                        INTERNAL
                    },
                )?;
                ControlServer::start_with_policy(&path, Arc::clone(&runtime), policy).map_err(
                    |error| {
                        eprintln!("zmux local control startup failed: {error}");
                        INTERNAL
                    },
                )
            })
            .transpose()?;
        registry.insert(Entry::Desktop(Arc::new(NativeDesktop { runtime, control })))
    })
}
/// # Safety
/// Request storage must remain readable for length bytes throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_desktop_dispatch(
    id: u64,
    request: *const u8,
    length: usize,
) -> ZmuxResult {
    guard(|| {
        if length > MAX_REQUEST_BYTES || (request.is_null() && length != 0) {
            return Err(INVALID_ARGUMENT);
        }
        let desktop = desktop(id)?;
        let reservation = Reservation::new()?;
        // SAFETY: same checked pointer/length and caller ownership contract as core dispatch.
        let bytes = if length == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(request, length) }
        };
        let reply = match serde_json::from_slice::<DesktopRequest>(bytes) {
            Err(error) => error_reply("invalid_request", error.to_string()),
            Ok(request) if request.version != ABI_VERSION => {
                error_reply("unsupported_version", "expected protocol version 1".into())
            }
            Ok(request) => match desktop.submit(request) {
                Ok(()) => json!({"version": ABI_VERSION, "ok": true}),
                Err(DesktopError::QueueFull) => return Err(BUSY),
                Err(error) => error_reply("command_rejected", error.to_string()),
            },
        };
        reservation.publish(serde_json::to_vec(&reply).map_err(|_| INTERNAL)?)
    })
}

/// Execute one session-bound native file transfer and return its worker acknowledgement.
///
/// # Safety
/// Request storage must remain readable for length bytes throughout the call. This call may
/// block for up to two seconds and must run off the native UI thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_desktop_transfer_files(
    id: u64,
    request: *const u8,
    length: usize,
) -> ZmuxResult {
    guard(|| {
        if length > MAX_REQUEST_BYTES || (request.is_null() && length != 0) {
            return Err(INVALID_ARGUMENT);
        }
        let desktop = desktop(id)?;
        // Reserve the only response resource that can be exhausted before enqueueing work.
        let reservation = Reservation::new()?;
        // SAFETY: same checked pointer/length and caller ownership contract as desktop dispatch.
        let bytes = if length == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(request, length) }
        };
        let reply = match serde_json::from_slice::<DesktopRequest>(bytes) {
            Err(error) => transfer_error_reply(error.to_string(), false),
            Ok(request) if request.version != ABI_VERSION => {
                transfer_error_reply("expected protocol version 1".into(), false)
            }
            Ok(request) => match &request.payload {
                DesktopCommand::Transfer {
                    action:
                        TransferCommand::PasteFiles {
                            expected_session_id: Some(_),
                            ..
                        },
                } => match desktop.execute_transfer_receipt(request) {
                    Ok(execution) => match execution.result {
                        Ok(()) => json!({
                            "ok": true,
                            "revision": execution.revision,
                            "upload_id": execution.upload_id,
                        }),
                        Err(error) => transfer_error_reply(error.to_string(), false),
                    },
                    Err(error) => {
                        transfer_error_reply(error.error.to_string(), error.outcome_unknown)
                    }
                },
                _ => transfer_error_reply(
                    "native transfer receipt requires paste_files with expected_session_id".into(),
                    false,
                ),
            },
        };
        let response = serde_json::to_vec(&reply).unwrap_or_else(|_| {
            br#"{"ok":false,"error":{"message":"response serialization failed"},"outcome_unknown":true}"#.to_vec()
        });
        reservation.publish(response)
    })
}
/// OK/value=0 means no change since revision; otherwise value owns a JSON snapshot buffer.
#[unsafe(no_mangle)]
pub extern "C" fn zmux_desktop_snapshot(id: u64, revision: u64) -> ZmuxResult {
    guard(|| {
        let snapshot = desktop(id)?.snapshot().map_err(|error| match error {
            DesktopError::Busy => BUSY,
            _ => INTERNAL,
        })?;
        if snapshot.revision == revision {
            return Ok(0);
        }
        let reservation = Reservation::new()?;
        reservation.publish(serde_json::to_vec(&*snapshot).map_err(|_| INTERNAL)?)
    })
}
/// Shutdown can wait up to five seconds; native hosts must call it off the AppKit thread.
#[unsafe(no_mangle)]
pub extern "C" fn zmux_desktop_destroy(id: u64) -> i32 {
    guard(|| {
        let desktop = native_desktop(id)?;
        if let Some(control) = &desktop.control {
            control
                .shutdown(std::time::Duration::from_secs(2))
                .map_err(|error| {
                    eprintln!("zmux local control shutdown failed: {error}");
                    INTERNAL
                })?;
        }
        desktop
            .runtime
            .shutdown(std::time::Duration::from_secs(5))
            .map_err(|_| INTERNAL)?;
        let mut registry = registry().try_lock().map_err(lock_status)?;
        if !matches!(registry.entries.get(&id), Some(Entry::Desktop(_))) {
            return Err(INVALID_HANDLE);
        }
        registry.entries.remove(&id);
        Ok(0)
    })
    .status
}
