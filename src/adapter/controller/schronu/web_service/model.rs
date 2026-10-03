use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

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
pub struct ScheduledTaskRowDto {
    pub task: SessionTaskDto,
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
    pub defer_plan: DeferPlanDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AllTaskRowDto {
    pub task: SessionTaskDto,
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
    pub weekly_average_seconds: i64,
    pub occurrence_day_count: usize,
    pub peak_date: NaiveDate,
    pub peak_work_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoutineLoadReportDto {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub rows: Vec<RoutineLoadRowDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LoadDataDto {
    pub band_days: Vec<BandDayDto>,
    pub routine_load: RoutineLoadReportDto,
}
