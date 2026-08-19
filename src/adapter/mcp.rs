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

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LifecycleState {
    Uninitialized,
    InitializeResponded,
    Initialized,
}

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
                Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {
                        "tools": {"listChanged": false}
                    },
                    "serverInfo": {
                        "name": "schronu",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
                }))
            }
            "initialize" => Some(error_response(id, -32600, "Invalid Request")),
            "tools/list" if self.lifecycle_state != LifecycleState::Initialized => {
                Some(error_response(id, -32002, "Server not initialized"))
            }
            "tools/list" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"tools": tool_definitions()}
            })),
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

fn validate_request_envelope(request: &Value) -> Result<(String, Option<Value>), Value> {
    let Some(request) = request.as_object() else {
        return Err(Value::Null);
    };
    let response_id = request
        .get("id")
        .filter(|id| id.is_string() || id.is_number())
        .cloned()
        .unwrap_or(Value::Null);

    if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(response_id);
    }
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return Err(response_id);
    };
    let id = match request.get("id") {
        Some(id) if id.is_string() || id.is_number() => Some(id.clone()),
        Some(_) => return Err(Value::Null),
        None if matches!(method, "initialize" | "tools/list" | "tools/call") => {
            return Err(Value::Null)
        }
        None => None,
    };

    Ok((method.to_string(), id))
}

fn initialized_notification_params_are_valid(request: &Value) -> bool {
    let Some(params) = request.get("params") else {
        return true;
    };
    let Some(params) = params.as_object() else {
        return false;
    };
    optional_object_field(params, "_meta", "params._meta").is_ok()
}

fn validate_initialize_params(request: &Value) -> Result<(), InvalidParams> {
    let params = request.get("params").ok_or_else(|| InvalidParams {
        field: "params".to_string(),
        reason: "is required",
    })?;
    let params = params.as_object().ok_or_else(|| InvalidParams {
        field: "params".to_string(),
        reason: "must be an object",
    })?;

    required_string_field(params, "protocolVersion", "params.protocolVersion")?;
    let capabilities = required_object_field(params, "capabilities", "params.capabilities")?;
    validate_client_capabilities(capabilities)?;
    let client_info = required_object_field(params, "clientInfo", "params.clientInfo")?;
    required_string_field(client_info, "name", "params.clientInfo.name")?;
    required_string_field(client_info, "version", "params.clientInfo.version")?;
    if client_info
        .get("title")
        .is_some_and(|title| !title.is_string())
    {
        return Err(InvalidParams {
            field: "params.clientInfo.title".to_string(),
            reason: "must be a string",
        });
    }
    if let Some(meta) = optional_object_field(params, "_meta", "params._meta")? {
        optional_string_or_number_field(meta, "progressToken", "params._meta.progressToken")?;
    }

    Ok(())
}

fn validate_client_capabilities(capabilities: &Map<String, Value>) -> Result<(), InvalidParams> {
    if let Some(roots) = optional_object_field(capabilities, "roots", "params.capabilities.roots")?
    {
        optional_boolean_field(
            roots,
            "listChanged",
            "params.capabilities.roots.listChanged",
        )?;
    }
    optional_object_field(capabilities, "sampling", "params.capabilities.sampling")?;
    optional_object_field(
        capabilities,
        "elicitation",
        "params.capabilities.elicitation",
    )?;
    if let Some(experimental) = optional_object_field(
        capabilities,
        "experimental",
        "params.capabilities.experimental",
    )? {
        for (name, capability) in experimental {
            if !capability.is_object() {
                return Err(InvalidParams {
                    field: format!("params.capabilities.experimental.{name}"),
                    reason: "must be an object",
                });
            }
        }
    }

    Ok(())
}

fn required_string_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    field_path: &str,
) -> Result<&'a str, InvalidParams> {
    let value = object.get(field).ok_or_else(|| InvalidParams {
        field: field_path.to_string(),
        reason: "is required",
    })?;
    value.as_str().ok_or_else(|| InvalidParams {
        field: field_path.to_string(),
        reason: "must be a string",
    })
}

fn required_object_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    field_path: &str,
) -> Result<&'a Map<String, Value>, InvalidParams> {
    let value = object.get(field).ok_or_else(|| InvalidParams {
        field: field_path.to_string(),
        reason: "is required",
    })?;
    value.as_object().ok_or_else(|| InvalidParams {
        field: field_path.to_string(),
        reason: "must be an object",
    })
}

fn optional_object_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    field_path: &str,
) -> Result<Option<&'a Map<String, Value>>, InvalidParams> {
    object
        .get(field)
        .map(|value| {
            value.as_object().ok_or_else(|| InvalidParams {
                field: field_path.to_string(),
                reason: "must be an object",
            })
        })
        .transpose()
}

fn optional_boolean_field(
    object: &Map<String, Value>,
    field: &str,
    field_path: &str,
) -> Result<Option<bool>, InvalidParams> {
    object
        .get(field)
        .map(|value| {
            value.as_bool().ok_or_else(|| InvalidParams {
                field: field_path.to_string(),
                reason: "must be a boolean",
            })
        })
        .transpose()
}

fn optional_string_or_number_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    field_path: &str,
) -> Result<Option<&'a Value>, InvalidParams> {
    object
        .get(field)
        .map(|value| {
            if value.is_string() || value.is_number() {
                Ok(value)
            } else {
                Err(InvalidParams {
                    field: field_path.to_string(),
                    reason: "must be a string or number",
                })
            }
        })
        .transpose()
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
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

fn invalid_params_response(id: Value, error: InvalidParams) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32602,
            "message": "Invalid params",
            "data": {
                "code": "invalid_input",
                "field": error.field,
                "reason": error.reason
            }
        }
    })
}

fn tool_result_response(id: Value, structured_content: Value, is_error: bool) -> Value {
    let text = structured_content.to_string();
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured_content,
            "isError": is_error
        }
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
mod tests {
    use super::McpServer;
    use crate::adapter::gateway::task_repository::TaskRepository;
    use crate::application::interface::{
        RepositoryReloadOutcome, TaskRepositoryError, TaskRepositoryOperation, TaskRepositoryTrait,
    };
    use crate::entity::datetime::get_next_morning_datetime;
    use crate::entity::task::{ProjectCategory, RepetitionAnchor, Status, TaskAttr, TaskHandle};
    use chrono::{DateTime, Duration, Local, TimeZone};
    use serde_json::json;
    use std::cell::{Cell, RefCell};
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;
    use uuid::Uuid;

    struct McpCacheTestStorage {
        path: PathBuf,
    }

    impl McpCacheTestStorage {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("schronu-mcp-cache-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for McpCacheTestStorage {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    struct RecordingRepository {
        projects: Vec<TaskHandle>,
        now: DateTime<Local>,
        focus_task_id: Option<Uuid>,
        fail_load_once: bool,
        fail_save: bool,
        load_count: Rc<Cell<usize>>,
        reload_if_changed_count: Rc<Cell<usize>>,
        project_count: Rc<Cell<usize>>,
        save_count: Rc<Cell<usize>>,
        mutation_count: Rc<Cell<usize>>,
        persisted_project_revisions: RefCell<Vec<u64>>,
        operation_order: Rc<RefCell<Vec<&'static str>>>,
        sync_clock_times: Rc<RefCell<Vec<DateTime<Local>>>>,
    }

    impl RecordingRepository {
        fn new(projects: Vec<TaskHandle>) -> Self {
            let project_count = projects.len();
            let persisted_project_revisions = projects
                .iter()
                .map(TaskHandle::get_persistent_mutation_revision)
                .collect::<Result<Vec<_>, _>>()
                .expect("recording repository projects must be readable");
            Self {
                projects,
                now: fixed_now(),
                focus_task_id: None,
                fail_load_once: false,
                fail_save: false,
                load_count: Rc::new(Cell::new(0)),
                reload_if_changed_count: Rc::new(Cell::new(0)),
                project_count: Rc::new(Cell::new(project_count)),
                save_count: Rc::new(Cell::new(0)),
                mutation_count: Rc::new(Cell::new(0)),
                persisted_project_revisions: RefCell::new(persisted_project_revisions),
                operation_order: Rc::new(RefCell::new(Vec::new())),
                sync_clock_times: Rc::new(RefCell::new(Vec::new())),
            }
        }

        fn with_focus_task_id(mut self, task_id: Uuid) -> Self {
            self.focus_task_id = Some(task_id);
            self
        }

        fn with_save_failure(mut self) -> Self {
            self.fail_save = true;
            self
        }

        fn with_load_failure_once(mut self) -> Self {
            self.fail_load_once = true;
            self
        }
    }

    impl TaskRepositoryTrait for RecordingRepository {
        fn get_project_storage_dir_name(&self) -> &str {
            "unused"
        }

        fn get_all_projects(&self) -> Vec<&TaskHandle> {
            self.projects.iter().collect()
        }

        fn load(&mut self) -> Result<(), TaskRepositoryError> {
            self.load_count.set(self.load_count.get() + 1);
            self.operation_order.borrow_mut().push("load");
            if self.fail_load_once {
                self.fail_load_once = false;
                return Err(TaskRepositoryError::new(
                    TaskRepositoryOperation::Load,
                    std::io::Error::other("test load failure"),
                ));
            }
            Ok(())
        }

        fn save(&self) -> Result<(), TaskRepositoryError> {
            self.save_count.set(self.save_count.get() + 1);
            self.operation_order.borrow_mut().push("save");
            if self.fail_save {
                Err(TaskRepositoryError::new(
                    TaskRepositoryOperation::Save,
                    std::io::Error::other("test save failure"),
                ))
            } else {
                *self.persisted_project_revisions.borrow_mut() = self
                    .projects
                    .iter()
                    .map(TaskHandle::get_persistent_mutation_revision)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| {
                        TaskRepositoryError::new(TaskRepositoryOperation::Save, error)
                    })?;
                Ok(())
            }
        }

        fn reload_if_changed(
            &mut self,
            now: DateTime<Local>,
        ) -> Result<RepositoryReloadOutcome, TaskRepositoryError> {
            self.reload_if_changed_count
                .set(self.reload_if_changed_count.get() + 1);
            self.sync_clock(now)
                .map_err(|error| TaskRepositoryError::new(TaskRepositoryOperation::Load, error))?;
            self.load()?;
            Ok(RepositoryReloadOutcome::Reloaded)
        }

        fn has_pending_changes(&self) -> Result<bool, crate::entity::task::TaskTreeError> {
            let persisted = self.persisted_project_revisions.borrow();
            Ok(self.projects.len() != persisted.len()
                || self
                    .projects
                    .iter()
                    .map(TaskHandle::get_persistent_mutation_revision)
                    .collect::<Result<Vec<_>, _>>()?
                    .iter()
                    .zip(persisted.iter())
                    .any(|(current, persisted)| *current != *persisted))
        }

        fn sync_clock(
            &mut self,
            now: DateTime<Local>,
        ) -> Result<(), crate::entity::task::TaskTreeError> {
            self.sync_clock_times.borrow_mut().push(now);
            self.operation_order.borrow_mut().push("sync_clock");
            self.now = now;
            Ok(())
        }

        fn get_last_synced_time(&self) -> DateTime<Local> {
            self.now
        }

        fn get_highest_priority_project(&mut self) -> Option<&TaskHandle> {
            self.projects.first()
        }

        fn get_highest_priority_leaf_task_id(
            &mut self,
        ) -> Result<Option<Uuid>, crate::entity::task::TaskTreeError> {
            Ok(self.focus_task_id)
        }

        fn get_defer_candidate_leaf_task_id(
            &mut self,
            _recent_days: i64,
        ) -> Result<Option<Uuid>, crate::entity::task::TaskTreeError> {
            Ok(None)
        }

        fn get_by_id(
            &self,
            id: Uuid,
        ) -> Result<Option<TaskHandle>, crate::entity::task::TaskTreeError> {
            for task in &self.projects {
                if let Some(found) = task.get_by_id(id)? {
                    return Ok(Some(found));
                }
            }
            Ok(None)
        }

        fn start_new_project(
            &mut self,
            root_task: TaskHandle,
        ) -> Result<(), crate::entity::task::TaskTreeError> {
            self.mutation_count.set(self.mutation_count.get() + 1);
            self.operation_order.borrow_mut().push("mutation");
            self.projects.push(root_task);
            self.project_count.set(self.projects.len());
            Ok(())
        }
    }

