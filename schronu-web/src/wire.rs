use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ServerSnapshot {
    pub observed_at_epoch_ms: i64,
    pub logical_date: String,
    pub buffer_seconds: i64,
}

pub type CompleteSessionResponse = ServerSnapshot;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionTask {
    pub task_id: String,
    pub task_name: String,
    pub estimated_work_seconds: i64,
    pub actual_work_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScheduledTaskRow {
    pub task: SessionTask,
    pub schedule_start_epoch_ms: i64,
    pub schedule_end_epoch_ms: i64,
    pub deadline_epoch_ms: Option<i64>,
    pub deadline_label: String,
    pub misses_deadline: bool,
    pub is_leaf: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebSuccess<T> {
    pub snapshot: ServerSnapshot,
    pub data: T,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListTasksRequest {
    pub logical_date: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecordSessionRequest {
    pub task_id: String,
    pub started_at_epoch_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_epoch_ms: Option<i64>,
    pub expected_actual_work_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompleteSessionRequest {
    pub task_id: String,
    pub started_at_epoch_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_epoch_ms: Option<i64>,
    pub expected_actual_work_seconds: i64,
    pub record_elapsed_seconds: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discard_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_name_at_start: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiscardSessionRequest {
    pub event_id: String,
    pub task_id: String,
    pub task_name_at_start: String,
    pub started_at_epoch_ms: i64,
    pub ended_at_epoch_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListDiscardedSessionsRequest {
    pub logical_date: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiscardedSessionTaskTotal {
    pub task_id: String,
    pub task_name: String,
    pub total_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiscardedSessionEvent {
    pub event_id: String,
    pub task_id: String,
    pub task_name_at_start: String,
    pub started_at_epoch_ms: i64,
    pub ended_at_epoch_ms: i64,
    pub elapsed_seconds: i64,
    pub source: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiscardedSessionDay {
    pub logical_date: String,
    pub total_seconds: i64,
    pub task_totals: Vec<DiscardedSessionTaskTotal>,
    pub events: Vec<DiscardedSessionEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecordSessionResult {
    pub actual_work_seconds: i64,
}

pub mod web_error_codes {
    pub const INVALID_INPUT: &str = "invalid_input";
    pub const TASK_NOT_FOUND: &str = "task_not_found";
    pub const TASK_ALREADY_COMPLETED: &str = "task_already_completed";
    pub const ACTUAL_WORK_CONFLICT: &str = "actual_work_conflict";
    pub const ARITHMETIC_OVERFLOW: &str = "arithmetic_overflow";
    pub const TASK_NOT_COMPLETABLE: &str = "task_not_completable";
    pub const CONFIGURATION_ERROR: &str = "configuration_error";
    pub const REPOSITORY_UNAVAILABLE: &str = "repository_unavailable";
    pub const OPERATION_FAILED: &str = "operation_failed";
    pub const WORKER_UNAVAILABLE: &str = "worker_unavailable";
    pub const REPOSITORY_SAVE_FAILED: &str = "repository_save_failed";
    pub const REPOSITORY_STATE_UNCERTAIN: &str = "repository_state_uncertain";
    pub const DISCARDED_SESSION_CONFLICT: &str = "discarded_session_conflict";
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryAdvice {
    Retry,
    ManualCheck,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebError {
    pub code: String,
    pub message: String,
    pub retry_advice: RetryAdvice,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_actual_work_seconds: Option<i64>,
}
