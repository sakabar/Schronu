use super::history_view::HistoryEntryViewModel;
use super::list_view::DateButtonViewModel;
use crate::client::state::{ActiveTab, ClientState, Locality, Operation, Outcome};
use crate::client::view_projection::{
    project_list_rows_for_browser, project_session_cards_for_browser, ListRowViewModel,
    SessionCardViewModel,
};

pub(crate) struct BrowserPageModel {
    pub active_tab: ActiveTab,
    pub tick_now_epoch_ms: i64,
    pub buffer: Option<i128>,
    pub sessions: Vec<SessionCardViewModel>,
    pub rows: Vec<ListRowViewModel>,
    pub active_task_ids: Vec<String>,
    pub dates: Vec<DateButtonViewModel>,
    pub history: Vec<HistoryEntryViewModel>,
    pub warnings: Vec<String>,
    pub safety_warning: Option<&'static str>,
    pub display_error: Option<String>,
    pub global_blocked: bool,
    pub can_confirm: bool,
    pub auto_session_in_flight: bool,
    pub auto_session_empty: bool,
}

impl BrowserPageModel {
    pub fn from_state(state: &ClientState) -> Self {
        let tick_now_epoch_ms = state.tick_now_epoch_ms();
        Self {
            active_tab: state.active_tab(),
            tick_now_epoch_ms,
            buffer: state.display_buffer_seconds(),
            sessions: project_session_cards_for_browser(state),
            rows: project_list_rows_for_browser(state),
            active_task_ids: state
                .sessions()
                .iter()
                .map(|session| session.task_id.clone())
                .collect(),
            dates: state
                .date_buttons()
                .iter()
                .map(|date| DateButtonViewModel {
                    logical_date: date.logical_date.clone(),
                    label: date.label.clone(),
                    selected: state.selected_logical_date() == Some(date.logical_date.as_str()),
                })
                .collect(),
            history: history_view_models(state),
            warnings: state.storage_warnings().to_vec(),
            safety_warning: state.mutation_safety_warning(),
            display_error: state
                .display_error()
                .map(|error| error.message().to_owned()),
            global_blocked: state.mutation_globally_blocked(),
            can_confirm: state.can_confirm_repository_checked(),
            auto_session_in_flight: state.auto_session_in_flight(),
            auto_session_empty: state.auto_session_empty(),
        }
    }
}

pub(crate) fn browser_now_epoch_ms() -> i64 {
    js_sys::Date::now() as i64
}

fn history_view_models(state: &ClientState) -> Vec<HistoryEntryViewModel> {
    state
        .history()
        .iter()
        .rev()
        .map(|entry| HistoryEntryViewModel {
            occurred_at_hh_mm_ss: browser_hh_mm_ss(entry.occurred_at_epoch_ms),
            operation: operation_label(entry.operation).to_owned(),
            task_id: entry.task_id.clone(),
            locality: match entry.locality {
                Locality::Local => "local",
                Locality::Server => "server",
            }
            .to_owned(),
            outcome: match entry.outcome {
                Outcome::Success => "success",
                Outcome::Failure => "failure",
            }
            .to_owned(),
            summary: entry.summary.clone(),
            failed: entry.outcome == Outcome::Failure,
        })
        .collect()
}

fn operation_label(operation: Operation) -> &'static str {
    match operation {
        Operation::Bootstrap => "bootstrap",
        Operation::ListTasks => "list_tasks",
        Operation::AutoSession => "auto_session",
        Operation::AddSession => "add_session",
        Operation::DiscardSession => "discard_session",
        Operation::RecordSession => "record_session",
        Operation::CompleteSession => "complete_session",
        Operation::CompleteSessionWithoutRecording => "complete_session_without_recording",
        Operation::ConfirmRepositoryCheck => "confirm_repository_check",
    }
}

fn browser_hh_mm_ss(epoch_ms: i64) -> String {
    let date = js_sys::Date::new_0();
    date.set_time(epoch_ms as f64);
    if !date.get_time().is_finite() {
        return "--:--:--".to_owned();
    }
    format!(
        "{:02}:{:02}:{:02}",
        date.get_hours(),
        date.get_minutes(),
        date.get_seconds()
    )
}
