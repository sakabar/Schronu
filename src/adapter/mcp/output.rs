use crate::application::schedule_use_case::ScheduleOccurrenceKey;
use crate::application::schedule_use_case::ScheduledTaskView;
use crate::application::task_use_case::TaskView;
use serde_json::Value;

pub(super) fn task_view_json(task: &TaskView) -> Value {
    serde_json::to_value(task).expect("TaskView serialization is infallible")
}

pub(super) fn scheduled_task_view_json(
    scheduled: &ScheduledTaskView,
    actual_task: Option<&TaskView>,
) -> Value {
    let mut value =
        serde_json::to_value(scheduled).expect("ScheduledTaskView serialization is infallible");
    let object = value
        .as_object_mut()
        .expect("ScheduledTaskView serializes as an object");
    match scheduled.occurrence {
        ScheduleOccurrenceKey::Actual { task_id } => {
            let task = actual_task.expect("actual schedule output requires its stored task");
            object.insert("task".to_string(), task_view_json(task));
            object.insert("task_id".to_string(), Value::String(task_id.to_string()));
        }
        ScheduleOccurrenceKey::Projected { source_task_id, .. } => {
            object.insert(
                "occurrence_key".to_string(),
                serde_json::to_value(scheduled.occurrence)
                    .expect("ScheduleOccurrenceKey serialization is infallible"),
            );
            object.insert(
                "source_task_id".to_string(),
                Value::String(source_task_id.to_string()),
            );
        }
    }
    value
}
