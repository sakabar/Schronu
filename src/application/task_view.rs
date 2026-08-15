use crate::entity::task::{ProjectCategory, RepetitionAnchor, Status, TaskHandle, TaskTreeError};
use chrono::{DateTime, Local};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq)]
pub struct TaskView {
    pub id: Uuid,
    pub root_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub child_ids: Vec<Uuid>,
    pub name: String,
    pub status: Status,
    pub original_status: Status,
    pub is_on_other_side: bool,
    pub atomic: bool,
    pub pending_until: Option<DateTime<Local>>,
    pub priority: i64,
    pub create_time: DateTime<Local>,
    pub start_time: DateTime<Local>,
    pub end_time: Option<DateTime<Local>>,
    pub deadline_time: Option<DateTime<Local>>,
    pub estimated_work_seconds: i64,
    pub actual_work_seconds: i64,
    pub repetition_interval_days: Option<i64>,
    pub repetition_anchor: RepetitionAnchor,
    pub days_in_advance: i64,
    pub project_category: Option<ProjectCategory>,
}

impl TryFrom<&TaskHandle> for TaskView {
    type Error = TaskTreeError;

    fn try_from(task: &TaskHandle) -> Result<Self, Self::Error> {
        Ok(Self {
            id: task.get_id(),
            root_id: task.root().get_id(),
            parent_id: task.parent().map(|parent| parent.get_id()),
            child_ids: task.get_children().iter().map(TaskHandle::get_id).collect(),
            name: task.get_name()?,
            status: task.get_status(),
            original_status: task.get_orig_status(),
            is_on_other_side: task.get_is_on_other_side(),
            atomic: task.get_atomic(),
            pending_until: (task.get_orig_status() == Status::Pending)
                .then(|| task.get_pending_until()),
            priority: task.get_priority(),
            create_time: task.get_create_time(),
            start_time: task.get_start_time(),
            end_time: task.get_end_time_opt(),
            deadline_time: task.get_deadline_time_opt(),
            estimated_work_seconds: task.get_estimated_work_seconds(),
            actual_work_seconds: task.get_actual_work_seconds(),
            repetition_interval_days: task.get_repetition_interval_days_opt(),
            repetition_anchor: task.get_repetition_anchor(),
            days_in_advance: task.get_days_in_advance(),
            project_category: task.get_project_category_opt(),
        })
    }
}
