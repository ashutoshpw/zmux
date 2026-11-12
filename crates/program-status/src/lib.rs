//! Bounded, engine-independent semantics for OSC 7501 Program Status Protocol 0.4.
//!
//! This crate does not frame VT sequences. The authoritative terminal parser must
//! call [`ProgramStatusStore::ingest`] with the OSC arguments (the bytes after
//! `7501;` and before the terminator) and the exact encoded sequence length,
//! including `ESC ] 7501 ;` and its BEL or ST terminator.

use std::{collections::VecDeque, mem::size_of, sync::Arc};

use data_encoding::{BASE64, BASE64_NOPAD};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PROTOCOL_REVISION: &str = "0.4";

pub const MAX_SEQUENCE_BYTES: usize = 4096;
pub const MAX_KEY_BYTES: usize = 16;
pub const MAX_ID_BYTES: usize = 128;
pub const MAX_ID_SEGMENT_BYTES: usize = 32;
pub const MAX_ID_DEPTH: usize = 8;
pub const MAX_APP_BYTES: usize = 32;
pub const MAX_MSG_ENCODED_BYTES: usize = 2732;
pub const MAX_MSG_DECODED_BYTES: usize = 2048;
pub const MAX_TITLE_ENCODED_BYTES: usize = 256;
pub const MAX_TITLE_DECODED_BYTES: usize = 192;

/// Zmux supports the required minimum record count from protocol revision 0.4.
pub const MAX_RECORDS: usize = 64;
/// Status-changing event snapshots are bounded separately from current records.
pub const MAX_TRANSITIONS: usize = 128;

/// Retained store budget, excluding PTY/parser buffers, owned terminal snapshots,
/// serialized frames, and fan-out copies. See [`ESTIMATED_MAX_RETAINED_BYTES`].
pub const RETAINED_PROTOCOL_BUDGET_BYTES: usize = 512 * 1024;

/// Constant response for the optional `OSC 7501;?` feature query.
///
/// It contains no report data and uses ST so the same bytes work regardless of
/// which valid OSC terminator the program used.
pub const FEATURE_DETECTION_REPLY: &[u8] =
    b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\";

const MAX_RECORD_PAYLOAD_BYTES: usize =
    MAX_ID_BYTES + MAX_APP_BYTES + MAX_MSG_DECODED_BYTES + MAX_TITLE_DECODED_BYTES;
const MAX_RECORD_ARCS: usize = 4;
const ARC_HEADER_ESTIMATE_BYTES: usize = 16;

/// Compile-time conservative ledger for the retained store on a 64-bit target.
/// It includes maximum current records, maximum after-record event payloads,
/// Arc headers, and the fixed-capacity record/journal slots. Allocator bookkeeping
/// and transient/snapshot clones are outside this retained-store estimate.
pub const ESTIMATED_MAX_RETAINED_BYTES: usize = (MAX_RECORDS + MAX_TRANSITIONS)
    * MAX_RECORD_PAYLOAD_BYTES
    + (MAX_RECORDS + MAX_TRANSITIONS) * MAX_RECORD_ARCS * ARC_HEADER_ESTIMATE_BYTES
    + MAX_RECORDS * size_of::<StoredRecord>()
    + MAX_TRANSITIONS * size_of::<Transition>();

const _: () = assert!(ESTIMATED_MAX_RETAINED_BYTES <= RETAINED_PROTOCOL_BUDGET_BYTES);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionEpoch(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandEpoch(pub u64);

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct TransitionCursor(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
}

impl State {
    fn parse(value: &[u8]) -> Option<Self> {
        match value {
            b"idle" => Some(Self::Idle),
            b"working" => Some(Self::Working),
            b"done" => Some(Self::Done),
            b"blocked" => Some(Self::Blocked),
            b"error" => Some(Self::Error),
            _ => None,
        }
    }

    fn is_retired_at_lifetime_boundary(self) -> bool {
        matches!(self, Self::Working | Self::Blocked)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Permission,
    Question,
    Auth,
}

impl Kind {
    fn parse(value: &[u8]) -> Option<Self> {
        match value {
            b"permission" => Some(Self::Permission),
            b"question" => Some(Self::Question),
            b"auth" => Some(Self::Auth),
            _ => None,
        }
    }
}

/// Current presentation data for one terminal-local program record.
///
/// `app` is the value directly reported on this record. `effective_app` is
/// calculated from the nearest current ancestor when a snapshot is made; it is
/// never copied into stored records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub id: Arc<str>,
    pub state: State,
    pub kind: Option<Kind>,
    pub progress: Option<u8>,
    pub app: Option<Arc<str>>,
    pub effective_app: Option<Arc<str>>,
    pub msg: Option<Arc<str>>,
    pub title: Option<Arc<str>>,
}

/// Why a transition entered the journal. Consumers should treat only `Report`
/// transitions as candidates for user-facing attention; lifetime and removal
/// causes exist to keep the ordered state history explicit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionCause {
    Report,
    Clear,
    Eviction,
    Prompt,
    ChildExit,
    Ris,
    TerminalClosed,
}

