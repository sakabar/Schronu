use chrono::DateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ServerSnapshot {
    pub observed_at_epoch_ms: i64,
    pub logical_date: String,
    pub buffer_seconds: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BandDurations {
    pub unavailable_seconds: i64,
    pub elapsed_seconds: i64,
    pub repetitive_seconds: i64,
    pub non_repetitive_seconds: i64,
    pub rho_leeway_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BandDay {
    pub logical_date: String,
    pub accumulated_rho_diff_seconds: i64,
    pub accumulated_free_diff_seconds: i64,
    pub durations: BandDurations,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoutineLoadRow {
    pub project_task_id: String,
    pub project_name: String,
    pub routine_task_id: String,
    pub routine_name: String,
    pub repetition_interval_days: i64,
    pub total_work_seconds: i64,
    pub occurrence_day_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average_work_seconds: Option<i64>,
    pub peak_date: String,
    pub peak_work_seconds: i64,
}

impl RoutineLoadRow {
    pub fn display_average_work_seconds(&self) -> Option<i64> {
        if self.average_work_seconds.is_some() {
            return self.average_work_seconds;
        }
        let occurrence_day_count = i64::try_from(self.occurrence_day_count).ok()?;
        self.total_work_seconds.checked_div(occurrence_day_count)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoutineLoadReport {
    pub start_date: String,
    pub end_date: String,
    #[serde(default = "legacy_routine_load_horizon_day_count")]
    pub horizon_day_count: u64,
    pub rows: Vec<RoutineLoadRow>,
}

fn legacy_routine_load_horizon_day_count() -> u64 {
    8
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LoadData {
    pub band_days: Vec<BandDay>,
    pub routine_load: RoutineLoadReport,
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
pub struct ScheduledTask {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub task_name: String,
    pub estimated_work_seconds: i64,
    pub actual_work_seconds: i64,
}

impl ScheduledTask {
    pub fn actionable_task(&self) -> Option<SessionTask> {
        Some(SessionTask {
            task_id: self.task_id.clone()?,
            task_name: self.task_name.clone(),
            estimated_work_seconds: self.estimated_work_seconds,
            actual_work_seconds: self.actual_work_seconds,
        })
    }
}

impl From<SessionTask> for ScheduledTask {
    fn from(task: SessionTask) -> Self {
        Self {
            task_id: Some(task.task_id),
            task_name: task.task_name,
            estimated_work_seconds: task.estimated_work_seconds,
            actual_work_seconds: task.actual_work_seconds,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleOccurrence {
    Actual {
        task_id: String,
    },
    Projected {
        occurrence_key: String,
        source_task_id: String,
    },
    #[default]
    LegacyActual,
}

impl ScheduleOccurrence {
    pub fn is_projected(&self) -> bool {
        matches!(self, Self::Projected { .. })
    }

    pub fn occurrence_key(&self) -> Option<&str> {
        match self {
            Self::Projected { occurrence_key, .. } => Some(occurrence_key),
            Self::Actual { .. } | Self::LegacyActual => None,
        }
    }

    pub fn source_task_id(&self) -> Option<&str> {
        match self {
            Self::Projected { source_task_id, .. } => Some(source_task_id),
            Self::Actual { .. } | Self::LegacyActual => None,
        }
    }

    pub(crate) fn projected_identity(&self) -> Option<(Uuid, i64)> {
        let Self::Projected {
            occurrence_key,
            source_task_id,
        } = self
        else {
            return None;
        };
        let source_uuid = Uuid::parse_str(source_task_id).ok()?;
        if source_uuid.hyphenated().to_string() != *source_task_id {
            return None;
        }
        let (key_source, deadline_text) = occurrence_key.split_once(':')?;
        if key_source != source_task_id {
            return None;
        }
        let deadline_epoch_ms = deadline_text.parse::<i64>().ok()?;
        if deadline_epoch_ms.to_string() != deadline_text {
            return None;
        }
        DateTime::from_timestamp_millis(deadline_epoch_ms)?;
        Some((source_uuid, deadline_epoch_ms))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDisplayKind {
    Fixed,
    Repetitive,
    #[default]
    NonRepetitive,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeadlineDisplayKind {
    #[default]
    None,
    Overrun,
    Today,
    Future,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScheduledTaskRow {
    pub task: ScheduledTask,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_task_name: Option<String>,
    #[serde(default)]
    pub occurrence: ScheduleOccurrence,
    pub schedule_start_epoch_ms: i64,
    pub schedule_end_epoch_ms: i64,
    pub deadline_epoch_ms: Option<i64>,
    pub deadline_label: String,
    pub misses_deadline: bool,
    #[serde(default)]
    pub task_display_kind: TaskDisplayKind,
    #[serde(default)]
    pub deadline_display_kind: DeadlineDisplayKind,
    pub is_leaf: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defer_plan: Option<DeferPlan>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompletedTaskRow {
    pub task_id: String,
    pub task_name: String,
    pub project_name: String,
    pub completed_at_epoch_ms: i64,
    pub actual_work_seconds: i64,
    pub estimated_work_seconds: i64,
    #[serde(default)]
    pub task_display_kind: TaskDisplayKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompletedTaskReport {
    pub rows: Vec<CompletedTaskRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_progress_actual_work_seconds: Option<i64>,
    pub total_actual_work_seconds: i64,
    pub available_seconds: i64,
    pub recorded_percentage: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AllTaskRow {
    pub task: ScheduledTask,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_task_name: Option<String>,
    #[serde(default)]
    pub occurrence: ScheduleOccurrence,
    pub segment_index: usize,
    pub schedule_date: String,
    pub deadline_epoch_ms: Option<i64>,
    pub deadline_label: String,
    pub misses_deadline: bool,
    #[serde(default)]
    pub task_display_kind: TaskDisplayKind,
    #[serde(default)]
    pub deadline_display_kind: DeadlineDisplayKind,
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
pub struct ListCompletedTasksRequest {
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
            }
            .into(),
            display_task_name: None,
            occurrence: ScheduleOccurrence::Actual {
                task_id: "task".to_owned(),
            },
            segment_index: 0,
            schedule_date: "2026-09-05".to_owned(),
            deadline_epoch_ms: None,
            deadline_label: "____/__/__".to_owned(),
            misses_deadline: false,
            task_display_kind: TaskDisplayKind::NonRepetitive,
            deadline_display_kind: DeadlineDisplayKind::None,
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

    #[test]
    fn routine_load_averageは発生日数zeroで値を返さない() {
        let row = RoutineLoadRow {
            project_task_id: "project".to_owned(),
            project_name: "生活".to_owned(),
            routine_task_id: "routine".to_owned(),
            routine_name: "繰返".to_owned(),
            repetition_interval_days: 7,
            total_work_seconds: 60,
            occurrence_day_count: 0,
            average_work_seconds: None,
            peak_date: "2026-10-03".to_owned(),
            peak_work_seconds: 60,
        };

        assert_eq!(row.display_average_work_seconds(), None);
    }

    #[test]
    fn routine_load_averageは輸送済み値をlegacy再計算より優先する() {
        let row = RoutineLoadRow {
            project_task_id: "project".to_owned(),
            project_name: "生活".to_owned(),
            routine_task_id: "routine".to_owned(),
            routine_name: "繰返".to_owned(),
            repetition_interval_days: 7,
            total_work_seconds: 600,
            occurrence_day_count: 2,
            average_work_seconds: Some(240),
            peak_date: "2026-10-03".to_owned(),
            peak_work_seconds: 300,
        };

        assert_eq!(row.display_average_work_seconds(), Some(240));
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
