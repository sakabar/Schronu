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
            "description": "Get the task that should be worked on now.",
            "inputSchema": generated_input_schema::<GetFocusInput>()
        }),
        json!({
            "name": "get_task",
            "description": "Get one task by UUID.",
            "inputSchema": generated_input_schema::<GetTaskInput>()
        }),
        json!({
            "name": "list_tasks",
            "description": "List tasks filtered by period, status, and category.",
            "inputSchema": generated_input_schema::<ListTasksInput>()
        }),
        json!({
            "name": "get_schedule",
            "description": "Get Schronu's calculated task schedule for a date range.",
            "inputSchema": generated_input_schema::<GetScheduleInput>()
        }),
        json!({
            "name": "create_task",
            "description": "Create a new root project task.",
            "inputSchema": generated_input_schema::<CreateTaskInput>()
        }),
        json!({
            "name": "breakdown_task",
            "description": "Add child tasks to an existing task.",
            "inputSchema": generated_input_schema::<BreakdownTaskInput>()
        }),
        json!({
            "name": "defer_task",
            "description": "Defer a task until an absolute date and time.",
            "inputSchema": generated_input_schema::<DeferTaskInput>()
        }),
        json!({
            "name": "defer_routine_task",
            "description": "Defer a routine task to its next repetition cycle.",
            "inputSchema": generated_input_schema::<DeferRoutineTaskInput>()
        }),
        json!({
            "name": "complete_task",
            "description": "Complete a task, optionally recording finish time and work seconds.",
            "inputSchema": generated_input_schema::<CompleteTaskInput>()
        }),
        json!({
            "name": "update_task",
            "description": "Update a task's estimate, deadline, or category.",
            "inputSchema": generated_input_schema::<UpdateTaskInput>()
        }),
    ]
}
