use super::input::{
    decode_input, BreakdownTaskInput, CompleteTaskInput, CreateTaskInput, DeferTaskInput,
    GetFocusInput, GetScheduleInput, GetTaskInput, ListTasksInput, ToolInputError, UpdateTaskInput,
};
use super::internal_error_response;
use super::output::{scheduled_task_view_json, task_view_json};
use super::protocol::{error_response, invalid_params_response, tool_result_response};
use crate::application::interface::TaskRepositoryTrait;
use crate::application::schedule_use_case::get_schedule;
use crate::application::task_use_case::{
    breakdown_task as breakdown_task_use_case, complete_task as complete_task_use_case,
    create_task as create_task_use_case, defer_task as defer_task_use_case, get_focus, get_task,
    list_tasks, set_category, set_deadline, set_estimate, ApplicationError, TaskFactory,
};
use chrono::{DateTime, Local};
use serde_json::{json, Value};
use uuid::Uuid;

pub(super) fn call_tool<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    request: &Value,
    operation_now: DateTime<Local>,
    factory: &mut TaskFactory<'_>,
) -> Value {
    let params = &request["params"];
    match params["name"].as_str() {
        Some("get_focus") => {
            let empty_arguments = json!({});
            let input = match decode_input::<GetFocusInput>(
                params.get("arguments").unwrap_or(&empty_arguments),
            ) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_get_focus(repository, id, input)
        }
        Some("get_task") => {
            let input = match decode_input::<GetTaskInput>(&params["arguments"]) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_get_task(repository, id, input)
        }
        Some("list_tasks") => {
            let empty_arguments = json!({});
            let input = match decode_input::<ListTasksInput>(
                params.get("arguments").unwrap_or(&empty_arguments),
            ) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_list_tasks(repository, id, input)
        }
        Some("get_schedule") => {
            let empty_arguments = json!({});
            let input = match decode_input::<GetScheduleInput>(
                params.get("arguments").unwrap_or(&empty_arguments),
            ) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_get_schedule(repository, id, input)
        }
        Some("create_task") => {
            let input = match decode_input::<CreateTaskInput>(&params["arguments"]) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_create_task(repository, id, input, factory)
        }
        Some("breakdown_task") => {
            let input = match decode_input::<BreakdownTaskInput>(&params["arguments"]) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_breakdown_task(repository, id, input, factory)
        }
        Some("defer_task") => {
            let input = match decode_input::<DeferTaskInput>(&params["arguments"]) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_defer_task(repository, id, input)
        }
        Some("complete_task") => {
            let input = match decode_input::<CompleteTaskInput>(&params["arguments"]) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_complete_task(repository, id, input, operation_now, factory)
        }
        Some("update_task") => {
            let input = match decode_input::<UpdateTaskInput>(&params["arguments"]) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_update_task(repository, id, input)
        }
        _ => error_response(id, -32602, "Unknown tool"),
    }
}

fn call_get_focus<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    _input: GetFocusInput,
) -> Value {
    match get_focus(repository) {
        Ok(task) => {
            let task = task.as_ref().map(task_view_json).unwrap_or(Value::Null);
            tool_result_response(id, json!({"task": task}), false)
        }
        Err(error) => internal_error_response(id, &error.to_string()),
    }
}

fn call_get_task<R: TaskRepositoryTrait>(repository: &R, id: Value, input: GetTaskInput) -> Value {
    let task_id = input.task_id.0;

    match get_task(repository, task_id) {
        Ok(Some(task)) => tool_result_response(id, json!({"task": task_view_json(&task)}), false),
        Ok(None) => task_not_found_response(id, task_id, None),
        Err(error) => internal_error_response(id, &error.to_string()),
    }
}

fn call_list_tasks<R: TaskRepositoryTrait>(
    repository: &R,
    id: Value,
    input: ListTasksInput,
) -> Value {
    match list_tasks(repository, input.into_filter()) {
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
    input: GetScheduleInput,
) -> Value {
    let (from, until) = match input.into_period(repository.get_last_synced_time()) {
        Ok(period) => period,
        Err(ToolInputError::Schema(error)) => return invalid_params_response(id, error),
        Err(ToolInputError::Semantic { field, message }) => {
            return invalid_input_response(id, &field, message)
        }
        Err(ToolInputError::Application(error)) => {
            return internal_error_response(id, &error.to_string())
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
    input: CreateTaskInput,
    factory: &mut TaskFactory<'_>,
) -> Value {
    let input = input.into_application();

    let task_id = match create_task_use_case(repository, input, factory) {
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
    input: BreakdownTaskInput,
    factory: &mut TaskFactory<'_>,
) -> Value {
    let input = input.into_application();
    let child_ids = match breakdown_task_use_case(repository, input, factory) {
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
    input: DeferTaskInput,
) -> Value {
    let (task_id, pending_until) = input.into_parts();

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
    input: CompleteTaskInput,
    operation_now: DateTime<Local>,
    factory: &mut TaskFactory<'_>,
) -> Value {
    let input = input.into_application(operation_now);
    let task_id = input.task_id;
    let output = match complete_task_use_case(repository, input, factory) {
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
        Err(error) => return internal_error_response(id, &error.to_string()),
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
    input: UpdateTaskInput,
) -> Value {
    let input = input.into_changes();

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

fn tool_input_error_response(id: Value, error: ToolInputError) -> Value {
    match error {
        ToolInputError::Schema(error) => invalid_params_response(id, error),
        ToolInputError::Semantic { field, message } => invalid_input_response(id, &field, message),
        ToolInputError::Application(error) => internal_error_response(id, &error.to_string()),
    }
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

#[cfg(test)]
#[path = "handler_tests.rs"]
mod typed_handler_contract_tests;
