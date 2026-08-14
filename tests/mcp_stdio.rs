#![cfg(unix)]

use chrono::{Local, Timelike};
use schronu::adapter::gateway::storage_lock::{LockMode, StorageLock};
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;
use schronu::entity::task::{Status, TaskHandle};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use uuid::Uuid;

struct TestStorageDirectory {
    path: PathBuf,
}

impl TestStorageDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "schronu-mcp-stdio-test-{}",
            Uuid::new_v4().hyphenated()
        ));
        fs::create_dir(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestStorageDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
struct PermissionRestoreGuard {
    path: PathBuf,
    original: fs::Permissions,
}

#[cfg(unix)]
impl PermissionRestoreGuard {
    fn set_mode(path: &Path, mode: u32) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let original = fs::metadata(path).unwrap().permissions();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        Self {
            path: path.to_path_buf(),
            original,
        }
    }
}

#[cfg(unix)]
impl Drop for PermissionRestoreGuard {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, self.original.clone());
    }
}

#[test]
fn mcp_stdio_stdoutにinitializeとtools_listのjson_rpc応答だけを出力する() {
    let storage = TestStorageDirectory::new();
    let mut child = spawn_mcp(storage.path());

    let requests = [
        json!({
            "jsonrpc": "2.0",
            "id": "initialize",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "integration-test", "version": "1.0"}
            }
        }),
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
        json!({
            "jsonrpc": "2.0",
            "id": "tools-list",
            "method": "tools/list"
        }),
    ];
    {
        let stdin = child.stdin.as_mut().unwrap();
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
    }
    drop(child.stdin.take());

    let output = wait_with_output(child);
    assert_process_succeeded(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let responses = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["jsonrpc"], "2.0");
    assert_eq!(responses[0]["id"], "initialize");
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(responses[1]["jsonrpc"], "2.0");
    assert_eq!(responses[1]["id"], "tools-list");
    assert_eq!(responses[1]["result"]["tools"].as_array().unwrap().len(), 9);
}

#[test]
fn mcp_stdio_壊れたjsonにparse_errorを返し次のrequestも処理する() {
    let storage = TestStorageDirectory::new();
    let mut child = spawn_mcp(storage.path());
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": "initialize-after-parse-error",
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "integration-test", "version": "1.0"}
        }
    });
    let input = format!("not-json\n{initialize}\n");
    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(input.as_bytes()).unwrap();
    }
    drop(child.stdin.take());

    let output = wait_with_output(child);
    assert_process_succeeded(&output);
    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["jsonrpc"], "2.0");
    assert_eq!(responses[0]["id"], serde_json::Value::Null);
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[0]["error"]["message"], "Parse error");
    assert_eq!(responses[1]["id"], "initialize-after-parse-error");
    assert_eq!(responses[1]["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn mcp_stdio_不正なinitialize_request後も同一processで正常に初期化できる() {
    let storage = TestStorageDirectory::new();
    let mut child = spawn_mcp(storage.path());
    let requests = [
        json!({
            "jsonrpc": "2.0",
            "id": "invalid-initialize",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "integration-test"}
            }
        }),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({
            "jsonrpc": "2.0",
            "id": "valid-initialize",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "integration-test", "version": "1.0"}
            }
        }),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({
            "jsonrpc": "2.0",
            "id": "tools-after-valid-initialize",
            "method": "tools/list"
        }),
    ];
    {
        let stdin = child.stdin.as_mut().unwrap();
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
    }
    drop(child.stdin.take());

    let output = wait_with_output(child);
    assert_process_succeeded(&output);
    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["id"], "invalid-initialize");
    assert_eq!(responses[0]["error"]["code"], -32602);
    assert_eq!(responses[0]["error"]["message"], "Invalid params");
    assert!(responses[0]["error"]["data"]["field"]
        .as_str()
        .is_some_and(|field| !field.is_empty()));
    assert!(responses[0]["error"]["data"]["reason"]
        .as_str()
        .is_some_and(|reason| !reason.is_empty()));
    assert_eq!(responses[1]["id"], "valid-initialize");
    assert_eq!(responses[1]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(responses[2]["id"], "tools-after-valid-initialize");
    assert_eq!(responses[2]["result"]["tools"].as_array().unwrap().len(), 9);
}