    #[test]
    fn initializeはserver情報とtools能力を返す() {
        let mut server = McpServer::new(TaskRepository::new(""));
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            }
        });

        let response = server.handle_request(request).unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(response["result"]["serverInfo"]["name"], "schronu");
        assert_eq!(
            response["result"]["serverInfo"]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(
            response["result"]["capabilities"]["tools"]["listChanged"],
            false
        );
    }

    #[test]
    fn initializeは非対応version要求にserver対応versionを返す() {
        let mut server = McpServer::new(TaskRepository::new(""));
        let request = json!({
            "jsonrpc": "2.0",
            "id": "initialize-unsupported-version",
            "method": "initialize",
            "params": {
                "protocolVersion": "2099-01-01",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            }
        });

        let response = server.handle_request(request).unwrap();

        assert_eq!(response["id"], "initialize-unsupported-version");
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn initializeとtools_listではrepository_clockを同期もloadもしない() {
        let repository = RecordingRepository::new(vec![]);
        let load_count = Rc::clone(&repository.load_count);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mut server = McpServer::new(repository);

        server.handle_request(initialize_request()).unwrap();
        server.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
        server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "tools-list",
                "method": "tools/list"
            }))
            .unwrap();

        assert!(sync_clock_times.borrow().is_empty());
        assert_eq!(load_count.get(), 0);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 初期化完了前のtools_callはUninitializedとInitializeRespondedの両方で拒否しrepository_clockを同期もloadもしない(
    ) {
        let repository = RecordingRepository::new(vec![]);
        let load_count = Rc::clone(&repository.load_count);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mut server = McpServer::new(repository);
        let request = tool_call_request("before-initialized", "list_tasks", json!({}));

        let uninitialized = server.handle_request(request.clone()).unwrap();
        assert_eq!(uninitialized["error"]["code"], -32002);
        assert_eq!(uninitialized["error"]["message"], "Server not initialized");
        assert!(sync_clock_times.borrow().is_empty());
        assert_eq!(load_count.get(), 0);

        server.handle_request(initialize_request()).unwrap();
        let initialize_responded = server.handle_request(request).unwrap();
        assert_eq!(initialize_responded["error"]["code"], -32002);
        assert_eq!(
            initialize_responded["error"]["message"],
            "Server not initialized"
        );
        assert!(sync_clock_times.borrow().is_empty());
        assert_eq!(load_count.get(), 0);
    }

    #[test]
    fn 初期化済みtools_callは検証結果によらずdispatch直前にrepository_clockを同期してloadする() {
        let cases = [
            ("valid", "list_tasks", json!({})),
            ("invalid-arguments", "get_task", json!({})),
            ("unknown-tool", "unknown_tool", json!({})),
        ];

        for (id, tool_name, arguments) in cases {
            let repository = RecordingRepository::new(vec![]);
            let load_count = Rc::clone(&repository.load_count);
            let reload_if_changed_count = Rc::clone(&repository.reload_if_changed_count);
            let operation_order = Rc::clone(&repository.operation_order);
            let sync_clock_times = Rc::clone(&repository.sync_clock_times);
            let mut server = initialized_server(repository);
            let before = Local::now();

            server
                .handle_request(tool_call_request(id, tool_name, arguments))
                .unwrap();

            let after = Local::now();
            let sync_clock_times = sync_clock_times.borrow();
            assert_eq!(sync_clock_times.len(), 1, "case: {id}");
            assert_eq!(load_count.get(), 1, "case: {id}");
            assert_eq!(reload_if_changed_count.get(), 1, "case: {id}");
            assert_eq!(
                *operation_order.borrow(),
                vec!["sync_clock", "load"],
                "case: {id}"
            );
            assert!(
                before <= sync_clock_times[0] && sync_clock_times[0] <= after,
                "case: {id}, actual: {}",
                sync_clock_times[0]
            );
        }
    }

    #[test]
    fn 同一mcp_processの連続read_toolはreload_if_changed経路を使う() {
        let repository = RecordingRepository::new(vec![]);
        let reload_if_changed_count = Rc::clone(&repository.reload_if_changed_count);
        let mut server = initialized_server(repository);

        server
            .handle_request(tool_call_request("first", "list_tasks", json!({})))
            .unwrap();
        server
            .handle_request(tool_call_request("second", "list_tasks", json!({})))
            .unwrap();

        assert_eq!(reload_if_changed_count.get(), 2);
    }

    #[test]
    fn get_scheduleは借用競合を既存internal_error形式で返す() {
        let task = TaskHandle::new("借用競合").unwrap();
        let server = McpServer::new(RecordingRepository::new(vec![task.clone()]));

        let response = task.with_exclusive_data_borrow_for_test(|| {
            McpServer::call_get_schedule(&server.repository, json!("borrow"), Some(&json!({})))
        });

        assert_eq!(response["result"]["isError"], true);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "internal_error"
        );
        assert_eq!(
            response["result"]["structuredContent"]["error"]["message"],
            "task tree operation failed: cannot borrow task tree data"
        );
    }

    #[test]
    fn 同一mcp_processの2回目のread_toolは実repositoryのcacheを使う() {
        let storage = McpCacheTestStorage::new();
        let storage_path = storage.path.to_str().unwrap();
        let now = fixed_now();
        let mut source = TaskRepository::new(storage_path);
        source.sync_clock(now).unwrap();
        source
            .start_new_project(TaskHandle::new("MCP cache対象").unwrap())
            .unwrap();
        source.save().unwrap();
        let project_yaml_path = storage
            .path
            .join("20260811-MCP cache対象")
            .join("project.yaml");
        let repository = TaskRepository::new(storage_path);
        let mut server = McpServer::with_storage_directory(repository, &storage.path);
        server.handle_request(initialize_request()).unwrap();
        server.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));

        let first = server
            .handle_request(tool_call_request("first", "list_tasks", json!({})))
            .unwrap();
        assert_eq!(first["result"]["isError"], false);
        fs::write(project_yaml_path, "project: [").unwrap();

        let second = server
            .handle_request(tool_call_request("second", "list_tasks", json!({})))
            .unwrap();

        assert_eq!(second["result"]["isError"], false);
        assert_eq!(
            second["result"]["structuredContent"]["tasks"][0]["name"],
            "MCP cache対象"
        );
    }

    #[test]
    fn repository_load失敗はtaskを作成せずstructured_errorを返し同一sessionの次回callで再試行する()
    {
        let repository = RecordingRepository::new(vec![]).with_load_failure_once();
        let load_count = Rc::clone(&repository.load_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let operation_order = Rc::clone(&repository.operation_order);
        let project_count = Rc::clone(&repository.project_count);
        let save_count = Rc::clone(&repository.save_count);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mut server = initialized_server(repository);

        let failed = server
            .handle_request(tool_call_request(
                "load-failure",
                "create_task",
                json!({"name": "must not be created before load"}),
            ))
            .unwrap();

        assert_eq!(failed["jsonrpc"], "2.0");
        assert_eq!(failed["id"], "load-failure");
        assert_eq!(failed["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&failed);
        let error = &failed["result"]["structuredContent"]["error"];
        assert_eq!(error["code"], "repository_load_failed");
        assert_eq!(error["recovery"], "repair_repository");
        assert!(!error["message"].as_str().unwrap().is_empty());
        assert_eq!(sync_clock_times.borrow().len(), 1);
        assert_eq!(load_count.get(), 1);
        assert_eq!(mutation_count.get(), 0);
        assert_eq!(project_count.get(), 0);
        assert_eq!(save_count.get(), 0);
        assert_eq!(*operation_order.borrow(), vec!["sync_clock", "load"]);

        let retried = server
            .handle_request(tool_call_request(
                "load-retry",
                "create_task",
                json!({"name": "created after successful retry"}),
            ))
            .unwrap();

        assert_eq!(retried["jsonrpc"], "2.0");
        assert_eq!(retried["id"], "load-retry");
        assert_eq!(retried["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&retried);
        assert_eq!(sync_clock_times.borrow().len(), 2);
        assert_eq!(load_count.get(), 2);
        assert_eq!(mutation_count.get(), 1);
        assert_eq!(project_count.get(), 1);
        assert_eq!(save_count.get(), 1);
        assert_eq!(
            *operation_order.borrow(),
            vec![
                "sync_clock",
                "load",
                "sync_clock",
                "load",
                "mutation",
                "save"
            ]
        );
    }

    #[test]
    fn json_rpc_requestは未知methodにmethod_not_foundを返す() {
        let mut server = McpServer::new(TaskRepository::new(""));
        let request = json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "unknown/method"
        });

        let response = server.handle_request(request).unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 99);
        assert_eq!(response["error"]["code"], -32601);
        assert_eq!(response["error"]["message"], "Method not found");
    }

    #[test]
    fn json_rpc_requestのenvelope不正時も有効なidをerror応答へ引き継ぐ() {
        let cases = [
            ("non-object", json!([]), json!(null)),
            (
                "missing-jsonrpc-without-id",
                json!({"method": "initialize", "params": {}}),
                json!(null),
            ),
            (
                "initialize-without-id",
                json!({
                    "jsonrpc": "2.0",
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": {"name": "test-client", "version": "1.0"}
                    }
                }),
                json!(null),
            ),
            (
                "missing-jsonrpc",
                json!({"id": "missing-jsonrpc", "method": "initialize", "params": {}}),
                json!("missing-jsonrpc"),
            ),
            (
                "wrong-jsonrpc",
                json!({"jsonrpc": "1.0", "id": 1, "method": "initialize", "params": {}}),
                json!(1),
            ),
            (
                "non-string-jsonrpc",
                json!({"jsonrpc": 2, "id": 2, "method": "initialize", "params": {}}),
                json!(2),
            ),
            (
                "missing-method",
                json!({"jsonrpc": "2.0", "id": 3}),
                json!(3),
            ),
            (
                "non-string-method",
                json!({"jsonrpc": "2.0", "id": 4, "method": false}),
                json!(4),
            ),
            (
                "null-id",
                json!({"jsonrpc": "2.0", "id": null, "method": "initialize", "params": {}}),
                json!(null),
            ),
            (
                "boolean-id",
                json!({"jsonrpc": "2.0", "id": true, "method": "initialize", "params": {}}),
                json!(null),
            ),
            (
                "object-id",
                json!({"jsonrpc": "2.0", "id": {"invalid": true}, "method": "initialize", "params": {}}),
                json!(null),
            ),
        ];

        for (label, request, expected_id) in cases {
            let repository = RecordingRepository::new(vec![]);
            let sync_clock_times = Rc::clone(&repository.sync_clock_times);
            let mutation_count = Rc::clone(&repository.mutation_count);
            let save_count = Rc::clone(&repository.save_count);
            let mut server = McpServer::new(repository);

            let response = server
                .handle_request(request)
                .unwrap_or_else(|| panic!("case {label} must receive an Invalid Request response"));

            assert_eq!(response["jsonrpc"], "2.0", "case: {label}");
            assert_eq!(response["id"], expected_id, "case: {label}");
            assert_eq!(response["error"]["code"], -32600, "case: {label}");
            assert_eq!(
                response["error"]["message"], "Invalid Request",
                "case: {label}"
            );
            assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
            assert_eq!(mutation_count.get(), 0, "case: {label}");
            assert_eq!(save_count.get(), 0, "case: {label}");

            let valid_initialize = server
                .handle_request(json!({
                    "jsonrpc": "2.0",
                    "id": format!("valid-after-{label}"),
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": {"name": "test-client", "version": "1.0"}
                    }
                }))
                .unwrap();
            assert_eq!(
                valid_initialize["result"]["protocolVersion"], "2025-06-18",
                "case: {label}"
            );
            assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
            assert_eq!(mutation_count.get(), 0, "case: {label}");
            assert_eq!(save_count.get(), 0, "case: {label}");
        }
    }

    #[test]
    fn initializeの不正paramsをinvalid_paramsとして拒否しlifecycleを進めない() {
        let cases = [
            ("missing-params", None, "params", "required"),
            ("null-params", Some(json!(null)), "params", "object"),
            ("array-params", Some(json!([])), "params", "object"),
            (
                "missing-protocol-version",
                Some(json!({
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": "1.0"}
                })),
                "params.protocolVersion",
                "required",
            ),
            (
                "wrong-protocol-version-type",
                Some(json!({
                    "protocolVersion": 1,
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": "1.0"}
                })),
                "params.protocolVersion",
                "string",
            ),
            (
                "missing-capabilities",
                Some(json!({
                    "protocolVersion": "2025-06-18",
                    "clientInfo": {"name": "test-client", "version": "1.0"}
                })),
                "params.capabilities",
                "required",
            ),
            (
                "wrong-capabilities-type",
                Some(json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": [],
                    "clientInfo": {"name": "test-client", "version": "1.0"}
                })),
                "params.capabilities",
                "object",
            ),
            (
                "missing-client-info",
                Some(json!({"protocolVersion": "2025-06-18", "capabilities": {}})),
                "params.clientInfo",
                "required",
            ),
            (
                "wrong-client-info-type",
                Some(json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": []
                })),
                "params.clientInfo",
                "object",
            ),
            (
                "missing-client-name",
                Some(json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"version": "1.0"}
                })),
                "params.clientInfo.name",
                "required",
            ),
            (
                "wrong-client-name-type",
                Some(json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": false, "version": "1.0"}
                })),
                "params.clientInfo.name",
                "string",
            ),
            (
                "missing-client-version",
                Some(json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-client"}
                })),
                "params.clientInfo.version",
                "required",
            ),
            (
                "wrong-client-version-type",
                Some(json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": 1}
                })),
                "params.clientInfo.version",
                "string",
            ),
            (
                "wrong-client-title-type",
                Some(json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": "1.0", "title": 1}
                })),
                "params.clientInfo.title",
                "string",
            ),
        ];

        for (label, params, expected_field, expected_reason_token) in cases {
            let repository = RecordingRepository::new(vec![]);
            let sync_clock_times = Rc::clone(&repository.sync_clock_times);
            let mutation_count = Rc::clone(&repository.mutation_count);
            let save_count = Rc::clone(&repository.save_count);
            let mut server = McpServer::new(repository);
            let mut request = json!({
                "jsonrpc": "2.0",
                "id": label,
                "method": "initialize"
            });
            if let Some(params) = params {
                request["params"] = params;
            }

            let response = server.handle_request(request).unwrap();

            assert_eq!(response["jsonrpc"], "2.0", "case: {label}");
            assert_eq!(response["id"], label, "case: {label}");
            assert_eq!(response["error"]["code"], -32602, "case: {label}");
            assert_eq!(
                response["error"]["message"], "Invalid params",
                "case: {label}"
            );
            assert_eq!(
                response["error"]["data"]["field"], expected_field,
                "case: {label}"
            );
            let reason = response["error"]["data"]["reason"]
                .as_str()
                .unwrap_or_else(|| panic!("case {label} must include a reason"));
            assert!(
                reason.to_ascii_lowercase().contains(expected_reason_token),
                "case: {label}, reason: {reason}"
            );
            assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
            assert_eq!(mutation_count.get(), 0, "case: {label}");
            assert_eq!(save_count.get(), 0, "case: {label}");

            assert_eq!(
                server.handle_request(json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized"
                })),
                None,
                "case: {label}"
            );
            let before_valid_initialize = server
                .handle_request(json!({
                    "jsonrpc": "2.0",
                    "id": format!("valid-after-{label}"),
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": {"name": "test-client", "version": "1.0"}
                    }
                }))
                .unwrap();
            assert_eq!(
                before_valid_initialize["result"]["protocolVersion"], "2025-06-18",
                "case: {label}"
            );
            assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
            assert_eq!(mutation_count.get(), 0, "case: {label}");
            assert_eq!(save_count.get(), 0, "case: {label}");
        }
    }

    #[test]
    fn initializeの既知client_capabilityをschemaどおり検証する() {
        let cases = [
            (
                "wrong-roots-type",
                json!({"roots": []}),
                "params.capabilities.roots",
                "object",
            ),
            (
                "wrong-roots-list-changed-type",
                json!({"roots": {"listChanged": "yes"}}),
                "params.capabilities.roots.listChanged",
                "boolean",
            ),
            (
                "wrong-sampling-type",
                json!({"sampling": false}),
                "params.capabilities.sampling",
                "object",
            ),
            (
                "wrong-elicitation-type",
                json!({"elicitation": []}),
                "params.capabilities.elicitation",
                "object",
            ),
            (
                "wrong-experimental-type",
                json!({"experimental": []}),
                "params.capabilities.experimental",
                "object",
            ),
            (
                "wrong-experimental-capability-type",
                json!({"experimental": {"feature": true}}),
                "params.capabilities.experimental.feature",
                "object",
            ),
        ];

        for (label, capabilities, expected_field, expected_reason_token) in cases {
            let repository = RecordingRepository::new(vec![]);
            let sync_clock_times = Rc::clone(&repository.sync_clock_times);
            let mutation_count = Rc::clone(&repository.mutation_count);
            let save_count = Rc::clone(&repository.save_count);
            let mut server = McpServer::new(repository);

            let response = server
                .handle_request(json!({
                    "jsonrpc": "2.0",
                    "id": label,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": capabilities,
                        "clientInfo": {"name": "test-client", "version": "1.0"}
                    }
                }))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0", "case: {label}");
            assert_eq!(response["id"], label, "case: {label}");
            assert_eq!(response["error"]["code"], -32602, "case: {label}");
            assert_eq!(
                response["error"]["message"], "Invalid params",
                "case: {label}"
            );
            assert_eq!(
                response["error"]["data"]["field"], expected_field,
                "case: {label}"
            );
            let reason = response["error"]["data"]["reason"]
                .as_str()
                .unwrap_or_else(|| panic!("case {label} must include a reason"));
            assert!(
                reason.to_ascii_lowercase().contains(expected_reason_token),
                "case: {label}, reason: {reason}"
            );
            assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
            assert_eq!(mutation_count.get(), 0, "case: {label}");
            assert_eq!(save_count.get(), 0, "case: {label}");

            let valid_initialize = server
                .handle_request(json!({
                    "jsonrpc": "2.0",
                    "id": format!("valid-after-{label}"),
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": {"name": "test-client", "version": "1.0"}
                    }
                }))
                .unwrap();
            assert_eq!(
                valid_initialize["result"]["protocolVersion"], "2025-06-18",
                "case: {label}"
            );
            assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
            assert_eq!(mutation_count.get(), 0, "case: {label}");
            assert_eq!(save_count.get(), 0, "case: {label}");
        }
    }

    #[test]
    fn initialize_paramsの_metaをobjectに限定する() {
        let repository = RecordingRepository::new(vec![]);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = McpServer::new(repository);

        let invalid_initialize = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "invalid-initialize-meta",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": "1.0"},
                    "_meta": false
                }
            }))
            .unwrap();
        assert_eq!(invalid_initialize["id"], "invalid-initialize-meta");
        assert_eq!(invalid_initialize["error"]["code"], -32602);
        assert_eq!(invalid_initialize["error"]["message"], "Invalid params");
        assert_eq!(invalid_initialize["error"]["data"]["field"], "params._meta");
        assert!(invalid_initialize["error"]["data"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.to_ascii_lowercase().contains("object")));
        assert!(sync_clock_times.borrow().is_empty());
        assert_eq!(mutation_count.get(), 0);
        assert_eq!(save_count.get(), 0);

        let valid_initialize = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "valid-initialize-meta",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": "1.0"},
                    "_meta": {"trace": "test"},
                    "vendorExtension": true
                }
            }))
            .unwrap();
        assert_eq!(valid_initialize["result"]["protocolVersion"], "2025-06-18");
        assert!(sync_clock_times.borrow().is_empty());
        assert_eq!(mutation_count.get(), 0);
        assert_eq!(save_count.get(), 0);
    }

    #[test]
    #[allow(non_snake_case)]
    fn initialize_paramsの_meta内progressTokenをstringまたはnumberに限定する() {
        let invalid_cases = [
            ("boolean", json!(false)),
            ("null", json!(null)),
            ("object", json!({})),
            ("array", json!([])),
        ];

        for (label, progress_token) in invalid_cases {
            let repository = RecordingRepository::new(vec![]);
            let sync_clock_times = Rc::clone(&repository.sync_clock_times);
            let mutation_count = Rc::clone(&repository.mutation_count);
            let save_count = Rc::clone(&repository.save_count);
            let mut server = McpServer::new(repository);

            let response = server
                .handle_request(json!({
                    "jsonrpc": "2.0",
                    "id": format!("invalid-progress-token-{label}"),
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": {"name": "test-client", "version": "1.0"},
                        "_meta": {"progressToken": progress_token}
                    }
                }))
                .unwrap();

            assert_eq!(
                response["id"],
                format!("invalid-progress-token-{label}"),
                "case: {label}"
            );
            assert_eq!(response["error"]["code"], -32602, "case: {label}");
            assert_eq!(
                response["error"]["message"], "Invalid params",
                "case: {label}"
            );
            assert_eq!(
                response["error"]["data"]["field"], "params._meta.progressToken",
                "case: {label}"
            );
            let reason = response["error"]["data"]["reason"]
                .as_str()
                .unwrap_or_else(|| panic!("case {label} must include a reason"))
                .to_ascii_lowercase();
            assert!(reason.contains("string"), "case: {label}, reason: {reason}");
            assert!(reason.contains("number"), "case: {label}, reason: {reason}");
            assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
            assert_eq!(mutation_count.get(), 0, "case: {label}");
            assert_eq!(save_count.get(), 0, "case: {label}");

            let recovered = server
                .handle_request(json!({
                    "jsonrpc": "2.0",
                    "id": format!("valid-after-progress-token-{label}"),
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": {"name": "test-client", "version": "1.0"},
                        "_meta": {"progressToken": "recovered", "vendorExtension": true}
                    }
                }))
                .unwrap();
            assert_eq!(
                recovered["result"]["protocolVersion"], "2025-06-18",
                "case: {label}"
            );
            assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
            assert_eq!(mutation_count.get(), 0, "case: {label}");
            assert_eq!(save_count.get(), 0, "case: {label}");
        }

        for (label, progress_token) in [("string", json!("token")), ("number", json!(1.5))] {
            let mut server = McpServer::new(TaskRepository::new(""));
            let response = server
                .handle_request(json!({
                    "jsonrpc": "2.0",
                    "id": format!("valid-progress-token-{label}"),
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": {"name": "test-client", "version": "1.0"},
                        "_meta": {
                            "progressToken": progress_token,
                            "vendorExtension": {"enabled": true}
                        }
                    }
                }))
                .unwrap();
            assert_eq!(
                response["result"]["protocolVersion"], "2025-06-18",
                "case: {label}"
            );
        }
    }

    #[test]
    fn initialized通知はparamsの_metaをobjectに限定する() {
        let repository = RecordingRepository::new(vec![]);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = McpServer::new(repository);
        server.handle_request(initialize_request()).unwrap();

        assert_eq!(
            server.handle_request(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {"_meta": false}
            })),
            None
        );
        let before_valid_notification = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "before-valid-meta-notification",
                "method": "tools/list"
            }))
            .unwrap();
        assert_eq!(before_valid_notification["error"]["code"], -32002);

        assert_eq!(
            server.handle_request(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {
                    "_meta": {"trace": "test"},
                    "vendorExtension": true
                }
            })),
            None
        );
        let after_valid_notification = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "after-valid-meta-notification",
                "method": "tools/list"
            }))
            .unwrap();
        assert!(after_valid_notification["result"]["tools"].is_array());
        assert!(sync_clock_times.borrow().is_empty());
        assert_eq!(mutation_count.get(), 0);
        assert_eq!(save_count.get(), 0);
    }

    #[test]
    #[allow(non_snake_case)]
    fn initialized通知の_meta内progressTokenはbooleanも許容する() {
        let mut server = McpServer::new(TaskRepository::new(""));
        server.handle_request(initialize_request()).unwrap();

        assert_eq!(
            server.handle_request(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {
                    "_meta": {
                        "progressToken": false,
                        "vendorExtension": [1, 2, 3]
                    },
                    "vendorExtension": true
                }
            })),
            None
        );
        let tools = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "tools-after-generic-notification-meta",
                "method": "tools/list"
            }))
            .unwrap();
        assert!(tools["result"]["tools"].is_array());
    }

    #[test]
    fn 不正なinitialized通知ではlifecycleを進めない() {
        let mut server = McpServer::new(TaskRepository::new(""));
        server.handle_request(initialize_request()).unwrap();

        let malformed_notification = server
            .handle_request(json!({"method": "notifications/initialized"}))
            .expect("malformed notification must receive Invalid Request");
        assert_eq!(malformed_notification["jsonrpc"], "2.0");
        assert_eq!(malformed_notification["id"], serde_json::Value::Null);
        assert_eq!(malformed_notification["error"]["code"], -32600);
        assert_eq!(
            malformed_notification["error"]["message"],
            "Invalid Request"
        );

        assert_eq!(
            server.handle_request(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": []
            })),
            None
        );
        let before_valid_notification = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "before-valid-notification",
                "method": "tools/list"
            }))
            .unwrap();
        assert_eq!(before_valid_notification["error"]["code"], -32002);

        assert_eq!(
            server.handle_request(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {
                    "_meta": {"trace": "test"},
                    "vendorExtension": true
                }
            })),
            None
        );
        let after_valid_notification = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "after-valid-notification",
                "method": "tools/list"
            }))
            .unwrap();
        assert!(after_valid_notification["result"]["tools"].is_array());
    }

    #[test]
    #[allow(non_snake_case)]
    fn initializeはclientInfoとcapabilitiesの拡張fieldを許容する() {
        let mut server = McpServer::new(TaskRepository::new(""));

        let response = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "initialize-with-extensions",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {
                        "roots": {"listChanged": true, "vendorOption": true},
                        "sampling": {"vendorOption": true},
                        "elicitation": {"vendorOption": true},
                        "experimental": {"feature": {"enabled": true}},
                        "vendorCapability": true
                    },
                    "clientInfo": {
                        "name": "test-client",
                        "title": "Test Client",
                        "version": "1.0",
                        "vendorExtension": true
                    },
                    "_meta": {"trace": "test"},
                    "vendorExtension": true
                }
            }))
            .unwrap();

        assert_eq!(response["id"], "initialize-with-extensions");
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn notification_未知methodには応答せずrepository_clockを同期もloadもしない() {
        let repository = RecordingRepository::new(vec![]);
        let load_count = Rc::clone(&repository.load_count);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mut server = McpServer::new(repository);
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/unknown"
        });

        assert_eq!(server.handle_request(notification), None);
        assert!(sync_clock_times.borrow().is_empty());
        assert_eq!(load_count.get(), 0);
    }

    #[test]
    fn tools_list_initialized通知前は拒否する() {
        let mut server = McpServer::new(TaskRepository::new(""));
        let tools_list = json!({
            "jsonrpc": "2.0",
            "id": "before-initialized",
            "method": "tools/list"
        });

        let before_initialize = server.handle_request(tools_list.clone()).unwrap();
        assert_eq!(before_initialize["jsonrpc"], "2.0");
        assert_eq!(before_initialize["id"], "before-initialized");
        assert_eq!(before_initialize["error"]["code"], -32002);
        assert_eq!(
            before_initialize["error"]["message"],
            "Server not initialized"
        );

        assert_eq!(
            server.handle_request(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            })),
            None
        );
        let after_premature_notification = server.handle_request(tools_list.clone()).unwrap();
        assert_eq!(after_premature_notification["error"]["code"], -32002);

        server.handle_request(initialize_request()).unwrap();
        let before_notification = server.handle_request(tools_list).unwrap();
        assert_eq!(before_notification["error"]["code"], -32002);
    }

    #[test]
    fn tools_list_initialized通知後に9つのtoolのschemaを返す() {
        let mut server = McpServer::new(TaskRepository::new(""));
        server.handle_request(initialize_request()).unwrap();
        assert_eq!(
            server.handle_request(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            })),
            None
        );

        let response = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "tools-list",
                "method": "tools/list"
            }))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "tools-list");
        assert_eq!(
            response["result"]["tools"],
            json_fixture(
                include_str!("../../tests/fixtures/mcp/tools-list.json"),
                &[]
            )
        );
        let tools = response["result"]["tools"].as_array().unwrap();
        let mut names = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        names.sort_unstable();
        let mut expected_names = vec![
            "get_focus",
            "get_task",
            "list_tasks",
            "get_schedule",
            "create_task",
            "breakdown_task",
            "defer_task",
            "complete_task",
            "update_task",
        ];
        expected_names.sort_unstable();
        assert_eq!(names, expected_names);

        for tool in tools {
            assert!(!tool["description"].as_str().unwrap().is_empty());
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            for field in required_fields_for_tool(tool) {
                assert!(tool["inputSchema"]["properties"].get(field).is_some());
            }
        }

        assert_eq!(property_names(tools, "get_focus"), Vec::<&str>::new());
        assert_eq!(property_names(tools, "get_task"), vec!["task_id"]);
        assert_eq!(
            property_names(tools, "list_tasks"),
            vec!["categories", "period", "statuses"]
        );
        assert_eq!(property_names(tools, "get_schedule"), vec!["from", "until"]);
        assert_eq!(
            property_names(tools, "create_task"),
            vec!["estimated_work_minutes", "name", "pending_until"]
        );
        assert_eq!(
            property_names(tools, "breakdown_task"),
            vec!["names", "parent_id", "pending_until"]
        );
        assert_eq!(
            property_names(tools, "defer_task"),
            vec!["pending_until", "task_id"]
        );
        assert_eq!(
            property_names(tools, "complete_task"),
            vec!["additional_actual_work_seconds", "finished_at", "task_id"]
        );
        assert_eq!(
            property_names(tools, "update_task"),
            vec![
                "category",
                "deadline_time",
                "estimated_work_minutes",
                "task_id"
            ]
        );

        assert_eq!(required_fields(tools, "get_focus"), Vec::<&str>::new());
        assert_eq!(required_fields(tools, "get_task"), vec!["task_id"]);
        assert_eq!(required_fields(tools, "list_tasks"), Vec::<&str>::new());
        assert_eq!(required_fields(tools, "get_schedule"), Vec::<&str>::new());
        assert_eq!(required_fields(tools, "create_task"), vec!["name"]);
        assert_eq!(
            required_fields(tools, "breakdown_task"),
            vec!["names", "parent_id"]
        );
        assert_eq!(
            required_fields(tools, "defer_task"),
            vec!["pending_until", "task_id"]
        );
        assert_eq!(required_fields(tools, "complete_task"), vec!["task_id"]);
        assert_eq!(required_fields(tools, "update_task"), vec!["task_id"]);

        assert_string_property(tools, "get_task", "task_id", Some("uuid"));
        assert_string_property(tools, "get_schedule", "from", Some("date"));
        assert_string_property(tools, "get_schedule", "until", Some("date"));
        assert_string_property(tools, "create_task", "name", None);
        assert_eq!(property(tools, "create_task", "name")["minLength"], 1);
        assert_non_negative_integer_property(tools, "create_task", "estimated_work_minutes");
        assert_string_property(tools, "create_task", "pending_until", Some("date-time"));
        assert_string_property(tools, "breakdown_task", "parent_id", Some("uuid"));
        let names_schema = property(tools, "breakdown_task", "names");
        assert_eq!(names_schema["type"], "array");
        assert_eq!(names_schema["items"]["type"], "string");
        assert_eq!(names_schema["items"]["minLength"], 1);
        assert_eq!(names_schema["minItems"], 1);
        assert_string_property(tools, "breakdown_task", "pending_until", Some("date-time"));
        assert_string_property(tools, "defer_task", "task_id", Some("uuid"));
        assert_string_property(tools, "defer_task", "pending_until", Some("date-time"));
        assert_string_property(tools, "complete_task", "task_id", Some("uuid"));
        assert_string_property(tools, "complete_task", "finished_at", Some("date-time"));
        assert_non_negative_integer_property(
            tools,
            "complete_task",
            "additional_actual_work_seconds",
        );
        assert_string_property(tools, "update_task", "task_id", Some("uuid"));
        assert_non_negative_integer_property(tools, "update_task", "estimated_work_minutes");
        assert_nullable_string_property(tools, "update_task", "deadline_time", Some("date-time"));
        assert_nullable_string_property(tools, "update_task", "category", None);

        let period = property(tools, "list_tasks", "period");
        assert_eq!(period["type"], "object");
        assert_eq!(period["additionalProperties"], false);
        assert_eq!(
            sorted_strings(&period["required"]),
            vec!["field", "from", "until"]
        );
        assert_eq!(period["properties"]["field"]["type"], "string");
        assert_eq!(
            sorted_strings(&period["properties"]["field"]["enum"]),
            vec!["completed_at", "created_at", "deadline", "scheduled_start"]
        );
        assert_eq!(period["properties"]["from"]["type"], "string");
        assert_eq!(period["properties"]["from"]["format"], "date-time");
        assert_eq!(period["properties"]["until"]["type"], "string");
        assert_eq!(period["properties"]["until"]["format"], "date-time");
        let statuses = property(tools, "list_tasks", "statuses");
        assert_eq!(statuses["type"], "array");
        assert_eq!(statuses["items"]["type"], "string");
        assert_eq!(
            sorted_strings(&statuses["items"]["enum"]),
            vec!["done", "pending", "todo"]
        );
        let categories = property(tools, "list_tasks", "categories");
        assert_eq!(categories["type"], "array");
        assert_nullable_category_schema(&categories["items"]);
        assert_nullable_category_schema(property(tools, "update_task", "category"));

        let update_branches = tool(tools, "update_task")["inputSchema"]["anyOf"]
            .as_array()
            .unwrap();
        let mut update_fields = update_branches
            .iter()
            .map(|branch| sorted_strings(&branch["required"]))
            .collect::<Vec<_>>();
        update_fields.sort_unstable();
        assert_eq!(
            update_fields,
            vec![
                vec!["category"],
                vec!["deadline_time"],
                vec!["estimated_work_minutes"]
            ]
        );
    }

    #[test]
    fn initialize_再送を拒否してlifecycleを維持する() {
        let mut server = McpServer::new(TaskRepository::new(""));
        server.handle_request(initialize_request()).unwrap();

        let before_initialized = server.handle_request(initialize_request()).unwrap();
        assert_eq!(before_initialized["jsonrpc"], "2.0");
        assert_eq!(before_initialized["id"], "initialize");
        assert_eq!(before_initialized["error"]["code"], -32600);
        assert_eq!(before_initialized["error"]["message"], "Invalid Request");

        server.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
        let after_initialized = server.handle_request(initialize_request()).unwrap();
        assert_eq!(after_initialized["error"]["code"], -32600);

        let tools_list = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "after-reinitialize",
                "method": "tools/list"
            }))
            .unwrap();
        assert!(tools_list["result"]["tools"].is_array());
    }

    #[test]
    #[allow(non_snake_case)]
    fn get_taskはTaskViewをstructured_contentで返してsaveしない() {
        let pending_until = Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap();
        let create_time = Local.with_ymd_and_hms(2026, 8, 1, 9, 0, 0).unwrap();
        let start_time = Local.with_ymd_and_hms(2026, 8, 10, 10, 0, 0).unwrap();
        let deadline_time = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
        let root = TaskHandle::new("MCP task").unwrap();
        root.set_orig_status(Status::Pending).unwrap();
        root.set_pending_until(pending_until).unwrap();
        root.set_priority(7).unwrap();
        root.set_create_time(create_time).unwrap();
        root.set_start_time(start_time).unwrap();
        root.set_deadline_time_opt(Some(deadline_time)).unwrap();
        root.set_estimated_work_seconds(1_800).unwrap();
        root.set_actual_work_seconds(900).unwrap();
        root.set_atomic(true).unwrap();
        root.set_is_on_other_side(true).unwrap();
        root.set_repetition_interval_days_opt(Some(7)).unwrap();
        root.set_repetition_anchor(RepetitionAnchor::Completion)
            .unwrap();
        root.set_days_in_advance(2).unwrap();
        root.set_project_category_opt(Some(ProjectCategory::Recovery))
            .unwrap();
        root.sync_clock(fixed_now()).unwrap();
        let child = root.create_as_last_child(TaskAttr::new("child"));
        let task_id = root.get_id().unwrap();
        let child_id = child.get_id().unwrap();
        let repository = RecordingRepository::new(vec![root]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "get-task",
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "get-task");
        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        let task = &response["result"]["structuredContent"]["task"];
        assert_eq!(
            sorted_object_keys(task),
            vec![
                "actual_work_seconds",
                "atomic",
                "child_ids",
                "create_time",
                "days_in_advance",
                "deadline_time",
                "end_time",
                "estimated_work_seconds",
                "id",
                "is_on_other_side",
                "name",
                "original_status",
                "parent_id",
                "pending_until",
                "priority",
                "project_category",
                "repetition_anchor",
                "repetition_interval_days",
                "root_id",
                "start_time",
                "status"
            ]
        );
        assert_eq!(
            task,
            &json_fixture(
                include_str!("../../tests/fixtures/mcp/task-view.json"),
                &[
                    ("{{task_id}}", &task_id.to_string()),
                    ("{{child_id}}", &child_id.to_string()),
                ],
            )
        );
        assert_eq!(task["id"], task_id.to_string());
        assert_eq!(task["root_id"], task_id.to_string());
        assert_eq!(task["parent_id"], serde_json::Value::Null);
        assert_eq!(task["child_ids"], json!([child_id.to_string()]));
        assert_eq!(task["name"], "MCP task");
        assert_eq!(task["status"], "pending");
        assert_eq!(task["original_status"], "pending");
        assert_eq!(task["is_on_other_side"], true);
        assert_eq!(task["atomic"], true);
        assert_eq!(task["pending_until"], pending_until.to_rfc3339());
        assert_eq!(task["priority"], 7);
        assert_eq!(task["create_time"], create_time.to_rfc3339());
        assert_eq!(task["start_time"], start_time.to_rfc3339());
        assert_eq!(task["end_time"], serde_json::Value::Null);
        assert_eq!(task["deadline_time"], deadline_time.to_rfc3339());
        assert_eq!(task["estimated_work_seconds"], 1_800);
        assert_eq!(task["actual_work_seconds"], 900);
        assert_eq!(task["repetition_interval_days"], 7);
        assert_eq!(task["repetition_anchor"], "completion");
        assert_eq!(task["days_in_advance"], 2);
        assert_eq!(task["project_category"], "recovery");

        let child_response = server
            .handle_request(tool_call_request(
                "get-child-task",
                "get_task",
                json!({"task_id": child_id.to_string()}),
            ))
            .unwrap();
        assert_eq!(child_response["jsonrpc"], "2.0");
        assert_eq!(child_response["id"], "get-child-task");
        assert_tool_result_content_matches_structured(&child_response);
        let child_task = &child_response["result"]["structuredContent"]["task"];
        assert_eq!(child_task["id"], child_id.to_string());
        assert_eq!(child_task["root_id"], task_id.to_string());
        assert_eq!(child_task["parent_id"], task_id.to_string());
        assert_eq!(child_task["child_ids"], json!([]));
        assert_eq!(child_task["pending_until"], serde_json::Value::Null);
        assert_eq!(child_task["end_time"], serde_json::Value::Null);
        assert_eq!(child_task["deadline_time"], serde_json::Value::Null);
        assert_eq!(
            child_task["repetition_interval_days"],
            serde_json::Value::Null
        );
        assert_eq!(child_task["project_category"], "recovery");
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }

    #[test]
    fn get_task_不正uuidをstructured_errorで返す() {
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "invalid-task-id",
                "get_task",
                json!({"task_id": "not-a-uuid"}),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "invalid-task-id");
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "invalid_input"
        );
        assert_eq!(
            response["result"]["structuredContent"]["error"]["field"],
            "task_id"
        );
        assert!(!response["result"]["structuredContent"]["error"]["message"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }

    #[test]
    fn get_task_schema違反をinvalid_paramsで返す() {
        let task_id = Uuid::new_v4().to_string();
        let cases = [
            (
                "extra-field",
                json!({"task_id": task_id, "extra": 1}),
                "arguments.extra",
            ),
            ("missing-task-id", json!({}), "task_id"),
            ("wrong-task-id-type", json!({"task_id": 1}), "task_id"),
            ("non-object-arguments", serde_json::Value::Null, "arguments"),
        ];

        for (id, arguments, expected_field) in cases {
            let repository = RecordingRepository::new(vec![]);
            let save_count = Rc::clone(&repository.save_count);
            let mutation_count = Rc::clone(&repository.mutation_count);
            let mut server = initialized_server(repository);

            let response = server
                .handle_request(tool_call_request(id, "get_task", arguments))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            assert_eq!(response["error"]["code"], -32602);
            assert_eq!(response["error"]["message"], "Invalid params");
            assert_eq!(response["error"]["data"]["code"], "invalid_input");
            assert_eq!(response["error"]["data"]["field"], expected_field);
            assert!(!response["error"]["data"]["reason"]
                .as_str()
                .unwrap()
                .is_empty());
            assert_eq!(save_count.get(), 0);
            assert_eq!(mutation_count.get(), 0);
        }
    }

    #[test]
    fn get_task_未知uuidをstructured_errorで返す() {
        let task_id = Uuid::new_v4();
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "missing-task",
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "missing-task");
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "task_not_found"
        );
        assert_eq!(
            response["result"]["structuredContent"]["error"]["task_id"],
            task_id.to_string()
        );
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }

    #[test]
    fn get_focus_選択taskを返してrepositoryを変更しない() {
        let non_focused_task = TaskHandle::new("not focused").unwrap();
        let focused_task = TaskHandle::new("focused task").unwrap();
        let task_id = focused_task.get_id().unwrap();
        let repository = RecordingRepository::new(vec![non_focused_task, focused_task])
            .with_focus_task_id(task_id);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request("get-focus", "get_focus", json!({})))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "get-focus");
        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["task"]["id"],
            task_id.to_string()
        );
        assert_eq!(
            response["result"]["structuredContent"]["task"]["name"],
            "focused task"
        );
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }

    #[test]
    fn get_focus_候補なしをtask_nullで返す() {
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "no-focus",
                "method": "tools/call",
                "params": {"name": "get_focus"}
            }))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "no-focus");
        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["task"],
            serde_json::Value::Null
        );
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }

    #[test]
    fn get_focus_extra_fieldをinvalid_paramsで返す() {
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "get-focus-extra",
                "get_focus",
                json!({"extra": true}),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "get-focus-extra");
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(response["error"]["message"], "Invalid params");
        assert_eq!(response["error"]["data"]["code"], "invalid_input");
        assert_eq!(response["error"]["data"]["field"], "arguments.extra");
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }

    #[test]
    fn list_tasks_期間status_categoryで絞ってrepositoryを変更しない() {
        let matching = task_for_list(
            "matching",
            Status::Pending,
            ProjectCategory::Recovery,
            Local.with_ymd_and_hms(2026, 8, 10, 9, 0, 0).unwrap(),
        );
        let matching_id = matching.get_id().unwrap();
        let wrong_status = task_for_list(
            "wrong status",
            Status::Todo,
            ProjectCategory::Recovery,
            Local.with_ymd_and_hms(2026, 8, 10, 10, 0, 0).unwrap(),
        );
        let wrong_category = task_for_list(
            "wrong category",
            Status::Pending,
            ProjectCategory::Investment,
            Local.with_ymd_and_hms(2026, 8, 10, 11, 0, 0).unwrap(),
        );
        let outside_period = task_for_list(
            "outside period",
            Status::Pending,
            ProjectCategory::Recovery,
            Local.with_ymd_and_hms(2026, 8, 9, 23, 59, 59).unwrap(),
        );
        let repository =
            RecordingRepository::new(vec![matching, wrong_status, wrong_category, outside_period]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "list-filtered",
                "list_tasks",
                json!({
                    "period": {
                        "field": "created_at",
                        "from": "2026-08-10T00:00:00+09:00",
                        "until": "2026-08-11T00:00:00+09:00"
                    },
                    "statuses": ["pending"],
                    "categories": ["recovery"]
                }),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "list-filtered");
        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        let tasks = response["result"]["structuredContent"]["tasks"]
            .as_array()
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["id"], matching_id.to_string());
        assert_eq!(tasks[0]["name"], "matching");
        assert_eq!(tasks[0]["status"], "pending");
        assert_eq!(tasks[0]["project_category"], "recovery");
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }

    #[test]
    fn list_tasks_arguments省略で全taskを返す() {
        let first = TaskHandle::new("first").unwrap();
        let first_id = first.get_id().unwrap();
        let child = first.create_as_last_child(TaskAttr::new("child"));
        let child_id = child.get_id().unwrap();
        let second = TaskHandle::new("second").unwrap();
        let second_id = second.get_id().unwrap();
        let repository = RecordingRepository::new(vec![first, second]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "list-all",
                "method": "tools/call",
                "params": {"name": "list_tasks"}
            }))
            .unwrap();

        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        let ids = response["result"]["structuredContent"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                first_id.to_string(),
                child_id.to_string(),
                second_id.to_string()
            ]
        );
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }

    #[test]
    fn list_tasks_null_categoryで未分類taskを絞る() {
        let uncategorized = TaskHandle::new("uncategorized").unwrap();
        let uncategorized_id = uncategorized.get_id().unwrap();
        let categorized = TaskHandle::new("categorized").unwrap();
        categorized
            .set_project_category_opt(Some(ProjectCategory::Recovery))
            .unwrap();
        let repository = RecordingRepository::new(vec![uncategorized, categorized]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "list-uncategorized",
                "list_tasks",
                json!({"categories": [null]}),
            ))
            .unwrap();

        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        let tasks = response["result"]["structuredContent"]["tasks"]
            .as_array()
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["id"], uncategorized_id.to_string());
        assert_eq!(tasks[0]["project_category"], serde_json::Value::Null);
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }

    #[test]
    fn list_tasks_schema違反をinvalid_paramsで返す() {
        let cases = [
            ("statuses-type", json!({"statuses": "pending"}), "statuses"),
            (
                "status-value",
                json!({"statuses": ["invalid"]}),
                "statuses[0]",
            ),
            (
                "category-value",
                json!({"categories": ["invalid"]}),
                "categories[0]",
            ),
            (
                "period-required",
                json!({
                    "period": {
                        "field": "created_at",
                        "from": "2026-08-10T00:00:00+09:00"
                    }
                }),
                "period.until",
            ),
            ("extra", json!({"extra": true}), "arguments.extra"),
        ];

        for (id, arguments, field) in cases {
            let repository = RecordingRepository::new(vec![]);
            let save_count = Rc::clone(&repository.save_count);
            let mutation_count = Rc::clone(&repository.mutation_count);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(id, "list_tasks", arguments))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            assert_eq!(response["error"]["code"], -32602);
            assert_eq!(response["error"]["message"], "Invalid params");
            assert_eq!(response["error"]["data"]["code"], "invalid_input");
            assert_eq!(response["error"]["data"]["field"], field);
            assert!(!response["error"]["data"]["reason"]
                .as_str()
                .unwrap()
                .is_empty());
            assert_eq!(save_count.get(), 0);
            assert_eq!(mutation_count.get(), 0);
        }
    }

    #[test]
    fn list_tasks_不正日時と逆転期間をstructured_errorで返す() {
        let cases = [
            (
                "invalid-date",
                json!({
                    "period": {
                        "field": "created_at",
                        "from": "not-a-date",
                        "until": "2026-08-11T00:00:00+09:00"
                    }
                }),
                "period.from",
            ),
            (
                "reversed-period",
                json!({
                    "period": {
                        "field": "created_at",
                        "from": "2026-08-11T00:00:00+09:00",
                        "until": "2026-08-10T00:00:00+09:00"
                    }
                }),
                "period",
            ),
        ];

        for (id, arguments, field) in cases {
            let repository = RecordingRepository::new(vec![]);
            let save_count = Rc::clone(&repository.save_count);
            let mutation_count = Rc::clone(&repository.mutation_count);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(id, "list_tasks", arguments))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            assert_eq!(response["result"]["isError"], true);
            assert_tool_result_content_matches_structured(&response);
            assert_eq!(
                response["result"]["structuredContent"]["error"]["code"],
                "invalid_input"
            );
            assert_eq!(
                response["result"]["structuredContent"]["error"]["field"],
                field
            );
            assert!(!response["result"]["structuredContent"]["error"]["message"]
                .as_str()
                .unwrap()
                .is_empty());
            assert_eq!(save_count.get(), 0);
            assert_eq!(mutation_count.get(), 0);
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn get_scheduleは予定をScheduledTaskViewの全field付きで返しrepositoryを変更しない() {
        let task = TaskHandle::new("scheduled task").unwrap();
        let task_id = task.get_id().unwrap();
        task.set_create_time(Local.with_ymd_and_hms(2026, 8, 1, 9, 0, 0).unwrap())
            .unwrap();
        task.set_start_time(fixed_now()).unwrap();
        task.set_estimated_work_seconds(15 * 60).unwrap();
        task.set_priority(5).unwrap();
        task.sync_clock(fixed_now()).unwrap();
        let repository = RecordingRepository::new(vec![task]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "get-schedule",
                "method": "tools/call",
                "params": {"name": "get_schedule"}
            }))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "get-schedule");
        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        let schedule = response["result"]["structuredContent"]["schedule"]
            .as_array()
            .unwrap();
        assert_eq!(schedule.len(), 1);
        assert_eq!(
            sorted_object_keys(&schedule[0]),
            vec![
                "first_available_time",
                "rank",
                "scheduled_end",
                "scheduled_start",
                "scheduled_work_seconds",
                "task",
                "total_work_seconds"
            ]
        );
        let synced_now = sync_clock_times.borrow()[0];
        assert_eq!(
            schedule[0],
            json_fixture(
                include_str!("../../tests/fixtures/mcp/scheduled-task-view.json"),
                &[
                    ("{{task_id}}", &task_id.to_string()),
                    ("{{scheduled_start}}", &synced_now.to_rfc3339()),
                    (
                        "{{scheduled_end}}",
                        &(synced_now + Duration::minutes(15)).to_rfc3339(),
                    ),
                ],
            )
        );
        assert_eq!(schedule[0]["task"]["id"], task_id.to_string());
        assert_eq!(schedule[0]["task"]["name"], "scheduled task");
        assert_eq!(schedule[0]["first_available_time"], synced_now.to_rfc3339());
        assert_eq!(schedule[0]["scheduled_start"], synced_now.to_rfc3339());
        assert_eq!(
            schedule[0]["scheduled_end"],
            (synced_now + Duration::minutes(15)).to_rfc3339()
        );
        assert_eq!(schedule[0]["scheduled_work_seconds"], 15 * 60);
        assert_eq!(schedule[0]["total_work_seconds"], 15 * 60);
        assert_eq!(schedule[0]["rank"], 0);
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }

    #[test]
    fn get_scheduleは引数なしで現在から次の業務日境界までの予定だけを返す() {
        let now = Local::now();
        let current_task = TaskHandle::new("current task").unwrap();
        current_task.set_start_time(now).unwrap();
        current_task.set_estimated_work_seconds(15 * 60).unwrap();

        let future_task = TaskHandle::new("future task").unwrap();
        future_task
            .set_start_time(get_next_morning_datetime(now) + Duration::hours(1))
            .unwrap();
        future_task.set_estimated_work_seconds(15 * 60).unwrap();

        let repository = RecordingRepository::new(vec![current_task, future_task]);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "default-range",
                "get_schedule",
                json!({}),
            ))
            .unwrap();

        let schedule = response["result"]["structuredContent"]["schedule"]
            .as_array()
            .unwrap();
        assert_eq!(schedule.len(), 1);
        assert_eq!(schedule[0]["task"]["name"], "current task");
    }

    #[test]
    fn get_scheduleは日付範囲と片方の日付指定で重なる予定を返す() {
        let now = Local::now();
        let from_boundary = get_next_morning_datetime(now);
        let until_boundary = get_next_morning_datetime(from_boundary);

        let crossing_task = TaskHandle::new("crossing task").unwrap();
        crossing_task
            .set_start_time(from_boundary - Duration::minutes(30))
            .unwrap();
        crossing_task
            .set_estimated_work_seconds(2 * 60 * 60)
            .unwrap();

        let inside_task = TaskHandle::new("inside task").unwrap();
        inside_task
            .set_start_time(from_boundary + Duration::hours(3))
            .unwrap();
        inside_task.set_estimated_work_seconds(15 * 60).unwrap();

        let later_task = TaskHandle::new("later task").unwrap();
        later_task
            .set_start_time(until_boundary + Duration::hours(1))
            .unwrap();
        later_task.set_estimated_work_seconds(15 * 60).unwrap();

        let repository = RecordingRepository::new(vec![crossing_task, inside_task, later_task]);
        let mut server = initialized_server(repository);
        let from = from_boundary.format("%F").to_string();
        let until = until_boundary.format("%F").to_string();

        for (id, arguments) in [
            (
                "range",
                json!({"from": from.clone(), "until": until.clone()}),
            ),
            ("from-only", json!({"from": from.clone()})),
            ("until-only", json!({"until": until.clone()})),
        ] {
            let response = server
                .handle_request(tool_call_request(id, "get_schedule", arguments))
                .unwrap();
            let schedule = response["result"]["structuredContent"]["schedule"]
                .as_array()
                .unwrap();
            let names = schedule
                .iter()
                .map(|scheduled| scheduled["task"]["name"].as_str().unwrap())
                .collect::<Vec<_>>();

            assert_eq!(names, vec!["crossing task", "inside task"]);
            assert_eq!(
                schedule[0]["scheduled_start"],
                (from_boundary - Duration::minutes(30)).to_rfc3339()
            );
        }
    }

    #[test]
    fn get_schedule_予定なしを空配列で返す() {
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "empty-schedule",
                "get_schedule",
                json!({}),
            ))
            .unwrap();

        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["schedule"],
            json!([])
        );
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }

    #[test]
    fn get_schedule_schema違反をinvalid_paramsで返す() {
        let cases = [
            ("extra", json!({"extra": true}), "arguments.extra"),
            ("null", serde_json::Value::Null, "arguments"),
            ("invalid-from-type", json!({"from": true}), "from"),
        ];

        for (id, arguments, field) in cases {
            let repository = RecordingRepository::new(vec![]);
            let save_count = Rc::clone(&repository.save_count);
            let mutation_count = Rc::clone(&repository.mutation_count);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(id, "get_schedule", arguments))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            assert_eq!(response["error"]["code"], -32602);
            assert_eq!(response["error"]["message"], "Invalid params");
            assert_eq!(response["error"]["data"]["code"], "invalid_input");
            assert_eq!(response["error"]["data"]["field"], field);
            assert_eq!(save_count.get(), 0);
            assert_eq!(mutation_count.get(), 0);
        }
    }

    #[test]
    fn get_scheduleは不正な日付範囲をinvalid_inputで返す() {
        let cases = [
            ("invalid-date", json!({"from": "2026-02-30"}), "from"),
            (
                "reversed-range",
                json!({"from": "2026-08-13", "until": "2026-08-12"}),
                "until",
            ),
        ];

        for (id, arguments, field) in cases {
            let repository = RecordingRepository::new(vec![]);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(id, "get_schedule", arguments))
                .unwrap();

            assert_eq!(response["result"]["isError"], true);
            assert_eq!(
                response["result"]["structuredContent"]["error"]["code"],
                "invalid_input"
            );
            assert_eq!(
                response["result"]["structuredContent"]["error"]["field"],
                field
            );
        }
    }

    #[test]
    fn create_task_作成して成功時に1回saveする() {
        let pending_until = fixed_now() + Duration::hours(18);
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "create-task",
                "create_task",
                json!({
                    "name": "created by MCP",
                    "estimated_work_minutes": 30,
                    "pending_until": pending_until.to_rfc3339()
                }),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "create-task");
        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        let task_id = response["result"]["structuredContent"]["task_id"]
            .as_str()
            .unwrap();
        assert!(Uuid::parse_str(task_id).is_ok());
        assert_eq!(save_count.get(), 1);
        assert_eq!(mutation_count.get(), 1);

        let created = server
            .handle_request(tool_call_request(
                "created-task",
                "get_task",
                json!({"task_id": task_id}),
            ))
            .unwrap();
        let task = &created["result"]["structuredContent"]["task"];
        assert_eq!(task["name"], "created by MCP");
        assert_eq!(task["estimated_work_seconds"], 30 * 60);
        assert_eq!(task["original_status"], "pending");
        assert_eq!(task["pending_until"], pending_until.to_rfc3339());
        assert_eq!(save_count.get(), 1);
        assert_eq!(mutation_count.get(), 1);
    }

    #[test]
    fn create_task_schema違反ではtaskを作成せずsaveもしない() {
        let cases = [
            ("missing-name", json!({}), "name"),
            ("name-type", json!({"name": 1}), "name"),
            (
                "negative-estimate",
                json!({"name": "task", "estimated_work_minutes": -1}),
                "estimated_work_minutes",
            ),
            (
                "extra",
                json!({"name": "task", "extra": true}),
                "arguments.extra",
            ),
        ];

        for (id, arguments, field) in cases {
            let repository = RecordingRepository::new(vec![]);
            let save_count = Rc::clone(&repository.save_count);
            let mutation_count = Rc::clone(&repository.mutation_count);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(id, "create_task", arguments))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            assert_eq!(response["error"]["code"], -32602);
            assert_eq!(response["error"]["message"], "Invalid params");
            assert_eq!(response["error"]["data"]["code"], "invalid_input");
            assert_eq!(response["error"]["data"]["field"], field);
            assert_eq!(save_count.get(), 0);
            assert_eq!(mutation_count.get(), 0);
        }
    }

    #[test]
    fn create_task_意味的不正では作成もsaveもしない() {
        let cases = [
            ("empty-name", json!({"name": "  "}), "name"),
            (
                "invalid-pending",
                json!({"name": "task", "pending_until": "not-a-date"}),
                "pending_until",
            ),
            (
                "estimate-overflow",
                json!({"name": "task", "estimated_work_minutes": i64::MAX}),
                "estimated_work_minutes",
            ),
            (
                "estimate-out-of-range",
                json!({"name": "task", "estimated_work_minutes": u64::MAX}),
                "estimated_work_minutes",
            ),
        ];

        for (id, arguments, field) in cases {
            let repository = RecordingRepository::new(vec![]);
            let save_count = Rc::clone(&repository.save_count);
            let mutation_count = Rc::clone(&repository.mutation_count);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(id, "create_task", arguments))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            assert_eq!(response["result"]["isError"], true);
            assert_tool_result_content_matches_structured(&response);
            assert_eq!(
                response["result"]["structuredContent"]["error"]["code"],
                "invalid_input"
            );
            assert_eq!(
                response["result"]["structuredContent"]["error"]["field"],
                field
            );
            assert!(!response["result"]["structuredContent"]["error"]["message"]
                .as_str()
                .unwrap()
                .is_empty());
            assert_eq!(save_count.get(), 0);
            assert_eq!(mutation_count.get(), 0);
        }
    }

    #[test]
    fn create_task_save失敗を成功扱いしない() {
        let repository = RecordingRepository::new(vec![]).with_save_failure();
        let load_count = Rc::clone(&repository.load_count);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "save-failure",
                "create_task",
                json!({"name": "not persisted"}),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "save-failure");
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "repository_save_failed"
        );
        assert!(!response["result"]["structuredContent"]["error"]["message"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(save_count.get(), 1);
        assert_eq!(mutation_count.get(), 1);
        assert_eq!(sync_clock_times.borrow().len(), 1);
        assert_eq!(load_count.get(), 1);

        let poisoned_calls = [
            tool_call_request("read-after-save-failure", "list_tasks", json!({})),
            tool_call_request(
                "write-after-save-failure",
                "create_task",
                json!({"name": "must not be created"}),
            ),
            tool_call_request("unknown-after-save-failure", "unknown_tool", json!({})),
            tool_call_request("invalid-after-save-failure", "get_task", json!({})),
        ];
        for request in poisoned_calls {
            let expected_id = request["id"].clone();
            let response = server.handle_request(request).unwrap();
            assert_repository_state_uncertain_response(&response, &expected_id);
        }

        assert_eq!(save_count.get(), 1);
        assert_eq!(mutation_count.get(), 1);
        assert_eq!(sync_clock_times.borrow().len(), 1);
        assert_eq!(load_count.get(), 1);
    }

    #[test]
    fn breakdown_task_子を入力順に追加して1回saveする() {
        let pending_until = fixed_now() + Duration::hours(18);
        let parent = TaskHandle::new("parent").unwrap();
        let parent_id = parent.get_id().unwrap();
        let repository = RecordingRepository::new(vec![parent]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "breakdown",
                "breakdown_task",
                json!({
                    "parent_id": parent_id.to_string(),
                    "names": ["first child", "second child"],
                    "pending_until": pending_until.to_rfc3339()
                }),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "breakdown");
        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        let child_ids = response["result"]["structuredContent"]["child_ids"]
            .as_array()
            .unwrap();
        assert_eq!(child_ids.len(), 2);
        assert!(child_ids
            .iter()
            .all(|id| Uuid::parse_str(id.as_str().unwrap()).is_ok()));
        assert_eq!(save_count.get(), 1);

        let parent_response = server
            .handle_request(tool_call_request(
                "parent-after-breakdown",
                "get_task",
                json!({"task_id": parent_id.to_string()}),
            ))
            .unwrap();
        assert_eq!(
            parent_response["result"]["structuredContent"]["task"]["child_ids"],
            serde_json::Value::Array(child_ids.clone())
        );
        for (index, expected_name) in ["first child", "second child"].iter().enumerate() {
            let child_response = server
                .handle_request(tool_call_request(
                    "child-after-breakdown",
                    "get_task",
                    json!({"task_id": child_ids[index].as_str().unwrap()}),
                ))
                .unwrap();
            let child = &child_response["result"]["structuredContent"]["task"];
            assert_eq!(child["name"], *expected_name);
            assert_eq!(child["original_status"], "pending");
            assert_eq!(child["pending_until"], pending_until.to_rfc3339());
        }
        assert_eq!(save_count.get(), 1);
    }

    #[test]
    fn breakdown_task_schema違反では親を変更しない() {
        let cases = [
            ("missing-parent", json!({"names": ["child"]}), "parent_id"),
            (
                "missing-names",
                json!({"parent_id": Uuid::new_v4().to_string()}),
                "names",
            ),
            (
                "empty-names",
                json!({"parent_id": Uuid::new_v4().to_string(), "names": []}),
                "names",
            ),
            (
                "empty-name",
                json!({"parent_id": Uuid::new_v4().to_string(), "names": [""]}),
                "names[0]",
            ),
            (
                "name-type",
                json!({"parent_id": Uuid::new_v4().to_string(), "names": [1]}),
                "names[0]",
            ),
        ];

        for (id, arguments, field) in cases {
            let repository = RecordingRepository::new(vec![]);
            let save_count = Rc::clone(&repository.save_count);
            let mutation_count = Rc::clone(&repository.mutation_count);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(id, "breakdown_task", arguments))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            assert_eq!(response["error"]["code"], -32602);
            assert_eq!(response["error"]["message"], "Invalid params");
            assert_eq!(response["error"]["data"]["code"], "invalid_input");
            assert_eq!(response["error"]["data"]["field"], field);
            assert_eq!(save_count.get(), 0);
            assert_eq!(mutation_count.get(), 0);
        }
    }

    #[test]
    fn breakdown_task_意味的不正と未知parentでは変更もsaveもしない() {
        let parent = TaskHandle::new("parent").unwrap();
        let parent_id = parent.get_id().unwrap();
        let cases = [
            (
                "invalid-parent-id",
                json!({"parent_id": "not-a-uuid", "names": ["child"]}),
                "invalid_input",
                "parent_id",
            ),
            (
                "invalid-pending",
                json!({
                    "parent_id": parent_id.to_string(),
                    "names": ["child"],
                    "pending_until": "not-a-date"
                }),
                "invalid_input",
                "pending_until",
            ),
            (
                "invalid-name",
                json!({"parent_id": parent_id.to_string(), "names": ["  "]}),
                "invalid_input",
                "names",
            ),
            (
                "missing-parent",
                json!({"parent_id": Uuid::new_v4().to_string(), "names": ["child"]}),
                "task_not_found",
                "parent_id",
            ),
        ];

        for (id, arguments, code, field) in cases {
            let repository = RecordingRepository::new(vec![parent.clone()]);
            let save_count = Rc::clone(&repository.save_count);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(id, "breakdown_task", arguments))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            assert_eq!(response["result"]["isError"], true);
            assert_tool_result_content_matches_structured(&response);
            assert_eq!(
                response["result"]["structuredContent"]["error"]["code"],
                code
            );
            assert_eq!(
                response["result"]["structuredContent"]["error"]["field"],
                field
            );
            assert!(!response["result"]["structuredContent"]["error"]["message"]
                .as_str()
                .unwrap()
                .is_empty());
            assert_eq!(save_count.get(), 0);

            let parent_response = server
                .handle_request(tool_call_request(
                    "parent-after-error",
                    "get_task",
                    json!({"task_id": parent_id.to_string()}),
                ))
                .unwrap();
            assert_eq!(
                parent_response["result"]["structuredContent"]["task"]["child_ids"],
                json!([])
            );
        }
    }

    #[test]
    fn breakdown_task_save失敗を成功扱いしない() {
        let parent = TaskHandle::new("parent").unwrap();
        let parent_id = parent.get_id().unwrap();
        let parent_observer = parent.clone();
        let repository = RecordingRepository::new(vec![parent]).with_save_failure();
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "breakdown-save-failure",
                "breakdown_task",
                json!({"parent_id": parent_id.to_string(), "names": ["child"]}),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "breakdown-save-failure");
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "repository_save_failed"
        );
        assert!(!response["result"]["structuredContent"]["error"]["message"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(save_count.get(), 1);
        assert_eq!(parent_observer.get_children().unwrap().len(), 1);

        let parent_response = server
            .handle_request(tool_call_request(
                "parent-after-save-failure",
                "get_task",
                json!({"task_id": parent_id.to_string()}),
            ))
            .unwrap();
        assert_repository_state_uncertain_response(
            &parent_response,
            &json!("parent-after-save-failure"),
        );
    }

    #[test]
    fn defer_task_絶対時刻まで延期して1回saveする() {
        let pending_until = fixed_now() + Duration::hours(18);
        let task = TaskHandle::new("deferred task").unwrap();
        let task_id = task.get_id().unwrap();
        let repository = RecordingRepository::new(vec![task]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "defer-task",
                "defer_task",
                json!({
                    "task_id": task_id.to_string(),
                    "pending_until": pending_until.to_rfc3339()
                }),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "defer-task");
        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"],
            json!({"task_id": task_id.to_string()})
        );
        assert_eq!(save_count.get(), 1);

        let task_response = server
            .handle_request(tool_call_request(
                "deferred-task",
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        let deferred = &task_response["result"]["structuredContent"]["task"];
        assert_eq!(deferred["original_status"], "pending");
        assert_eq!(deferred["pending_until"], pending_until.to_rfc3339());
        assert_eq!(save_count.get(), 1);
    }

    #[test]
    fn defer_task_入力不正と未知taskでは変更もsaveもしない() {
        let task = TaskHandle::new("unchanged task").unwrap();
        let task_id = task.get_id().unwrap();
        let cases = [
            (
                "missing-task-id",
                json!({"pending_until": fixed_now().to_rfc3339()}),
                Some(-32602),
                "invalid_input",
                "task_id",
            ),
            (
                "missing-pending",
                json!({"task_id": task_id.to_string()}),
                Some(-32602),
                "invalid_input",
                "pending_until",
            ),
            (
                "task-id-type",
                json!({"task_id": 1, "pending_until": fixed_now().to_rfc3339()}),
                Some(-32602),
                "invalid_input",
                "task_id",
            ),
            (
                "pending-type",
                json!({"task_id": task_id.to_string(), "pending_until": 1}),
                Some(-32602),
                "invalid_input",
                "pending_until",
            ),
            (
                "extra",
                json!({
                    "task_id": task_id.to_string(),
                    "pending_until": fixed_now().to_rfc3339(),
                    "extra": true
                }),
                Some(-32602),
                "invalid_input",
                "arguments.extra",
            ),
            (
                "invalid-task-id",
                json!({"task_id": "invalid", "pending_until": fixed_now().to_rfc3339()}),
                None,
                "invalid_input",
                "task_id",
            ),
            (
                "invalid-pending",
                json!({"task_id": task_id.to_string(), "pending_until": "invalid"}),
                None,
                "invalid_input",
                "pending_until",
            ),
            (
                "missing-task",
                json!({"task_id": Uuid::new_v4().to_string(), "pending_until": fixed_now().to_rfc3339()}),
                None,
                "task_not_found",
                "task_id",
            ),
        ];

        for (id, arguments, protocol_code, tool_code, field) in cases {
            let repository = RecordingRepository::new(vec![task.clone()]);
            let save_count = Rc::clone(&repository.save_count);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(id, "defer_task", arguments))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            if let Some(protocol_code) = protocol_code {
                assert_eq!(response["error"]["code"], protocol_code);
                assert_eq!(response["error"]["message"], "Invalid params");
                assert_eq!(response["error"]["data"]["code"], tool_code);
                assert_eq!(response["error"]["data"]["field"], field);
            } else {
                assert_eq!(response["result"]["isError"], true);
                assert_tool_result_content_matches_structured(&response);
                assert_eq!(
                    response["result"]["structuredContent"]["error"]["code"],
                    tool_code
                );
                assert_eq!(
                    response["result"]["structuredContent"]["error"]["field"],
                    field
                );
                assert!(!response["result"]["structuredContent"]["error"]["message"]
                    .as_str()
                    .unwrap()
                    .is_empty());
            }
            assert_eq!(save_count.get(), 0);

            let task_response = server
                .handle_request(tool_call_request(
                    "task-after-defer-error",
                    "get_task",
                    json!({"task_id": task_id.to_string()}),
                ))
                .unwrap();
            let unchanged = &task_response["result"]["structuredContent"]["task"];
            assert_eq!(unchanged["original_status"], "todo");
            assert_eq!(unchanged["pending_until"], serde_json::Value::Null);
        }
    }

    #[test]
    fn defer_task_save失敗を成功扱いしない() {
        let pending_until = fixed_now() + Duration::hours(18);
        let task = TaskHandle::new("deferred task").unwrap();
        let task_id = task.get_id().unwrap();
        let task_observer = task.clone();
        let repository = RecordingRepository::new(vec![task]).with_save_failure();
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "defer-save-failure",
                "defer_task",
                json!({
                    "task_id": task_id.to_string(),
                    "pending_until": pending_until.to_rfc3339()
                }),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "defer-save-failure");
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "repository_save_failed"
        );
        assert!(!response["result"]["structuredContent"]["error"]["message"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(save_count.get(), 1);
        assert_eq!(task_observer.get_orig_status().unwrap(), Status::Pending);
        assert_eq!(task_observer.get_pending_until().unwrap(), pending_until);

        let task_response = server
            .handle_request(tool_call_request(
                "deferred-after-save-failure",
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        assert_repository_state_uncertain_response(
            &task_response,
            &json!("deferred-after-save-failure"),
        );
    }

    #[test]
    fn complete_task_完了と実績を反映して1回saveする() {
        let finished_at = fixed_now() + Duration::hours(1);
        let task = TaskHandle::new("completed task").unwrap();
        task.set_actual_work_seconds(60).unwrap();
        let task_id = task.get_id().unwrap();
        let repository = RecordingRepository::new(vec![task]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "complete-task",
                "complete_task",
                json!({
                    "task_id": task_id.to_string(),
                    "finished_at": finished_at.to_rfc3339(),
                    "additional_actual_work_seconds": 120
                }),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "complete-task");
        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"],
            json!({
                "task_id": task_id.to_string(),
                "next_focus_task_id": null,
                "next_repetition_task_id": null
            })
        );
        assert_eq!(save_count.get(), 1);

        let task_response = server
            .handle_request(tool_call_request(
                "completed-task",
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        let completed = &task_response["result"]["structuredContent"]["task"];
        assert_eq!(completed["original_status"], "done");
        assert_eq!(completed["end_time"], finished_at.to_rfc3339());
        assert_eq!(completed["actual_work_seconds"], 180);
        assert_eq!(save_count.get(), 1);
    }

    #[test]
    fn complete_task_optional省略時は現在時刻と追加実績0を使う() {
        let task = TaskHandle::new("completed with defaults").unwrap();
        task.set_actual_work_seconds(60).unwrap();
        let task_id = task.get_id().unwrap();
        let repository = RecordingRepository::new(vec![task]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);
        let before = Local::now();

        let response = server
            .handle_request(tool_call_request(
                "complete-defaults",
                "complete_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        let after = Local::now();

        assert_eq!(response["result"]["isError"], false);
        assert_eq!(save_count.get(), 1);
        let task_response = server
            .handle_request(tool_call_request(
                "completed-default-task",
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        let completed = &task_response["result"]["structuredContent"]["task"];
        let end_time = DateTime::parse_from_rfc3339(completed["end_time"].as_str().unwrap())
            .unwrap()
            .with_timezone(&Local);
        assert!(before <= end_time && end_time <= after);
        assert_eq!(completed["actual_work_seconds"], 60);
    }

    #[test]
    fn complete_task_next_focusとnext_repetitionのuuidを返す() {
        let parent = TaskHandle::new("parent").unwrap();
        let child = parent.create_as_last_child(TaskAttr::new("only child"));
        let parent_id = parent.get_id().unwrap();
        let child_id = child.get_id().unwrap();
        let mut focus_server = initialized_server(RecordingRepository::new(vec![parent]));

        let focus_response = focus_server
            .handle_request(tool_call_request(
                "complete-for-focus",
                "complete_task",
                json!({"task_id": child_id.to_string(), "finished_at": fixed_now().to_rfc3339()}),
            ))
            .unwrap();

        assert_eq!(
            focus_response["result"]["structuredContent"]["next_focus_task_id"],
            parent_id.to_string()
        );
        assert_eq!(
            focus_response["result"]["structuredContent"]["next_repetition_task_id"],
            serde_json::Value::Null
        );

        let repetition_parent = TaskHandle::new("weekly").unwrap();
        repetition_parent
            .set_repetition_interval_days_opt(Some(7))
            .unwrap();
        let repetition_child =
            repetition_parent.create_as_last_child(TaskAttr::new("weekly occurrence"));
        let repetition_parent_id = repetition_parent.get_id().unwrap();
        let repetition_child_id = repetition_child.get_id().unwrap();
        let mut repetition_server =
            initialized_server(RecordingRepository::new(vec![repetition_parent]));

        let repetition_response = repetition_server
            .handle_request(tool_call_request(
                "complete-for-repetition",
                "complete_task",
                json!({
                    "task_id": repetition_child_id.to_string(),
                    "finished_at": fixed_now().to_rfc3339()
                }),
            ))
            .unwrap();

        let next_repetition_id = repetition_response["result"]["structuredContent"]
            ["next_repetition_task_id"]
            .as_str()
            .unwrap();
        assert!(Uuid::parse_str(next_repetition_id).is_ok());
        let parent_response = repetition_server
            .handle_request(tool_call_request(
                "repetition-parent",
                "get_task",
                json!({"task_id": repetition_parent_id.to_string()}),
            ))
            .unwrap();
        assert!(
            parent_response["result"]["structuredContent"]["task"]["child_ids"]
                .as_array()
                .unwrap()
                .iter()
                .any(|id| id == next_repetition_id)
        );
    }

    #[test]
    fn complete_task_未完了childと未知taskを区別してsaveしない() {
        let parent = TaskHandle::new("parent").unwrap();
        parent.create_as_last_child(TaskAttr::new("undone child"));
        let parent_id = parent.get_id().unwrap();
        let cases = [
            ("undone-child", parent_id, "has_undone_children", "task_id"),
            ("missing-task", Uuid::new_v4(), "task_not_found", "task_id"),
        ];

        for (id, task_id, code, field) in cases {
            let repository = RecordingRepository::new(vec![parent.clone()]);
            let save_count = Rc::clone(&repository.save_count);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(
                    id,
                    "complete_task",
                    json!({
                        "task_id": task_id.to_string(),
                        "finished_at": fixed_now().to_rfc3339()
                    }),
                ))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            assert_eq!(response["result"]["isError"], true);
            assert_tool_result_content_matches_structured(&response);
            assert_eq!(
                response["result"]["structuredContent"]["error"]["code"],
                code
            );
            assert_eq!(
                response["result"]["structuredContent"]["error"]["field"],
                field
            );
            assert_eq!(
                response["result"]["structuredContent"]["error"]["task_id"],
                task_id.to_string()
            );
            assert!(!response["result"]["structuredContent"]["error"]["message"]
                .as_str()
                .unwrap()
                .is_empty());
            assert_eq!(save_count.get(), 0);
        }
    }

    #[test]
    fn complete_task_入力不正では変更もsaveもしない() {
        let task = TaskHandle::new("unchanged task").unwrap();
        let task_id = task.get_id().unwrap();
        let cases = [
            ("missing-task-id", json!({}), Some(-32602), "task_id"),
            (
                "negative-work",
                json!({"task_id": task_id.to_string(), "additional_actual_work_seconds": -1}),
                Some(-32602),
                "additional_actual_work_seconds",
            ),
            (
                "invalid-task-id",
                json!({"task_id": "invalid"}),
                None,
                "task_id",
            ),
            (
                "invalid-finished-at",
                json!({"task_id": task_id.to_string(), "finished_at": "invalid"}),
                None,
                "finished_at",
            ),
            (
                "work-out-of-range",
                json!({"task_id": task_id.to_string(), "additional_actual_work_seconds": u64::MAX}),
                None,
                "additional_actual_work_seconds",
            ),
            (
                "extra",
                json!({"task_id": task_id.to_string(), "extra": true}),
                Some(-32602),
                "arguments.extra",
            ),
        ];

        for (id, arguments, protocol_code, field) in cases {
            let repository = RecordingRepository::new(vec![task.clone()]);
            let save_count = Rc::clone(&repository.save_count);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(id, "complete_task", arguments))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            if let Some(protocol_code) = protocol_code {
                assert_eq!(response["error"]["code"], protocol_code);
                assert_eq!(response["error"]["message"], "Invalid params");
                assert_eq!(response["error"]["data"]["code"], "invalid_input");
                assert_eq!(response["error"]["data"]["field"], field);
            } else {
                assert_eq!(response["result"]["isError"], true);
                assert_tool_result_content_matches_structured(&response);
                assert_eq!(
                    response["result"]["structuredContent"]["error"]["code"],
                    "invalid_input"
                );
                assert_eq!(
                    response["result"]["structuredContent"]["error"]["field"],
                    field
                );
                assert!(!response["result"]["structuredContent"]["error"]["message"]
                    .as_str()
                    .unwrap()
                    .is_empty());
            }
            assert_eq!(save_count.get(), 0);
            let task_response = server
                .handle_request(tool_call_request(
                    &format!("{id}-unchanged"),
                    "get_task",
                    json!({"task_id": task_id.to_string()}),
                ))
                .unwrap();
            let unchanged = &task_response["result"]["structuredContent"]["task"];
            assert_eq!(unchanged["original_status"], "todo");
            assert_eq!(unchanged["end_time"], serde_json::Value::Null);
            assert_eq!(unchanged["actual_work_seconds"], 0);
        }
    }

    #[test]
    fn complete_task_save失敗を成功扱いしない() {
        let task = TaskHandle::new("completed task").unwrap();
        let task_id = task.get_id().unwrap();
        let task_observer = task.clone();
        let repository = RecordingRepository::new(vec![task]).with_save_failure();
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "complete-save-failure",
                "complete_task",
                json!({"task_id": task_id.to_string(), "finished_at": fixed_now().to_rfc3339()}),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "complete-save-failure");
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "repository_save_failed"
        );
        assert!(!response["result"]["structuredContent"]["error"]["message"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(save_count.get(), 1);
        assert_eq!(task_observer.get_orig_status().unwrap(), Status::Done);
        let task_response = server
            .handle_request(tool_call_request(
                "completed-after-save-failure",
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        assert_repository_state_uncertain_response(
            &task_response,
            &json!("completed-after-save-failure"),
        );
    }

    #[test]
    fn update_task_指定fieldをまとめて更新して1回saveする() {
        let deadline = fixed_now() + Duration::days(10);
        let task = TaskHandle::new("updated task").unwrap();
        let task_id = task.get_id().unwrap();
        let repository = RecordingRepository::new(vec![task]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "update-all-fields",
                "update_task",
                json!({
                    "task_id": task_id.to_string(),
                    "estimated_work_minutes": 45,
                    "deadline_time": deadline.to_rfc3339(),
                    "category": "recovery"
                }),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "update-all-fields");
        assert_eq!(response["result"]["isError"], false);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"],
            json!({"task_id": task_id.to_string()})
        );
        assert_eq!(save_count.get(), 1);

        let task_response = server
            .handle_request(tool_call_request(
                "updated-task",
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        let updated = &task_response["result"]["structuredContent"]["task"];
        assert_eq!(updated["estimated_work_seconds"], 45 * 60);
        assert_eq!(updated["deadline_time"], deadline.to_rfc3339());
        assert_eq!(updated["project_category"], "recovery");
        assert_eq!(save_count.get(), 1);
    }

    #[test]
    fn update_task_同じ値ならsaveしない() {
        let task = TaskHandle::new("unchanged task").unwrap();
        let task_id = task.get_id().unwrap();
        let repository = RecordingRepository::new(vec![task]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "update-with-same-value",
                "update_task",
                json!({
                    "task_id": task_id.to_string(),
                    "estimated_work_minutes": 15
                }),
            ))
            .unwrap();

        assert_eq!(response["result"]["isError"], false);
        assert_eq!(save_count.get(), 0);
    }

    #[test]
    fn update_task_nullでdeadlineとcategoryを解除する() {
        let task = TaskHandle::new("cleared task").unwrap();
        task.set_deadline_time_opt(Some(fixed_now() + Duration::days(10)))
            .unwrap();
        task.set_project_category_opt(Some(ProjectCategory::Investment))
            .unwrap();
        let task_id = task.get_id().unwrap();
        let repository = RecordingRepository::new(vec![task]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "clear-fields",
                "update_task",
                json!({
                    "task_id": task_id.to_string(),
                    "deadline_time": null,
                    "category": null
                }),
            ))
            .unwrap();

        assert_eq!(response["result"]["isError"], false);
        assert_eq!(save_count.get(), 1);
        let task_response = server
            .handle_request(tool_call_request(
                "cleared-task",
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        let cleared = &task_response["result"]["structuredContent"]["task"];
        assert_eq!(cleared["deadline_time"], serde_json::Value::Null);
        assert_eq!(cleared["project_category"], serde_json::Value::Null);
    }

    #[test]
    fn update_task_schemaで公開した全categoryを設定できる() {
        for category in [
            "recovery",
            "investment",
            "consumption",
            "earning",
            "sustaining",
        ] {
            let task = TaskHandle::new("categorized task").unwrap();
            let task_id = task.get_id().unwrap();
            let mut server = initialized_server(RecordingRepository::new(vec![task]));

            let response = server
                .handle_request(tool_call_request(
                    &format!("set-{category}"),
                    "update_task",
                    json!({"task_id": task_id.to_string(), "category": category}),
                ))
                .unwrap();

            assert_eq!(response["result"]["isError"], false);
            let task_response = server
                .handle_request(tool_call_request(
                    &format!("get-{category}"),
                    "get_task",
                    json!({"task_id": task_id.to_string()}),
                ))
                .unwrap();
            assert_eq!(
                task_response["result"]["structuredContent"]["task"]["project_category"],
                category
            );
        }
    }

    #[test]
    fn update_task_入力不正では変更もsaveもしない() {
        let deadline = fixed_now() + Duration::days(10);
        let task = TaskHandle::new("unchanged update task").unwrap();
        task.set_estimated_work_seconds(30 * 60).unwrap();
        task.set_deadline_time_opt(Some(deadline)).unwrap();
        task.set_project_category_opt(Some(ProjectCategory::Consumption))
            .unwrap();
        let task_id = task.get_id().unwrap();
        let cases = [
            (
                "no-update-field",
                json!({"task_id": task_id.to_string()}),
                Some(-32602),
                "arguments",
            ),
            (
                "missing-task-id",
                json!({"category": null}),
                Some(-32602),
                "task_id",
            ),
            (
                "negative-estimate",
                json!({"task_id": task_id.to_string(), "estimated_work_minutes": -1}),
                Some(-32602),
                "estimated_work_minutes",
            ),
            (
                "invalid-category",
                json!({"task_id": task_id.to_string(), "category": "unknown"}),
                Some(-32602),
                "category",
            ),
            (
                "extra",
                json!({"task_id": task_id.to_string(), "category": null, "extra": true}),
                Some(-32602),
                "arguments.extra",
            ),
            (
                "invalid-task-id",
                json!({"task_id": "invalid", "category": null}),
                None,
                "task_id",
            ),
            (
                "invalid-deadline",
                json!({"task_id": task_id.to_string(), "deadline_time": "invalid"}),
                None,
                "deadline_time",
            ),
            (
                "estimate-out-of-range",
                json!({"task_id": task_id.to_string(), "estimated_work_minutes": u64::MAX}),
                None,
                "estimated_work_minutes",
            ),
        ];

        for (id, arguments, protocol_code, field) in cases {
            let repository = RecordingRepository::new(vec![task.clone()]);
            let save_count = Rc::clone(&repository.save_count);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(id, "update_task", arguments))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            if let Some(protocol_code) = protocol_code {
                assert_eq!(response["error"]["code"], protocol_code);
                assert_eq!(response["error"]["message"], "Invalid params");
                assert_eq!(response["error"]["data"]["code"], "invalid_input");
                assert_eq!(response["error"]["data"]["field"], field);
            } else {
                assert_eq!(response["result"]["isError"], true);
                assert_tool_result_content_matches_structured(&response);
                assert_eq!(
                    response["result"]["structuredContent"]["error"]["code"],
                    "invalid_input"
                );
                assert_eq!(
                    response["result"]["structuredContent"]["error"]["field"],
                    field
                );
                assert!(!response["result"]["structuredContent"]["error"]["message"]
                    .as_str()
                    .unwrap()
                    .is_empty());
            }
            assert_eq!(save_count.get(), 0);
            let task_response = server
                .handle_request(tool_call_request(
                    &format!("{id}-unchanged"),
                    "get_task",
                    json!({"task_id": task_id.to_string()}),
                ))
                .unwrap();
            let unchanged = &task_response["result"]["structuredContent"]["task"];
            assert_eq!(unchanged["estimated_work_seconds"], 30 * 60);
            assert_eq!(unchanged["deadline_time"], deadline.to_rfc3339());
            assert_eq!(unchanged["project_category"], "consumption");
        }
    }

    #[test]
    fn update_task_application_errorと未知taskでは変更もsaveもしない() {
        let original_deadline = fixed_now() + Duration::days(10);
        let requested_deadline = fixed_now() + Duration::days(20);
        let task = TaskHandle::new("unchanged application task").unwrap();
        task.set_estimated_work_seconds(30 * 60).unwrap();
        task.set_deadline_time_opt(Some(original_deadline)).unwrap();
        task.set_project_category_opt(Some(ProjectCategory::Consumption))
            .unwrap();
        let task_id = task.get_id().unwrap();
        let missing_task_id = Uuid::new_v4();
        let cases = [
            (
                "estimate-overflow",
                task_id,
                json!({
                    "task_id": task_id.to_string(),
                    "estimated_work_minutes": i64::MAX,
                    "deadline_time": requested_deadline.to_rfc3339(),
                    "category": "investment"
                }),
                "invalid_input",
                "estimated_work_minutes",
            ),
            (
                "missing-task",
                missing_task_id,
                json!({"task_id": missing_task_id.to_string(), "category": "investment"}),
                "task_not_found",
                "task_id",
            ),
        ];

        for (id, requested_task_id, arguments, code, field) in cases {
            let repository = RecordingRepository::new(vec![task.clone()]);
            let save_count = Rc::clone(&repository.save_count);
            let mut server = initialized_server(repository);
            let response = server
                .handle_request(tool_call_request(id, "update_task", arguments))
                .unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], id);
            assert_eq!(response["result"]["isError"], true);
            assert_tool_result_content_matches_structured(&response);
            let error = &response["result"]["structuredContent"]["error"];
            assert_eq!(error["code"], code);
            assert_eq!(error["field"], field);
            assert!(!error["message"].as_str().unwrap().is_empty());
            if code == "task_not_found" {
                assert_eq!(error["task_id"], requested_task_id.to_string());
            }
            assert_eq!(save_count.get(), 0);
            let task_response = server
                .handle_request(tool_call_request(
                    &format!("{id}-unchanged"),
                    "get_task",
                    json!({"task_id": task_id.to_string()}),
                ))
                .unwrap();
            let unchanged = &task_response["result"]["structuredContent"]["task"];
            assert_eq!(unchanged["estimated_work_seconds"], 30 * 60);
            assert_eq!(unchanged["deadline_time"], original_deadline.to_rfc3339());
            assert_eq!(unchanged["project_category"], "consumption");
        }
    }

    #[test]
    fn update_task_save失敗を成功扱いしない() {
        let task = TaskHandle::new("updated before save failure").unwrap();
        let task_id = task.get_id().unwrap();
        let task_observer = task.clone();
        let repository = RecordingRepository::new(vec![task]).with_save_failure();
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(
                "update-save-failure",
                "update_task",
                json!({"task_id": task_id.to_string(), "estimated_work_minutes": 20}),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "update-save-failure");
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "repository_save_failed"
        );
        assert!(!response["result"]["structuredContent"]["error"]["message"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(save_count.get(), 1);
        assert_eq!(task_observer.get_estimated_work_seconds().unwrap(), 20 * 60);
        let task_response = server
            .handle_request(tool_call_request(
                "updated-after-save-failure",
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        assert_repository_state_uncertain_response(
            &task_response,
            &json!("updated-after-save-failure"),
        );
    }

    fn initialize_request() -> serde_json::Value {
        json!({
            "jsonrpc": "2.0",
            "id": "initialize",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            }
        })
    }

    fn initialized_server<R: TaskRepositoryTrait>(repository: R) -> McpServer<R> {
        let mut server = McpServer::new(repository);
        server.handle_request(initialize_request()).unwrap();
        server.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
        server
    }

    fn tool_call_request(id: &str, name: &str, arguments: serde_json::Value) -> serde_json::Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments}
        })
    }

    fn fixed_now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap()
    }

    fn task_for_list(
        name: &str,
        status: Status,
        category: ProjectCategory,
        create_time: DateTime<Local>,
    ) -> TaskHandle {
        let task = TaskHandle::new(name).unwrap();
        task.set_orig_status(status).unwrap();
        if status == Status::Pending {
            task.set_pending_until(Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap())
                .unwrap();
        }
        task.set_project_category_opt(Some(category)).unwrap();
        task.set_create_time(create_time).unwrap();
        task.set_start_time(create_time).unwrap();
        task.sync_clock(fixed_now()).unwrap();
        task
    }

    fn json_fixture(source: &str, replacements: &[(&str, &str)]) -> serde_json::Value {
        let mut source = source.to_owned();
        for (placeholder, value) in replacements {
            source = source.replace(placeholder, value);
        }
        serde_json::from_str(&source).unwrap()
    }

    fn sorted_object_keys(value: &serde_json::Value) -> Vec<&str> {
        let mut keys = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        keys.sort_unstable();
        keys
    }

    fn assert_tool_result_content_matches_structured(response: &serde_json::Value) {
        assert_eq!(response["result"]["content"][0]["type"], "text");
        let content = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(!content.is_empty());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(content).unwrap(),
            response["result"]["structuredContent"]
        );
    }

    fn assert_repository_state_uncertain_response(
        response: &serde_json::Value,
        expected_id: &serde_json::Value,
    ) {
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(&response["id"], expected_id);
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(response);
        let error = &response["result"]["structuredContent"]["error"];
        assert_eq!(error["code"], "repository_state_uncertain");
        assert_eq!(error["recovery"], "restart_server");
        let message = error["message"].as_str().unwrap();
        assert!(!message.is_empty());
        assert!(
            message.to_ascii_lowercase().contains("restart"),
            "{message}"
        );
    }

    fn tool<'a>(tools: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
        tools.iter().find(|tool| tool["name"] == name).unwrap()
    }

    fn required_fields<'a>(tools: &'a [serde_json::Value], name: &str) -> Vec<&'a str> {
        let mut fields = required_fields_for_tool(tool(tools, name));
        fields.sort_unstable();
        fields
    }

    fn required_fields_for_tool(tool: &serde_json::Value) -> Vec<&str> {
        tool["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field.as_str().unwrap())
            .collect()
    }

    fn property_names<'a>(tools: &'a [serde_json::Value], name: &str) -> Vec<&'a str> {
        let mut names = tool(tools, name)["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    fn property<'a>(
        tools: &'a [serde_json::Value],
        tool_name: &str,
        property_name: &str,
    ) -> &'a serde_json::Value {
        &tool(tools, tool_name)["inputSchema"]["properties"][property_name]
    }

    fn assert_string_property(
        tools: &[serde_json::Value],
        tool_name: &str,
        property_name: &str,
        format: Option<&str>,
    ) {
        let schema = property(tools, tool_name, property_name);
        assert_eq!(schema["type"], "string");
        if let Some(format) = format {
            assert_eq!(schema["format"], format);
        }
    }

    fn assert_non_negative_integer_property(
        tools: &[serde_json::Value],
        tool_name: &str,
        property_name: &str,
    ) {
        let schema = property(tools, tool_name, property_name);
        assert_eq!(schema["type"], "integer");
        assert_eq!(schema["minimum"], 0);
    }

    fn assert_nullable_string_property(
        tools: &[serde_json::Value],
        tool_name: &str,
        property_name: &str,
        format: Option<&str>,
    ) {
        let alternatives = property(tools, tool_name, property_name)["anyOf"]
            .as_array()
            .unwrap();
        assert!(alternatives.iter().any(|schema| schema["type"] == "null"));
        assert!(alternatives.iter().any(|schema| {
            schema["type"] == "string"
                && match format {
                    Some(format) => schema["format"] == format,
                    None => true,
                }
        }));
    }

    fn assert_nullable_category_schema(schema: &serde_json::Value) {
        let alternatives = schema["anyOf"].as_array().unwrap();
        assert!(alternatives.iter().any(|schema| schema["type"] == "null"));
        let string_schema = alternatives
            .iter()
            .find(|schema| schema["type"] == "string")
            .unwrap();
        assert_eq!(
            sorted_strings(&string_schema["enum"]),
            vec![
                "consumption",
                "earning",
                "investment",
                "recovery",
                "sustaining"
            ]
        );
    }

    fn sorted_strings(value: &serde_json::Value) -> Vec<&str> {
        let mut values = value
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry.as_str().unwrap())
            .collect::<Vec<_>>();
        values.sort_unstable();
        values
    }
}
