mod diagnostics;
mod read_state;
mod session_state;

use super::date_buttons::LogicalDateButton;
pub use super::effect::ClientEffect;
pub use super::history::{Locality, Operation, OperationHistoryEntry, Outcome};
use super::safety_state::{load_mutation_safety, MutationSafetyState};
use super::time_model::buffer_timing;
use super::work_sessions::{
    load_work_sessions, unavailable_state, KeyValueStorage, StorageError, WorkSession,
    WorkSessionsState,
};
use crate::{ScheduledTaskRow, ServerSnapshot, WebError};
use diagnostics::DiagnosticsState;
pub use diagnostics::DisplayError;
use read_state::ReadState;
use session_state::SessionState;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveTab {
    Session,
    List,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerFailure {
    Operation(WebError),
    Transport(String),
}

pub struct ClientState {
    active_tab: ActiveTab,
    read: ReadState,
    sessions: SessionState,
    diagnostics: DiagnosticsState,
    tick_now_epoch_ms: i64,
}

impl ClientState {
    fn new(
        work_sessions: WorkSessionsState,
        mutation_safety: MutationSafetyState,
        tick_now_epoch_ms: i64,
    ) -> Self {
        Self {
            active_tab: ActiveTab::Session,
            read: ReadState::new(),
            sessions: SessionState::new(work_sessions, mutation_safety),
            diagnostics: DiagnosticsState::new(),
            tick_now_epoch_ms,
        }
    }

    pub fn active_tab(&self) -> ActiveTab {
        self.active_tab
    }

    pub fn sessions(&self) -> &[WorkSession] {
        self.sessions.work_sessions.sessions()
    }

    pub fn storage_warnings(&self) -> &[String] {
        self.sessions.work_sessions.warnings()
    }

    pub fn storage_write_blocked(&self) -> bool {
        self.sessions.work_sessions.write_blocked()
    }

    pub fn snapshot(&self) -> Option<&ServerSnapshot> {
        self.read.snapshot.as_ref()
    }

    pub fn date_buttons(&self) -> &[LogicalDateButton] {
        &self.read.date_buttons
    }

    pub fn selected_logical_date(&self) -> Option<&str> {
        self.read.selected_logical_date.as_deref()
    }

    pub fn scheduled_rows(&self) -> &[ScheduledTaskRow] {
        &self.read.scheduled_rows
    }

    pub fn display_error(&self) -> Option<&DisplayError> {
        self.diagnostics.display_error.as_ref()
    }

    pub fn history(&self) -> &VecDeque<OperationHistoryEntry> {
        &self.diagnostics.history
    }

    pub fn tick_now_epoch_ms(&self) -> i64 {
        self.tick_now_epoch_ms
    }

    pub fn auto_session_empty(&self) -> bool {
        self.read.auto_session_empty
    }

    pub fn auto_session_in_flight(&self) -> bool {
        self.read.auto_session_in_flight
    }

    pub fn is_session_in_flight(&self, task_id: &str) -> bool {
        self.sessions.in_flight_task_ids.contains(task_id)
    }

    pub fn is_session_manual_check_blocked(&self, task_id: &str) -> bool {
        self.sessions
            .manual_check_blocked_task_ids
            .contains(task_id)
    }

    pub fn is_session_committed_blocked(&self, task_id: &str) -> bool {
        self.sessions.committed_blocked_task_ids.contains(task_id)
    }

    pub fn mutation_globally_blocked(&self) -> bool {
        self.sessions.mutation_globally_blocked
    }

    pub fn mutation_safety_warning(&self) -> Option<&'static str> {
        self.sessions.mutation_globally_blocked.then_some(
            "repositoryの状態を手動確認するまで、セッションの記録と完了は停止されています。",
        )
    }

    pub fn can_confirm_repository_checked(&self) -> bool {
        self.sessions.mutation_globally_blocked && self.sessions.pending_mutations.is_empty()
    }

    pub fn display_actual_work_seconds(&self, task_id: &str) -> Option<i64> {
        self.sessions
            .committed_actual_work_seconds
            .get(task_id)
            .copied()
            .or_else(|| {
                self.sessions()
                    .iter()
                    .find(|session| session.task_id == task_id)
                    .map(|session| session.actual_work_seconds_at_start)
            })
    }

    pub fn display_buffer_seconds(&self) -> Option<i128> {
        let snapshot = self.snapshot()?;
        let active_session_starts: Vec<_> = self
            .sessions()
            .iter()
            .filter(|session| !self.is_session_committed_blocked(&session.task_id))
            .map(|session| session.started_at_epoch_ms)
            .collect();
        Some(
            buffer_timing(
                snapshot.observed_at_epoch_ms,
                snapshot.buffer_seconds,
                self.tick_now_epoch_ms,
                &active_session_starts,
            )
            .display_buffer_seconds,
        )
    }

    pub fn switch_tab(&mut self, tab: ActiveTab) -> ClientEffect {
        self.active_tab = tab;
        ClientEffect::None
    }

    pub fn tick(&mut self, now_epoch_ms: i64) -> ClientEffect {
        self.tick_now_epoch_ms = now_epoch_ms;
        ClientEffect::None
    }
}

pub fn load_client_state<S: KeyValueStorage>(
    storage: &S,
    tick_now_epoch_ms: i64,
) -> Result<ClientState, StorageError> {
    Ok(ClientState::new(
        load_work_sessions(storage)?,
        load_mutation_safety(storage)?,
        tick_now_epoch_ms,
    ))
}

pub fn load_client_state_for_ui<S: KeyValueStorage>(
    storage: &S,
    tick_now_epoch_ms: i64,
) -> ClientState {
    let work_sessions = load_work_sessions(storage).unwrap_or_else(|_| unavailable_state());
    let mutation_safety =
        load_mutation_safety(storage).unwrap_or_else(|_| MutationSafetyState::blocked());
    ClientState::new(work_sessions, mutation_safety, tick_now_epoch_ms)
}
