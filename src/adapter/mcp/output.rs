use crate::application::schedule_use_case::ScheduledTaskView;
use crate::application::task_use_case::TaskView;
use serde_json::{json, Value};
use uuid::Uuid;

pub(super) fn task_view_json(task: &TaskView) -> Value {
    json!({
        "id": task.id.to_string(),
        "root_id": task.root_id.to_string(),
        "parent_id": task.parent_id.map(|id| id.to_string()),
        "child_ids": task.child_ids.iter().map(Uuid::to_string).collect::<Vec<_>>(),
        "name": task.name,
        "status": task.status.to_string(),
        "original_status": task.original_status.to_string(),
        "is_on_other_side": task.is_on_other_side,
        "atomic": task.atomic,
        "pending_until": task.pending_until.map(|time| time.to_rfc3339()),
        "priority": task.priority,
        "create_time": task.create_time.to_rfc3339(),
        "start_time": task.start_time.to_rfc3339(),
        "end_time": task.end_time.map(|time| time.to_rfc3339()),
        "deadline_time": task.deadline_time.map(|time| time.to_rfc3339()),
        "estimated_work_seconds": task.estimated_work_seconds,
        "actual_work_seconds": task.actual_work_seconds,
        "repetition_interval_days": task.repetition_interval_days,
        "repetition_anchor": task.repetition_anchor.to_string(),
        "days_in_advance": task.days_in_advance,
        "project_category": task.project_category.map(|category| category.to_string())
    })
}

pub(super) fn scheduled_task_view_json(scheduled: &ScheduledTaskView) -> Value {
    json!({
        "task": task_view_json(&scheduled.task),
        "first_available_time": scheduled.first_available_time.to_rfc3339(),
        "scheduled_start": scheduled.scheduled_start.to_rfc3339(),
        "scheduled_end": scheduled.scheduled_end.to_rfc3339(),
        "scheduled_work_seconds": scheduled.scheduled_work_seconds,
        "total_work_seconds": scheduled.total_work_seconds,
        "rank": scheduled.rank
    })
}
