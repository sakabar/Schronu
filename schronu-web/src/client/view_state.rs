use super::date_buttons::logical_date_buttons_for_mode;
use super::state::{ActiveTab, ListMode};
use super::work_sessions::{KeyValueStorage, StorageError};
use crate::{
    CompletedTaskRow, DeferMode, DeferPlan, ScheduleOccurrence, ScheduledTaskRow, ServerSnapshot,
};
use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

pub const VIEW_STATE_STORAGE_KEY: &str = "schronu_web.view_state.v1";
const STORAGE_VERSION: u64 = 3;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoredListView {
    pub logical_date: String,
    pub rows: Vec<ScheduledTaskRow>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StoredActiveList {
    Scheduled {
        logical_date: String,
        rows: Vec<ScheduledTaskRow>,
    },
    Completed {
        logical_date: String,
        rows: Vec<CompletedTaskRow>,
        total_actual_work_seconds: i64,
        available_seconds: i64,
        recorded_percentage: Option<i64>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewState {
    pub snapshot: ServerSnapshot,
    pub list_mode: ListMode,
    pub list: Option<StoredActiveList>,
    pub active_tab: ActiveTab,
    pub task_name_filter: String,
    pub date_input_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedViewState {
    state: Option<ViewState>,
    warning: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewStateStoreError {
    InvalidState,
    SerializationFailed,
    Storage(StorageError),
}

impl LoadedViewState {
    pub fn state(&self) -> Option<&ViewState> {
        self.state.as_ref()
    }

    pub fn into_state(self) -> Option<ViewState> {
        self.state
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredViewState {
    version: u64,
    #[serde(flatten)]
    state: ViewState,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredViewStateV2 {
    version: u64,
    snapshot: ServerSnapshot,
    list: Option<StoredListView>,
    active_tab: ActiveTab,
    task_name_filter: String,
    date_input_text: String,
}

pub fn load_view_state<S: KeyValueStorage>(storage: &S) -> LoadedViewState {
    let raw = match storage.get(VIEW_STATE_STORAGE_KEY) {
        Ok(Some(raw)) => raw,
        Ok(None) => return loaded(None, None),
        Err(_) => {
            return loaded(
                None,
                Some("前回の画面状態を読み取れませんでした。".to_owned()),
            );
        }
    };
    let version = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|value| value.get("version").and_then(serde_json::Value::as_u64));
    let state = match version {
        Some(STORAGE_VERSION) => serde_json::from_str::<StoredViewState>(&raw)
            .ok()
            .filter(|stored| valid_view_state(&stored.state))
            .map(|stored| stored.state),
        Some(2) => serde_json::from_str::<StoredViewStateV2>(&raw)
            .ok()
            .filter(|stored| stored.version == 2)
            .map(|stored| ViewState {
                snapshot: stored.snapshot,
                list_mode: ListMode::Scheduled,
                list: stored.list.map(|list| StoredActiveList::Scheduled {
                    logical_date: list.logical_date,
                    rows: list.rows,
                }),
                active_tab: stored.active_tab,
                task_name_filter: stored.task_name_filter,
                date_input_text: stored.date_input_text,
            })
            .filter(valid_view_state),
        _ => None,
    };
    state.map_or_else(
        || {
            loaded(
                None,
                Some("前回の画面状態が不正なため復元しませんでした。".to_owned()),
            )
        },
        |state| loaded(Some(state), None),
    )
}

pub fn store_view_state<S: KeyValueStorage>(
    storage: &S,
    state: &ViewState,
) -> Result<(), ViewStateStoreError> {
    if !valid_view_state(state) {
        return Err(ViewStateStoreError::InvalidState);
    }
    let serialized = serde_json::to_string(&StoredViewState {
        version: STORAGE_VERSION,
        state: state.clone(),
    })
    .map_err(|_| ViewStateStoreError::SerializationFailed)?;
    storage
        .set(VIEW_STATE_STORAGE_KEY, &serialized)
        .map_err(ViewStateStoreError::Storage)
}

impl fmt::Display for ViewStateStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState => formatter.write_str("画面状態の内容が不正です。"),
            Self::SerializationFailed => {
                formatter.write_str("画面状態の保存形式を生成できません。")
            }
            Self::Storage(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ViewStateStoreError {}

fn loaded(state: Option<ViewState>, warning: Option<String>) -> LoadedViewState {
    LoadedViewState { state, warning }
}

fn valid_view_state(state: &ViewState) -> bool {
    valid_snapshot(&state.snapshot)
        && logical_date_buttons_for_mode(&state.snapshot.logical_date, state.list_mode).is_ok()
        && state.list.as_ref().is_none_or(|list| match list {
            StoredActiveList::Scheduled { logical_date, rows } => {
                state.list_mode == ListMode::Scheduled
                    && valid_logical_date(logical_date)
                    && rows.iter().all(valid_row)
            }
            StoredActiveList::Completed {
                logical_date,
                rows,
                total_actual_work_seconds,
                available_seconds,
                recorded_percentage,
            } => {
                state.list_mode == ListMode::Completed
                    && valid_logical_date(logical_date)
                    && rows.iter().all(valid_completed_row)
                    && valid_completed_summary(
                        rows,
                        *total_actual_work_seconds,
                        *available_seconds,
                        *recorded_percentage,
                    )
            }
        })
}

fn valid_snapshot(snapshot: &ServerSnapshot) -> bool {
    DateTime::from_timestamp_millis(snapshot.observed_at_epoch_ms).is_some()
        && valid_logical_date(&snapshot.logical_date)
}

fn valid_logical_date(logical_date: &str) -> bool {
    NaiveDate::parse_from_str(logical_date, "%Y-%m-%d")
        .is_ok_and(|date| date.format("%Y-%m-%d").to_string() == logical_date)
}

fn valid_row(row: &ScheduledTaskRow) -> bool {
    valid_occurrence(row)
        && !row.task.task_name.trim().is_empty()
        && row.task.estimated_work_seconds >= 0
        && row.task.actual_work_seconds >= 0
        && valid_epoch(row.schedule_start_epoch_ms)
        && valid_epoch(row.schedule_end_epoch_ms)
        && row.schedule_start_epoch_ms <= row.schedule_end_epoch_ms
        && row.deadline_epoch_ms.is_none_or(valid_epoch)
        && row.defer_plan.as_ref().is_none_or(valid_defer_plan)
}

fn valid_occurrence(row: &ScheduledTaskRow) -> bool {
    match &row.occurrence {
        ScheduleOccurrence::Actual { task_id } => {
            row.task.task_id.as_deref() == Some(task_id.as_str())
                && Uuid::parse_str(task_id).is_ok()
                && row.defer_plan.is_some()
        }
        ScheduleOccurrence::Projected {
            occurrence_key: _,
            source_task_id: _,
        } => {
            row.task.task_id.is_none()
                && row.defer_plan.is_none()
                && row
                    .occurrence
                    .projected_identity()
                    .is_some_and(|(_, deadline_epoch_ms)| {
                        row.deadline_epoch_ms == Some(deadline_epoch_ms)
                    })
        }
        ScheduleOccurrence::LegacyActual => {
            row.task
                .task_id
                .as_deref()
                .is_some_and(|task_id| Uuid::parse_str(task_id).is_ok())
                && row.defer_plan.is_some()
        }
    }
}

fn valid_defer_plan(defer_plan: &DeferPlan) -> bool {
    valid_epoch(defer_plan.requested_pending_until_epoch_ms)
        && match defer_plan.mode {
            DeferMode::Normal => {
                defer_plan.effective_pending_until_epoch_ms.is_none()
                    && defer_plan.repetition_interval_days.is_none()
            }
            DeferMode::DeadlineLimited => {
                defer_plan
                    .effective_pending_until_epoch_ms
                    .is_some_and(|effective| {
                        valid_epoch(effective)
                            && effective < defer_plan.requested_pending_until_epoch_ms
                    })
                    && defer_plan.repetition_interval_days.is_none()
            }
            DeferMode::RoutinePeriod => {
                defer_plan.effective_pending_until_epoch_ms.is_none()
                    && defer_plan
                        .repetition_interval_days
                        .is_some_and(|days| days > 0)
            }
        }
}

fn valid_completed_row(row: &CompletedTaskRow) -> bool {
    Uuid::parse_str(&row.task_id).is_ok()
        && !row.task_name.trim().is_empty()
        && !row.project_name.trim().is_empty()
        && row.actual_work_seconds >= 0
        && row.estimated_work_seconds >= 0
        && valid_epoch(row.completed_at_epoch_ms)
}

fn valid_completed_summary(
    rows: &[CompletedTaskRow],
    total_actual_work_seconds: i64,
    available_seconds: i64,
    recorded_percentage: Option<i64>,
) -> bool {
    if total_actual_work_seconds < 0 || available_seconds < 0 {
        return false;
    }
    let row_total = rows
        .iter()
        .map(|row| i128::from(row.actual_work_seconds))
        .sum::<i128>();
    if row_total != i128::from(total_actual_work_seconds) {
        return false;
    }
    let expected_percentage = if available_seconds == 0 {
        None
    } else {
        let total = i128::from(total_actual_work_seconds);
        let available = i128::from(available_seconds);
        i64::try_from((total * 100 + available / 2) / available).ok()
    };
    recorded_percentage == expected_percentage
}

fn valid_epoch(epoch_ms: i64) -> bool {
    DateTime::from_timestamp_millis(epoch_ms).is_some()
}
