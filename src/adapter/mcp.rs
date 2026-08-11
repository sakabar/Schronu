use crate::application::interface::TaskRepositoryTrait;
use crate::application::schedule_use_case::{get_schedule, ScheduledTaskView};
use crate::application::task_use_case::{
    get_focus, get_task, list_tasks, ApplicationError, ListTasksFilter, TaskPeriodField,
    TaskPeriodFilter, TaskView,
};
use crate::entity::task::{ProjectCategory, Status};
use chrono::{DateTime, Local};
use serde_json::Map;
use serde_json::{json, Value};
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
    lifecycle_state: LifecycleState,
}

impl<R: TaskRepositoryTrait> McpServer<R> {
    pub fn new(repository: R) -> Self {
        Self {
            repository,
            lifecycle_state: LifecycleState::Uninitialized,
        }
    }

    pub fn handle_request(&mut self, request: Value) -> Option<Value> {
        let method = request.get("method").and_then(Value::as_str);
        let Some(id) = request.get("id").cloned() else {
            if method == Some("notifications/initialized")
                && self.lifecycle_state == LifecycleState::InitializeResponded
            {
                self.lifecycle_state = LifecycleState::Initialized;
            }
            return None;
        };

        match method {
            Some("initialize") if self.lifecycle_state == LifecycleState::Uninitialized => {
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
            Some("initialize") => Some(error_response(id, -32600, "Invalid Request")),
            Some("tools/list") if self.lifecycle_state != LifecycleState::Initialized => {
                Some(error_response(id, -32002, "Server not initialized"))
            }
            Some("tools/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"tools": tool_definitions()}
            })),
            Some("tools/call") if self.lifecycle_state != LifecycleState::Initialized => {
                Some(error_response(id, -32002, "Server not initialized"))
            }
            Some("tools/call") => Some(self.call_tool(id, &request)),
            _ => Some(error_response(id, -32601, "Method not found")),
        }
    }

    fn call_tool(&mut self, id: Value, request: &Value) -> Value {
        let params = &request["params"];
        match params["name"].as_str() {
            Some("get_focus") => self.call_get_focus(id, params.get("arguments")),
            Some("get_task") => self.call_get_task(id, &params["arguments"]),
            Some("list_tasks") => self.call_list_tasks(id, params.get("arguments")),
            Some("get_schedule") => self.call_get_schedule(id, params.get("arguments")),
            _ => error_response(id, -32602, "Unknown tool"),
        }
    }

    fn call_get_focus(&mut self, id: Value, arguments: Option<&Value>) -> Value {
        if let Some(arguments) = arguments {
            if let Err(error) = validate_argument_object(arguments, &[], &[]) {
                return invalid_params_response(id, error);
            }
        }

        let task = get_focus(&mut self.repository)
            .as_ref()
            .map(task_view_json)
            .unwrap_or(Value::Null);
        tool_result_response(id, json!({"task": task}), false)
    }

    fn call_get_task(&self, id: Value, arguments: &Value) -> Value {
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

        match get_task(&self.repository, task_id) {
            Some(task) => tool_result_response(id, json!({"task": task_view_json(&task)}), false),
            None => tool_result_response(
                id,
                json!({
                    "error": {
                        "code": "task_not_found",
                        "message": format!("task not found: {task_id}"),
                        "task_id": task_id.to_string()
                    }
                }),
                true,
            ),
        }
    }

    fn call_list_tasks(&self, id: Value, arguments: Option<&Value>) -> Value {
        let filter = match list_tasks_filter(arguments) {
            Ok(filter) => filter,
            Err(ListTasksInputError::Schema(error)) => return invalid_params_response(id, error),
            Err(ListTasksInputError::Semantic { field, message }) => {
                return invalid_input_response(id, field, message)
            }
        };

        match list_tasks(&self.repository, filter) {
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
            Err(error) => tool_result_response(
                id,
                json!({
                    "error": {
                        "code": "internal_error",
                        "message": error.to_string()
                    }
                }),
                true,
            ),
        }
    }

    fn call_get_schedule(&self, id: Value, arguments: Option<&Value>) -> Value {
        if let Some(arguments) = arguments {
            if let Err(error) = validate_argument_object(arguments, &[], &[]) {
                return invalid_params_response(id, error);
            }
        }

        let schedule = get_schedule(&self.repository)
            .iter()
            .map(scheduled_task_view_json)
            .collect::<Vec<_>>();
        tool_result_response(id, json!({"schedule": schedule}), false)
    }
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

#[derive(Debug)]
struct InvalidParams {
    field: String,
    reason: &'static str,
}

enum ListTasksInputError {
    Schema(InvalidParams),
    Semantic {
        field: &'static str,
        message: &'static str,
    },
}

