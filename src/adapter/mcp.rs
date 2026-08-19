use crate::adapter::gateway::storage_lock::{
    LockMode, StorageLock, StorageLockError, StorageLockErrorKind,
};
use crate::application::interface::TaskRepositoryTrait;
use crate::application::repository_transaction::{
    run_repository_transaction, RepositoryTransactionError,
};
use crate::application::schedule_use_case::{get_schedule, ScheduledTaskView};
use crate::application::task_use_case::{
    breakdown_task as breakdown_task_use_case, complete_task as complete_task_use_case,
    create_task as create_task_use_case, defer_task as defer_task_use_case, get_focus, get_task,
    list_tasks, set_category, set_deadline, set_estimate, ApplicationError, BreakdownTaskInput,
    CompleteTaskInput, CreateTaskInput, ListTasksFilter, TaskPeriodField, TaskPeriodFilter,
    TaskView,
};
use crate::entity::datetime::get_next_morning_datetime;
use crate::entity::task::{ProjectCategory, Status};
use chrono::{DateTime, Duration, Local, LocalResult, NaiveDate};
use serde_json::Map;
use serde_json::{json, Value};
use std::path::PathBuf;
use uuid::Uuid;

mod protocol;

use protocol::{
    error_response, initialize_response, initialized_notification_params_are_valid,
    invalid_params_response, tool_result_response, tools_list_response, validate_initialize_params,
    validate_request_envelope, LifecycleState,
};

pub struct McpServer<R> {
    repository: R,
    storage_directory: Option<PathBuf>,
    lifecycle_state: LifecycleState,
    repository_state_uncertain: bool,
}

impl<R: TaskRepositoryTrait> McpServer<R> {
    pub fn with_storage_directory(repository: R, storage_directory: impl Into<PathBuf>) -> Self {
        Self {
            repository,
            storage_directory: Some(storage_directory.into()),
            lifecycle_state: LifecycleState::Uninitialized,
            repository_state_uncertain: false,
        }
    }

    #[cfg(test)]
    fn new(repository: R) -> Self {
        Self {
            repository,
            storage_directory: None,
            lifecycle_state: LifecycleState::Uninitialized,
            repository_state_uncertain: false,
        }
    }

    pub fn handle_request(&mut self, request: Value) -> Option<Value> {
        let (method, id) = match validate_request_envelope(&request) {
            Ok(envelope) => envelope,
            Err(id) => return Some(error_response(id, -32600, "Invalid Request")),
        };
        let Some(id) = id else {
            if method == "notifications/initialized"
                && self.lifecycle_state == LifecycleState::InitializeResponded
                && initialized_notification_params_are_valid(&request)
            {
                self.lifecycle_state = LifecycleState::Initialized;
            }
            return None;
        };

        match method.as_str() {
            "initialize" if self.lifecycle_state == LifecycleState::Uninitialized => {
                if let Err(error) = validate_initialize_params(&request) {
                    return Some(invalid_params_response(id, error));
                }
                self.lifecycle_state = LifecycleState::InitializeResponded;
                Some(initialize_response(id))
            }
            "initialize" => Some(error_response(id, -32600, "Invalid Request")),
            "tools/list" if self.lifecycle_state != LifecycleState::Initialized => {
                Some(error_response(id, -32002, "Server not initialized"))
            }
            "tools/list" => Some(tools_list_response(id, tool_definitions())),
            "tools/call" if self.lifecycle_state != LifecycleState::Initialized => {
                Some(error_response(id, -32002, "Server not initialized"))
            }
            "tools/call" if self.repository_state_uncertain => {
                Some(repository_state_uncertain_response(id))
            }
            "tools/call" => Some(self.run_transaction_and_call(id, &request)),
            _ => Some(error_response(id, -32601, "Method not found")),
        }
    }

