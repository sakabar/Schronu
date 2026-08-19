use crate::adapter::gateway::storage_lock::{
    LockMode, StorageLock, StorageLockError, StorageLockErrorKind,
};
use crate::application::interface::TaskRepositoryTrait;
use crate::application::repository_transaction::{
    run_repository_transaction, RepositoryTransactionError,
};
use crate::application::schedule_use_case::ScheduledTaskView;
use crate::application::task_use_case::{ApplicationError, TaskView};
use chrono::Local;
use serde_json::{json, Value};
use std::path::PathBuf;
use uuid::Uuid;

mod error;
mod handler;
mod input;
mod protocol;
mod registry;

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
            "tools/list" => Some(tools_list_response(id, registry::tool_definitions())),
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
                let response = handler::call_tool(repository, id.clone(), request);
                let should_save = handler::tool_call_succeeded_with_mutation(request, &response)
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

#[cfg(test)]
mod protocol_contract_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tool_contract_tests;
