//! Explicit host selection; dependency feature unification never selects a backend.
use crate::DesktopError;
use serde::Deserialize;
use zmux_session_runtime::EngineOptions;

/// Opt-in host policy. Candidate limits are supplied by the caller and are not
/// default application budgets. Ordinary hosts continue using vt100.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(tag = "engine", rename_all = "snake_case", deny_unknown_fields)]
pub enum TerminalEngineConfig {
    Vt100 {},
    Ghostty {
        engine_bytes: usize,
        general_bytes: usize,
        page_bytes: usize,
        reply_timeout_ms: u64,
    },
}

impl Default for TerminalEngineConfig {
    fn default() -> Self {
        Self::Vt100 {}
    }
}

impl TerminalEngineConfig {
    pub(crate) fn session_options(self) -> Result<Option<EngineOptions>, DesktopError> {
        match self {
            Self::Vt100 {} => Ok(None),
            Self::Ghostty {
                engine_bytes,
                general_bytes,
                page_bytes,
                reply_timeout_ms,
            } => {
                if engine_bytes == 0
                    || engine_bytes > 64 * 1024 * 1024
                    || general_bytes > engine_bytes
                    || page_bytes > engine_bytes
                    || !(1..=30_000).contains(&reply_timeout_ms)
                {
                    return Err(DesktopError::Failure(
                        "invalid explicit Ghostty mapping caps or reply lifetime".into(),
                    ));
                }
                #[cfg(feature = "ghostty")]
                {
                    Ok(Some(EngineOptions {
                        kind: zmux_session_runtime::EngineKind::Ghostty(
                            zmux_session_runtime::GhosttyLimits {
                                engine_bytes,
                                general_bytes,
                                page_bytes,
                            },
                        ),
                        reply_timeout: std::time::Duration::from_millis(reply_timeout_ms),
                    }))
                }
                #[cfg(not(feature = "ghostty"))]
                Err(DesktopError::Failure(
                    "explicit Ghostty engine requested but this desktop was built without its feature"
                        .into(),
                ))
            }
        }
    }
}

/// Ordinary, SSH and local-multiplexer launches share explicit engine selection.
/// The worker still constructs the actual terminal before spawning its child.
pub(crate) fn spawn(
    launch: zmux_session_runtime::Launch,
    environment: &[(String, String)],
    removals: &[std::ffi::OsString],
    engine: Option<EngineOptions>,
    resource: impl Send + 'static,
) -> Result<zmux_session_runtime::Session, zmux_session_runtime::SessionError> {
    match engine {
        Some(engine) => zmux_session_runtime::Session::spawn_with_engine(
            launch,
            environment,
            removals,
            zmux_session_runtime::ShutdownPolicy::Kill,
            engine,
            resource,
        ),
        None => zmux_session_runtime::Session::spawn_with_resource_and_removals(
            launch,
            environment,
            removals,
            resource,
        ),
    }
}
