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
