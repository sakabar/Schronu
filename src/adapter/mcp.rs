use crate::application::interface::TaskRepositoryTrait;
use serde_json::{json, Value};

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LifecycleState {
    Uninitialized,
    InitializeResponded,
    Initialized,
}

pub struct McpServer<R> {
    _repository: R,
    lifecycle_state: LifecycleState,
}

impl<R: TaskRepositoryTrait> McpServer<R> {
    pub fn new(repository: R) -> Self {
        Self {
            _repository: repository,
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
            _ => Some(error_response(id, -32601, "Method not found")),
        }
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
    use serde_json::json;

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
