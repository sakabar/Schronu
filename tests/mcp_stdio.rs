#![cfg(unix)]

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
fn mcp_stdio_serverは稼働中にlockを保持し終了後に解放する() {
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
fn mcp_stdio_lock取得後のrepository_load失敗をstderrへ出力する() {
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
fn mcp_stdio_起動時にload前の現在時刻を同期して期限切れpendingをtodoとして読む() {
    let storage = TestStorageDirectory::new();
    let pending_until = (Local::now() - chrono::Duration::hours(1))
        .with_nanosecond(0)
        .unwrap();
    let create = call_tool(
        storage.path(),
        "create-expired-pending",
        "create_task",
        Some(json!({
            "name": "expired pending",
            "pending_until": pending_until.to_rfc3339()
        })),
    );
    assert_eq!(create["result"]["isError"], false);
    let task_id = create["result"]["structuredContent"]["task_id"]
        .as_str()
        .unwrap();

    let reloaded = call_tool(
        storage.path(),
        "get-expired-pending",
        "get_task",
        Some(json!({"task_id": task_id})),
    );

    assert_eq!(
        reloaded["result"]["structuredContent"]["task"]["original_status"],
        "pending"
    );
    assert_eq!(
        reloaded["result"]["structuredContent"]["task"]["status"],
        "todo"
    );
}

#[test]
fn mcp_stdio稼働中は同じ保存先を使うcli_processの起動を拒否する() {
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
    let child = spawn_mcp(storage.path());
    let (mut child, _) = wait_for_lock_metadata(child, &storage.path().join(".lock"));
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
    let requests = [
        json!({
            "jsonrpc": "2.0",
            "id": "initialize-save-failure",
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
            "id": "save-failure",
            "method": "tools/call",
            "params": {
                "name": "create_task",
                "arguments": {"name": "cannot save"}
            }
        }),
        json!({
            "jsonrpc": "2.0",
            "id": "read-after-save-failure",
            "method": "tools/call",
            "params": {
                "name": "list_tasks",
                "arguments": {}
            }
        }),
        json!({
            "jsonrpc": "2.0",
            "id": "write-after-save-failure",
            "method": "tools/call",
            "params": {
                "name": "create_task",
                "arguments": {"name": "must not be created"}
            }
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
    drop(permission_guard);
    assert_eq!(
        fs::metadata(storage.path()).unwrap().permissions().mode(),
        original_mode
    );

    assert_process_succeeded(&output);
    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 4);
    assert_eq!(responses[1]["id"], "save-failure");
    assert_eq!(responses[1]["result"]["isError"], true);
    assert_eq!(
        responses[1]["result"]["structuredContent"]["error"]["code"],
        "repository_save_failed"
    );
    for (response, expected_id) in [
        (&responses[2], "read-after-save-failure"),
        (&responses[3], "write-after-save-failure"),
    ] {
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], expected_id);
        assert_eq!(response["result"]["isError"], true);
        let structured = &response["result"]["structuredContent"];
        let content = response["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(serde_json::from_str::<Value>(content).unwrap(), *structured);
        let error = &structured["error"];
        assert_eq!(error["code"], "repository_state_uncertain");
        assert_eq!(error["recovery"], "restart_server");
        let message = error["message"].as_str().unwrap();
        assert!(!message.is_empty());
        assert!(
            message.to_ascii_lowercase().contains("restart"),
            "{message}"
        );
    }
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