#[test]
fn mcp_stdio_initialized後のidle中はcliがlockを取得できる() {
    let storage = TestStorageDirectory::new();
    let mut mcp = McpSession::spawn(storage.path());

    mcp.initialize("idle");
    let lock = StorageLock::acquire(storage.path(), LockMode::Cli).unwrap();
    assert!(fs::read_to_string(lock.path())
        .unwrap()
        .contains("mode=cli"));
    drop(lock);

    assert_process_succeeded(&mcp.finish());
}

#[test]
fn mcp_stdio_lock競合時はload前にerrorを返し修復後に同一sessionで再試行できる() {
    let storage = TestStorageDirectory::new();
    let project_directory = storage.path().join("broken");
    fs::create_dir(&project_directory).unwrap();
    let project_yaml = project_directory.join("project.yaml");
    fs::write(&project_yaml, "project: [").unwrap();
    let mut mcp = McpSession::spawn(storage.path());
    mcp.initialize("lock-contention");

    let cli_lock = StorageLock::acquire(storage.path(), LockMode::Cli).unwrap();
    let contended = mcp.call_tool(
        "contended",
        "create_task",
        json!({"name": "must not be created while contended"}),
    );
    assert_structured_tool_error(
        &contended,
        "contended",
        "repository_lock_contended",
        "retry",
    );
    let holder_metadata = contended["result"]["structuredContent"]["error"]["holder_metadata"]
        .as_str()
        .unwrap();
    assert!(holder_metadata.contains("mode=cli"));
    assert_eq!(fs::read_to_string(&project_yaml).unwrap(), "project: [");

    drop(cli_lock);
    fs::remove_dir_all(&project_directory).unwrap();
    let retried = mcp.call_tool("retry-list", "list_tasks", json!({}));
    assert_eq!(retried["jsonrpc"], "2.0");
    assert_eq!(retried["id"], "retry-list");
    assert_eq!(retried["result"]["isError"], false);
    assert_eq!(
        retried["result"]["structuredContent"]["tasks"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let created = mcp.call_tool(
        "retry-create",
        "create_task",
        json!({"name": "created after lock release"}),
    );
    assert_eq!(created["result"]["isError"], false);

    assert_process_succeeded(&mcp.finish());
}

#[test]
fn mcp_stdio_壊れたrepositoryはcallでerrorとなり修復後に同一sessionで再試行できる() {
    let storage = TestStorageDirectory::new();
    let project_directory = storage.path().join("broken");
    fs::create_dir(&project_directory).unwrap();
    fs::write(project_directory.join("project.yaml"), "project: [").unwrap();
    let mut mcp = McpSession::spawn(storage.path());
    mcp.initialize("broken-repository");

    let failed = mcp.call_tool("load-failure", "list_tasks", json!({}));
    assert_structured_tool_error(
        &failed,
        "load-failure",
        "repository_load_failed",
        "repair_repository",
    );

    fs::remove_dir_all(&project_directory).unwrap();
    let retried = mcp.call_tool("load-retry", "list_tasks", json!({}));
    assert_eq!(retried["jsonrpc"], "2.0");
    assert_eq!(retried["id"], "load-retry");
    assert_eq!(retried["result"]["isError"], false);

    assert_process_succeeded(&mcp.finish());
}

#[test]
fn mcp_stdio_lock_symlinkはcallでstructured_errorとなり参照先を変更しない() {
    use std::os::unix::fs::symlink;

    let storage = TestStorageDirectory::new();
    let sentinel = storage.path().join("sentinel");
    let sentinel_content = "sentinel must not change\n";
    fs::write(&sentinel, sentinel_content).unwrap();
    let lock_path = storage.path().join(".lock");
    symlink(&sentinel, &lock_path).unwrap();
    let mut mcp = McpSession::spawn(storage.path());
    mcp.initialize("lock-symlink");

    let failed = mcp.call_tool("lock-symlink", "list_tasks", json!({}));
    assert_structured_tool_error(
        &failed,
        "lock-symlink",
        "repository_lock_failed",
        "inspect_storage",
    );
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), sentinel_content);
    assert!(fs::symlink_metadata(&lock_path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_link(&lock_path).unwrap(), sentinel);

    fs::remove_file(&lock_path).unwrap();
    let retried = mcp.call_tool("lock-symlink-retry", "list_tasks", json!({}));
    assert_eq!(retried["jsonrpc"], "2.0");
    assert_eq!(retried["id"], "lock-symlink-retry");
    assert_eq!(retried["result"]["isError"], false);
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), sentinel_content);
    assert_process_succeeded(&mcp.finish());
}

