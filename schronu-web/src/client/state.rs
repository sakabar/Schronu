mod results;

use super::date_buttons::LogicalDateButton;
pub use super::effect::ClientEffect;
pub use super::history::{Locality, Operation, OperationHistoryEntry, Outcome};
use super::work_sessions::{KeyValueStorage, WorkSession, WorkSessionsState};
use crate::{
    ListTasksRequest, RecordSessionRequest, ScheduledTaskRow, ServerSnapshot, SessionTask, WebError,
};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveTab {
    Session,
    List,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayError {
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerFailure {
    Operation(WebError),
    Transport(String),
}

pub struct ClientState {
    active_tab: ActiveTab,
    work_sessions: WorkSessionsState,
    snapshot: Option<ServerSnapshot>,
    date_buttons: Vec<LogicalDateButton>,
    selected_logical_date: Option<String>,
    scheduled_rows: Vec<ScheduledTaskRow>,
    in_flight_task_ids: HashSet<String>,
    manual_check_blocked_task_ids: HashSet<String>,
    committed_blocked_task_ids: HashSet<String>,
    committed_actual_work_seconds: HashMap<String, i64>,
    mutation_globally_blocked: bool,
    display_error: Option<DisplayError>,
    history: VecDeque<OperationHistoryEntry>,
    tick_now_epoch_ms: i64,
    auto_session_empty: bool,
}

impl ClientState {
    pub fn new(work_sessions: WorkSessionsState, tick_now_epoch_ms: i64) -> Self {
        Self {
            active_tab: ActiveTab::Session,
            work_sessions,
            snapshot: None,
            date_buttons: Vec::new(),
            selected_logical_date: None,
            scheduled_rows: Vec::new(),
            in_flight_task_ids: HashSet::new(),
            manual_check_blocked_task_ids: HashSet::new(),
            committed_blocked_task_ids: HashSet::new(),
            committed_actual_work_seconds: HashMap::new(),
            mutation_globally_blocked: false,
            display_error: None,
            history: VecDeque::new(),
            tick_now_epoch_ms,
            auto_session_empty: false,
        }
    }

    pub fn active_tab(&self) -> ActiveTab {
        self.active_tab
    }

    pub fn sessions(&self) -> &[WorkSession] {
        self.work_sessions.sessions()
    }

    pub fn snapshot(&self) -> Option<&ServerSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn date_buttons(&self) -> &[LogicalDateButton] {
        &self.date_buttons
    }

    pub fn selected_logical_date(&self) -> Option<&str> {
        self.selected_logical_date.as_deref()
    }

    pub fn scheduled_rows(&self) -> &[ScheduledTaskRow] {
        &self.scheduled_rows
    }

    pub fn display_error(&self) -> Option<&DisplayError> {
        self.display_error.as_ref()
    }

    pub fn history(&self) -> &VecDeque<OperationHistoryEntry> {
        &self.history
    }

    pub fn tick_now_epoch_ms(&self) -> i64 {
        self.tick_now_epoch_ms
    }

    pub fn auto_session_empty(&self) -> bool {
        self.auto_session_empty
    }

    pub fn is_session_in_flight(&self, task_id: &str) -> bool {
        self.in_flight_task_ids.contains(task_id)
    }

    pub fn is_session_manual_check_blocked(&self, task_id: &str) -> bool {
        self.manual_check_blocked_task_ids.contains(task_id)
    }

    pub fn is_session_committed_blocked(&self, task_id: &str) -> bool {
        self.committed_blocked_task_ids.contains(task_id)
    }

    pub fn display_actual_work_seconds(&self, task_id: &str) -> Option<i64> {
        self.committed_actual_work_seconds
            .get(task_id)
            .copied()
            .or_else(|| {
                self.sessions()
                    .iter()
                    .find(|session| session.task_id == task_id)
                    .map(|session| session.actual_work_seconds_at_start)
            })
    }

    pub fn switch_tab(&mut self, tab: ActiveTab) -> ClientEffect {
        self.active_tab = tab;
        ClientEffect::None
    }

    pub fn tick(&mut self, now_epoch_ms: i64) -> ClientEffect {
        self.tick_now_epoch_ms = now_epoch_ms;
        ClientEffect::None
    }

    pub fn request_bootstrap(&self) -> ClientEffect {
        ClientEffect::Bootstrap
    }

    pub fn request_list(&self, logical_date: &str) -> ClientEffect {
        ClientEffect::ListTasks(ListTasksRequest {
            logical_date: logical_date.to_owned(),
        })
    }

    pub fn request_auto_session(&self) -> ClientEffect {
        ClientEffect::AutoSession
    }

    pub fn add_session_from_row<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        row: &ScheduledTaskRow,
    ) -> ClientEffect {
        self.add_session(storage, &row.task);
        ClientEffect::None
    }

    pub fn discard_session<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
    ) -> ClientEffect {
        if self.in_flight_task_ids.contains(task_id)
            || self.committed_blocked_task_ids.contains(task_id)
        {
            return ClientEffect::None;
        }
        let candidate: Vec<_> = self
            .sessions()
            .iter()
            .filter(|session| session.task_id != task_id)
            .cloned()
            .collect();
        let found = candidate.len() != self.sessions().len();
        let result = found
            .then(|| self.work_sessions.replace_sessions(storage, candidate))
            .transpose()
            .map(|_| ());
        self.record_local_result(Operation::DiscardSession, Some(task_id), result.is_ok());
        ClientEffect::None
    }

    pub fn begin_record_session(&mut self, task_id: &str) -> ClientEffect {
        self.begin_mutation(task_id, false)
    }

    pub fn begin_complete_session(&mut self, task_id: &str) -> ClientEffect {
        self.begin_mutation(task_id, true)
    }

    fn add_session<S: KeyValueStorage>(&mut self, storage: &S, task: &SessionTask) {
        if self
            .sessions()
            .iter()
            .any(|session| session.task_id == task.task_id)
        {
            self.record_local_result(Operation::AddSession, Some(&task.task_id), false);
            return;
        }
        let mut candidate = self.sessions().to_vec();
        candidate.push(WorkSession {
            task_id: task.task_id.clone(),
            task_name: task.task_name.clone(),
            started_at_epoch_ms: self.tick_now_epoch_ms,
            estimated_work_seconds_at_start: task.estimated_work_seconds,
            actual_work_seconds_at_start: task.actual_work_seconds,
        });
        let result = self.work_sessions.replace_sessions(storage, candidate);
        self.record_local_result(Operation::AddSession, Some(&task.task_id), result.is_ok());
    }

    fn begin_mutation(&mut self, task_id: &str, complete: bool) -> ClientEffect {
        if self.mutation_globally_blocked
            || self.in_flight_task_ids.contains(task_id)
            || self.manual_check_blocked_task_ids.contains(task_id)
            || self.committed_blocked_task_ids.contains(task_id)
        {
            return ClientEffect::None;
        }
        let Some(session) = self
            .sessions()
            .iter()
            .find(|session| session.task_id == task_id)
        else {
            return ClientEffect::None;
        };
        let request = RecordSessionRequest {
            task_id: session.task_id.clone(),
            started_at_epoch_ms: session.started_at_epoch_ms,
            expected_actual_work_seconds: session.actual_work_seconds_at_start,
        };
        self.in_flight_task_ids.insert(task_id.to_owned());
        if complete {
            ClientEffect::CompleteSession(request)
        } else {
            ClientEffect::RecordSession(request)
        }
    }
}
