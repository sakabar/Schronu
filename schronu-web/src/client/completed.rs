use crate::{CompletedTaskRow, TaskDisplayKind};
use serde::{Deserialize, Serialize};

/// A split has an operation ID locally; its task ID is assigned by the server.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum CompletionId {
    Task(String),
    OfflineSplit(String),
}

impl CompletionId {
    #[cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Task(id) | Self::OfflineSplit(id) => id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct CompletionRow {
    pub id: CompletionId,
    pub task_name: String,
    pub project_name: Option<String>,
    pub task_display_kind: Option<TaskDisplayKind>,
    pub completed_at_epoch_ms: i64,
    pub actual_work_seconds: Option<i64>,
    pub estimated_work_seconds: Option<i64>,
}

impl From<CompletedTaskRow> for CompletionRow {
    fn from(row: CompletedTaskRow) -> Self {
        Self {
            id: CompletionId::Task(row.task_id),
            task_name: row.task_name,
            project_name: Some(row.project_name),
            task_display_kind: Some(row.task_display_kind),
            completed_at_epoch_ms: row.completed_at_epoch_ms,
            actual_work_seconds: Some(row.actual_work_seconds),
            estimated_work_seconds: Some(row.estimated_work_seconds),
        }
    }
}