    fn run_transaction_and_call(&mut self, id: Value, request: &Value) -> Value {
        let storage_directory = self.storage_directory.clone();
        match run_repository_transaction(
            &mut self.repository,
            Local::now(),
            || match storage_directory {
                Some(storage_directory) => {
                    StorageLock::acquire(&storage_directory, LockMode::Mcp).map(Some)
                }
                None => Ok(None),
            },
            |repository| {
                let response = Self::call_tool(repository, id.clone(), request);
                let should_save = tool_call_succeeded_with_mutation(request, &response)
                    && repository
                        .has_pending_changes()
                        .map_err(ApplicationError::TaskTree)?;
                Ok::<_, ApplicationError>((response, should_save))
            },
        ) {
            Ok(response) => response,
            Err(RepositoryTransactionError::Lock(error)) => {
                repository_lock_error_response(id, &error)
            }
            Err(RepositoryTransactionError::Load(error)) => {
                repository_load_error_response(id, &error.to_string())
            }
            Err(RepositoryTransactionError::Operation(error)) => {
                internal_error_response(id, &error.to_string())
            }
            Err(RepositoryTransactionError::StateUncertain(error)) => {
                self.repository_state_uncertain = true;
                repository_save_error_response(id, &error.to_string())
            }
        }
    }

    fn call_tool(repository: &mut R, id: Value, request: &Value) -> Value {
        let params = &request["params"];
        match params["name"].as_str() {
            Some("get_focus") => Self::call_get_focus(repository, id, params.get("arguments")),
            Some("get_task") => Self::call_get_task(repository, id, &params["arguments"]),
            Some("list_tasks") => Self::call_list_tasks(repository, id, params.get("arguments")),
            Some("get_schedule") => {
                Self::call_get_schedule(repository, id, params.get("arguments"))
            }
            Some("create_task") => Self::call_create_task(repository, id, &params["arguments"]),
            Some("breakdown_task") => {
                Self::call_breakdown_task(repository, id, &params["arguments"])
            }
            Some("defer_task") => Self::call_defer_task(repository, id, &params["arguments"]),
            Some("complete_task") => Self::call_complete_task(repository, id, &params["arguments"]),
            Some("update_task") => Self::call_update_task(repository, id, &params["arguments"]),
            _ => error_response(id, -32602, "Unknown tool"),
        }
    }

