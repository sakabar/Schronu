use super::date_buttons::logical_date_buttons;
use super::state::ActiveTab;
use super::work_sessions::{KeyValueStorage, StorageError};
use crate::{ScheduledTaskRow, ServerSnapshot};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

pub const VIEW_STATE_STORAGE_KEY: &str = "schronu_web.view_state.v1";
const STORAGE_VERSION: u64 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoredListView {
    pub logical_date: String,
    pub rows: Vec<ScheduledTaskRow>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewState {
    pub snapshot: ServerSnapshot,
    pub list: Option<StoredListView>,
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
    let parsed = serde_json::from_str::<StoredViewState>(&raw)
        .ok()
        .filter(|stored| stored.version == STORAGE_VERSION)
        .filter(|stored| valid_view_state(&stored.state));
    match parsed {
        Some(stored) => loaded(Some(stored.state), None),
        None => loaded(
            None,
            Some("前回の画面状態が不正なため復元しませんでした。".to_owned()),
        ),
    }
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
        && state.list.as_ref().is_none_or(|list| {
            valid_logical_date(&list.logical_date) && list.rows.iter().all(valid_row)
        })
}

fn valid_snapshot(snapshot: &ServerSnapshot) -> bool {
    DateTime::from_timestamp_millis(snapshot.observed_at_epoch_ms).is_some()
        && valid_logical_date(&snapshot.logical_date)
}

fn valid_logical_date(logical_date: &str) -> bool {
    logical_date_buttons(logical_date).is_ok()
}

fn valid_row(row: &ScheduledTaskRow) -> bool {
    Uuid::parse_str(&row.task.task_id).is_ok()
        && !row.task.task_name.trim().is_empty()
        && row.task.estimated_work_seconds >= 0
        && row.task.actual_work_seconds >= 0
        && valid_epoch(row.schedule_start_epoch_ms)
        && valid_epoch(row.schedule_end_epoch_ms)
        && row.schedule_start_epoch_ms <= row.schedule_end_epoch_ms
        && row.deadline_epoch_ms.is_none_or(valid_epoch)
}

fn valid_epoch(epoch_ms: i64) -> bool {
    DateTime::from_timestamp_millis(epoch_ms).is_some()
}
