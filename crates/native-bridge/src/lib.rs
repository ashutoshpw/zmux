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
// A whole16KiB paste may require six JSON bytes per UTF-8 byte. Only the strict
// Paste command may exceed the ordinary envelope cap; parsed text stays bounded.
const MAX_PASTE_REQUEST_BYTES: usize = 6 * 16_384 + 1_024;
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
    control_cleanup_failure: Mutex<Option<String>>,
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
    guard(|| create_desktop(None))
}

/// Create a desktop with durable session state. File I/O and restoration run on workers.
/// # Safety
/// The path must remain readable for length bytes; it is UTF-8, without a trailing NUL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_desktop_create_persistent(
    path: *const u8,
    length: usize,
) -> ZmuxResult {
    guard(|| {
        if path.is_null() || length == 0 || length > 4096 {
            return Err(INVALID_ARGUMENT);
        }
        // SAFETY: checked nonnull bounded storage, readable by the caller contract.
        let bytes = unsafe { std::slice::from_raw_parts(path, length) };
        let text = std::str::from_utf8(bytes).map_err(|_| INVALID_ARGUMENT)?;
        if text.contains('\0') || !std::path::Path::new(text).is_absolute() {
            return Err(INVALID_ARGUMENT);
        }
        create_desktop(Some(PathBuf::from(text)))
    })
}