#[test]
fn mcp_stdio_9つのtoolをfilesystem上のrepositoryで実行し再起動後も保存内容を読む() {
    let storage = TestStorageDirectory::new();
    let create = call_tool(
        storage.path(),
        "create",
        "create_task",
        Some(json!({"name": "integration project", "estimated_work_minutes": 30})),
    );
    let parent_id = create["result"]["structuredContent"]["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let breakdown = call_tool(
        storage.path(),
        "breakdown",
        "breakdown_task",
        Some(json!({"parent_id": parent_id, "names": ["integration child"]})),
    );
    let child_id = breakdown["result"]["structuredContent"]["child_ids"][0]
        .as_str()
        .unwrap()
        .to_string();

    let focus = call_tool(storage.path(), "focus", "get_focus", None);
    assert_eq!(focus["result"]["isError"], false);
    assert!(focus["result"]["structuredContent"]
        .as_object()
        .unwrap()
        .contains_key("task"));

    let task = call_tool(
        storage.path(),
        "task",
        "get_task",
        Some(json!({"task_id": child_id})),
    );
    assert_eq!(
        task["result"]["structuredContent"]["task"]["name"],
        "integration child"
    );

    let tasks = call_tool(storage.path(), "tasks", "list_tasks", None);
    assert_eq!(
        tasks["result"]["structuredContent"]["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let schedule = call_tool(storage.path(), "schedule", "get_schedule", None);
    assert!(schedule["result"]["structuredContent"]["schedule"].is_array());

    let pending_until = (Local::now() + chrono::Duration::hours(2))
        .with_nanosecond(0)
        .unwrap();
    let deferred = call_tool(
        storage.path(),
        "defer",
        "defer_task",
        Some(json!({
            "task_id": child_id,
            "pending_until": pending_until.to_rfc3339()
        })),
    );
    assert_eq!(deferred["result"]["isError"], false);

    let deadline = (Local::now() + chrono::Duration::days(10))
        .with_nanosecond(0)
        .unwrap();
    let updated = call_tool(
        storage.path(),
        "update",
        "update_task",
        Some(json!({
            "task_id": child_id,
            "estimated_work_minutes": 10,
            "deadline_time": deadline.to_rfc3339(),
            "category": "recovery"
        })),
    );
    assert_eq!(updated["result"]["isError"], false);

    let reloaded_child = call_tool(
        storage.path(),
        "reloaded-child",
        "get_task",
        Some(json!({"task_id": child_id})),
    );
    let reloaded_child = &reloaded_child["result"]["structuredContent"]["task"];
    assert_eq!(reloaded_child["original_status"], "pending");
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(reloaded_child["pending_until"].as_str().unwrap())
            .unwrap(),
        pending_until.fixed_offset()
    );
    assert_eq!(reloaded_child["estimated_work_seconds"], 10 * 60);
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(reloaded_child["deadline_time"].as_str().unwrap())
            .unwrap(),
        deadline.fixed_offset()
    );
    assert_eq!(reloaded_child["project_category"], "recovery");

    let child_completed = call_tool(
        storage.path(),
        "complete-child",
        "complete_task",
        Some(json!({"task_id": child_id})),
    );
    assert_eq!(child_completed["result"]["isError"], false);
    let parent_completed = call_tool(
        storage.path(),
        "complete-parent",
        "complete_task",
        Some(json!({"task_id": parent_id})),
    );
    assert_eq!(parent_completed["result"]["isError"], false);

    let reloaded = call_tool(
        storage.path(),
        "reloaded-parent",
        "get_task",
        Some(json!({"task_id": parent_id})),
    );
    assert_eq!(
        reloaded["result"]["structuredContent"]["task"]["original_status"],
        "done"
    );
}

