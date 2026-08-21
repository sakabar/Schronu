use crate::application::schedule_use_case::ScheduledTaskView;
use crate::application::task_use_case::TaskView;
use serde_json::Value;

pub(super) fn task_view_json(task: &TaskView) -> Value {
    serde_json::to_value(task).expect("TaskView serialization is infallible")
}

pub(super) fn scheduled_task_view_json(scheduled: &ScheduledTaskView) -> Value {
    serde_json::to_value(scheduled).expect("ScheduledTaskView serialization is infallible")
}