fn list_tasks_filter(arguments: Option<&Value>) -> Result<ListTasksFilter, ListTasksInputError> {
    let Some(arguments) = arguments else {
        return Ok(ListTasksFilter {
            period: None,
            statuses: vec![],
            categories: vec![],
        });
    };
    let arguments = validate_argument_object(arguments, &["period", "statuses", "categories"], &[])
        .map_err(ListTasksInputError::Schema)?;

    Ok(ListTasksFilter {
        period: arguments
            .get("period")
            .map(parse_period_filter)
            .transpose()?,
        statuses: parse_status_filters(arguments.get("statuses"))?,
        categories: parse_category_filters(arguments.get("categories"))?,
    })
}

fn parse_period_filter(value: &Value) -> Result<TaskPeriodFilter, ListTasksInputError> {
    let period = value.as_object().ok_or_else(|| {
        ListTasksInputError::Schema(InvalidParams {
            field: "period".to_string(),
            reason: "must be an object",
        })
    })?;
    if let Some(field) = period
        .keys()
        .find(|field| !["field", "from", "until"].contains(&field.as_str()))
    {
        return Err(ListTasksInputError::Schema(InvalidParams {
            field: format!("period.{field}"),
            reason: "additional property is not allowed",
        }));
    }
    for field in ["field", "from", "until"] {
        if !period.contains_key(field) {
            return Err(ListTasksInputError::Schema(InvalidParams {
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
            return Err(ListTasksInputError::Schema(InvalidParams {
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
) -> Result<&'a str, ListTasksInputError> {
    object.get(field).and_then(Value::as_str).ok_or_else(|| {
        ListTasksInputError::Schema(InvalidParams {
            field: format!("{object_name}.{field}"),
            reason: "must be a string",
        })
    })
}

fn parse_datetime(
    value: &str,
    field: &'static str,
) -> Result<DateTime<Local>, ListTasksInputError> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Local))
        .map_err(|_| ListTasksInputError::Semantic {
            field,
            message: "must be a valid RFC 3339 date-time",
        })
}

fn parse_status_filters(value: Option<&Value>) -> Result<Vec<Status>, ListTasksInputError> {
    let Some(value) = value else {
        return Ok(vec![]);
    };
    let values = value.as_array().ok_or_else(|| {
        ListTasksInputError::Schema(InvalidParams {
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
            _ => Err(ListTasksInputError::Schema(InvalidParams {
                field: format!("statuses[{index}]"),
                reason: "must be todo, pending, or done",
            })),
        })
        .collect()
}

fn parse_category_filters(
    value: Option<&Value>,
) -> Result<Vec<Option<ProjectCategory>>, ListTasksInputError> {
    let Some(value) = value else {
        return Ok(vec![]);
    };
    let values = value.as_array().ok_or_else(|| {
        ListTasksInputError::Schema(InvalidParams {
            field: "categories".to_string(),
            reason: "must be an array",
        })
    })?;

    values
        .iter()
        .enumerate()
        .map(|(index, value)| match value.as_str() {
            None if value.is_null() => Ok(None),
            Some("earning") => Ok(Some(ProjectCategory::Earning)),
            Some("sustaining") => Ok(Some(ProjectCategory::Sustaining)),
            Some("recovery") => Ok(Some(ProjectCategory::Recovery)),
            Some("investment") => Ok(Some(ProjectCategory::Investment)),
            Some("consumption") => Ok(Some(ProjectCategory::Consumption)),
            _ => Err(ListTasksInputError::Schema(InvalidParams {
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
            "description": "Get Schronu's calculated task schedule.",
            "inputSchema": {
                "type": "object",
                "properties": {},
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
    use crate::application::interface::{TaskRepositoryError, TaskRepositoryTrait};
    use crate::entity::task::{ProjectCategory, RepetitionAnchor, Status, Task, TaskAttr};
    use chrono::{DateTime, Duration, Local, TimeZone};
    use serde_json::json;
    use std::cell::Cell;
    use std::rc::Rc;
    use uuid::Uuid;

    struct RecordingRepository {
        projects: Vec<Task>,
        now: DateTime<Local>,
        focus_task_id: Option<Uuid>,
        save_count: Rc<Cell<usize>>,
        mutation_count: Rc<Cell<usize>>,
    }

    impl RecordingRepository {
        fn new(projects: Vec<Task>) -> Self {
            Self {
                projects,
                now: fixed_now(),
                focus_task_id: None,
                save_count: Rc::new(Cell::new(0)),
                mutation_count: Rc::new(Cell::new(0)),
            }
        }

        fn with_focus_task_id(mut self, task_id: Uuid) -> Self {
            self.focus_task_id = Some(task_id);
            self
        }
    }

    impl TaskRepositoryTrait for RecordingRepository {
        fn get_project_storage_dir_name(&self) -> &str {
            "unused"
        }

        fn get_all_projects(&self) -> Vec<&Task> {
            self.projects.iter().collect()
        }

        fn load(&mut self) -> Result<(), TaskRepositoryError> {
            self.mutation_count.set(self.mutation_count.get() + 1);
            Ok(())
        }

        fn save(&self) -> Result<(), TaskRepositoryError> {
            self.save_count.set(self.save_count.get() + 1);
            Ok(())
        }

        fn sync_clock(&mut self, now: DateTime<Local>) {
            self.mutation_count.set(self.mutation_count.get() + 1);
            self.now = now;
        }

        fn get_last_synced_time(&self) -> DateTime<Local> {
            self.now
        }

        fn get_highest_priority_project(&mut self) -> Option<&Task> {
            self.projects.first()
        }

        fn get_highest_priority_leaf_task_id(&mut self) -> Option<Uuid> {
            self.focus_task_id
        }

        fn get_defer_candidate_leaf_task_id(&mut self, _recent_days: i64) -> Option<Uuid> {
            None
        }

        fn get_by_id(&self, id: Uuid) -> Option<Task> {
            self.projects.iter().find_map(|task| task.get_by_id(id))
        }

        fn start_new_project(&mut self, root_task: Task) {
            self.mutation_count.set(self.mutation_count.get() + 1);
            self.projects.push(root_task);
        }
    }

    #[test]
    fn initialize_server情報とtool能力を返す() {
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
    fn initialize_非対応version要求にはserver対応versionを返す() {
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
    fn request_未知methodにはmethod_not_foundを返す() {
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
    fn notification_未知methodには応答しない() {
        let mut server = McpServer::new(TaskRepository::new(""));
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/unknown"
        });

        assert_eq!(server.handle_request(notification), None);
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
    fn tools_list_initialized通知後に9toolのschemaを返す() {
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
        assert_eq!(property_names(tools, "get_schedule"), Vec::<&str>::new());
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
    fn get_task_task_viewをstructured_contentで返してsaveしない() {
        let pending_until = Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap();
        let create_time = Local.with_ymd_and_hms(2026, 8, 1, 9, 0, 0).unwrap();
        let start_time = Local.with_ymd_and_hms(2026, 8, 10, 10, 0, 0).unwrap();
        let deadline_time = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
        let root = Task::new("MCP task");
        root.set_orig_status(Status::Pending);
        root.set_pending_until(pending_until);
        root.set_priority(7);
        root.set_create_time(create_time);
        root.set_start_time(start_time);
        root.set_deadline_time_opt(Some(deadline_time));
        root.set_estimated_work_seconds(1_800);
        root.set_actual_work_seconds(900);
        root.set_atomic(true);
        root.set_is_on_other_side(true);
        root.set_repetition_interval_days_opt(Some(7));
        root.set_repetition_anchor(RepetitionAnchor::Completion);
        root.set_days_in_advance(2);
        root.set_project_category_opt(Some(ProjectCategory::Recovery));
        root.sync_clock(fixed_now());
        let child = root.create_as_last_child(TaskAttr::new("child"));
        let task_id = root.get_id();
        let child_id = child.get_id();
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
        let non_focused_task = Task::new("not focused");
        let focused_task = Task::new("focused task");
        let task_id = focused_task.get_id();
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
        let matching_id = matching.get_id();
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
        let first = Task::new("first");
        let first_id = first.get_id();
        let child = first.create_as_last_child(TaskAttr::new("child"));
        let child_id = child.get_id();
        let second = Task::new("second");
        let second_id = second.get_id();
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
        let uncategorized = Task::new("uncategorized");
        let uncategorized_id = uncategorized.get_id();
        let categorized = Task::new("categorized");
        categorized.set_project_category_opt(Some(ProjectCategory::Recovery));
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
    fn get_schedule_typed予定を返してrepositoryを変更しない() {
        let task = Task::new("scheduled task");
        let task_id = task.get_id();
        task.set_start_time(fixed_now());
        task.set_estimated_work_seconds(15 * 60);
        task.set_priority(5);
        task.sync_clock(fixed_now());
        let repository = RecordingRepository::new(vec![task]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
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
        assert_eq!(schedule[0]["task"]["id"], task_id.to_string());
        assert_eq!(schedule[0]["task"]["name"], "scheduled task");
        assert_eq!(
            schedule[0]["first_available_time"],
            fixed_now().to_rfc3339()
        );
        assert_eq!(schedule[0]["scheduled_start"], fixed_now().to_rfc3339());
        assert_eq!(
            schedule[0]["scheduled_end"],
            (fixed_now() + Duration::minutes(15)).to_rfc3339()
        );
        assert_eq!(schedule[0]["scheduled_work_seconds"], 15 * 60);
        assert_eq!(schedule[0]["total_work_seconds"], 15 * 60);
        assert_eq!(schedule[0]["rank"], 0);
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
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
    ) -> Task {
        let task = Task::new(name);
        task.set_orig_status(status);
        if status == Status::Pending {
            task.set_pending_until(Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap());
        }
        task.set_project_category_opt(Some(category));
        task.set_create_time(create_time);
        task.set_start_time(create_time);
        task.sync_clock(fixed_now());
        task
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
