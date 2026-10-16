//! Native notification replies retain exact owner and once-only PTY receipts.
//! The workspace executor polls completion; it never waits for terminal I/O.
use super::*;
use attention::{NotificationReply, NotificationReplyOutcome, NotificationTarget};
use zmux_session_runtime::{InputReceipt, InputWriteResult, InputWriteStatus};

pub(super) const REPLY_WRITE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_PENDING_REPLIES: usize = 8;

pub(super) struct PendingReply {
    target: NotificationTarget,
    input: InputReceipt,
    total_bytes: usize,
    response_deadline: Instant,
    acknowledgement: SyncSender<Execution>,
}

impl Worker {
    pub(super) fn admit_reply(
        &mut self,
        target: NotificationTarget,
        text: String,
        deadline: Option<Instant>,
        acknowledgement: &mut Option<SyncSender<Execution>>,
    ) -> Result<(), DesktopError> {
        if acknowledgement.is_none() || deadline.is_none() {
            return Err(DesktopError::Failure(
                "notification replies require the acknowledged native path".into(),
            ));
        }
        let deadline = deadline.expect("native reply deadline checked");
        let response_deadline = deadline + (NATIVE_COMMAND_TIMEOUT - REPLY_WRITE_TIMEOUT);
        if Instant::now() >= response_deadline {
            return Err(DesktopError::Timeout);
        }
        // Reserve bounded completion capacity before consuming the generation.
        if self.pending_replies.len() >= MAX_PENDING_REPLIES {
            return Err(DesktopError::Busy);
        }
        if self.session_transition_pending() {
            return Err(DesktopError::Failure(
                "session restore is pending; notification reply was not admitted".into(),
            ));
        }
        if text.len() >= INPUT_CHUNK_BYTES {
            return Err(attention::AttentionError::ReplyTextTooLarge.into());
        }
        let text = text.trim();
        let session_id = self
            .attention
            .claim_reply(&self.model.snapshot(), &target)?;
        if text.is_empty() {
            return self.finish_immediate_reply(NotificationReply {
                target,
                outcome: NotificationReplyOutcome::Empty,
                bytes_written: 0,
                total_bytes: 0,
                reason: Some("empty_reply".into()),
            });
        }
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(b'\r');
        let total_bytes = bytes.len();
        let input = if Instant::now() >= deadline {
            Err("reply_write_deadline".into())
        } else {
            self.writable_session(session_id)
                .map_err(|error| error.to_string())
                .and_then(|session| {
                    session
                        .send_input_with_receipt(&bytes, deadline)
                        .map_err(|error| error.to_string())
                })
        };
        match input {
            Ok(input) => {
                self.pending_replies.push(PendingReply {
                    target,
                    input,
                    total_bytes,
                    response_deadline,
                    acknowledgement: acknowledgement
                        .take()
                        .expect("native reply acknowledgement checked"),
                });
                Ok(())
            }
            // Admission errors from Session guarantee no bytes were queued.
            Err(reason) => self.finish_immediate_reply(NotificationReply {
                target,
                outcome: NotificationReplyOutcome::NotWritten,
                bytes_written: 0,
                total_bytes,
                reason: Some(reason.chars().take(512).collect()),
            }),
        }
    }

    fn finish_immediate_reply(&mut self, reply: NotificationReply) -> Result<(), DesktopError> {
        let result = self.finish_reply_owner(&reply);
        self.notification_reply = Some(reply);
        self.notification_navigation = result?;
        Ok(())
    }

    fn finish_reply_owner(
        &mut self,
        reply: &NotificationReply,
    ) -> Result<Option<NotificationNavigation>, DesktopError> {
        self.attention.finish_reply(&self.model.snapshot(), reply)?;
        if !matches!(
            reply.outcome,
            NotificationReplyOutcome::Empty | NotificationReplyOutcome::NotWritten
        ) {
            return Ok(None);
        }
        // A definite zero-byte result may navigate, but never masquerades as
        // successful input. Exact ownership is revalidated by the shared open.
        self.open_notification(&reply.target, false)?;
        let row = self
            .attention
            .find(reply.target.notification_id)
            .map_err(DesktopError::Failure)?;
        Ok(Some(NotificationNavigation {
            opened: true,
            notification_id: Some(reply.target.notification_id),
            notification_uuid: Some(row.uuid),
            target: Some(reply.target.clone()),
        }))
    }