fn create_desktop(state_directory: Option<PathBuf>) -> Result<u64, i32> {
    create_configured_desktop(zmux_desktop_runtime::DesktopOptions {
        state_directory,
        ..Default::default()
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesktopConfiguration {
    state_directory: Option<PathBuf>,
    #[serde(default)]
    terminal_engine: zmux_desktop_runtime::terminal_engine::TerminalEngineConfig,
}

/// Explicit host configuration; ordinary constructors keep their default engine.
/// # Safety
/// Request storage must remain readable for length bytes throughout this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_desktop_create_configured(
    request: *const u8,
    length: usize,
) -> ZmuxResult {
    guard(|| {
        if request.is_null() || length == 0 || length > 8192 {
            return Err(INVALID_ARGUMENT);
        }
        // SAFETY: bounded nonnull caller-owned storage is readable for this call.
        let bytes = unsafe { std::slice::from_raw_parts(request, length) };
        let configuration: DesktopConfiguration =
            serde_json::from_slice(bytes).map_err(|_| INVALID_ARGUMENT)?;
        if let Some(path) = &configuration.state_directory {
            let text = path.to_str().ok_or(INVALID_ARGUMENT)?;
            if text.is_empty() || text.len() > 4096 || text.contains('\0') || !path.is_absolute() {
                return Err(INVALID_ARGUMENT);
            }
        }
        create_configured_desktop(zmux_desktop_runtime::DesktopOptions {
            state_directory: configuration.state_directory,
            terminal_engine: configuration.terminal_engine,
            ..Default::default()
        })
    })
}

fn create_configured_desktop(options: zmux_desktop_runtime::DesktopOptions) -> Result<u64, i32> {
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
    let runtime = Arc::new(
        Desktop::with_options(Shell::default(), options).map_err(|error| {
            eprintln!("zmux desktop startup failed: {error}");
            INTERNAL
        })?,
    );
    // Native tests create multiple contexts, so local control is explicitly opt-in.
    // The app supplies one absolute, owner-private path before creating its desktop.
    let control = std::env::var_os("ZMUX_SOCKET")
        .map(PathBuf::from)
        .map(|path| {
            let policy =
                zmux_desktop_runtime::control_credentials::server_policy().map_err(|error| {
                    eprintln!("zmux local control policy configuration failed: {error}");
                    INTERNAL
                })?;
            ControlServer::start_with_policy(&path, Arc::clone(&runtime), policy).map_err(|error| {
                eprintln!("zmux local control startup failed: {error}");
                INTERNAL
            })
        })
        .transpose()?;
    registry.insert(Entry::Desktop(Arc::new(NativeDesktop {
        runtime,
        control,
        control_cleanup_failure: Mutex::new(None),
    })))
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
        if length > MAX_PASTE_REQUEST_BYTES || (request.is_null() && length != 0) {
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
            Ok(request)
                if length > MAX_REQUEST_BYTES
                    && !matches!(request.payload, DesktopCommand::Paste { .. }) =>
            {
                return Err(INVALID_ARGUMENT);
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
/// Admit exactly one profile Add using the shared catalog owner, not a native store.
///
/// # Safety
/// Name storage must remain readable for length bytes throughout the call. This
/// call waits for executor acknowledgement and must run off the native UI thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_desktop_profile_add(
    id: u64,
    name: *const u8,
    length: usize,
) -> ZmuxResult {
    guard(|| {
        if name.is_null() || length == 0 || length > 128 {
            return Err(INVALID_ARGUMENT);
        }
        // SAFETY: caller guarantees the checked, bounded storage remains readable.
        let bytes = unsafe { std::slice::from_raw_parts(name, length) };
        let name = std::str::from_utf8(bytes).map_err(|_| INVALID_ARGUMENT)?;
        let desktop = desktop(id)?;
        let reservation = Reservation::new()?;
        let reply = match desktop.native_profile_add(name.into()) {
            Ok(receipt) => json!({"ok": true, "receipt": receipt}),
            Err(error) => json!({
                "ok": false,
                "error": {"message": error.error.to_string()},
                "outcome_unknown": error.outcome_unknown,
            }),
        };
        reservation.publish(serde_json::to_vec(&reply).map_err(|_| INTERNAL)?)
    })
}

/// Trusted native window completion/key observations, excluded from generic dispatch.
///
/// # Safety
/// Request storage remains readable for length bytes; call off the UI thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_desktop_window_event(
    id: u64,
    request: *const u8,
    length: usize,
) -> ZmuxResult {
    guard(|| {
        if length > MAX_REQUEST_BYTES || request.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        // SAFETY: the caller retains this checked bounded readable storage.
        let bytes = unsafe { std::slice::from_raw_parts(request, length) };
        let event =
            serde_json::from_slice::<zmux_desktop_runtime::windows::NativeWindowEvent>(bytes)
                .map_err(|_| INVALID_ARGUMENT)?;
        let desktop = desktop(id)?;
        let reservation = Reservation::new()?;
        let reply = match desktop.native_window_event(event) {
            Ok(result) => json!({"ok":true,"result":result}),
            Err(error) => {
                json!({"ok":false,"error":{"message":error.to_string()},"outcome_unknown":matches!(error,DesktopError::Timeout|DesktopError::Closed)})
            }
        };
        reservation.publish(serde_json::to_vec(&reply).map_err(|_| INTERNAL)?)
    })
}

/// Trusted native download channel, excluded from socket and generic JSON dispatch.
///
/// # Safety
/// Storage must remain readable for length bytes. Call off the UI thread; a lost
/// acknowledgement must be correlated through its token, never replayed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_desktop_download(
    id: u64,
    request: *const u8,
    length: usize,
) -> ZmuxResult {
    guard(|| {
        if length > MAX_REQUEST_BYTES || request.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        // SAFETY: caller guarantees checked bounded storage remains readable.
        let bytes = unsafe { std::slice::from_raw_parts(request, length) };
        let request =
            serde_json::from_slice::<zmux_desktop_runtime::downloads::NativeDownload>(bytes)
                .map_err(|_| INVALID_ARGUMENT)?;
        let desktop = desktop(id)?;
        let reservation = Reservation::new()?;
        let reply = match desktop.native_download(request) {
            Ok(result) => json!({"ok": true, "result": result}),
            Err(error) => {
                json!({"ok": false, "error": {"message": error.to_string()}, "outcome_unknown": matches!(error, DesktopError::Timeout | DesktopError::Closed)})
            }
        };
        reservation.publish(serde_json::to_vec(&reply).map_err(|_| INTERNAL)?)
    })
}

/// Drain trusted native browser work. May wait for executor acknowledgement; call off UI.
/// A failed call must not be replayed automatically: delivery may have occurred.
#[unsafe(no_mangle)]
pub extern "C" fn zmux_desktop_browser_drain(id: u64) -> ZmuxResult {
    guard(|| {
        let desktop = desktop(id)?;
        // Claim output capacity before dequeuing any at-most-once browser operation.
        let reservation = Reservation::new()?;
        let batch = desktop
            .browser_drain_requests(std::time::Duration::from_secs(1))
            .map_err(|_| INTERNAL)?;
        reservation.publish(serde_json::to_vec(&batch).map_err(|_| INTERNAL)?)
    })
}

/// Trusted native host lifecycle/completion channel; never exposed to socket clients.
/// # Safety
/// Request storage must remain readable for length bytes throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_desktop_browser_event(
    id: u64,
    request: *const u8,
    length: usize,
) -> i32 {
    guard(|| {
        if request.is_null() || length == 0 || length > 300_000 {
            return Err(INVALID_ARGUMENT);
        }
        // SAFETY: bounded nonnull storage, readable by the caller contract.
        let bytes = unsafe { std::slice::from_raw_parts(request, length) };
        let event: zmux_desktop_runtime::browser::BrowserHostEvent =
            serde_json::from_slice(bytes).map_err(|_| INVALID_ARGUMENT)?;
        desktop(id)?
            .browser_host_event(event)
            .map_err(|error| match error {
                DesktopError::QueueFull | DesktopError::Busy => BUSY,
                _ => INTERNAL,
            })?;
        Ok(0)
    })
    .status
}

/// Trusted native notification lifecycle channel. Waits off the UI thread.
/// # Safety
/// Request storage must remain readable for length bytes throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_desktop_notification_event(
    id: u64,
    request: *const u8,
    length: usize,
) -> i32 {
    guard(|| {
        if request.is_null() || length == 0 || length > 16_384 {
            return Err(INVALID_ARGUMENT);
        }
        // SAFETY: caller provides bounded readable storage for the duration of this call.
        let bytes = unsafe { std::slice::from_raw_parts(request, length) };
        let event: zmux_desktop_runtime::attention::NativeNotificationEvent =
            serde_json::from_slice(bytes).map_err(|_| INVALID_ARGUMENT)?;
        desktop(id)?
            .native_notification_event(event)
            .map_err(|error| match error {
                DesktopError::QueueFull | DesktopError::Busy => BUSY,
                _ => INTERNAL,
            })?;
        Ok(0)
    })
    .status
}

