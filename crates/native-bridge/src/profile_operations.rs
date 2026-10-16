//! Trusted-only native destructive owner. Generic socket clients cannot report
//! WebKit success or obtain this controller's physical dispatch authority.
use super::{CAPACITY, INTERNAL, INVALID_ARGUMENT, Reservation, ZmuxResult, guard};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};
use uuid::Uuid;
use zmux_desktop_runtime::{
    profile_leases::{
        LeaseError, LeaseToken, MutationRequest, MutationToken, NativeMutationOutcome,
        journal::MutationPhase,
    },
    profile_native::NativeProfileOperations,
};
static CONTROLLERS: OnceLock<Mutex<HashMap<Uuid, Arc<NativeProfileOperations>>>> = OnceLock::new();
fn controllers() -> &'static Mutex<HashMap<Uuid, Arc<NativeProfileOperations>>> {
    CONTROLLERS.get_or_init(Default::default)
}
pub(super) fn register(
    controller: Uuid,
    catalog: &Path,
    root: &Path,
) -> Result<Arc<NativeProfileOperations>, i32> {
    if controller.is_nil() {
        return Err(INVALID_ARGUMENT);
    }
    let mut contexts = controllers().try_lock().map_err(|_| super::BUSY)?;
    if contexts.contains_key(&controller) {
        return Err(INVALID_ARGUMENT);
    }
    // Retain unresolved callback authority after Desktop release, never evict it.
    if contexts.len() >= 4 {
        return Err(CAPACITY);
    }
    let service = Arc::clone(
        &super::profile_leases::process(root)
            .map_err(|_| INTERNAL)?
            .service,
    );
    let owner = Arc::new(
        NativeProfileOperations::new(controller, catalog.to_path_buf(), service)
            .map_err(|_| INVALID_ARGUMENT)?,
    );
    contexts.insert(controller, Arc::clone(&owner));
    Ok(owner)
}
fn context(controller: Uuid) -> Result<Arc<NativeProfileOperations>, LeaseError> {
    controllers()
        .lock()
        .map_err(|_| LeaseError::Closed)?
        .get(&controller)
        .cloned()
        .ok_or(LeaseError::StaleToken)
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Drain {
        controller: Uuid,
    },
    Begin {
        controller: Uuid,
        intent: Uuid,
        stores: Vec<LeaseToken>,
    },
    Advance {
        token: MutationToken,
        phase: MutationPhase,
    },
    Finish {
        token: MutationToken,
        result: NativeMutationOutcome,
    },
    Reject {
        controller: Uuid,
        intent: Uuid,
        message: String,
    },
    Status {
        controller: Uuid,
        profile: Uuid,
        intent: Uuid,
    },
}
fn execute(command: Command) -> Result<Value, LeaseError> {
    match command {
        Command::Drain { controller } => {
            context(controller)?.drain().map(|v| json!({"requests": v}))
        }
        Command::Begin {
            controller,
            intent,
            stores,
        } => {
            if stores.len() > zmux_desktop_runtime::profile_leases::MAX_LEASES {
                return Err(LeaseError::Capacity);
            }
            let owner = context(controller)?;
            let request = owner.request(intent)?;
            owner
                .service
                .mutation_start(MutationRequest {
                    catalog: owner.catalog.clone(),
                    controller,
                    intent,
                    profile: request.profile,
                    kind: request.kind,
                    local_stores: stores,
                })
                .and_then(|token| {
                    let fallback =
                        owner
                            .service
                            .status(zmux_desktop_runtime::profile_leases::LeaseOwner {
                                epoch: controller,
                                store: intent,
                            })?;
                    Ok(json!({"token": token, "fallback": fallback}))
                })
        }
        Command::Advance { token, phase } => {
            let owner = context(token.controller)?;
            if owner.request(token.intent)?.profile != token.profile {
                return Err(LeaseError::StaleToken);
            }
            owner
                .service
                .mutation_advance(token, phase)
                .map(|()| json!({}))
        }
        Command::Finish { token, result } => {
            let owner = context(token.controller)?;
            if owner.request(token.intent)?.profile != token.profile {
                return Err(LeaseError::StaleToken);
            }
            let completion = match &result {
                NativeMutationOutcome::Completed => Ok(()),
                NativeMutationOutcome::AbortedBeforeClear { message }
                | NativeMutationOutcome::RemovalFailed { message } => Err(message.clone()),
            };
            owner.service.mutation_finish(token, result)?;
            owner.complete(token.intent, completion)?;
            Ok(json!({}))
        }
        Command::Reject {
            controller,
            intent,
            message,
        } => {
            if message.len() > 1024 {
                return Err(LeaseError::Capacity);
            }
            context(controller)?.complete(intent, Err(message))?;
            Ok(json!({}))
        }
        Command::Status {
            controller,
            profile,
            intent,
        } => context(controller)?.lookup(profile, intent),
    }
}
/// # Safety
/// Caller retains readable request bytes for the bounded length. Call off UI/core.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zmux_desktop_profile_native(
    request: *const u8,
    length: usize,
) -> ZmuxResult {
    guard(|| {
        if request.is_null() || length == 0 || length > 16384 {
            return Err(INVALID_ARGUMENT);
        }
        // SAFETY: bounded nonnull caller-owned bytes live throughout this call.
        let bytes = unsafe { std::slice::from_raw_parts(request, length) };
        let command = serde_json::from_slice(bytes).map_err(|_| INVALID_ARGUMENT)?;
        let reservation = Reservation::new()?;
        let reply = match execute(command) {
            Ok(value) => json!({"ok": true, "value": value}),
            Err(error) => {
                json!({"ok": false, "error": {"message": error.to_string()}, "outcome_unknown": matches!(error, LeaseError::OutcomeUnknown | LeaseError::Io(_) | LeaseError::UnsafeIntent | LeaseError::UnsafeLock)})
            }
        };
        reservation.publish(serde_json::to_vec(&reply).map_err(|_| INTERNAL)?)
    })
}