    pub(super) fn poll_replies(&mut self) -> bool {
        let mut changed = false;
        let mut index = 0;
        while index < self.pending_replies.len() {
            let write = match self.pending_replies[index].input.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty)
                    if Instant::now() < self.pending_replies[index].response_deadline =>
                {
                    index += 1;
                    continue;
                }
                // An unresponsive PTY worker must not retain a desktop completion
                // slot indefinitely. Dropping the receipt cannot replay input;
                // its once-only notification claim remains consumed.
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => None,
            };
            let pending = self.pending_replies.swap_remove(index);
            let mut reply = write_reply(pending.target, pending.total_bytes, write);
            let expired = Instant::now() >= pending.response_deadline;
            if expired {
                // Local acceptance remains in the byte count, but a response
                // timeout cannot authorize a late read or focus side effect.
                reply.outcome = NotificationReplyOutcome::Unknown;
                reply.reason = Some("reply_response_deadline".into());
            }
            let navigation = self.finish_reply_owner(&reply);
            let (result, navigation) = match navigation {
                Ok(navigation) if !expired => (Ok(()), navigation),
                Ok(_) => (Err(DesktopError::Timeout), None),
                Err(error) => (Err(error), None),
            };
            let mutation = if navigation.is_some() {
                self.notification_mutation.take()
            } else {
                None
            };
            let snapshot = self.publish(false);
            let _ = pending.acknowledgement.try_send(reply_execution(
                result, snapshot, reply, navigation, mutation,
            ));
            changed = true;
        }
        changed
    }

    pub(super) fn cancel_replies(&mut self) {
        let snapshot = self.publish(false);
        for pending in self.pending_replies.drain(..) {
            let reply = NotificationReply {
                target: pending.target,
                outcome: NotificationReplyOutcome::Unknown,
                bytes_written: 0,
                total_bytes: pending.total_bytes,
                reason: Some("desktop_shutdown".into()),
            };
            let _ = pending.acknowledgement.try_send(reply_execution(
                Err(DesktopError::Closed),
                Arc::clone(&snapshot),
                reply,
                None,
                None,
            ));
        }
    }
}

fn write_reply(
    target: NotificationTarget,
    total_bytes: usize,
    write: Option<InputWriteResult>,
) -> NotificationReply {
    let Some(write) = write else {
        return NotificationReply {
            target,
            outcome: NotificationReplyOutcome::Unknown,
            bytes_written: 0,
            total_bytes,
            reason: Some("input_receipt_lost".into()),
        };
    };
    let valid_size = write.total_bytes == total_bytes && write.bytes_written <= total_bytes;
    let outcome = match write.status {
        InputWriteStatus::Complete if valid_size && write.bytes_written == total_bytes => {
            NotificationReplyOutcome::Complete
        }
        InputWriteStatus::NotWritten if valid_size && write.bytes_written == 0 => {
            NotificationReplyOutcome::NotWritten
        }
        InputWriteStatus::Partial
            if valid_size && write.bytes_written > 0 && write.bytes_written < total_bytes =>
        {
            NotificationReplyOutcome::Partial
        }
        _ => NotificationReplyOutcome::Unknown,
    };
    NotificationReply {
        target,
        outcome,
        bytes_written: write.bytes_written,
        total_bytes,
        reason: write.reason.map(|reason| format!("{reason:?}")),
    }
}

fn reply_execution(
    result: Result<(), DesktopError>,
    snapshot: Arc<Snapshot>,
    reply: NotificationReply,
    navigation: Option<NotificationNavigation>,
    mutation: Option<NotificationMutation>,
) -> Execution {
    Execution {
        agent_message_result: None,
        metadata_result: None,
        result,
        snapshot,
        notification_reply: Some(reply),
        notification_navigation: navigation,
        notification_mutation: mutation,
        created_workspaces: Vec::new(),
        created_panes: Vec::new(),
        created_surfaces: Vec::new(),
        created_uploads: Vec::new(),
        profile_operation_id: None,
        download_result: None,
        window_result: None,
        workspace_order_result: None,
        workspace_group_result: None,
        surface_order_result: None,
        tmux_operation_id: None,
        session_operation_id: None,
        browser_operation_id: None,
        browser_dispatch: None,
    }
}