#[test]
fn mcp_stdio_tools_call直前の現在時刻同期で期限切れpendingをtodoとして読む() {
    let storage = TestStorageDirectory::new();
    let pending_until = Local::now() + chrono::Duration::seconds(3);
    let mut repository = TaskRepository::new(storage.path().to_str().unwrap());
    repository.sync_clock(Local::now());
    let task = TaskHandle::new("pending across MCP idle time");
    let task_id = task.get_id().to_owned();
    task.set_start_time(Local::now() - chrono::Duration::hours(1));
    task.set_pending_until(pending_until);
    task.set_orig_status(Status::Pending);
    repository.start_new_project(task);
    repository.save().unwrap();

    let mut mcp = McpSession::spawn(storage.path());
    mcp.initialize("clock-before-call");
    let initialized_at = Local::now();
    assert!(
        initialized_at < pending_until,
        "MCP初期化完了時点でpending_untilを過ぎています: initialized_at={initialized_at}, pending_until={pending_until}"
    );
    let remaining = (pending_until - initialized_at)
        .to_std()
        .expect("pending_untilはMCP初期化完了時刻より後であるべきです");
    thread::sleep(remaining + Duration::from_millis(50));
    let call_started_at = Local::now();
    assert!(
        call_started_at > pending_until,
        "tools/call開始前にpending_untilを過ぎていません: call_started_at={call_started_at}, pending_until={pending_until}"
    );
    let reloaded = mcp.call_tool(
        "get-expired-pending",
        "get_task",
        json!({"task_id": task_id.to_string()}),
    );

    assert_eq!(
        reloaded["result"]["structuredContent"]["task"]["original_status"],
        "pending"
    );
    assert_eq!(
        reloaded["result"]["structuredContent"]["task"]["status"],
        "todo"
    );
    assert_process_succeeded(&mcp.finish());
}

#[test]
fn mcp_stdio複数processはcallごとの再読込で互いのwriteを保持する() {
    let storage = TestStorageDirectory::new();
    let mut mcp_a = McpSession::spawn(storage.path());
    let mut mcp_b = McpSession::spawn(storage.path());
    mcp_a.initialize("freshness-a");
    mcp_b.initialize("freshness-b");

    let created_a = mcp_a.call_tool("create-a", "create_task", json!({"name": "created by A"}));
    assert_eq!(created_a["result"]["isError"], false);
    let created_b = mcp_b.call_tool("create-b", "create_task", json!({"name": "created by B"}));
    assert_eq!(created_b["result"]["isError"], false);

    let listed = mcp_a.call_tool("list-after-b", "list_tasks", json!({}));
    let names = listed["result"]["structuredContent"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"created by A"));
    assert!(names.contains(&"created by B"));

    assert_process_succeeded(&mcp_a.finish());
    assert_process_succeeded(&mcp_b.finish());
}

#[test]
fn mcp_stdio_task不明と入力不正と未完了childのerror_codeを区別する() {
    let storage = TestStorageDirectory::new();
    let missing_id = Uuid::new_v4().to_string();
    let missing = call_tool(
        storage.path(),
        "missing",
        "get_task",
        Some(json!({"task_id": missing_id})),
    );
    assert_eq!(
        missing["result"]["structuredContent"]["error"]["code"],
        "task_not_found"
    );

    let invalid_name = call_tool(
        storage.path(),
        "invalid-name",
        "create_task",
        Some(json!({"name": ""})),
    );
    assert_eq!(invalid_name["error"]["code"], -32602);
    assert_eq!(invalid_name["error"]["data"]["field"], "name");
    let invalid_number = call_tool(
        storage.path(),
        "invalid-number",
        "create_task",
        Some(json!({"name": "invalid estimate", "estimated_work_minutes": -1})),
    );
    assert_eq!(invalid_number["error"]["code"], -32602);
    assert_eq!(
        invalid_number["error"]["data"]["field"],
        "estimated_work_minutes"
    );
    let invalid_datetime = call_tool(
        storage.path(),
        "invalid-datetime",
        "defer_task",
        Some(json!({"task_id": missing_id, "pending_until": "invalid"})),
    );
    assert_eq!(
        invalid_datetime["result"]["structuredContent"]["error"]["code"],
        "invalid_input"
    );
    assert_eq!(
        invalid_datetime["result"]["structuredContent"]["error"]["field"],
        "pending_until"
    );

    let created = call_tool(
        storage.path(),
        "create-parent-with-child",
        "create_task",
        Some(json!({"name": "parent with child"})),
    );
    let parent_id = created["result"]["structuredContent"]["task_id"]
        .as_str()
        .unwrap();
    let breakdown = call_tool(
        storage.path(),
        "add-undone-child",
        "breakdown_task",
        Some(json!({"parent_id": parent_id, "names": ["undone child"]})),
    );
    assert_eq!(breakdown["result"]["isError"], false);
    let rejected = call_tool(
        storage.path(),
        "reject-parent-completion",
        "complete_task",
        Some(json!({"task_id": parent_id})),
    );
    assert_eq!(rejected["result"]["isError"], true);
    assert_eq!(
        rejected["result"]["structuredContent"]["error"]["code"],
        "has_undone_children"
    );
}

