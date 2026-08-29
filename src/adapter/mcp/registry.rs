use super::input::{
    generated_input_schema, BreakdownTaskInput, CompleteTaskInput, CreateTaskInput,
    DeferRoutineTaskInput, DeferTaskInput, GetFocusInput, GetScheduleInput, GetTaskInput,
    ListTasksInput, UpdateTaskInput,
};
use serde_json::{json, Value};

pub(super) fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "get_focus",
            "description": "Return the task Schronu currently recommends working on in the task field; task is null when no task is available.",
            "inputSchema": generated_input_schema::<GetFocusInput>()
        }),
        json!({
            "name": "get_task",
            "description": "Return an existing task by its UUID.",
            "inputSchema": generated_input_schema::<GetTaskInput>()
        }),
        json!({
            "name": "list_tasks",
            "description": "List tasks in project-tree pre-order, optionally filtered by period, effective status, and category. Different filters are combined using AND.",
            "inputSchema": generated_input_schema::<ListTasksInput>()
        }),
        json!({
            "name": "get_schedule",
            "description": "Return calculated schedule segments whose intervals overlap the selected range of local logical days. Logical days start at 06:00 local time.",
            "inputSchema": generated_input_schema::<GetScheduleInput>()
        }),
        json!({
            "name": "create_task",
            "description": "Create a new root project task. Its original status is Todo when pending_until is omitted and Pending when provided; if that time is not in the future, its effective status can be Todo.",
            "inputSchema": generated_input_schema::<CreateTaskInput>()
        }),
        json!({
            "name": "breakdown_task",
            "description": "Add child tasks to an existing parent task in the order supplied. Each child inherits the parent's deadline when present. Its original status is Todo when pending_until is omitted and Pending when provided; its effective status can be Todo if that time is not in the future, and deadline policy can move pending_until earlier.",
            "inputSchema": generated_input_schema::<BreakdownTaskInput>()
        }),
        json!({
            "name": "defer_task",
            "description": "Set an existing task's pending-until time and original status to Pending. Its effective status can be Todo when the time is not in the future, and the existing deadline policy can move the pending-until time earlier.",
            "inputSchema": generated_input_schema::<DeferTaskInput>()
        }),
        json!({
            "name": "defer_routine_task",
            "description": "Move an existing routine task to its next repetition cycle by shifting its deadline and start time, then restore its original status to Todo. Its effective status can be Pending when the shifted start time is in the future. The task must have a deadline and a parent whose repetition interval is set.",
            "inputSchema": generated_input_schema::<DeferRoutineTaskInput>()
        }),
        json!({
            "name": "complete_task",
            "description": "Complete an existing task that has no unfinished children. This records an end time and can add work seconds; completing a child of a routine parent can create the next occurrence and adjust the parent's estimate. This operation is not idempotent: calling it again for an already completed task adds the requested work again and can create another routine occurrence.",
            "inputSchema": generated_input_schema::<CompleteTaskInput>()
        }),
        json!({
            "name": "update_task",
            "description": "Update an existing task's estimate, deadline, or project category. Include at least one update field; when multiple fields are supplied, they are applied in estimate, deadline, then category order.",
            "inputSchema": generated_input_schema::<UpdateTaskInput>()
        }),
    ]
}
