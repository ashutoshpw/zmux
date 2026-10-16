//! Native-only store ownership. The host must use the fixed OS Library root,
//! acquire before construction, and never release a constructed store's token.
use super::{INTERNAL, INVALID_ARGUMENT, Reservation, ZmuxResult, guard, native_desktop};
use serde::Deserialize;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use uuid::Uuid;
use zmux_desktop_runtime::profile_leases::{
    ConstructionPermit, LeaseError, LeaseMode, LeaseOwner, LeaseService, LeaseToken,
};

pub(super) struct ProcessLeases {
    root: PathBuf,
    pub(super) service: Arc<LeaseService>,
}
static PROCESS_LEASES: OnceLock<Result<ProcessLeases, String>> = OnceLock::new();

pub(super) fn process(root: &std::path::Path) -> Result<&'static ProcessLeases, LeaseError> {
    match PROCESS_LEASES.get_or_init(|| {
        LeaseService::start(root.to_path_buf())
            .map(|service| ProcessLeases {
                root: root.to_path_buf(),
                service: Arc::new(service),
            })
            .map_err(|e| e.to_string())
    }) {
        Ok(leases) if leases.root == root => Ok(leases),
        Ok(_) => Err(LeaseError::Io(
            "native lease namespace differs from this process's fixed root".into(),
        )),
        Err(error) => Err(LeaseError::Io(error.clone())),
    }
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Constructed {
        token: LeaseToken,
    },
    Reuse {
        token: LeaseToken,
        construction: Uuid,
    },
    ConstructionStatus {
        construction: Uuid,
    },
    ConstructionDone {
        permit: ConstructionPermit,
    },
    Acquire {
        owner: LeaseOwner,
        profile: Uuid,
    },
    Status {
        owner: LeaseOwner,
    },
    ReleaseUnconstructed {
        token: LeaseToken,
    },
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
                    (Some(catalog), Some(root)) => process(root)
                        .and_then(|leases| {
                            leases.service.acquire(
                                catalog.clone(),
                                owner,
                                profile,
                                LeaseMode::Shared,
                            )
                        })
                        .map(|token| json!({"token": token})),
                    _ => Err(LeaseError::Io(
                        "native persistent profile lease context is unavailable".into(),
                    )),
                }
            }
            Command::Reuse {
                token,
                construction,
            } => {
                let desktop = native_desktop(id)?;
                match (
                    &desktop.profile_catalog_directory,
                    &desktop.profile_lease_directory,
                ) {
                    (Some(catalog), Some(root)) => process(root)
                        .and_then(|leases| {
                            leases.service.reuse(catalog.clone(), token, construction)
                        })
                        .map(|permit| json!({"permit": permit})),
                    _ => Err(LeaseError::Closed),
                }
            }
            Command::Constructed { token } if id == 0 => match PROCESS_LEASES.get() {
                Some(Ok(leases)) => leases.service.constructed(token).map(|()| json!({})),
                _ => Err(LeaseError::StaleToken),
            },
            Command::ConstructionStatus { construction } if id == 0 => match PROCESS_LEASES.get() {
                Some(Ok(leases)) => leases
                    .service
                    .construction_status(construction)
                    .map(|permit| json!({"permit": permit})),
                _ => Err(LeaseError::StaleToken),
            },
            Command::ConstructionDone { permit } if id == 0 => match PROCESS_LEASES.get() {
                Some(Ok(leases)) => leases.service.construction_done(permit).map(|()| json!({})),
                _ => Err(LeaseError::StaleToken),
            },
            Command::Status { owner } if id == 0 => match PROCESS_LEASES.get() {
                Some(Ok(leases)) => leases
                    .service
                    .status(owner)
                    .map(|token| json!({"token": token})),
                Some(Err(error)) => Err(LeaseError::Io(error.clone())),
                None => Ok(json!({"token": null})),
            },
            Command::ReleaseUnconstructed { token } if id == 0 => match PROCESS_LEASES.get() {
                Some(Ok(leases)) => leases
                    .service
                    .release_unconstructed(token)
                    .map(|()| json!({"token": null})),
                Some(Err(error)) => Err(LeaseError::Io(error.clone())),
                None => Err(LeaseError::StaleToken),
            },
            _ => return Err(INVALID_ARGUMENT),
        };
        let reply = match result {
            Ok(mut value) => {
                value["ok"] = json!(true);
                value
            }
            Err(error) => json!({
                "ok": false,
                "error": {"message": error.to_string()},
                "outcome_unknown": matches!(error, LeaseError::OutcomeUnknown),
            }),
        };
        reservation.publish(serde_json::to_vec(&reply).map_err(|_| INTERNAL)?)
    })
}