#[cfg(unix)]
#[test]
fn mcp_stdio_filesystemへのsave失敗後は後続tool_callを拒否する() {
    use std::os::unix::fs::PermissionsExt;

    let storage = TestStorageDirectory::new();
    let mut mcp = McpSession::spawn(storage.path());
    mcp.initialize("save-failure");
    let lock_file_initializer = StorageLock::acquire(storage.path(), LockMode::Cli).unwrap();
    drop(lock_file_initializer);
    let original_mode = fs::metadata(storage.path()).unwrap().permissions().mode();
    let permission_guard = PermissionRestoreGuard::set_mode(storage.path(), 0o500);
    let probe_path = storage.path().join("permission-probe");
    match fs::write(&probe_path, b"probe") {
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied),
        Ok(()) => {
            let _ = fs::remove_file(probe_path);
            panic!("test requires directory write permission to be denied");
        }
    }
    let save_failed = mcp.call_tool(
        "save-failure",
        "create_task",
        json!({"name": "cannot save"}),
    );
    drop(permission_guard);
    assert_eq!(
        fs::metadata(storage.path()).unwrap().permissions().mode(),
        original_mode
    );

    assert_eq!(save_failed["id"], "save-failure");
    assert_eq!(save_failed["result"]["isError"], true);
    assert_eq!(
        save_failed["result"]["structuredContent"]["error"]["code"],
        "repository_save_failed"
    );

    let cli_lock = StorageLock::acquire(storage.path(), LockMode::Cli).unwrap();
    let poisoned = mcp.call_tool("read-after-save-failure", "list_tasks", json!({}));
    assert_structured_tool_error(
        &poisoned,
        "read-after-save-failure",
        "repository_state_uncertain",
        "restart_server",
    );
    drop(cli_lock);

    assert_process_succeeded(&mcp.finish());
}

fn call_tool(storage_directory: &Path, id: &str, name: &str, arguments: Option<Value>) -> Value {
    let mut child = spawn_mcp(storage_directory);
    let mut tool_params = json!({"name": name});
    if let Some(arguments) = arguments {
        tool_params["arguments"] = arguments;
    }
    let requests = [
        json!({
            "jsonrpc": "2.0",
            "id": format!("initialize-{id}"),
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "integration-test", "version": "1.0"}
            }
        }),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": tool_params
        }),
    ];
    {
        let stdin = child.stdin.as_mut().unwrap();
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
    }
    drop(child.stdin.take());
    let output = wait_with_output(child);
    assert_process_succeeded(&output);
    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[1]["id"], id);
    responses.into_iter().nth(1).unwrap()
}

struct McpSession {
    child: Option<Child>,
    responses: mpsc::Receiver<Result<String, std::io::Error>>,
    stdout_log: Arc<Mutex<Vec<String>>>,
    stdout_reader: Option<JoinHandle<()>>,
    stderr_log: Arc<Mutex<Vec<String>>>,
    stderr_reader: Option<JoinHandle<()>>,
}

