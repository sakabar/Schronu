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
        let attr = task.try_get_attr()?;
        let root_attr = task.try_root()?.try_get_attr()?;
        let root_id = *root_attr.get_id();
        let parent_id = task
            .try_parent()?
            .map(|parent| parent.try_get_attr().map(|attr| *attr.get_id()))
            .transpose()?;
        let child_ids = task
            .try_get_children()?
            .into_iter()
            .map(|child| child.try_get_attr().map(|attr| *attr.get_id()))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            id: *attr.get_id(),
            root_id,
            parent_id,
            child_ids,
            name: attr.get_name().to_string(),
            status: *attr.get_status(),
            original_status: *attr.get_orig_status(),
            is_on_other_side: *attr.get_is_on_other_side(),
            atomic: attr.get_atomic(),
            pending_until: (*attr.get_orig_status() == Status::Pending)
                .then(|| *attr.get_pending_until()),
            priority: root_attr.get_priority(),
            create_time: *attr.get_create_time(),
            start_time: *attr.get_start_time(),
            end_time: *attr.get_end_time_opt(),
            deadline_time: *attr.get_deadline_time_opt(),
            estimated_work_seconds: attr.get_estimated_work_seconds(),
            actual_work_seconds: attr.get_actual_work_seconds(),
            repetition_interval_days: attr.get_repetition_interval_days_opt(),
            repetition_anchor: attr.get_repetition_anchor(),
            days_in_advance: attr.get_days_in_advance(),
            project_category: root_attr.get_project_category_opt(),
        })
    }
}
