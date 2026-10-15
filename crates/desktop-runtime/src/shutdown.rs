//! One shutdown ticket retains physical owners after its caller's budget expires.
use crate::{Command, Desktop, DesktopError, Envelope, Worker};
use serde::Serialize;
use std::{
    sync::{
        atomic::Ordering,
        mpsc::{Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Quiescing,
    Draining,
    Complete,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Pending {
    pub sessions: usize,
    pub launches: usize,
    pub tmux: usize,
    pub transfers: usize,
    pub profiles: usize,
    pub persistence: usize,
    pub downloads: usize,
    pub connections: usize,
    pub native_downloads: usize,
    pub native_cleanup: bool,
    pub control: usize,
}
impl Pending {
    fn runtime_done(&self) -> bool {
        self.sessions
            + self.launches
            + self.tmux
            + self.transfers
            + self.profiles
            + self.persistence
            + self.downloads
            + self.connections
            + self.control
            == 0
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub ticket: u64,
    pub phase: Phase,
    pub deadline_expired: bool,
    pub remaining_ms: u64,
    pub runtime_cleanup_complete: bool,
    pub physical_cleanup_complete: bool,
    pub pending: Pending,
    pub issues: Vec<String>,
    pub issues_dropped: u64,
}
pub(crate) struct Operation {
    pub deadline: Instant,
    report: Report,
    join_failed: bool,
}
impl Operation {
    fn new(budget: Duration, native_cleanup: bool) -> Self {
        Self {
            deadline: Instant::now() + budget,
            join_failed: false,
            report: Report {
                ticket: 1,
                phase: Phase::Quiescing,
                deadline_expired: false,
                remaining_ms: budget.as_millis().try_into().unwrap_or(u64::MAX),
                runtime_cleanup_complete: false,
                physical_cleanup_complete: false,
                pending: Pending {
                    native_cleanup,
                    ..Default::default()
                },
                issues: Vec::new(),
                issues_dropped: 0,
            },
        }
    }
    fn report(&self) -> Report {
        let mut report = self.report.clone();
        report.remaining_ms = self
            .deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        report.deadline_expired |= Instant::now() >= self.deadline;
        report
    }
    fn issue(&mut self, message: String) {
        let message: String = message.chars().take(512).collect();
        if self.report.issues.contains(&message) {
            return;
        }
        if self.report.issues.len() < 16 {
            self.report.issues.push(message);
        } else {
            self.report.issues_dropped = self.report.issues_dropped.saturating_add(1);
        }
    }
}
pub(crate) fn cleanup_command(command: &Command) -> bool {
    matches!(command, Command::NativeDownload { request } if !matches!(request, crate::downloads::NativeDownload::Begin { .. }))
}
impl Desktop {
    /// Installs one deadline. Repeated calls never renew it or register new barriers.
    pub fn begin_shutdown(
        &self,
        budget: Duration,
        native_cleanup: bool,
    ) -> Result<Report, DesktopError> {
        if budget > Duration::from_secs(30) {
            return Err(DesktopError::Failure("shutdown budget exceeds 30s".into()));
        }
        let mut operation = self
            .shared
            .shutdown
            .lock()
            .map_err(|_| DesktopError::Failure("shutdown state poisoned".into()))?;
        let operation = operation.get_or_insert_with(|| Operation::new(budget, native_cleanup));
        self.shared.stop.store(true, Ordering::Release);
        Ok(operation.report())
    }
    pub fn shutdown_native_complete(&self, ticket: u64) -> Result<(), DesktopError> {
        let mut operation = self
            .shared
            .shutdown
            .lock()
            .map_err(|_| DesktopError::Failure("shutdown state poisoned".into()))?;
        let operation = operation
            .as_mut()
            .filter(|o| o.report.ticket == ticket)
            .ok_or_else(|| DesktopError::Failure("invalid shutdown ticket".into()))?;
        operation.report.pending.native_cleanup = false;
        Ok(())
    }
    pub fn shutdown_status(&self, ticket: u64) -> Result<Report, DesktopError> {
        {
            let operation = self
                .shared
                .shutdown
                .lock()
                .map_err(|_| DesktopError::Failure("shutdown state poisoned".into()))?;
            if operation.as_ref().is_none_or(|o| o.report.ticket != ticket) {
                return Err(DesktopError::Failure("invalid shutdown ticket".into()));
            }
        }
        let mut handle = self
            .worker
            .lock()
            .map_err(|_| DesktopError::Failure("desktop worker lock poisoned".into()))?;
        let finished = handle.as_ref().is_none_or(|worker| worker.is_finished());
        let joined = if finished {
            handle.take().map(|worker| worker.join())
        } else {
            None
        };
        let mut operation = self
            .shared
            .shutdown
            .lock()
            .map_err(|_| DesktopError::Failure("shutdown state poisoned".into()))?;
        let operation = operation.as_mut().expect("validated ticket");
        if let Some(Err(_)) = joined {
            operation.join_failed = true;
            operation.issue("desktop worker panicked; physical cleanup is unconfirmed".into());
        } else if finished
            && !operation.join_failed
            && operation.report.runtime_cleanup_complete
            && !operation.report.pending.native_cleanup
            && operation.report.pending.native_downloads == 0
        {
            operation.report.phase = Phase::Complete;
            operation.report.physical_cleanup_complete = true;
        }
        Ok(operation.report())
    }
    /// Bounded off-UI wait. Expiry returns Pending state; polling can continue.
    pub fn wait_shutdown(&self, ticket: u64, wait: Duration) -> Result<Report, DesktopError> {
        let deadline = {
            let operation = self
                .shared
                .shutdown
                .lock()
                .map_err(|_| DesktopError::Failure("shutdown state poisoned".into()))?;
            operation
                .as_ref()
                .filter(|o| o.report.ticket == ticket)
                .ok_or_else(|| DesktopError::Failure("invalid shutdown ticket".into()))?
                .deadline
                .min(Instant::now() + wait.min(Duration::from_secs(30)))
        };
        loop {
            let report = self.shutdown_status(ticket)?;
            if report.physical_cleanup_complete || Instant::now() >= deadline {
                return Ok(report);
            }
            thread::sleep(
                Duration::from_millis(2).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
}
impl Worker {
    fn shutdown_issue(&mut self, message: String) {
        self.cleanup_error = Some(message.clone());
        if let Ok(mut operation) = self.shared.shutdown.lock()
            && let Some(operation) = operation.as_mut()
        {
            operation.issue(message);
        }
    }
    pub(crate) fn reap_launch_workers(&mut self) {
        let mut active = Vec::new();
        for worker in std::mem::take(&mut self.launch_workers) {
            if worker.is_finished() {
                if worker.join().is_err() {
                    self.shutdown_issue(
                        "terminal launch worker panicked; creation outcome is unknown".into(),
                    );
                }
            } else {
                active.push(worker);
            }
        }
        self.launch_workers = active;
    }
    pub(crate) fn run_shutdown(&mut self, receiver: Receiver<Envelope>) {
        if let Ok(mut operation) = self.shared.shutdown.lock() {
            operation.get_or_insert_with(|| Operation::new(Duration::from_secs(3), false));
        }
        self.browser
            .reset("desktop shutdown cancelled browser operations");
        self.cancel_replies();
        self.local_tmux.cancel_all();
        self.cancel_transfers();
        self.attention
            .reset_native("desktop shutdown cancelled native notifications");
        self.prepare_shutdown_persistence();
        for slot in self.slots.values() {
            slot.cancellation.store(true, Ordering::Release);
        }
        let sessions: Vec<_> = self
            .slots
            .values_mut()
            .filter_map(|s| s.session.take())
            .collect();
        for session in sessions {
            self.retire(session);
        }
        self.slots.clear();
        let deadline = self
            .shared
            .shutdown
            .lock()
            .ok()
            .and_then(|operation| operation.as_ref().map(|operation| operation.deadline))
            .unwrap_or_else(Instant::now);
        for session in &self.retired {
            if let Err(error) = session.request_close_until(deadline) {
                eprintln!("zmux shared session cancellation: {error}");
            }
        }
        self.downloads.begin_shutdown();
        let mut flush_submitted = false;
        let mut connections = None;
        let mut connections_done = false;
        let mut expiry_recorded = false;
        loop {
            for _ in 0..super::MAX_COMMANDS {
                match receiver.try_recv() {
                    Ok(envelope) => self.command(envelope),
                    Err(_) => break,
                }
            }
            while let Ok(mut completion) = self.completion_rx.try_recv() {
                if let Some(session) = completion.cleanup.take() {
                    self.retire(session);
                }
                self.pending_launches = self.pending_launches.saturating_sub(1);
                if let Ok(session) = completion.result {
                    self.retire(session);
                }
            }
            self.reap_launch_workers();
            self.local_tmux.drain();
            self.poll_transfers();
            let mut active = Vec::new();
            for mut session in std::mem::take(&mut self.retired) {
                if session.is_finished() {
                    if let Err(error) = session.shutdown(Duration::ZERO) {
                        self.shutdown_issue(error.to_string());
                    }
                } else {
                    active.push(session);
                }
            }
            self.retired = active;
            for session in &self.retired {
                if let Err(error) = session.request_close_until(deadline) {
                    eprintln!("zmux shared session cancellation: {error}");
                }
            }
            let profiles_done = match self.profiles.poll_shutdown() {
                Ok(done) => done,
                Err(error) => {
                    self.shutdown_issue(error.to_string());
                    true
                }
            };
            let persistence_done = self.poll_shutdown_persistence(&mut flush_submitted, deadline);
            let (downloads_done, native_downloads) = match self.downloads.poll_shutdown() {
                Ok(state) => state,
                Err(error) => {
                    self.shutdown_issue(error);
                    (true, self.downloads.physical_count())
                }
            };
            if self.pending_launches == 0
                && self.retired.is_empty()
                && self.launch_workers.is_empty()
                && self.transfers_finished()
                && connections.is_none()
                && !connections_done
            {
                match zmux_remote_runtime::connection::request_cleanup_barrier() {
                    Ok(receiver) => connections = Some(receiver),
                    Err(error) => {
                        self.shutdown_issue(error.to_string()); /* No physical receipt: retain pending. */
                    }
                }
            }
            if let Some(receiver) = &connections
                && !connections_done
            {
                match receiver.try_recv() {
                    Ok(report) => {
                        connections_done = true;
                        for issue in report.issues {
                            self.shutdown_issue(format!("process-wide SSH cleanup: {issue}"));
                        }
                        if report.issues_dropped > 0 {
                            self.shutdown_issue(format!(
                                "{} additional process-wide SSH cleanup failures",
                                report.issues_dropped
                            ));
                        }
                    }
                    Err(TryRecvError::Empty) => (),
                    Err(TryRecvError::Disconnected) => {
                        self.shutdown_issue(
                            "SSH cleanup barrier disconnected; physical cleanup unconfirmed".into(),
                        );
                    }
                }
            }
            if let Some(error) = self.cleanup_error.clone() {
                self.shutdown_issue(error);
            }
            let pending = Pending {
                sessions: self.retired.len() + self.pending_launches,
                launches: self.pending_launches.max(self.launch_workers.len()),
                tmux: self.local_tmux.worker_count(),
                transfers: usize::from(!self.transfers_finished()),
                profiles: usize::from(!profiles_done),
                persistence: usize::from(!persistence_done),
                downloads: usize::from(!downloads_done),
                connections: usize::from(!connections_done),
                native_downloads,
                native_cleanup: false,
                control: 0,
            };
            let done = if let Ok(mut operation) = self.shared.shutdown.lock() {
                let operation = operation.as_mut().expect("shutdown installed");
                let native_cleanup = operation.report.pending.native_cleanup;
                operation.report.pending = Pending {
                    native_cleanup,
                    ..pending
                };
                operation.report.phase = Phase::Draining;
                operation.report.runtime_cleanup_complete = operation.report.pending.runtime_done();
                let expired = Instant::now() >= operation.deadline;
                operation.report.deadline_expired |= expired;
                if expired && !expiry_recorded {
                    operation.issue("shutdown deadline expired; physical cleanup remains owned and is still being observed".into());
                    expiry_recorded = true;
                }
                operation.report.runtime_cleanup_complete
                    && native_downloads == 0
                    && !native_cleanup
            } else {
                false
            };
            self.publish(done);
            if done {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}