    fn call_get_focus(repository: &mut R, id: Value, arguments: Option<&Value>) -> Value {
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

    fn call_get_task(repository: &R, id: Value, arguments: &Value) -> Value {
        let argument_object = match validate_argument_object(arguments, &["task_id"], &["task_id"])
        {
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
            Ok(Some(task)) => {
                tool_result_response(id, json!({"task": task_view_json(&task)}), false)
            }
            Ok(None) => task_not_found_response(id, task_id, None),
            Err(error) => internal_error_response(id, &error.to_string()),
        }
    }

    fn call_list_tasks(repository: &R, id: Value, arguments: Option<&Value>) -> Value {
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

    fn call_get_schedule(repository: &R, id: Value, arguments: Option<&Value>) -> Value {
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

    fn call_create_task(repository: &mut R, id: Value, arguments: &Value) -> Value {
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

    fn call_breakdown_task(repository: &mut R, id: Value, arguments: &Value) -> Value {
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

    fn call_defer_task(repository: &mut R, id: Value, arguments: &Value) -> Value {
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

    fn call_complete_task(repository: &mut R, id: Value, arguments: &Value) -> Value {
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

    fn call_update_task(repository: &mut R, id: Value, arguments: &Value) -> Value {
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
}

fn tool_call_succeeded_with_mutation(request: &Value, response: &Value) -> bool {
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

fn repository_save_error_response(id: Value, message: &str) -> Value {
    tool_result_response(
        id,
        json!({
            "error": {
                "code": "repository_save_failed",
                "message": message
            }
        }),
        true,
    )
}

fn repository_load_error_response(id: Value, message: &str) -> Value {
    tool_result_response(
        id,
        json!({
            "error": {
                "code": "repository_load_failed",
                "message": message,
                "recovery": "repair_repository"
            }
        }),
        true,
    )
}

fn repository_lock_error_response(id: Value, error: &StorageLockError) -> Value {
    let (code, recovery) = match error.kind() {
        StorageLockErrorKind::Contended => ("repository_lock_contended", "retry"),
        StorageLockErrorKind::Io => ("repository_lock_failed", "inspect_storage"),
    };
    let mut structured_error = json!({
        "code": code,
        "message": error.to_string(),
        "recovery": recovery,
    });
    if let Some(holder_metadata) = error.holder_metadata() {
        structured_error["holder_metadata"] = Value::String(holder_metadata.to_string());
    }
    tool_result_response(id, json!({"error": structured_error}), true)
}

fn repository_state_uncertain_response(id: Value) -> Value {
    tool_result_response(
        id,
        json!({
            "error": {
                "code": "repository_state_uncertain",
                "message": "Repository state is uncertain after a save failure; restart the server before making another tool call",
                "recovery": "restart_server"
            }
        }),
        true,
    )
}

fn internal_error_response(id: Value, message: &str) -> Value {
    tool_result_response(
        id,
        json!({
            "error": {
                "code": "internal_error",
                "message": message
            }
        }),
        true,
    )
}

#[derive(Debug)]
struct InvalidParams {
    field: String,
    reason: &'static str,
}

enum ToolInputError {
    Schema(InvalidParams),
    Semantic {
        field: &'static str,
        message: &'static str,
    },
}

struct UpdateTaskInput {
    task_id: Uuid,
    estimated_work_minutes: Option<i64>,
    deadline_time: Option<Option<DateTime<Local>>>,
    category: Option<Option<ProjectCategory>>,
}

fn uuid_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<Uuid, ToolInputError> {
    let value = string_argument(arguments, field).map_err(ToolInputError::Schema)?;
    Uuid::parse_str(value).map_err(|_| ToolInputError::Semantic {
        field,
        message: "must be a valid UUID",
    })
}

fn datetime_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<DateTime<Local>, ToolInputError> {
    let value = string_argument(arguments, field).map_err(ToolInputError::Schema)?;
    parse_local_datetime(value).map_err(|_| ToolInputError::Semantic {
        field,
        message: "must be a valid RFC 3339 date-time",
    })
}

fn optional_datetime_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<DateTime<Local>>, ToolInputError> {
    arguments
        .get(field)
        .map(|value| {
            let value = value.as_str().ok_or_else(|| {
                ToolInputError::Schema(InvalidParams {
                    field: field.to_string(),
                    reason: "must be a string",
                })
            })?;
            parse_local_datetime(value).map_err(|_| ToolInputError::Semantic {
                field,
                message: "must be a valid RFC 3339 date-time",
            })
        })
        .transpose()
}

fn optional_non_negative_i64_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<i64>, ToolInputError> {
    arguments
        .get(field)
        .map(|value| {
            if let Some(value) = value.as_i64() {
                return if value >= 0 {
                    Ok(value)
                } else {
                    Err(ToolInputError::Schema(InvalidParams {
                        field: field.to_string(),
                        reason: "must be a non-negative integer",
                    }))
                };
            }
            if value.as_u64().is_some() {
                return Err(ToolInputError::Semantic {
                    field,
                    message: "is outside the supported integer range",
                });
            }
            Err(ToolInputError::Schema(InvalidParams {
                field: field.to_string(),
                reason: "must be a non-negative integer",
            }))
        })
        .transpose()
}

fn update_task_input(arguments: &Value) -> Result<UpdateTaskInput, ToolInputError> {
    let arguments = validate_argument_object(
        arguments,
        &[
            "task_id",
            "estimated_work_minutes",
            "deadline_time",
            "category",
        ],
        &["task_id"],
    )
    .map_err(ToolInputError::Schema)?;
    if !["estimated_work_minutes", "deadline_time", "category"]
        .iter()
        .any(|field| arguments.contains_key(*field))
    {
        return Err(ToolInputError::Schema(InvalidParams {
            field: "arguments".to_string(),
            reason: "must include at least one field to update",
        }));
    }

    let task_id = uuid_argument(arguments, "task_id")?;
    let estimated_work_minutes =
        optional_non_negative_i64_argument(arguments, "estimated_work_minutes")?;
    let deadline_time = nullable_datetime_argument(arguments, "deadline_time")?;
    let category = nullable_category_argument(arguments, "category")?;

    Ok(UpdateTaskInput {
        task_id,
        estimated_work_minutes,
        deadline_time,
        category,
    })
}

fn nullable_datetime_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<Option<DateTime<Local>>>, ToolInputError> {
    match arguments.get(field) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(value) => {
            let value = value.as_str().ok_or_else(|| {
                ToolInputError::Schema(InvalidParams {
                    field: field.to_string(),
                    reason: "must be a string or null",
                })
            })?;
            parse_local_datetime(value)
                .map(|value| Some(Some(value)))
                .map_err(|_| ToolInputError::Semantic {
                    field,
                    message: "must be a valid RFC 3339 date-time",
                })
        }
    }
}

fn nullable_category_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<Option<ProjectCategory>>, ToolInputError> {
    match arguments.get(field) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(value)) => parse_mcp_category(value)
            .map(|category| Some(Some(category)))
            .ok_or_else(|| {
                ToolInputError::Schema(InvalidParams {
                    field: field.to_string(),
                    reason: "must be a supported category or null",
                })
            }),
        Some(_) => Err(ToolInputError::Schema(InvalidParams {
            field: field.to_string(),
            reason: "must be a supported category or null",
        })),
    }
}

fn parse_mcp_category(value: &str) -> Option<ProjectCategory> {
    match value {
        "earning" => Some(ProjectCategory::Earning),
        "sustaining" => Some(ProjectCategory::Sustaining),
        "recovery" => Some(ProjectCategory::Recovery),
        "investment" => Some(ProjectCategory::Investment),
        "consumption" => Some(ProjectCategory::Consumption),
        _ => None,
    }
}

fn complete_task_input(arguments: &Value) -> Result<CompleteTaskInput, ToolInputError> {
    let arguments = validate_argument_object(
        arguments,
        &["task_id", "finished_at", "additional_actual_work_seconds"],
        &["task_id"],
    )
    .map_err(ToolInputError::Schema)?;
    let task_id = uuid_argument(arguments, "task_id")?;
    let finished_at =
        optional_datetime_argument(arguments, "finished_at")?.unwrap_or_else(Local::now);
    let additional_actual_work_seconds =
        optional_non_negative_i64_argument(arguments, "additional_actual_work_seconds")?
            .unwrap_or(0);

    Ok(CompleteTaskInput {
        task_id,
        finished_at,
        additional_actual_work_seconds,
    })
}

fn defer_task_input(arguments: &Value) -> Result<(Uuid, DateTime<Local>), ToolInputError> {
    let arguments = validate_argument_object(
        arguments,
        &["task_id", "pending_until"],
        &["task_id", "pending_until"],
    )
    .map_err(ToolInputError::Schema)?;
    let task_id = uuid_argument(arguments, "task_id")?;
    let pending_until = datetime_argument(arguments, "pending_until")?;
    Ok((task_id, pending_until))
}

fn breakdown_task_input(arguments: &Value) -> Result<BreakdownTaskInput, ToolInputError> {
    let arguments = validate_argument_object(
        arguments,
        &["parent_id", "names", "pending_until"],
        &["parent_id", "names"],
    )
    .map_err(ToolInputError::Schema)?;
    let parent_id = uuid_argument(arguments, "parent_id")?;
    let names = arguments
        .get("names")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ToolInputError::Schema(InvalidParams {
                field: "names".to_string(),
                reason: "must be an array",
            })
        })?;
    if names.is_empty() {
        return Err(ToolInputError::Schema(InvalidParams {
            field: "names".to_string(),
            reason: "must contain at least one item",
        }));
    }
    let names = names
        .iter()
        .enumerate()
        .map(|(index, value)| match value.as_str() {
            Some("") => Err(ToolInputError::Schema(InvalidParams {
                field: format!("names[{index}]"),
                reason: "must not be empty",
            })),
            Some(value) => Ok(value.to_string()),
            None => Err(ToolInputError::Schema(InvalidParams {
                field: format!("names[{index}]"),
                reason: "must be a string",
            })),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let pending_until = optional_datetime_argument(arguments, "pending_until")?;

    Ok(BreakdownTaskInput {
        parent_id,
        names,
        pending_until,
    })
}

fn create_task_input(arguments: &Value) -> Result<CreateTaskInput, ToolInputError> {
    let arguments = validate_argument_object(
        arguments,
        &["name", "estimated_work_minutes", "pending_until"],
        &["name"],
    )
    .map_err(ToolInputError::Schema)?;
    let name = string_argument(arguments, "name").map_err(ToolInputError::Schema)?;
    if name.is_empty() {
        return Err(ToolInputError::Schema(InvalidParams {
            field: "name".to_string(),
            reason: "must not be empty",
        }));
    }

    let estimated_work_minutes =
        optional_non_negative_i64_argument(arguments, "estimated_work_minutes")?;
    let pending_until = optional_datetime_argument(arguments, "pending_until")?;

    Ok(CreateTaskInput {
        name: name.to_string(),
        estimated_work_minutes,
        pending_until,
    })
}

fn list_tasks_filter(arguments: Option<&Value>) -> Result<ListTasksFilter, ToolInputError> {
    let Some(arguments) = arguments else {
        return Ok(ListTasksFilter {
            period: None,
            statuses: vec![],
            categories: vec![],
        });
    };
    let arguments = validate_argument_object(arguments, &["period", "statuses", "categories"], &[])
        .map_err(ToolInputError::Schema)?;

    Ok(ListTasksFilter {
        period: arguments
            .get("period")
            .map(parse_period_filter)
            .transpose()?,
        statuses: parse_status_filters(arguments.get("statuses"))?,
        categories: parse_category_filters(arguments.get("categories"))?,
    })
}

fn schedule_period(
    arguments: Option<&Value>,
    now: DateTime<Local>,
) -> Result<(DateTime<Local>, DateTime<Local>), ToolInputError> {
    let Some(arguments) = arguments else {
        return Ok((now, get_next_morning_datetime(now)));
    };
    let arguments = validate_argument_object(arguments, &["from", "until"], &[])
        .map_err(ToolInputError::Schema)?;
    let from = arguments
        .get("from")
        .map(|value| schedule_day_start(value, "from"))
        .transpose()?;
    let until = arguments
        .get("until")
        .map(|value| schedule_day_start(value, "until"))
        .transpose()?;

    let (from, until) = match (from, until) {
        (Some(from), Some(until)) => (from, until),
        (Some(from), None) => (from, get_next_morning_datetime(from)),
        (None, Some(until)) => (now, until),
        (None, None) => (now, get_next_morning_datetime(now)),
    };
    if from >= until {
        return Err(ToolInputError::Semantic {
            field: "until",
            message: "must be later than from",
        });
    }

    Ok((from, until))
}

fn schedule_day_start(
    value: &Value,
    field: &'static str,
) -> Result<DateTime<Local>, ToolInputError> {
    let value = value.as_str().ok_or_else(|| {
        ToolInputError::Schema(InvalidParams {
            field: field.to_string(),
            reason: "must be a date string",
        })
    })?;
    let date =
        NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| ToolInputError::Semantic {
            field,
            message: "must be a valid ISO 8601 date",
        })?;
    let local_noon = date.and_hms_opt(12, 0, 0).ok_or(ToolInputError::Semantic {
        field,
        message: "must be a valid ISO 8601 date",
    })?;
    let local_noon = match local_noon.and_local_timezone(Local) {
        LocalResult::Single(datetime) => datetime,
        _ => {
            return Err(ToolInputError::Semantic {
                field,
                message: "must resolve to a local date-time",
            })
        }
    };

    Ok(get_next_morning_datetime(local_noon) - Duration::days(1))
}

fn parse_period_filter(value: &Value) -> Result<TaskPeriodFilter, ToolInputError> {
    let period = value.as_object().ok_or_else(|| {
        ToolInputError::Schema(InvalidParams {
            field: "period".to_string(),
            reason: "must be an object",
        })
    })?;
    if let Some(field) = period
        .keys()
        .find(|field| !["field", "from", "until"].contains(&field.as_str()))
    {
        return Err(ToolInputError::Schema(InvalidParams {
            field: format!("period.{field}"),
            reason: "additional property is not allowed",
        }));
    }
    for field in ["field", "from", "until"] {
        if !period.contains_key(field) {
            return Err(ToolInputError::Schema(InvalidParams {
                field: format!("period.{field}"),
                reason: "field is required",
            }));
        }
    }

    let field = match required_nested_string(period, "period", "field")? {
        "scheduled_start" => TaskPeriodField::ScheduledStart,
        "created_at" => TaskPeriodField::CreatedAt,
        "deadline" => TaskPeriodField::Deadline,
        "completed_at" => TaskPeriodField::CompletedAt,
        _ => {
            return Err(ToolInputError::Schema(InvalidParams {
                field: "period.field".to_string(),
                reason: "must be a supported period field",
            }))
        }
    };
    let from = parse_datetime(
        required_nested_string(period, "period", "from")?,
        "period.from",
    )?;
    let until = parse_datetime(
        required_nested_string(period, "period", "until")?,
        "period.until",
    )?;

    Ok(TaskPeriodFilter { field, from, until })
}

fn required_nested_string<'a>(
    object: &'a Map<String, Value>,
    object_name: &str,
    field: &str,
) -> Result<&'a str, ToolInputError> {
    object.get(field).and_then(Value::as_str).ok_or_else(|| {
        ToolInputError::Schema(InvalidParams {
            field: format!("{object_name}.{field}"),
            reason: "must be a string",
        })
    })
}

fn parse_datetime(value: &str, field: &'static str) -> Result<DateTime<Local>, ToolInputError> {
    parse_local_datetime(value).map_err(|_| ToolInputError::Semantic {
        field,
        message: "must be a valid RFC 3339 date-time",
    })
}

fn parse_local_datetime(value: &str) -> Result<DateTime<Local>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|time| time.with_timezone(&Local))
}

fn parse_status_filters(value: Option<&Value>) -> Result<Vec<Status>, ToolInputError> {
    let Some(value) = value else {
        return Ok(vec![]);
    };
    let values = value.as_array().ok_or_else(|| {
        ToolInputError::Schema(InvalidParams {
            field: "statuses".to_string(),
            reason: "must be an array",
        })
    })?;

    values
        .iter()
        .enumerate()
        .map(|(index, value)| match value.as_str() {
            Some("todo") => Ok(Status::Todo),
            Some("pending") => Ok(Status::Pending),
            Some("done") => Ok(Status::Done),
            _ => Err(ToolInputError::Schema(InvalidParams {
                field: format!("statuses[{index}]"),
                reason: "must be todo, pending, or done",
            })),
        })
        .collect()
}

fn parse_category_filters(
    value: Option<&Value>,
) -> Result<Vec<Option<ProjectCategory>>, ToolInputError> {
    let Some(value) = value else {
        return Ok(vec![]);
    };
    let values = value.as_array().ok_or_else(|| {
        ToolInputError::Schema(InvalidParams {
            field: "categories".to_string(),
            reason: "must be an array",
        })
    })?;

    values
        .iter()
        .enumerate()
        .map(|(index, value)| match value {
            Value::Null => Ok(None),
            Value::String(value) => parse_mcp_category(value).map(Some).ok_or_else(|| {
                ToolInputError::Schema(InvalidParams {
                    field: format!("categories[{index}]"),
                    reason: "must be a supported category or null",
                })
            }),
            _ => Err(ToolInputError::Schema(InvalidParams {
                field: format!("categories[{index}]"),
                reason: "must be a supported category or null",
            })),
        })
        .collect()
}

fn validate_argument_object<'a>(
    arguments: &'a Value,
    allowed_fields: &[&str],
    required_fields: &[&str],
) -> Result<&'a Map<String, Value>, InvalidParams> {
    let Some(arguments) = arguments.as_object() else {
        return Err(InvalidParams {
            field: "arguments".to_string(),
            reason: "must be an object",
        });
    };

    if let Some(field) = arguments
        .keys()
        .find(|field| !allowed_fields.contains(&field.as_str()))
    {
        return Err(InvalidParams {
            field: format!("arguments.{field}"),
            reason: "additional property is not allowed",
        });
    }

    if let Some(field) = required_fields
        .iter()
        .find(|field| !arguments.contains_key(**field))
    {
        return Err(InvalidParams {
            field: (*field).to_string(),
            reason: "field is required",
        });
    }

    Ok(arguments)
}

