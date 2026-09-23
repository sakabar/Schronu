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
    pub defer_plan: DeferPlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AllTaskRow {
    pub task: SessionTask,
    pub segment_index: usize,
    pub schedule_date: String,
    pub deadline_epoch_ms: Option<i64>,
    pub deadline_label: String,
    pub misses_deadline: bool,
    pub is_leaf: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AllTaskPage {
    pub rows: Vec<AllTaskRow>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeferMode {
    Normal,
    DeadlineLimited,
    RoutinePeriod,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeferPlan {
    pub mode: DeferMode,
    pub requested_pending_until_epoch_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_pending_until_epoch_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repetition_interval_days: Option<i64>,
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
pub struct ListAllTasksRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[cfg(test)]
mod all_task_contract_tests {
    use super::*;

    #[test]
    fn all_task_wireはcursorと必須日付をjsonで保持する() {
        let request = ListAllTasksRequest {
            cursor: Some("00000000-0000-4000-8000-000000000001:500".to_owned()),
        };
        let encoded = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<ListAllTasksRequest>(&encoded).unwrap(),
            request
        );

        let row = AllTaskRow {
            task: SessionTask {
                task_id: "task".to_owned(),
                task_name: "name".to_owned(),
                estimated_work_seconds: 1,
                actual_work_seconds: 0,
            },
            segment_index: 0,
            schedule_date: "2026-09-05".to_owned(),
            deadline_epoch_ms: None,
            deadline_label: "____/__/__".to_owned(),
            misses_deadline: false,
            is_leaf: true,
        };
        let page = AllTaskPage {
            rows: vec![row],
            next_cursor: None,
        };
        let decoded: AllTaskPage =
            serde_json::from_str(&serde_json::to_string(&page).unwrap()).unwrap();
        assert_eq!(decoded, page);
        assert_eq!(web_error_codes::INVALID_CURSOR, "invalid_cursor");
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeferTaskRequest {
    pub task_id: String,
    pub selected_logical_date: String,
    pub expected_plan: DeferPlan,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecordSessionResult {
    pub actual_work_seconds: i64,
}

pub mod web_error_codes {
    pub const INVALID_CURSOR: &str = "invalid_cursor";
    pub const INVALID_INPUT: &str = "invalid_input";
    pub const TASK_NOT_FOUND: &str = "task_not_found";
    pub const TASK_ALREADY_COMPLETED: &str = "task_already_completed";
    pub const ACTUAL_WORK_CONFLICT: &str = "actual_work_conflict";
    pub const DEFER_PLAN_CHANGED: &str = "defer_plan_changed";
    pub const ARITHMETIC_OVERFLOW: &str = "arithmetic_overflow";
    pub const TASK_NOT_COMPLETABLE: &str = "task_not_completable";
    pub const CONFIGURATION_ERROR: &str = "configuration_error";
    pub const REPOSITORY_UNAVAILABLE: &str = "repository_unavailable";
    pub const OPERATION_FAILED: &str = "operation_failed";
    pub const WORKER_UNAVAILABLE: &str = "worker_unavailable";
    pub const REPOSITORY_SAVE_FAILED: &str = "repository_save_failed";
    pub const REPOSITORY_STATE_UNCERTAIN: &str = "repository_state_uncertain";
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
