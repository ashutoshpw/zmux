use crate::{EngineKind, EngineOptions, SessionError};
use zmux_program_status::SessionEpoch;
use zmux_terminal_adapter::{
    Engine, EngineBatch, EngineKind as AdapterEngineKind, Pointer, ProgramOpenRequest,
    ProgramOpenRequestError, Size, Viewport, Wheel,
};

/// Every session uses the staged owner so frame/status publication and causal
/// replies share one commit boundary on the sole PTY worker.
pub(crate) struct TerminalOwner(Engine);

impl TerminalOwner {
    pub(crate) fn encode_key(
        &mut self,
        event: zmux_terminal_adapter::KeyEvent,
    ) -> Result<Vec<u8>, SessionError> {
        self.0.encode_key(event).map_err(failure)
    }

    pub(crate) fn encode_paste(&mut self, text: &str) -> Result<Vec<u8>, SessionError> {
        self.0.encode_paste(text).map_err(failure)
    }

    pub(crate) fn new(
        size: Size,
        options: Option<EngineOptions>,
        expected_open_nonce: Option<[u8; 16]>,
        session_epoch: SessionEpoch,
    ) -> Result<Self, SessionError> {
        let kind = options.map_or(EngineKind::Vt100, |options| options.kind);
        let adapter_kind = match kind {
            EngineKind::Vt100 => AdapterEngineKind::Vt100,
            #[cfg(feature = "ghostty")]
            EngineKind::Ghostty(limits) => AdapterEngineKind::Ghostty(limits),
        };
        Engine::new_with_session_epoch(size, adapter_kind, expected_open_nonce, session_epoch)
            .map(Self)
            .map_err(failure)
    }

    pub(crate) fn stages_mutations(&self) -> bool {
        true
    }

    pub(crate) fn viewport(&self) -> Viewport {
        self.0.viewport().clone()
    }

    pub(crate) fn process(&mut self, bytes: &[u8]) -> Result<Option<EngineBatch>, SessionError> {
        self.0
            .stage_process_deferred(bytes)
            .map(Some)
            .map_err(failure)
    }

    pub(crate) fn take_open_requests(
        &mut self,
    ) -> Result<Vec<ProgramOpenRequest>, ProgramOpenRequestError> {
        self.0.take_open_requests()
    }

    pub(crate) fn resize(&mut self, size: Size) -> Result<Option<EngineBatch>, SessionError> {
        self.0.stage_resize(size).map(Some).map_err(failure)
    }

    pub(crate) fn scroll(&mut self, lines: i32) -> Result<Option<EngineBatch>, SessionError> {
        self.0.stage_scroll(lines).map(Some).map_err(failure)
    }

    pub(crate) fn pointer(
        &mut self,
        event: Pointer,
    ) -> Result<(Vec<u8>, EngineBatch), SessionError> {
        self.0.pointer(event).map_err(failure)
    }

    pub(crate) fn wheel(&mut self, event: Wheel) -> Result<(Vec<u8>, EngineBatch), SessionError> {
        self.0.wheel(event).map_err(failure)
    }

    pub(crate) fn attached_child_exited(&mut self) -> Result<EngineBatch, SessionError> {
        self.0.stage_attached_child_exited().map_err(failure)
    }

    pub(crate) fn terminal_closed(&mut self) -> Result<Viewport, SessionError> {
        self.0.terminal_closed_viewport().map_err(failure)
    }

    pub(crate) fn commit(&mut self, batch: &EngineBatch) -> Result<(), SessionError> {
        self.0.commit(batch.generation()).map_err(failure)
    }

    pub(crate) fn abort(&mut self) {
        self.0.abort();
    }

    pub(crate) fn refresh_viewport(&mut self) -> Result<(), SessionError> {
        self.0.refresh_viewport().map_err(failure)
    }
}

fn failure(error: impl std::fmt::Display) -> SessionError {
    SessionError::Failure(error.to_string())
}