impl McpSession {
    fn spawn(storage_directory: &Path) -> Self {
        let mut child = spawn_mcp(storage_directory);
        let stdout = child.stdout.take().unwrap();
        let (sender, responses) = mpsc::channel();
        let stdout_log = Arc::new(Mutex::new(Vec::new()));
        let stdout_log_for_reader = Arc::clone(&stdout_log);
        let stdout_reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if let Ok(line) = &line {
                    stdout_log_for_reader.lock().unwrap().push(line.clone());
                }
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let stderr = child.stderr.take().unwrap();
        let stderr_log = Arc::new(Mutex::new(Vec::new()));
        let stderr_log_for_reader = Arc::clone(&stderr_log);
        let stderr_reader = thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                match line {
                    Ok(line) => stderr_log_for_reader.lock().unwrap().push(line),
                    Err(error) => {
                        stderr_log_for_reader
                            .lock()
                            .unwrap()
                            .push(format!("stderr read failed: {error}"));
                        break;
                    }
                }
            }
        });
        Self {
            child: Some(child),
            responses,
            stdout_log,
            stdout_reader: Some(stdout_reader),
            stderr_log,
            stderr_reader: Some(stderr_reader),
        }
    }

    fn initialize(&mut self, id_suffix: &str) {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": format!("initialize-{id_suffix}"),
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "integration-test", "version": "1.0"}
            }
        }));
        let response = self.read_response();
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], format!("initialize-{id_suffix}"));
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        self.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        self.send(json!({
            "jsonrpc": "2.0",
            "id": format!("tools-list-after-initialize-{id_suffix}"),
            "method": "tools/list"
        }));
        let tools_list = self.read_response();
        assert_eq!(tools_list["jsonrpc"], "2.0");
        assert_eq!(
            tools_list["id"],
            format!("tools-list-after-initialize-{id_suffix}")
        );
        assert!(tools_list["result"]["tools"].is_array());
    }

    fn call_tool(&mut self, id: &str, name: &str, arguments: Value) -> Value {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments}
        }));
        self.read_response()
    }

    fn send(&mut self, request: Value) {
        let result = writeln!(
            self.child.as_mut().unwrap().stdin.as_mut().unwrap(),
            "{request}"
        );
        if let Err(error) = result {
            self.response_failure(&format!("stdin write failed: {error}"));
        }
    }

    fn read_response(&self) -> Value {
        let line = match self.responses.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => self.response_failure(&format!("stdout read failed: {error}")),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.response_failure("did not return a response within 5 seconds")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.response_failure("exited before returning a response")
            }
        };
        serde_json::from_str(&line).unwrap()
    }

    fn response_failure(&self, reason: &str) -> ! {
        panic!(
            "schronu-mcp {reason}\nstdout so far:\n{}\nstderr so far:\n{}",
            self.stdout_log.lock().unwrap().join("\n"),
            self.stderr_log.lock().unwrap().join("\n")
        )
    }

    fn finish(mut self) -> Output {
        drop(self.child.as_mut().unwrap().stdin.take());
        let (status, timed_out) = wait_for_child(self.child.as_mut().unwrap());
        self.stdout_reader.take().unwrap().join().unwrap();
        self.stderr_reader.take().unwrap().join().unwrap();
        let output = Output {
            status,
            stdout: self.stdout_log.lock().unwrap().join("\n").into_bytes(),
            stderr: self.stderr_log.lock().unwrap().join("\n").into_bytes(),
        };
        self.child.take();
        if timed_out {
            panic!(
                "schronu-mcp timed out\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        output
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        drop(child.stdin.take());
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
        if let Some(stdout_reader) = self.stdout_reader.take() {
            let _ = stdout_reader.join();
        }
        if let Some(stderr_reader) = self.stderr_reader.take() {
            let _ = stderr_reader.join();
        }
    }
}

fn assert_structured_tool_error(
    response: &Value,
    expected_id: &str,
    expected_code: &str,
    expected_recovery: &str,
) {
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], expected_id);
    assert_eq!(response["result"]["isError"], true);
    let structured = &response["result"]["structuredContent"];
    let content = response["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<Value>(content).unwrap(), *structured);
    let error = &structured["error"];
    assert_eq!(error["code"], expected_code);
    assert_eq!(error["recovery"], expected_recovery);
    assert!(error["message"]
        .as_str()
        .is_some_and(|message| !message.is_empty()));
}

fn spawn_mcp(storage_directory: &Path) -> Child {
    let executable = env!("CARGO_BIN_EXE_schronu-mcp");
    Command::new(executable)
        .env("SCHRONU_STORAGE_DIR", storage_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn assert_process_succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "schronu-mcp exited with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn wait_with_output(child: Child) -> Output {
    let mut child = child;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "schronu-mcp timed out\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_child(child: &mut Child) -> (std::process::ExitStatus, bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return (status, false);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return (child.wait().unwrap(), true);
        }
        thread::sleep(Duration::from_millis(10));
    }
}
