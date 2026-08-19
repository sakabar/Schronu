use super::input::{
    breakdown_task_input, complete_task_input, create_task_input, defer_task_input,
    list_tasks_filter, schedule_period, string_argument, update_task_input,
    validate_argument_object, ToolInputError,
};
use super::protocol::{error_response, invalid_params_response, tool_result_response};
use super::{internal_error_response, scheduled_task_view_json, task_view_json};
use crate::application::interface::TaskRepositoryTrait;
use crate::application::schedule_use_case::get_schedule;
use crate::application::task_use_case::{
    breakdown_task as breakdown_task_use_case, complete_task as complete_task_use_case,
    create_task as create_task_use_case, defer_task as defer_task_use_case, get_focus, get_task,
    list_tasks, set_category, set_deadline, set_estimate, ApplicationError,
};
use serde_json::{json, Value};
use uuid::Uuid;

pub(super) fn call_tool<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    request: &Value,
) -> Value {
    let params = &request["params"];
    match params["name"].as_str() {
        Some("get_focus") => call_get_focus(repository, id, params.get("arguments")),
        Some("get_task") => call_get_task(repository, id, &params["arguments"]),
        Some("list_tasks") => call_list_tasks(repository, id, params.get("arguments")),
        Some("get_schedule") => call_get_schedule(repository, id, params.get("arguments")),
        Some("create_task") => call_create_task(repository, id, &params["arguments"]),
        Some("breakdown_task") => call_breakdown_task(repository, id, &params["arguments"]),
        Some("defer_task") => call_defer_task(repository, id, &params["arguments"]),
        Some("complete_task") => call_complete_task(repository, id, &params["arguments"]),
        Some("update_task") => call_update_task(repository, id, &params["arguments"]),
        _ => error_response(id, -32602, "Unknown tool"),
    }
}

fn call_get_focus<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    arguments: Option<&Value>,
) -> Value {
    if let Some(arguments) = arguments {
        if let Err(error) = validate_argument_object(arguments, &[], &[]) {
            return invalid_params_response(id, error);
        }
    }

    match get_focus(repository) {
        Ok(task) => {
            let task = task.as_ref().map(task_view_json).unwrap_or(Value::Null);
            tool_result_response(id, json!({"task": task}), false)
        }
        Err(error) => internal_error_response(id, &error.to_string()),
    }
}

fn call_get_task<R: TaskRepositoryTrait>(repository: &R, id: Value, arguments: &Value) -> Value {
    let argument_object = match validate_argument_object(arguments, &["task_id"], &["task_id"]) {
        Ok(argument_object) => argument_object,
        Err(error) => return invalid_params_response(id, error),
    };
    let task_id_text = match string_argument(argument_object, "task_id") {
        Ok(task_id_text) => task_id_text,
        Err(error) => return invalid_params_response(id, error),
    };
    let Ok(task_id) = Uuid::parse_str(task_id_text) else {
        return invalid_input_response(id, "task_id", "must be a valid UUID");
    };

    match get_task(repository, task_id) {
        Ok(Some(task)) => tool_result_response(id, json!({"task": task_view_json(&task)}), false),
        Ok(None) => task_not_found_response(id, task_id, None),
        Err(error) => internal_error_response(id, &error.to_string()),
    }
}

fn call_list_tasks<R: TaskRepositoryTrait>(
    repository: &R,
    id: Value,
    arguments: Option<&Value>,
) -> Value {
    let filter = match list_tasks_filter(arguments) {
        Ok(filter) => filter,
        Err(ToolInputError::Schema(error)) => return invalid_params_response(id, error),
        Err(ToolInputError::Semantic { field, message }) => {
            return invalid_input_response(id, field, message)
        }
    };

    match list_tasks(repository, filter) {
        Ok(tasks) => tool_result_response(
            id,
            json!({
                "tasks": tasks.iter().map(task_view_json).collect::<Vec<_>>()
            }),
            false,
        ),
        Err(ApplicationError::InvalidInput { field, reason }) => {
            invalid_input_response(id, field, reason)
        }
        Err(error) => internal_error_response(id, &error.to_string()),
    }
}

pub(super) fn call_get_schedule<R: TaskRepositoryTrait>(
    repository: &R,
    id: Value,
    arguments: Option<&Value>,
) -> Value {
    let (from, until) = match schedule_period(arguments, repository.get_last_synced_time()) {
        Ok(period) => period,
        Err(ToolInputError::Schema(error)) => return invalid_params_response(id, error),
        Err(ToolInputError::Semantic { field, message }) => {
            return invalid_input_response(id, field, message)
        }
    };

    match get_schedule(repository) {
        Ok(schedule) => tool_result_response(
            id,
            json!({
                "schedule": schedule
                    .iter()
                    .filter(|scheduled| scheduled.scheduled_start < until && scheduled.scheduled_end > from)
                    .map(scheduled_task_view_json)
                    .collect::<Vec<_>>()
            }),
            false,
        ),
        Err(error) => internal_error_response(id, &error.to_string()),
    }
}

fn call_create_task<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    arguments: &Value,
) -> Value {
    let input = match create_task_input(arguments) {
        Ok(input) => input,
        Err(ToolInputError::Schema(error)) => return invalid_params_response(id, error),
        Err(ToolInputError::Semantic { field, message }) => {
            return invalid_input_response(id, field, message)
        }
    };

    let task_id = match create_task_use_case(repository, input) {
        Ok(task_id) => task_id,
        Err(ApplicationError::InvalidInput { field, reason }) => {
            return invalid_input_response(id, field, reason)
        }
        Err(error) => return internal_error_response(id, &error.to_string()),
    };

    tool_result_response(id, json!({"task_id": task_id.to_string()}), false)
}

