use super::input::{
    generated_input_schema, GetFocusInput, GetScheduleInput, GetTaskInput, ListTasksInput,
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
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "minLength": 1},
                    "estimated_work_minutes": {"type": "integer", "minimum": 0},
                    "pending_until": {"type": "string", "format": "date-time"}
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "breakdown_task",
            "description": "Add child tasks to an existing task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "parent_id": {"type": "string", "format": "uuid"},
                    "names": {
                        "type": "array",
                        "items": {"type": "string", "minLength": 1},
                        "minItems": 1
                    },
                    "pending_until": {"type": "string", "format": "date-time"}
                },
                "required": ["parent_id", "names"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "defer_task",
            "description": "Defer a task until an absolute date and time.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "format": "uuid"},
                    "pending_until": {"type": "string", "format": "date-time"}
                },
                "required": ["task_id", "pending_until"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "complete_task",
            "description": "Complete a task, optionally recording finish time and work seconds.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "format": "uuid"},
                    "finished_at": {"type": "string", "format": "date-time"},
                    "additional_actual_work_seconds": {
                        "type": "integer",
                        "minimum": 0,
                        "default": 0
                    }
                },
                "required": ["task_id"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "update_task",
            "description": "Update a task's estimate, deadline, or category.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "format": "uuid"},
                    "estimated_work_minutes": {"type": "integer", "minimum": 0},
                    "deadline_time": nullable_datetime_schema(),
                    "category": category_schema()
                },
                "required": ["task_id"],
                "anyOf": [
                    {"required": ["estimated_work_minutes"]},
                    {"required": ["deadline_time"]},
                    {"required": ["category"]}
                ],
                "additionalProperties": false
            }
        }),
    ]
}

fn nullable_datetime_schema() -> Value {
    json!({
        "anyOf": [
            {"type": "string", "format": "date-time"},
            {"type": "null"}
        ]
    })
}

fn category_schema() -> Value {
    json!({
        "anyOf": [
            {
                "type": "string",
                "enum": ["earning", "sustaining", "recovery", "investment", "consumption"]
            },
            {"type": "null"}
        ]
    })
}
