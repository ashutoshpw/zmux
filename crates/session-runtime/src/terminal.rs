use crate::{EngineKind, EngineOptions, SessionError};
#[cfg(feature = "ghostty")]
use zmux_terminal_adapter::Engine;
use zmux_terminal_adapter::{EngineBatch, Size, Terminal, Viewport, Wheel};

// Default sessions retain their existing immediate vt100 path and frame cadence.
// The larger worker-bound owner stays inline to avoid an extra infallible Box.
#[allow(clippy::large_enum_variant)]
pub(crate) enum TerminalOwner {
    Vt100(Terminal),
    #[cfg(feature = "ghostty")]
    Ghostty(Engine),
}

impl TerminalOwner {
    pub(crate) fn new(size: Size, options: Option<EngineOptions>) -> Result<Self, SessionError> {
        let kind = options.map_or(EngineKind::Vt100, |options| options.kind);
        match kind {
            EngineKind::Vt100 => Terminal::new(size).map(Self::Vt100).map_err(failure),
            #[cfg(feature = "ghostty")]
            EngineKind::Ghostty(limits) => {
                Engine::new(size, zmux_terminal_adapter::EngineKind::Ghostty(limits))
                    .map(Self::Ghostty)
                    .map_err(failure)
            }
        }
    }
    pub(crate) fn stages_mutations(&self) -> bool {
        match self {
            Self::Vt100(_) => false,
            #[cfg(feature = "ghostty")]
            Self::Ghostty(_) => true,
        }
    }
    pub(crate) fn viewport(&self) -> Viewport {
        match self {
            Self::Vt100(terminal) => terminal.viewport(),
            #[cfg(feature = "ghostty")]
            Self::Ghostty(terminal) => terminal.viewport().clone(),
        }
    }
    pub(crate) fn process(&mut self, bytes: &[u8]) -> Result<Option<EngineBatch>, SessionError> {
        match self {
            Self::Vt100(terminal) => terminal.process(bytes).map(|()| None).map_err(failure),
            #[cfg(feature = "ghostty")]
            Self::Ghostty(terminal) => terminal.stage_process(bytes).map(Some).map_err(failure),
        }
    }
    pub(crate) fn resize(&mut self, size: Size) -> Result<Option<EngineBatch>, SessionError> {
        match self {
            Self::Vt100(terminal) => terminal.resize(size).map(|()| None).map_err(failure),
            #[cfg(feature = "ghostty")]
            Self::Ghostty(terminal) => terminal.stage_resize(size).map(Some).map_err(failure),
        }
    }
    pub(crate) fn scroll(&mut self, lines: i32) -> Result<Option<EngineBatch>, SessionError> {
        match self {
            Self::Vt100(terminal) => {
                terminal.scroll(lines);
                Ok(None)
            }
            #[cfg(feature = "ghostty")]
            Self::Ghostty(terminal) => terminal.stage_scroll(lines).map(Some).map_err(failure),
        }
    }
    pub(crate) fn wheel(
        &mut self,
        event: Wheel,
    ) -> Result<(Vec<u8>, Option<EngineBatch>), SessionError> {
        match self {
            Self::Vt100(terminal) => Ok((terminal.wheel(event), None)),
            #[cfg(feature = "ghostty")]
            Self::Ghostty(terminal) if event.history => terminal.stage_scroll(event.lines.clamp(-1000,1000))
                .map(|batch| (Vec::new(), Some(batch))).map_err(failure),
            #[cfg(feature = "ghostty")]
            Self::Ghostty(_) => Err(SessionError::Failure("Ghostty application wheel encoding is not yet supported; explicit history navigation is available".into())),
        }
    }
    pub(crate) fn commit(&mut self, batch: &EngineBatch) -> Result<(), SessionError> {
        match self {
            #[cfg(feature = "ghostty")]
            Self::Ghostty(terminal) => terminal.commit(batch.generation()).map_err(failure),
            Self::Vt100(_) => {
                let _ = batch;
                Err(SessionError::Failure(
                    "unexpected staged vt100 batch".into(),
                ))
            }
        }
    }
    pub(crate) fn abort(&mut self) {
        #[cfg(feature = "ghostty")]
        if let Self::Ghostty(terminal) = self {
            terminal.abort();
        }
    }
}
fn failure(error: impl std::fmt::Display) -> SessionError {
    SessionError::Failure(error.to_string())
}
