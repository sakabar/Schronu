use chrono::{Local, Timelike};
use schronu::adapter::gateway::storage_lock::{LockMode, StorageLock, StorageLockErrorKind};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
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

#[test]
fn mcp_stdio_initializeとtools_listをprotocol専用stdoutへ返す() {
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
fn mcp_stdio_壊れたjsonへparse_errorを返して次のrequestを処理する() {
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
fn mcp_stdio_process中はlockを保持し終了後に解放する() {
    let storage = TestStorageDirectory::new();
    let child = spawn_mcp(storage.path());
    let lock_path = storage.path().join(".lock");

    let (mut child, metadata) = wait_for_lock_metadata(child, &lock_path);
    assert!(metadata.contains("mode=mcp"));
    let error = StorageLock::acquire(storage.path(), LockMode::Cli).unwrap_err();
    assert_eq!(error.kind(), StorageLockErrorKind::Contended);

    drop(child.stdin.take());
    let output = wait_with_output(child);
    assert_process_succeeded(&output);
    let _cli_lock = StorageLock::acquire(storage.path(), LockMode::Cli).unwrap();
}

#[test]
fn mcp_stdio_lock取得後のrepository_load失敗をstderrへ返す() {
    let storage = TestStorageDirectory::new();
    let project_directory = storage.path().join("broken");
    fs::create_dir(&project_directory).unwrap();
    fs::write(project_directory.join("project.yaml"), "project: [").unwrap();
    let mut child = spawn_mcp(storage.path());
    drop(child.stdin.take());

    let output = wait_with_output(child);
    assert_process_failed(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("repository Load failed"), "{stderr}");
    let metadata = fs::read_to_string(storage.path().join(".lock")).unwrap();
    assert!(metadata.contains("mode=mcp"));
}

#[test]
fn mcp_stdio_9toolを実repositoryで実行し再起動後も保存内容を読む() {
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
fn mcp_stdio稼働中は同じ保存先の実cli起動を拒否する() {
    let storage = TestStorageDirectory::new();
    let child = spawn_mcp(storage.path());
    let (mut mcp, _) = wait_for_lock_metadata(child, &storage.path().join(".lock"));
    let cli_executable = option_env!("CARGO_BIN_EXE_schronu")
        .expect("schronu binary must be built for integration tests");
    let cli = Command::new(cli_executable)
        .env("SCHRONU_STORAGE_DIR", storage.path())
        .arg("全")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let cli_output = wait_with_output(cli);
    assert_process_failed(&cli_output);
    assert_eq!(String::from_utf8_lossy(&cli_output.stdout), "");
    let stderr = String::from_utf8_lossy(&cli_output.stderr);
    assert!(stderr.contains("storage lock is already held"), "{stderr}");
    assert!(stderr.contains("mode=mcp"), "{stderr}");

    drop(mcp.stdin.take());
    assert_process_succeeded(&wait_with_output(mcp));
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

fn spawn_mcp(storage_directory: &Path) -> Child {
    let executable = option_env!("CARGO_BIN_EXE_schronu-mcp")
        .expect("schronu-mcp binary must be built for integration tests");
    Command::new(executable)
        .env("SCHRONU_STORAGE_DIR", storage_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn wait_for_lock_metadata(mut child: Child, lock_path: &Path) -> (Child, String) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_metadata = None;
    loop {
        if let Ok(metadata) = fs::read_to_string(lock_path) {
            if metadata.contains("mode=mcp") {
                return (child, metadata);
            }
            last_metadata = Some(metadata);
        }
        if let Some(status) = child.try_wait().unwrap() {
            let output = child.wait_with_output().unwrap();
            panic!(
                "schronu-mcp exited with {status} before acquiring lock\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "schronu-mcp did not acquire lock\nlast metadata:\n{}\nstdout:\n{}\nstderr:\n{}",
                last_metadata.as_deref().unwrap_or("<unreadable>"),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
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

fn assert_process_failed(output: &Output) {
    assert!(
        !output.status.success(),
        "schronu-mcp unexpectedly succeeded with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn wait_with_output(mut child: Child) -> Output {
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
