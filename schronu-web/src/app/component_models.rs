use super::carry_lock_view::CarryLockViewModel;
use super::component_runtime::project_date_button_models;
use super::history_view::HistoryEntryViewModel;
use super::list_view::DateButtonViewModel;
use crate::client::state::{ActiveTab, AllTasksStatus, ClientState, ListSelection, Outcome};
use crate::client::view_projection::{
    project_list_rows_for_browser, project_session_cards_for_browser,
    project_visible_all_task_rows, ListRowViewModel, SessionCardViewModel,
};

pub(crate) struct BrowserPageModel {
    pub active_tab: ActiveTab,
    pub buffer: Option<i128>,
    pub sessions: Vec<SessionCardViewModel>,
    pub rows: Vec<ListRowViewModel>,
    pub has_more_rows: bool,
    pub list_selection: ListSelection,
    pub all_tasks_status: AllTasksStatus,
    pub all_tasks_failure: Option<String>,
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
    pub carry_lock: CarryLockViewModel,
}

impl BrowserPageModel {
    #[cfg(test)]
    pub fn from_state(state: &ClientState) -> Self {
        Self::from_state_at(state, 0, "", 500)
    }

    pub fn from_state_at(
        state: &ClientState,
        monotonic_now_ms: u64,
        all_task_filter: &str,
        all_tasks_visible_limit: usize,
    ) -> Self {
        let (rows, has_more_rows) = if state.list_selection() == ListSelection::All {
            state
                .all_task_rows()
                .map(|rows| {
                    let projection = project_visible_all_task_rows(
                        rows,
                        all_task_filter,
                        all_tasks_visible_limit,
                    );
                    (projection.rows, projection.has_more)
                })
                .unwrap_or_default()
        } else {
            (project_list_rows_for_browser(state), false)
        };
        Self {
            active_tab: state.active_tab(),
            buffer: state.display_buffer_seconds(),
            sessions: project_session_cards_for_browser(state),
            rows,
            has_more_rows,
            list_selection: state.list_selection(),
            all_tasks_status: state.all_tasks_status(),
            all_tasks_failure: state.all_tasks_failure_message().map(str::to_owned),
            active_task_ids: state
                .sessions()
                .iter()
                .map(|session| session.task_id.clone())
                .collect(),
            dates: project_date_button_models(state),
            history: history_view_models(state),
            warnings: state.all_storage_warnings(),
            safety_warning: state.mutation_safety_warning(),
            display_error: state
                .display_error()
                .map(|error| error.message().to_owned()),
            global_blocked: state.mutation_globally_blocked(),
            can_confirm: state.can_confirm_repository_checked(),
            auto_session_in_flight: state.auto_session_in_flight(),
            auto_session_empty: state.auto_session_empty(),
            carry_lock: CarryLockViewModel::new(state.carry_lock_mode(), monotonic_now_ms),
        }
    }
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub(crate) fn browser_now_epoch_ms() -> i64 {
    js_sys::Date::now() as i64
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub(crate) fn browser_monotonic_now_ms() -> u64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now() as u64)
        .expect("browser Performance API must be available")
}

fn history_view_models(state: &ClientState) -> Vec<HistoryEntryViewModel> {
    state
        .history()
        .iter()
        .rev()
        .map(|entry| HistoryEntryViewModel {
            occurred_at_hh_mm_ss: browser_hh_mm_ss(entry.occurred_at_epoch_ms),
            invocation: entry.invocation.to_string(),
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