fn string_argument<'a>(
    arguments: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, InvalidParams> {
    arguments
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| InvalidParams {
            field: field.to_string(),
            reason: "must be a string",
        })
}

fn task_view_json(task: &TaskView) -> Value {
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

fn scheduled_task_view_json(scheduled: &ScheduledTaskView) -> Value {
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

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "get_focus",
            "description": "Get the task that should be worked on now.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "get_task",
            "description": "Get one task by UUID.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "format": "uuid"}
                },
                "required": ["task_id"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "list_tasks",
            "description": "List tasks filtered by period, status, and category.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "period": {
                        "type": "object",
                        "properties": {
                            "field": {
                                "type": "string",
                                "enum": [
                                    "scheduled_start",
                                    "created_at",
                                    "deadline",
                                    "completed_at"
                                ]
                            },
                            "from": {"type": "string", "format": "date-time"},
                            "until": {"type": "string", "format": "date-time"}
                        },
                        "required": ["field", "from", "until"],
                        "additionalProperties": false
                    },
                    "statuses": {
                        "type": "array",
                        "items": {"type": "string", "enum": ["todo", "pending", "done"]}
                    },
                    "categories": {
                        "type": "array",
                        "items": category_schema()
                    }
                },
                "required": [],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "get_schedule",
            "description": "Get Schronu's calculated task schedule for a date range.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": {"type": "string", "format": "date"},
                    "until": {"type": "string", "format": "date"}
                },
                "required": [],
                "additionalProperties": false
            }
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

#[cfg(test)]
mod protocol_contract_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tool_contract_tests;