fn call_breakdown_task<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    arguments: &Value,
) -> Value {
    let input = match breakdown_task_input(arguments) {
        Ok(input) => input,
        Err(ToolInputError::Schema(error)) => return invalid_params_response(id, error),
        Err(ToolInputError::Semantic { field, message }) => {
            return invalid_input_response(id, field, message)
        }
    };
    let child_ids = match breakdown_task_use_case(repository, input) {
        Ok(child_ids) => child_ids,
        Err(ApplicationError::TaskNotFound(task_id)) => {
            return task_not_found_response(id, task_id, Some("parent_id"))
        }
        Err(ApplicationError::InvalidInput { field, reason }) => {
            return invalid_input_response(id, field, reason)
        }
        Err(error) => return internal_error_response(id, &error.to_string()),
    };

    tool_result_response(
        id,
        json!({
            "child_ids": child_ids.iter().map(Uuid::to_string).collect::<Vec<_>>()
        }),
        false,
    )
}

fn call_defer_task<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    arguments: &Value,
) -> Value {
    let (task_id, pending_until) = match defer_task_input(arguments) {
        Ok(input) => input,
        Err(ToolInputError::Schema(error)) => return invalid_params_response(id, error),
        Err(ToolInputError::Semantic { field, message }) => {
            return invalid_input_response(id, field, message)
        }
    };

    match defer_task_use_case(repository, task_id, pending_until) {
        Ok(()) => {}
        Err(ApplicationError::TaskNotFound(task_id)) => {
            return task_not_found_response(id, task_id, Some("task_id"))
        }
        Err(ApplicationError::InvalidInput { field, reason }) => {
            return invalid_input_response(id, field, reason)
        }
        Err(error) => return internal_error_response(id, &error.to_string()),
    }

    tool_result_response(id, json!({"task_id": task_id.to_string()}), false)
}

fn call_complete_task<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    arguments: &Value,
) -> Value {
    let input = match complete_task_input(arguments) {
        Ok(input) => input,
        Err(ToolInputError::Schema(error)) => return invalid_params_response(id, error),
        Err(ToolInputError::Semantic { field, message }) => {
            return invalid_input_response(id, field, message)
        }
    };
    let task_id = input.task_id;
    let output = match complete_task_use_case(repository, input) {
        Ok(output) => output,
        Err(ApplicationError::TaskNotFound(task_id)) => {
            return task_not_found_response(id, task_id, Some("task_id"))
        }
        Err(ApplicationError::HasUndoneChildren(task_id)) => {
            return has_undone_children_response(id, task_id)
        }
        Err(ApplicationError::InvalidInput { field, reason }) => {
            return invalid_input_response(id, field, reason)
        }
        Err(ApplicationError::TaskTree(error)) => {
            return internal_error_response(id, &error.to_string())
        }
    };

    tool_result_response(
        id,
        json!({
            "task_id": task_id.to_string(),
            "next_focus_task_id": output.next_focus_task_id.map(|task_id| task_id.to_string()),
            "next_repetition_task_id": output.next_repetition_task_id.map(|task_id| task_id.to_string())
        }),
        false,
    )
}

fn call_update_task<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    arguments: &Value,
) -> Value {
    let input = match update_task_input(arguments) {
        Ok(input) => input,
        Err(ToolInputError::Schema(error)) => return invalid_params_response(id, error),
        Err(ToolInputError::Semantic { field, message }) => {
            return invalid_input_response(id, field, message)
        }
    };

    if let Some(estimated_work_minutes) = input.estimated_work_minutes {
        if let Err(error) = set_estimate(repository, input.task_id, estimated_work_minutes) {
            return update_task_application_error_response(id, error);
        }
    }
    if let Some(deadline_time) = input.deadline_time {
        if let Err(error) = set_deadline(repository, input.task_id, deadline_time) {
            return update_task_application_error_response(id, error);
        }
    }
    if let Some(category) = input.category {
        if let Err(error) = set_category(repository, input.task_id, category) {
            return update_task_application_error_response(id, error);
        }
    }

    tool_result_response(id, json!({"task_id": input.task_id.to_string()}), false)
}

pub(super) fn tool_call_succeeded_with_mutation(request: &Value, response: &Value) -> bool {
    matches!(
        request["params"]["name"].as_str(),
        Some("create_task" | "breakdown_task" | "defer_task" | "complete_task" | "update_task")
    ) && response.get("error").is_none()
        && response["result"]["isError"] != Value::Bool(true)
}

fn invalid_input_response(id: Value, field: &str, message: &str) -> Value {
    tool_result_response(
        id,
        json!({
            "error": {
                "code": "invalid_input",
                "message": message,
                "field": field
            }
        }),
        true,
    )
}

fn task_not_found_response(id: Value, task_id: Uuid, field: Option<&str>) -> Value {
    let mut error = json!({
        "code": "task_not_found",
        "message": format!("task not found: {task_id}"),
        "task_id": task_id.to_string()
    });
    if let Some(field) = field {
        error["field"] = Value::String(field.to_string());
    }
    tool_result_response(id, json!({"error": error}), true)
}

fn has_undone_children_response(id: Value, task_id: Uuid) -> Value {
    tool_result_response(
        id,
        json!({
            "error": {
                "code": "has_undone_children",
                "message": format!("task has undone children: {task_id}"),
                "task_id": task_id.to_string(),
                "field": "task_id"
            }
        }),
        true,
    )
}

fn update_task_application_error_response(id: Value, error: ApplicationError) -> Value {
    match error {
        ApplicationError::TaskNotFound(task_id) => {
            task_not_found_response(id, task_id, Some("task_id"))
        }
        ApplicationError::InvalidInput { field, reason } => {
            invalid_input_response(id, field, reason)
        }
        error => internal_error_response(id, &error.to_string()),
    }
}
