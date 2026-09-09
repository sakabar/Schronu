mod diagnostics;
mod read_state;
mod session_state;

use super::carry_lock::{load_carry_lock, CarryLockMode, CarryLockState};
use super::date_buttons::LogicalDateButton;
pub use super::effect::ClientEffect;
pub use super::history::{Operation, OperationHistoryEntry, Outcome, ServerActionInvocation};
use super::safety_state::{load_mutation_safety, MutationSafetyState};
use super::time_model::{buffer_timing_with_sessions, session_timing};
use super::work_sessions::{
    load_work_sessions, unavailable_state, KeyValueStorage, StorageError, WorkSession,
    WorkSessionsState,
};
use crate::{ScheduledTaskRow, ServerSnapshot, WebError};
use diagnostics::DiagnosticsState;
pub use diagnostics::DisplayError;
use read_state::ReadState;
use serde::{Deserialize, Serialize};
use session_state::SessionState;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveTab {
    Session,
    List,
    History,
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
    carry_lock: CarryLockState,
    tick_now_epoch_ms: i64,
}

impl ClientState {
    fn new(
        work_sessions: WorkSessionsState,
        mutation_safety: MutationSafetyState,
        carry_lock: CarryLockState,
        tick_now_epoch_ms: i64,
    ) -> Self {
        Self {
            active_tab: ActiveTab::Session,
            read: ReadState::new(),
            sessions: SessionState::new(work_sessions, mutation_safety),
            diagnostics: DiagnosticsState::new(),
            carry_lock,
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

    pub fn all_storage_warnings(&self) -> Vec<String> {
        self.storage_warnings()
            .iter()
            .cloned()
            .chain(self.carry_lock.warning().map(str::to_owned))
            .collect()
    }

    pub fn carry_lock_mode(&self) -> CarryLockMode {
        self.carry_lock.mode()
    }

    pub fn carry_lock_locked(&self) -> bool {
        self.carry_lock.mode() == CarryLockMode::Locked
    }

    pub fn enable_carry_lock<S: KeyValueStorage>(&mut self, storage: &S) -> ClientEffect {
        self.carry_lock.enable(storage);
        ClientEffect::None
    }

    pub fn arm_carry_lock(&mut self, monotonic_now_ms: u64) -> ClientEffect {
        self.carry_lock.arm(monotonic_now_ms);
        ClientEffect::None
    }

    pub fn relock_carry_lock(&mut self) -> ClientEffect {
        self.carry_lock.relock();
        ClientEffect::None
    }

    pub fn disable_carry_lock<S: KeyValueStorage>(&mut self, storage: &S) -> ClientEffect {
        self.carry_lock.disable(storage);
        ClientEffect::None
    }

    #[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
    pub(crate) fn observe_carry_lock_time(&mut self, monotonic_now_ms: u64) {
        self.carry_lock.observe_monotonic_time(monotonic_now_ms);
    }

    #[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
    pub(crate) fn authorize_carry_lock_mutation(&mut self, monotonic_now_ms: u64) -> bool {
        self.carry_lock.authorize_mutation(monotonic_now_ms)
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

    pub(crate) fn session_stopped_at_epoch_ms(&self, task_id: &str) -> Option<i64> {
        self.sessions
            .pending_mutations
            .values()
            .find(|pending| pending.invocation.task_id() == Some(task_id))
            .map(|pending| pending.ended_at_epoch_ms)
            .or_else(|| {
                self.sessions
                    .completion_conflicts
                    .get(task_id)
                    .map(|conflict| conflict.ended_at_epoch_ms)
            })
            .or_else(|| {
                self.sessions
                    .uncertain_stopped_at_epoch_ms
                    .get(task_id)
                    .copied()
            })
    }

    pub(crate) fn completion_conflict(
        &self,
        task_id: &str,
    ) -> Option<&session_state::CompletionConflict> {
        self.sessions.completion_conflicts.get(task_id)
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

    fn buffer_protected_intervals(&self, session: &WorkSession) -> Vec<(i64, Option<i64>)> {
        let estimated_completion_epoch_ms = session_timing(
            session.started_at_epoch_ms,
            session.estimated_work_seconds_at_start,
            session.actual_work_seconds_at_start,
            session.started_at_epoch_ms,
        )
        .estimated_completion_epoch_ms;
        if let Some(conflict) = self.sessions.completion_conflicts.get(&session.task_id) {
            if estimated_completion_epoch_ms.is_none_or(|estimated_completion| {
                conflict.ended_at_epoch_ms <= estimated_completion
            }) {
                return vec![(session.started_at_epoch_ms, None)];
            }
            return vec![
                (session.started_at_epoch_ms, estimated_completion_epoch_ms),
                (conflict.ended_at_epoch_ms, None),
            ];
        }
        let stopped_at_epoch_ms = self.session_stopped_at_epoch_ms(&session.task_id);
        let protected_until_epoch_ms = match (estimated_completion_epoch_ms, stopped_at_epoch_ms) {
            (Some(estimated_completion), Some(stopped_at)) => {
                Some(estimated_completion.min(stopped_at))
            }
            (Some(estimated_completion), None) => Some(estimated_completion),
            (None, stopped_at) => stopped_at,
        };
        vec![(session.started_at_epoch_ms, protected_until_epoch_ms)]
    }

    pub fn display_buffer_seconds(&self) -> Option<i128> {
        let snapshot = self.snapshot()?;
        let session_intervals: Vec<_> = self
            .sessions()
            .iter()
            .filter(|session| !self.is_session_committed_blocked(&session.task_id))
            .flat_map(|session| self.buffer_protected_intervals(session))
            .collect();
        let timing = buffer_timing_with_sessions(
            snapshot.observed_at_epoch_ms,
            snapshot.buffer_seconds,
            self.tick_now_epoch_ms,
            &session_intervals,
        );
        Some(timing.display_buffer_seconds)
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
        load_carry_lock(storage),
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
    ClientState::new(
        work_sessions,
        mutation_safety,
        load_carry_lock(storage),
        tick_now_epoch_ms,
    )
}
