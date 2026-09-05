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
pub enum DisplayError {
    Operation(WebError),
    Transport,
    LocalStorage { committed_on_server: bool },
}

impl DisplayError {
    pub fn message(&self) -> &str {
        match self {
            Self::Operation(error) => &error.message,
            Self::Transport => "通信に失敗しました。時間をおいて再試行してください。",
            Self::LocalStorage {
                committed_on_server: true,
            } => "serverでは保存済みですが、localStorageの更新に失敗しました。再送せず状態を確認してください。",
            Self::LocalStorage {
                committed_on_server: false,
            } => "localStorageを更新できませんでした。",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Transport
                | Self::Operation(WebError {
                    retry_advice: crate::RetryAdvice::Retry,
                    ..
                })
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerFailure {
    Operation(WebError),
    Transport(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MutationKind {
    Record,
    Complete,
}

struct PendingMutation {
    task_id: String,
    kind: MutationKind,
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
    next_read_request_id: u64,
    latest_bootstrap_request_id: Option<u64>,
    latest_list_request_id: Option<u64>,
    latest_auto_request_id: Option<u64>,
    next_mutation_request_id: u64,
    pending_mutations: HashMap<u64, PendingMutation>,
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
            next_read_request_id: 1,
            latest_bootstrap_request_id: None,
            latest_list_request_id: None,
            latest_auto_request_id: None,
            next_mutation_request_id: 1,
            pending_mutations: HashMap::new(),
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

    pub fn request_bootstrap(&mut self) -> ClientEffect {
        let Some(request_id) = self.next_read_request_id() else {
            return ClientEffect::None;
        };
        self.latest_bootstrap_request_id = Some(request_id);
        ClientEffect::Bootstrap { request_id }
    }

    pub fn request_list(&mut self, logical_date: &str) -> ClientEffect {
        let Some(request_id) = self.next_read_request_id() else {
            return ClientEffect::None;
        };
        self.latest_list_request_id = Some(request_id);
        ClientEffect::ListTasks {
            request_id,
            request: ListTasksRequest {
                logical_date: logical_date.to_owned(),
            },
        }
    }

    pub fn request_auto_session(&mut self) -> ClientEffect {
        let Some(request_id) = self.next_read_request_id() else {
            return ClientEffect::None;
        };
        self.latest_auto_request_id = Some(request_id);
        ClientEffect::AutoSession { request_id }
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
        if found && result.is_ok() {
            self.manual_check_blocked_task_ids.remove(task_id);
        }
        self.record_local_result(Operation::DiscardSession, Some(task_id), result.is_ok());
        ClientEffect::None
    }

    pub fn begin_record_session(&mut self, task_id: &str) -> ClientEffect {
        self.begin_mutation(task_id, MutationKind::Record)
    }

    pub fn begin_complete_session(&mut self, task_id: &str) -> ClientEffect {
        self.begin_mutation(task_id, MutationKind::Complete)
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

    fn begin_mutation(&mut self, task_id: &str, kind: MutationKind) -> ClientEffect {
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
        let request_id = self.next_mutation_request_id;
        let Some(next_request_id) = request_id.checked_add(1) else {
            return ClientEffect::None;
        };
        self.next_mutation_request_id = next_request_id;
        self.in_flight_task_ids.insert(task_id.to_owned());
        self.pending_mutations.insert(
            request_id,
            PendingMutation {
                task_id: task_id.to_owned(),
                kind,
            },
        );
        match kind {
            MutationKind::Complete => ClientEffect::CompleteSession {
                request_id,
                request,
            },
            MutationKind::Record => ClientEffect::RecordSession {
                request_id,
                request,
            },
        }
    }

    fn take_pending_mutation(&mut self, request_id: u64, kind: MutationKind) -> Option<String> {
        let pending = self.pending_mutations.get(&request_id)?;
        if pending.kind != kind {
            return None;
        }
        let task_id = pending.task_id.clone();
        self.pending_mutations.remove(&request_id);
        Some(task_id)
    }

    fn next_read_request_id(&mut self) -> Option<u64> {
        let request_id = self.next_read_request_id;
        self.next_read_request_id = request_id.checked_add(1)?;
        Some(request_id)
    }
}
