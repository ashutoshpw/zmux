//! Native-only store ownership. The host must use the fixed OS Library root,
//! acquire before construction, and never release a constructed store's token.
use super::{INTERNAL, INVALID_ARGUMENT, Reservation, ZmuxResult, guard, native_desktop};
use serde::Deserialize;
use serde_json::json;
use std::{path::PathBuf, sync::OnceLock};
use uuid::Uuid;
use zmux_desktop_runtime::profile_leases::{
    LeaseError, LeaseMode, LeaseOwner, LeaseService, LeaseToken,
};

struct ProcessLeases {
    root: PathBuf,
    service: LeaseService,
}
static PROCESS_LEASES: OnceLock<Result<ProcessLeases, String>> = OnceLock::new();

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Acquire { owner: LeaseOwner, profile: Uuid },
    Status { owner: LeaseOwner },
    ReleaseUnconstructed { token: LeaseToken },
}

/// Acquire/status exact native ownership, separate from generic control commands.
/// Status/release use id=0 and survive logical Desktop release. Release is valid
/// only when the trusted native owner has not constructed any WebKit store.
///
/// # Safety
/// Request storage is readable for length bytes. Call only off UI/core executors.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_desktop_profile_lease(
    id: u64,
    request: *const u8,
    length: usize,
) -> ZmuxResult {
    guard(|| {
        if request.is_null() || length == 0 || length > 2048 {
            return Err(INVALID_ARGUMENT);
        }
        // SAFETY: caller retains this checked bounded readable request storage.
        let bytes = unsafe { std::slice::from_raw_parts(request, length) };
        let command: Command = serde_json::from_slice(bytes).map_err(|_| INVALID_ARGUMENT)?;
        let reservation = Reservation::new()?;
        let result = match command {
            Command::Acquire { owner, profile } => {
                let desktop = native_desktop(id)?;
                match (
                    &desktop.profile_catalog_directory,
                    &desktop.profile_lease_directory,
                ) {
                    (Some(catalog), Some(root)) => match PROCESS_LEASES.get_or_init(|| {
                        LeaseService::start(root.clone())
                            .map(|service| ProcessLeases {
                                root: root.clone(),
                                service,
                            })
                            .map_err(|error| error.to_string())
                    }) {
                        Ok(leases) if leases.root == *root => leases
                            .service
                            .acquire(catalog.clone(), owner, profile, LeaseMode::Shared)
                            .map(Some),
                        Ok(_) => Err(LeaseError::Io(
                            "native lease namespace differs from this process's fixed root".into(),
                        )),
                        Err(error) => Err(LeaseError::Io(error.clone())),
                    },
                    _ => Err(LeaseError::Io(
                        "native persistent profile lease context is unavailable".into(),
                    )),
                }
            }
            Command::Status { owner } if id == 0 => match PROCESS_LEASES.get() {
                Some(Ok(leases)) => leases.service.status(owner),
                Some(Err(error)) => Err(LeaseError::Io(error.clone())),
                None => Ok(None),
            },
            Command::ReleaseUnconstructed { token } if id == 0 => match PROCESS_LEASES.get() {
                Some(Ok(leases)) => leases.service.release_unconstructed(token).map(|()| None),
                Some(Err(error)) => Err(LeaseError::Io(error.clone())),
                None => Err(LeaseError::StaleToken),
            },
            _ => return Err(INVALID_ARGUMENT),
        };
        let reply = match result {
            Ok(token) => json!({"ok": true, "token": token}),
            Err(error) => json!({
                "ok": false,
                "error": {"message": error.to_string()},
                "outcome_unknown": matches!(error, LeaseError::OutcomeUnknown),
            }),
        };
        reservation.publish(serde_json::to_vec(&reply).map_err(|_| INTERNAL)?)
    })
}
