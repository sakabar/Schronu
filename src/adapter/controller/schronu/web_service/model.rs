use serde::{Deserialize, Serialize};

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
    pub is_leaf: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebSuccess<T> {
    pub snapshot: ServerSnapshot,
    pub data: T,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiscardedSessionTaskTotalDto {
    pub task_id: String,
    pub task_name: String,
    pub total_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiscardedSessionEventDto {
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
pub struct DiscardedSessionDayDto {
    pub logical_date: String,
    pub total_seconds: i64,
    pub task_totals: Vec<DiscardedSessionTaskTotalDto>,
    pub events: Vec<DiscardedSessionEventDto>,
}