/// One ordered state/lifetime change. `before_state` is intentionally only the
/// old state; `after_record` retains bounded presentation data for the report
/// that caused the transition, even if a later report or clear arrives before a
/// consumer reads the snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub cursor: TransitionCursor,
    pub command_epoch: CommandEpoch,
    pub id: Arc<str>,
    pub before_state: Option<State>,
    pub after_record: Option<Record>,
    pub cause: TransitionCause,
}

/// An owned terminal-local view. The records are the latest full state; the
/// transitions are retained events newer than the requested cursor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramStatusSnapshot {
    pub session_epoch: SessionEpoch,
    pub command_epoch: CommandEpoch,
    pub cursor: TransitionCursor,
    pub dropped_through: Option<TransitionCursor>,
    pub records: Vec<Record>,
    pub transitions: Vec<Transition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IgnoreReason {
    FeatureQuery,
    MissingOrUnknownState,
    InvalidId,
    TerminalClosed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectionReason {
    InvalidSequenceLength,
    SequenceTooLong,
    KeyTooLong,
    IdTooLong,
    IdSegmentTooLong,
    IdTooDeep,
    AppTooLong,
    MsgEncodedTooLong,
    MsgDecodedTooLong,
    TitleEncodedTooLong,
    TitleDecodedTooLong,
    InvalidBase64,
    InvalidUtf8,
    ControlCharacter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreFailure {
    TransitionCursorExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IngestOutcome {
    Ignored(IgnoreReason),
    Rejected(RejectionReason),
    Failed(StoreFailure),
    Applied { records_changed: bool },
}

#[derive(Debug, Error)]
#[error("could not reserve bounded program-status storage: {0}")]
pub struct AllocationError(#[from] std::collections::TryReserveError);

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("cannot copy program-status state across terminal session epochs")]
pub struct StoreCopyError;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("program-status command epoch exhausted")]
    CommandEpochExhausted,
    #[error("program-status transition cursor exhausted")]
    TransitionCursorExhausted,
}

#[derive(Debug, Error)]
#[error("could not allocate a bounded program-status snapshot: {0}")]
pub struct SnapshotError(#[from] std::collections::TryReserveError);

#[derive(Clone, Debug, PartialEq, Eq)]
struct StoredRecord {
    id: Arc<str>,
    state: State,
    kind: Option<Kind>,
    progress: Option<u8>,
    app: Option<Arc<str>>,
    msg: Option<Arc<str>>,
    title: Option<Arc<str>>,
}

impl StoredRecord {
    fn public_record(&self, effective_app: Option<Arc<str>>) -> Record {
        Record {
            id: Arc::clone(&self.id),
            state: self.state,
            kind: self.kind,
            progress: self.progress,
            app: self.app.as_ref().map(Arc::clone),
            effective_app,
            msg: self.msg.as_ref().map(Arc::clone),
            title: self.title.as_ref().map(Arc::clone),
        }
    }

    fn has_same_contents(&self, other: &Self) -> bool {
        self.state == other.state
            && self.kind == other.kind
            && self.progress == other.progress
            && self.app == other.app
            && self.msg == other.msg
            && self.title == other.title
    }
}

/// Per-terminal bounded protocol state. Records are kept in least-to-most
/// recently updated order; the transition journal is independently bounded.
pub struct ProgramStatusStore {
    session_epoch: SessionEpoch,
    command_epoch: CommandEpoch,
    cursor: TransitionCursor,
    dropped_through: Option<TransitionCursor>,
    records: Vec<StoredRecord>,
    transitions: VecDeque<Transition>,
    closed: bool,
}

impl ProgramStatusStore {
    /// Allocate fixed-capacity record and transition storage for one terminal
    /// session owner. A fresh store should be created for a replacement session.
    pub fn try_new(session_epoch: SessionEpoch) -> Result<Self, AllocationError> {
        let mut records = Vec::new();
        records.try_reserve_exact(MAX_RECORDS)?;
        let mut transitions = VecDeque::new();
        transitions.try_reserve_exact(MAX_TRANSITIONS)?;
        Ok(Self {
            session_epoch,
            command_epoch: CommandEpoch::default(),
            cursor: TransitionCursor::default(),
            dropped_through: None,
            records,
            transitions,
            closed: false,
        })
    }

    /// Make an independently mutable staged copy. Strings remain shared and
    /// immutable; bounded container storage is reserved fallibly.
    pub fn try_clone(&self) -> Result<Self, AllocationError> {
        let mut records = Vec::new();
        records.try_reserve_exact(MAX_RECORDS)?;
        records.extend(self.records.iter().cloned());

        let mut transitions = VecDeque::new();
        transitions.try_reserve_exact(MAX_TRANSITIONS)?;
        transitions.extend(self.transitions.iter().cloned());

        Ok(Self {
            session_epoch: self.session_epoch,
            command_epoch: self.command_epoch,
            cursor: self.cursor,
            dropped_through: self.dropped_through,
            records,
            transitions,
            closed: self.closed,
        })
    }

    /// Replace this store's contents with another store from the same terminal
    /// session while reusing this store's fixed-capacity record and journal
    /// buffers. Create the destination with [`ProgramStatusStore::try_new`]; the
    /// copy itself performs no container allocation.
    pub fn copy_from(&mut self, source: &Self) -> Result<(), StoreCopyError> {
        if self.session_epoch != source.session_epoch {
            return Err(StoreCopyError);
        }
        debug_assert!(self.records.capacity() >= MAX_RECORDS);
        debug_assert!(self.transitions.capacity() >= MAX_TRANSITIONS);
        self.records.clear();
        self.records.extend(source.records.iter().cloned());
        self.transitions.clear();
        self.transitions.extend(source.transitions.iter().cloned());
        self.command_epoch = source.command_epoch;
        self.cursor = source.cursor;
        self.dropped_through = source.dropped_through;
        self.closed = source.closed;
        Ok(())
    }

    /// Apply a complete OSC 7501 argument string from the authoritative VT
    /// parser. This is the only report mutation entry point, so validation and
    /// record replacement remain atomic.
    pub fn ingest(&mut self, args: &[u8], full_sequence_len: usize) -> IngestOutcome {
        if self.closed {
            return IngestOutcome::Ignored(IgnoreReason::TerminalClosed);
        }
        let bel_length = args.len().checked_add(8);
        let st_length = args.len().checked_add(9);
        if Some(full_sequence_len) != bel_length && Some(full_sequence_len) != st_length {
            return IngestOutcome::Rejected(RejectionReason::InvalidSequenceLength);
        }
        if full_sequence_len > MAX_SEQUENCE_BYTES || args.len() > MAX_SEQUENCE_BYTES {
            return IngestOutcome::Rejected(RejectionReason::SequenceTooLong);
        }

        if args == b"?" {
            return IngestOutcome::Ignored(IgnoreReason::FeatureQuery);
        }

        let parsed = match parse_pairs(args) {
            Ok(fields) => fields,
            Err(reason) => return IngestOutcome::Rejected(reason),
        };

        let id = match parsed.id {
            None => Arc::<str>::from(""),
            Some(bytes) => match parse_id(bytes) {
                Ok(id) => Arc::<str>::from(id),
                Err(IdError::Invalid) => return IngestOutcome::Ignored(IgnoreReason::InvalidId),
                Err(IdError::TooLong) => {
                    return IngestOutcome::Rejected(RejectionReason::IdTooLong);
                }
                Err(IdError::SegmentTooLong) => {
                    return IngestOutcome::Rejected(RejectionReason::IdSegmentTooLong);
                }
                Err(IdError::TooDeep) => {
                    return IngestOutcome::Rejected(RejectionReason::IdTooDeep);
                }
            },
        };
        let Some(state_value) = parsed.state else {
            return IngestOutcome::Ignored(IgnoreReason::MissingOrUnknownState);
        };
        if state_value == b"clear" {
            let removal_count = self
                .records
                .iter()
                .filter(|record| is_at_or_below(record.id.as_ref(), &id))
                .count();
            if !self.can_append_transitions(removal_count) {
                return IngestOutcome::Failed(StoreFailure::TransitionCursorExhausted);
            }
            let removed = self
                .remove_subtree(&id, TransitionCause::Clear, self.command_epoch)
                .expect("transition cursor was preflighted");
            return IngestOutcome::Applied {
                records_changed: removed,
            };
        }
        let Some(state) = State::parse(state_value) else {
            return IngestOutcome::Ignored(IgnoreReason::MissingOrUnknownState);
        };

        let kind = if state == State::Blocked {
            parsed.kind.and_then(Kind::parse)
        } else {
            None
        };
        let progress = if matches!(state, State::Working | State::Blocked) {
            parsed.progress.and_then(parse_progress)
        } else {
            None
        };
        let app = parsed.app.and_then(parse_app);

        let mut incoming = StoredRecord {
            id,
            state,
            kind,
            progress,
            app,
            msg: parsed.msg,
            title: parsed.title,
        };

        let existing_index = self
            .records
            .iter()
            .position(|record| record.id == incoming.id);
        let (before_state, contents_changed) = if let Some(index) = existing_index {
            let previous = &self.records[index];
            let previous_state = previous.state;
            let previous_id = Arc::clone(&previous.id);
            let changed = !previous.has_same_contents(&incoming);
            incoming.id = previous_id;
            (Some(previous_state), changed)
        } else {
            (None, true)
        };

        let will_evict = existing_index.is_none() && self.records.len() == MAX_RECORDS;
        let transition_count = usize::from(will_evict) + usize::from(before_state != Some(state));
        if !self.can_append_transitions(transition_count) {
            return IngestOutcome::Failed(StoreFailure::TransitionCursorExhausted);
        }

        if let Some(index) = existing_index {
            self.records.remove(index);
        }
        let mut records_changed = contents_changed;
        if will_evict && let Some(evicted) = self.records.first().cloned() {
            self.records.remove(0);
            self.push_transition(
                Arc::clone(&evicted.id),
                Some(evicted.state),
                None,
                TransitionCause::Eviction,
                self.command_epoch,
            )
            .expect("transition cursor was preflighted");
            records_changed = true;
        }

        let state_changed = before_state != Some(state);
        self.records.push(incoming);
        if state_changed {
            let index = self.records.len() - 1;
            let after_record = self.public_record_at(index);
            let id = Arc::clone(&self.records[index].id);
            self.push_transition(
                id,
                before_state,
                Some(after_record),
                TransitionCause::Report,
                self.command_epoch,
            )
            .expect("transition cursor was preflighted");
        }

        IngestOutcome::Applied { records_changed }
    }

    /// Observe OSC 133 `A`: retire running/waiting records at the closing epoch,
    /// then advance to the next local command epoch.
    pub fn prompt_boundary(&mut self) -> Result<CommandEpoch, LifecycleError> {
        let next = self
            .command_epoch
            .0
            .checked_add(1)
            .ok_or(LifecycleError::CommandEpochExhausted)?;
        let closing_epoch = self.command_epoch;
        let retirement_count = self
            .records
            .iter()
            .filter(|record| record.state.is_retired_at_lifetime_boundary())
            .count();
        self.ensure_transition_capacity(retirement_count)?;
        self.retire_running(TransitionCause::Prompt, closing_epoch)
            .expect("transition cursor was preflighted");
        self.command_epoch = CommandEpoch(next);
        Ok(self.command_epoch)
    }

    /// Observe exit of the process attached to this terminal. This does not
    /// advance the command epoch; a prompt boundary is the reliable epoch edge.
    pub fn attached_child_exited(&mut self) -> Result<(), LifecycleError> {
        let retirement_count = self
            .records
            .iter()
            .filter(|record| record.state.is_retired_at_lifetime_boundary())
            .count();
        self.ensure_transition_capacity(retirement_count)?;
        self.retire_running(TransitionCause::ChildExit, self.command_epoch)
            .expect("transition cursor was preflighted");
        Ok(())
    }

    /// Apply RIS. DECSTR and alternate-screen transitions intentionally do not
    /// call a lifecycle operation and therefore preserve the records.
    pub fn ris(&mut self) -> Result<(), LifecycleError> {
        self.ensure_transition_capacity(self.records.len())?;
        self.remove_all(TransitionCause::Ris, self.command_epoch)
            .expect("transition cursor was preflighted");
        Ok(())
    }

    /// Remove all terminal-local records when their terminal owner closes.
    pub fn terminal_closed(&mut self) -> Result<(), LifecycleError> {
        if !self.closed {
            self.ensure_transition_capacity(self.records.len())?;
            self.remove_all(TransitionCause::TerminalClosed, self.command_epoch)
                .expect("transition cursor was preflighted");
            self.closed = true;
        }
        Ok(())
    }

    /// Return current records and retained events newer than `after_cursor`.
    /// If `after_cursor < dropped_through`, the consumer has a gap and must
    /// reconcile from `records` without replaying uncertain alerts.
    pub fn snapshot(
        &self,
        after_cursor: Option<TransitionCursor>,
    ) -> Result<ProgramStatusSnapshot, SnapshotError> {
        let mut records = Vec::new();
        records.try_reserve_exact(self.records.len())?;
        for index in 0..self.records.len() {
            records.push(self.public_record_at(index));
        }
        records.sort_unstable_by(|left, right| left.id.cmp(&right.id));

        let mut transitions = Vec::new();
        let transition_count = self
            .transitions
            .iter()
            .filter(|transition| after_cursor.is_none_or(|cursor| transition.cursor > cursor))
            .count();
        transitions.try_reserve_exact(transition_count)?;
        transitions.extend(
            self.transitions
                .iter()
                .filter(|transition| after_cursor.is_none_or(|cursor| transition.cursor > cursor))
                .cloned(),
        );

        Ok(ProgramStatusSnapshot {
            session_epoch: self.session_epoch,
            command_epoch: self.command_epoch,
            cursor: self.cursor,
            dropped_through: self.dropped_through,
            records,
            transitions,
        })
    }

    fn public_record_at(&self, index: usize) -> Record {
        let stored = &self.records[index];
        let effective_app = self.nearest_app(&stored.id);
        stored.public_record(effective_app)
    }

    fn nearest_app(&self, id: &str) -> Option<Arc<str>> {
        let mut candidate = id;
        loop {
            if let Some(record) = self
                .records
                .iter()
                .find(|record| record.id.as_ref() == candidate)
                && let Some(app) = &record.app
            {
                return Some(Arc::clone(app));
            }
            if candidate.is_empty() {
                return None;
            }
            candidate = match candidate.rsplit_once('/') {
                Some((parent, _)) => parent,
                None => "",
            };
        }
    }

    fn remove_subtree(
        &mut self,
        target: &str,
        cause: TransitionCause,
        event_epoch: CommandEpoch,
    ) -> Result<bool, StoreFailure> {
        let mut removed = false;
        let mut index = 0;
        while index < self.records.len() {
            if is_at_or_below(self.records[index].id.as_ref(), target) {
                let record = self.records.remove(index);
                self.push_transition(
                    Arc::clone(&record.id),
                    Some(record.state),
                    None,
                    cause,
                    event_epoch,
                )?;
                removed = true;
            } else {
                index += 1;
            }
        }
        Ok(removed)
    }

    fn remove_all(
        &mut self,
        cause: TransitionCause,
        event_epoch: CommandEpoch,
    ) -> Result<(), StoreFailure> {
        while let Some(record) = self.records.first().cloned() {
            self.records.remove(0);
            self.push_transition(
                Arc::clone(&record.id),
                Some(record.state),
                None,
                cause,
                event_epoch,
            )?;
        }
        Ok(())
    }

    fn retire_running(
        &mut self,
        cause: TransitionCause,
        event_epoch: CommandEpoch,
    ) -> Result<(), StoreFailure> {
        let mut index = 0;
        while index < self.records.len() {
            if self.records[index].state.is_retired_at_lifetime_boundary() {
                let record = self.records.remove(index);
                self.push_transition(
                    Arc::clone(&record.id),
                    Some(record.state),
                    None,
                    cause,
                    event_epoch,
                )?;
            } else {
                index += 1;
            }
        }
        Ok(())
    }

    fn can_append_transitions(&self, count: usize) -> bool {
        u64::try_from(count)
            .ok()
            .and_then(|count| self.cursor.0.checked_add(count))
            .is_some()
    }

    fn ensure_transition_capacity(&self, count: usize) -> Result<(), LifecycleError> {
        self.can_append_transitions(count)
            .then_some(())
            .ok_or(LifecycleError::TransitionCursorExhausted)
    }

    fn push_transition(
        &mut self,
        id: Arc<str>,
        before_state: Option<State>,
        after_record: Option<Record>,
        cause: TransitionCause,
        command_epoch: CommandEpoch,
    ) -> Result<(), StoreFailure> {
        let next = self
            .cursor
            .0
            .checked_add(1)
            .ok_or(StoreFailure::TransitionCursorExhausted)?;
        self.cursor = TransitionCursor(next);
        if self.transitions.len() == MAX_TRANSITIONS
            && let Some(dropped) = self.transitions.pop_front()
        {
            self.dropped_through = Some(dropped.cursor);
        }
        self.transitions.push_back(Transition {
            cursor: self.cursor,
            command_epoch,
            id,
            before_state,
            after_record,
            cause,
        });
        Ok(())
    }
}

#[derive(Default)]
struct RawFields<'a> {
    state: Option<&'a [u8]>,
    id: Option<&'a [u8]>,
    kind: Option<&'a [u8]>,
    progress: Option<&'a [u8]>,
    app: Option<&'a [u8]>,
    msg: Option<Arc<str>>,
    title: Option<Arc<str>>,
}

fn parse_pairs(args: &[u8]) -> Result<RawFields<'_>, RejectionReason> {
    let mut fields = RawFields::default();
    for pair in args.split(|byte| *byte == b':') {
        let pair = trim_ascii_whitespace(pair);
        if pair.is_empty() {
            continue;
        }
        let Some(value_start) = pair.iter().position(|byte| *byte == b'=') else {
            continue;
        };
        let key_end = value_start;
        let key = trim_ascii_whitespace(&pair[..key_end]);
        if key.len() > MAX_KEY_BYTES {
            return Err(RejectionReason::KeyTooLong);
        }
        if key.is_empty() || !key.iter().all(|byte| byte.is_ascii_lowercase()) {
            continue;
        }
        let value = trim_ascii_whitespace(&pair[value_start + 1..]);
        let valid_value = value.iter().all(|byte| is_value_byte(*byte));

        if key == b"id" {
            if value.len() > MAX_ID_BYTES {
                return Err(RejectionReason::IdTooLong);
            }
            check_id_limits(value)?;
            if !valid_value {
                // Revision 0.4 skips malformed pairs. A prior valid id remains
                // the last syntactically valid `id` pair.
                continue;
            }
            fields.id = Some(value);
            continue;
        }

        match key {
            b"app" => {
                if value.len() > MAX_APP_BYTES {
                    return Err(RejectionReason::AppTooLong);
                }
                if !valid_value {
                    continue;
                }
                fields.app = Some(value);
            }
            b"msg" => {
                if value.len() > MAX_MSG_ENCODED_BYTES {
                    return Err(RejectionReason::MsgEncodedTooLong);
                }
                if valid_value {
                    fields.msg = Some(decode_text(
                        value,
                        MAX_MSG_DECODED_BYTES,
                        RejectionReason::MsgDecodedTooLong,
                    )?);
                }
            }
            b"title" => {
                if value.len() > MAX_TITLE_ENCODED_BYTES {
                    return Err(RejectionReason::TitleEncodedTooLong);
                }
                if valid_value {
                    fields.title = Some(decode_text(
                        value,
                        MAX_TITLE_DECODED_BYTES,
                        RejectionReason::TitleDecodedTooLong,
                    )?);
                }
            }
            b"state" if valid_value => fields.state = Some(value),
            b"kind" if valid_value => fields.kind = Some(value),
            b"progress" if valid_value => fields.progress = Some(value),
            _ => {}
        }
    }
    Ok(fields)
}

