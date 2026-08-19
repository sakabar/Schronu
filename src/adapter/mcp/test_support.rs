use super::McpServer;
pub(super) use crate::adapter::gateway::task_repository::TaskRepository;
pub(super) use crate::application::interface::{
    RepositoryReloadOutcome, TaskRepositoryError, TaskRepositoryOperation, TaskRepositoryTrait,
};
pub(super) use crate::entity::datetime::get_next_morning_datetime;
pub(super) use crate::entity::task::{
    ProjectCategory, RepetitionAnchor, Status, TaskAttr, TaskHandle,
};
pub(super) use chrono::{DateTime, Duration, Local, TimeZone};
pub(super) use serde_json::json;
use std::cell::{Cell, RefCell};
pub(super) use std::fs;
use std::path::PathBuf;
pub(super) use std::rc::Rc;
pub(super) use uuid::Uuid;

pub(super) struct McpCacheTestStorage {
    pub(super) path: PathBuf,
}

impl McpCacheTestStorage {
    pub(super) fn new() -> Self {
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

pub(super) struct RecordingRepository {
    projects: Vec<TaskHandle>,
    now: DateTime<Local>,
    focus_task_id: Option<Uuid>,
    fail_load_once: bool,
    fail_save: bool,
    pub(super) load_count: Rc<Cell<usize>>,
    pub(super) reload_if_changed_count: Rc<Cell<usize>>,
    pub(super) project_count: Rc<Cell<usize>>,
    pub(super) save_count: Rc<Cell<usize>>,
    pub(super) mutation_count: Rc<Cell<usize>>,
    persisted_project_revisions: RefCell<Vec<u64>>,
    pub(super) operation_order: Rc<RefCell<Vec<&'static str>>>,
    pub(super) sync_clock_times: Rc<RefCell<Vec<DateTime<Local>>>>,
}

impl RecordingRepository {
    pub(super) fn new(projects: Vec<TaskHandle>) -> Self {
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

    pub(super) fn with_focus_task_id(mut self, task_id: Uuid) -> Self {
        self.focus_task_id = Some(task_id);
        self
    }

    pub(super) fn with_save_failure(mut self) -> Self {
        self.fail_save = true;
        self
    }

    pub(super) fn with_load_failure_once(mut self) -> Self {
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
                .map_err(|error| TaskRepositoryError::new(TaskRepositoryOperation::Save, error))?;
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

pub(super) fn initialize_request() -> serde_json::Value {
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

pub(super) fn initialized_server<R: TaskRepositoryTrait>(repository: R) -> McpServer<R> {
    let mut server = McpServer::new(repository);
    server.handle_request(initialize_request()).unwrap();
    server.handle_request(json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));
    server
}

pub(super) fn tool_call_request(
    id: &str,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments}
    })
}

pub(super) fn fixed_now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap()
}

pub(super) fn task_for_list(
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

pub(super) fn json_fixture(source: &str, replacements: &[(&str, &str)]) -> serde_json::Value {
    let mut source = source.to_owned();
    for (placeholder, value) in replacements {
        source = source.replace(placeholder, value);
    }
    serde_json::from_str(&source).unwrap()
}

pub(super) fn sorted_object_keys(value: &serde_json::Value) -> Vec<&str> {
    let mut keys = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys
}

pub(super) fn assert_tool_result_content_matches_structured(response: &serde_json::Value) {
    assert_eq!(response["result"]["content"][0]["type"], "text");
    let content = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(!content.is_empty());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(content).unwrap(),
        response["result"]["structuredContent"]
    );
}

pub(super) fn assert_repository_state_uncertain_response(
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

pub(super) fn tool<'a>(tools: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
    tools.iter().find(|tool| tool["name"] == name).unwrap()
}

pub(super) fn required_fields<'a>(tools: &'a [serde_json::Value], name: &str) -> Vec<&'a str> {
    let mut fields = required_fields_for_tool(tool(tools, name));
    fields.sort_unstable();
    fields
}

pub(super) fn required_fields_for_tool(tool: &serde_json::Value) -> Vec<&str> {
    tool["inputSchema"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| field.as_str().unwrap())
        .collect()
}

pub(super) fn property_names<'a>(tools: &'a [serde_json::Value], name: &str) -> Vec<&'a str> {
    let mut names = tool(tools, name)["inputSchema"]["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

pub(super) fn property<'a>(
    tools: &'a [serde_json::Value],
    tool_name: &str,
    property_name: &str,
) -> &'a serde_json::Value {
    &tool(tools, tool_name)["inputSchema"]["properties"][property_name]
}

pub(super) fn assert_string_property(
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

pub(super) fn assert_non_negative_integer_property(
    tools: &[serde_json::Value],
    tool_name: &str,
    property_name: &str,
) {
    let schema = property(tools, tool_name, property_name);
    assert_eq!(schema["type"], "integer");
    assert_eq!(schema["minimum"], 0);
}

pub(super) fn assert_nullable_string_property(
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

pub(super) fn assert_nullable_category_schema(schema: &serde_json::Value) {
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

pub(super) fn sorted_strings(value: &serde_json::Value) -> Vec<&str> {
    let mut values = value
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry.as_str().unwrap())
        .collect::<Vec<_>>();
    values.sort_unstable();
    values
}
