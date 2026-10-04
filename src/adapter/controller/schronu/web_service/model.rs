use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeferModeDto {
    Normal,
    DeadlineLimited,
    RoutinePeriod,
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
pub struct DeferPlanDto {
    pub mode: DeferModeDto,
    pub requested_pending_until_epoch_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_pending_until_epoch_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repetition_interval_days: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ServerSnapshot {
    pub observed_at_epoch_ms: i64,
    pub logical_date: String,
    pub buffer_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionTaskDto {
    pub task_id: String,
    pub task_name: String,
    pub estimated_work_seconds: i64,
    pub actual_work_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScheduledTaskDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub task_name: String,
    pub estimated_work_seconds: i64,
    pub actual_work_seconds: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleOccurrenceDto {
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

impl ScheduleOccurrenceDto {
    #[cfg(test)]
    pub fn source_task_id(&self) -> Option<uuid::Uuid> {
        match self {
            Self::Projected { source_task_id, .. } => uuid::Uuid::parse_str(source_task_id).ok(),
            Self::Actual { .. } | Self::LegacyActual => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScheduledTaskRowDto {
    pub task: ScheduledTaskDto,
    #[serde(default)]
    pub occurrence: ScheduleOccurrenceDto,
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
    pub defer_plan: Option<DeferPlanDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompletedTaskRowDto {
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
pub struct CompletedTaskReportDto {
    pub rows: Vec<CompletedTaskRowDto>,
    pub total_actual_work_seconds: i64,
    pub available_seconds: i64,
    pub recorded_percentage: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AllTaskRowDto {
    pub task: ScheduledTaskDto,
    #[serde(default)]
    pub occurrence: ScheduleOccurrenceDto,
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
pub struct AllTaskPageDto {
    pub rows: Vec<AllTaskRowDto>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebSuccess<T> {
    pub snapshot: ServerSnapshot,
    pub data: T,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BandDurationsDto {
    pub unavailable_seconds: i64,
    pub elapsed_seconds: i64,
    pub repetitive_seconds: i64,
    pub non_repetitive_seconds: i64,
    pub rho_leeway_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BandDayDto {
    pub logical_date: NaiveDate,
    pub accumulated_rho_diff_seconds: i64,
    pub accumulated_free_diff_seconds: i64,
    pub durations: BandDurationsDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoutineLoadRowDto {
    pub project_task_id: String,
    pub project_name: String,
    pub routine_task_id: String,
    pub routine_name: String,
    pub repetition_interval_days: i64,
    pub total_work_seconds: i64,
    pub occurrence_day_count: usize,
    pub average_work_seconds: i64,
    pub peak_date: NaiveDate,
    pub peak_work_seconds: i64,
    #[serde(default)]
    pub work_seconds_by_date: BTreeMap<NaiveDate, i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoutineLoadReportDto {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    #[serde(default = "legacy_routine_load_horizon_day_count")]
    pub horizon_day_count: u64,
    #[serde(default)]
    pub full_day_available_seconds_by_date: BTreeMap<NaiveDate, i64>,
    pub rows: Vec<RoutineLoadRowDto>,
}

fn legacy_routine_load_horizon_day_count() -> u64 {
    8
}

#[cfg(test)]
mod tests {
    use super::RoutineLoadReportDto;
    use serde_json::json;

    #[test]
    fn 旧繰返負荷payloadは日別mapを空として復元する() {
        let report: RoutineLoadReportDto = serde_json::from_value(json!({
            "start_date": "2026-10-04",
            "end_date": "2026-10-11",
            "rows": [{
                "project_task_id": "00000000-0000-0000-0000-000000000001",
                "project_name": "project",
                "routine_task_id": "00000000-0000-0000-0000-000000000002",
                "routine_name": "routine",
                "repetition_interval_days": 7,
                "total_work_seconds": 600,
                "occurrence_day_count": 1,
                "average_work_seconds": 600,
                "peak_date": "2026-10-04",
                "peak_work_seconds": 600
            }]
        }))
        .unwrap();

        assert_eq!(report.horizon_day_count, 8);
        assert!(report.full_day_available_seconds_by_date.is_empty());
        assert!(report.rows[0].work_seconds_by_date.is_empty());
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LoadDataDto {
    pub band_days: Vec<BandDayDto>,
    pub routine_load: RoutineLoadReportDto,
}