fn parse_id(bytes: &[u8]) -> Result<&str, IdError> {
    if bytes.len() > MAX_ID_BYTES {
        return Err(IdError::TooLong);
    }
    let value = std::str::from_utf8(bytes).map_err(|_| IdError::Invalid)?;
    check_id_limits(bytes).map_err(|reason| match reason {
        RejectionReason::IdTooLong => IdError::TooLong,
        RejectionReason::IdSegmentTooLong => IdError::SegmentTooLong,
        RejectionReason::IdTooDeep => IdError::TooDeep,
        _ => IdError::Invalid,
    })?;
    if value.split('/').any(|segment| {
        segment.is_empty()
            || !segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.+-".contains(&byte))
    }) {
        return Err(IdError::Invalid);
    }
    Ok(value)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdError {
    Invalid,
    TooLong,
    SegmentTooLong,
    TooDeep,
}

fn check_id_limits(value: &[u8]) -> Result<(), RejectionReason> {
    if value.len() > MAX_ID_BYTES {
        return Err(RejectionReason::IdTooLong);
    }
    for segment in value.split(|byte| *byte == b'/') {
        if segment.len() > MAX_ID_SEGMENT_BYTES {
            return Err(RejectionReason::IdSegmentTooLong);
        }
    }
    let depth = value.iter().filter(|byte| **byte == b'/').count() + 1;
    if depth > MAX_ID_DEPTH {
        return Err(RejectionReason::IdTooDeep);
    }
    Ok(())
}

fn parse_app(value: &[u8]) -> Option<Arc<str>> {
    if value.is_empty()
        || value.len() > MAX_APP_BYTES
        || !value
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.+-".contains(byte))
    {
        return None;
    }
    std::str::from_utf8(value).ok().map(Arc::from)
}

fn parse_progress(value: &[u8]) -> Option<u8> {
    if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let progress = std::str::from_utf8(value).ok()?.parse::<u8>().ok()?;
    (progress <= 100).then_some(progress)
}

fn decode_text(
    encoded: &[u8],
    maximum_decoded_len: usize,
    decoded_too_long: RejectionReason,
) -> Result<Arc<str>, RejectionReason> {
    let Some(decoded_len) = encoded_decoded_len(encoded) else {
        return Err(RejectionReason::InvalidBase64);
    };
    if decoded_len > maximum_decoded_len {
        return Err(decoded_too_long);
    }
    let decoded = if encoded.contains(&b'=') {
        BASE64.decode(encoded)
    } else {
        BASE64_NOPAD.decode(encoded)
    }
    .map_err(|_| RejectionReason::InvalidBase64)?;
    let text = std::str::from_utf8(&decoded).map_err(|_| RejectionReason::InvalidUtf8)?;
    if text.chars().any(is_disallowed_control) {
        return Err(RejectionReason::ControlCharacter);
    }
    Ok(Arc::from(text))
}

fn encoded_decoded_len(encoded: &[u8]) -> Option<usize> {
    let padding = encoded
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    if padding > 2 || (padding > 0 && !encoded.len().is_multiple_of(4)) {
        return None;
    }
    let data_len = encoded.len().checked_sub(padding)?;
    if encoded[..data_len]
        .iter()
        .any(|byte| !is_base64_alphabet(*byte))
    {
        return None;
    }
    let remainder = data_len % 4;
    match padding {
        0 if remainder == 1 => return None,
        1 if remainder != 3 => return None,
        2 if remainder != 2 => return None,
        _ => {}
    }
    let decoded_len = (data_len / 4).checked_mul(3)?;
    decoded_len.checked_add(match remainder {
        0 => 0,
        2 => 1,
        3 => 2,
        _ => return None,
    })
}

fn is_base64_alphabet(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/')
}

fn is_value_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"_.,+/=-".contains(&byte)
}

fn is_disallowed_control(character: char) -> bool {
    let codepoint = character as u32;
    codepoint <= 0x1f || codepoint == 0x7f || (0x80..=0x9f).contains(&codepoint)
}

fn trim_ascii_whitespace(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

fn is_at_or_below(candidate: &str, target: &str) -> bool {
    target.is_empty()
        || candidate == target
        || candidate
            .strip_prefix(target)
            .is_some_and(|suffix| suffix.starts_with('/'))
}