/// Confirm a generation-bound notification action before native focus changes.
/// # Safety
/// Request storage must remain readable for length bytes throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_desktop_notification_action(
    id: u64,
    request: *const u8,
    length: usize,
) -> ZmuxResult {
    guard(|| {
        // A 4095-byte reply can expand sixfold when JSON escapes control bytes.
        if request.is_null() || length == 0 || length > 32_768 {
            return Err(INVALID_ARGUMENT);
        }
        // SAFETY: caller provides bounded readable storage for the duration of this call.
        let bytes = unsafe { std::slice::from_raw_parts(request, length) };
        let action: zmux_desktop_runtime::attention::NativeAttentionAction =
            serde_json::from_slice(bytes).map_err(|_| INVALID_ARGUMENT)?;
        let desktop = desktop(id)?;
        let reservation = Reservation::new()?;
        let result = desktop
            .native_notification_action(action)
            .map_err(|_| INTERNAL)?;
        reservation.publish(serde_json::to_vec(&result).map_err(|_| INTERNAL)?)
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
fn shutdown_report(
    desktop: &NativeDesktop,
    ticket: u64,
) -> Result<zmux_desktop_runtime::shutdown::Report, i32> {
    let mut report = desktop
        .runtime
        .shutdown_status(ticket)
        .map_err(|_| INVALID_ARGUMENT)?;
    let mut failure = desktop
        .control_cleanup_failure
        .lock()
        .map_err(|_| INTERNAL)?;
    let control_pending = if failure.is_some() {
        true
    } else if let Some(control) = &desktop.control {
        match control.shutdown(std::time::Duration::ZERO) {
            Ok(()) => false,
            Err(zmux_desktop_runtime::control::ControlServerError::ShutdownTimeout) => true,
            Err(error) => {
                *failure = Some(error.to_string());
                true
            }
        }
    } else {
        false
    };
    if control_pending {
        report.pending.control = 1;
        report.runtime_cleanup_complete = false;
        report.physical_cleanup_complete = false;
        if report.phase == zmux_desktop_runtime::shutdown::Phase::Complete {
            report.phase = zmux_desktop_runtime::shutdown::Phase::Draining;
        }
    }
    if let Some(message) = failure.as_ref() {
        if report.issues.len() < 16 {
            report.issues.push(message.chars().take(512).collect());
        } else {
            report.issues_dropped = report.issues_dropped.saturating_add(1);
        }
    }
    Ok(report)
}
fn shutdown_buffer(
    report: &zmux_desktop_runtime::shutdown::Report,
    reservation: Reservation,
) -> Result<u64, i32> {
    reservation.publish(serde_json::to_vec(report).map_err(|_| INTERNAL)?)
}
/// Begin exactly once. Only the first call may register a real native cleanup barrier.
#[unsafe(no_mangle)]
pub extern "C" fn zmux_desktop_shutdown_begin(
    id: u64,
    budget_ms: u64,
    native_cleanup_required: u8,
) -> ZmuxResult {
    guard(|| {
        if budget_ms > 30_000 || native_cleanup_required > 1 {
            return Err(INVALID_ARGUMENT);
        }
        let desktop = native_desktop(id)?;
        let reservation = Reservation::new()?;
        let report = desktop
            .runtime
            .begin_shutdown(
                std::time::Duration::from_millis(budget_ms),
                native_cleanup_required == 1,
            )
            .map_err(|_| INTERNAL)?;
        if let Some(control) = &desktop.control {
            control.request_shutdown();
        }
        shutdown_buffer(&shutdown_report(&desktop, report.ticket)?, reservation)
    })
}
/// Polling Pending does not renew the one root deadline or release ownership.
#[unsafe(no_mangle)]
pub extern "C" fn zmux_desktop_shutdown_status(id: u64, ticket: u64) -> ZmuxResult {
    guard(|| {
        let desktop = native_desktop(id)?;
        let reservation = Reservation::new()?;
        shutdown_buffer(&shutdown_report(&desktop, ticket)?, reservation)
    })
}
/// Off-UI bounded wait, clamped to the root budget. Pending is a JSON report.
#[unsafe(no_mangle)]
pub extern "C" fn zmux_desktop_shutdown_wait(id: u64, ticket: u64, wait_ms: u64) -> ZmuxResult {
    guard(|| {
        if wait_ms > 30_000 {
            return Err(INVALID_ARGUMENT);
        }
        let desktop = native_desktop(id)?;
        let reservation = Reservation::new()?;
        desktop
            .runtime
            .wait_shutdown(ticket, std::time::Duration::from_millis(wait_ms))
            .map_err(|_| INVALID_ARGUMENT)?;
        shutdown_buffer(&shutdown_report(&desktop, ticket)?, reservation)
    })
}
/// Trusted native owner acknowledgment, after physical cleanup. Idempotent.
#[unsafe(no_mangle)]
pub extern "C" fn zmux_desktop_shutdown_native_complete(id: u64, ticket: u64) -> i32 {
    guard(|| {
        native_desktop(id)?
            .runtime
            .shutdown_native_complete(ticket)
            .map_err(|_| INVALID_ARGUMENT)?;
        Ok(0)
    })
    .status
}
/// Release only after all physical owners and the coordinator have joined.
#[unsafe(no_mangle)]
pub extern "C" fn zmux_desktop_shutdown_release(id: u64, ticket: u64) -> i32 {
    guard(|| {
        let desktop = native_desktop(id)?;
        if !shutdown_report(&desktop, ticket)?.physical_cleanup_complete {
            return Err(BUSY);
        }
        let mut registry = registry().try_lock().map_err(lock_status)?;
        if !matches!(registry.entries.get(&id), Some(Entry::Desktop(_))) {
            return Err(INVALID_HANDLE);
        }
        registry.entries.remove(&id);
        Ok(0)
    })
    .status
}
/// Compatibility bounded wait. Never release a desktop with pending physical owners.
#[unsafe(no_mangle)]
pub extern "C" fn zmux_desktop_destroy(id: u64) -> i32 {
    guard(|| {
        let desktop = native_desktop(id)?;
        let report = desktop
            .runtime
            .begin_shutdown(std::time::Duration::from_secs(5), false)
            .map_err(|_| INTERNAL)?;
        if let Some(control) = &desktop.control {
            control.request_shutdown();
        }
        desktop
            .runtime
            .wait_shutdown(report.ticket, std::time::Duration::from_secs(5))
            .map_err(|_| INTERNAL)?;
        if !shutdown_report(&desktop, report.ticket)?.physical_cleanup_complete {
            return Err(BUSY);
        }
        if !shutdown_report(&desktop, report.ticket)?.issues.is_empty() {
            return Err(INTERNAL);
        }
        let status = zmux_desktop_shutdown_release(id, report.ticket);
        if status != OK {
            return Err(status);
        }
        Ok(0)
    })
    .status
}
