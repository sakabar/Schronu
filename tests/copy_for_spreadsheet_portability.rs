#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const STANDARD_MACOS_PATH: &str = "/usr/bin:/bin";
static TEMPORARY_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        let sequence = TEMPORARY_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "schronu-copy-for-spreadsheet-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("temporary command directory is created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repository_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn run_copy_script(path: &str, input: &str) -> Output {
    let mut child = Command::new("/bin/zsh")
        .arg(repository_path("shell/copy_for_spreadsheet.sh"))
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("copy_for_spreadsheet.sh starts");
    child
        .stdin
        .take()
        .expect("script stdin")
        .write_all(input.as_bytes())
        .expect("fixture is written to script stdin");
    child.wait_with_output().expect("script exits")
}

fn assert_failure_without_output(output: Output) {
    assert!(
        !output.status.success(),
        "script unexpectedly succeeded with stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stdout.is_empty(),
        "failed conversion must not publish task rows or padding: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn copy_for_spreadsheetはmacos標準pathでtask行とpaddingを生成する() {
    let fixture_directory = repository_path("tests/fixtures/copy_for_spreadsheet_portability");
    let input = fs::read_to_string(fixture_directory.join("cli-output.txt"))
        .expect("CLI output fixture exists");
    let expected_task_rows = fs::read_to_string(fixture_directory.join("task-rows.tsv"))
        .expect("task rows fixture exists");

    let output = run_copy_script(STANDARD_MACOS_PATH, &input);
    assert!(
        output.status.success(),
        "script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("script output is UTF-8");
    let lines: Vec<_> = stdout.lines().collect();
    let expected_task_lines: Vec<_> = expected_task_rows.lines().collect();
    assert_eq!(&lines[..expected_task_lines.len()], expected_task_lines);
    assert_eq!(lines.len(), expected_task_lines.len() + 50);
    assert!(
        lines[expected_task_lines.len()..]
            .iter()
            .all(|line| *line == "\t\t\t\t\t\t\t\t\t\t"),
        "all padding rows have ten empty columns"
    );
}

#[test]
fn copy_for_spreadsheetは前段command失敗時に出力を公開しない() {
    let command_directory = TemporaryDirectory::new();
    let fake_awk = command_directory.path().join("awk");
    fs::write(
        &fake_awk,
        "#!/bin/sh\nprintf '%s\\n' '0000\t22222222-2222-2222-2222-222222222222\t!\t____-00:00\t06/21(土)-18:00~18:40\t0\t40\t01\t維\t日本語 task'\nexit 71\n",
    )
    .expect("fake awk is written");
    let mut permissions = fs::metadata(&fake_awk)
        .expect("fake awk metadata exists")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_awk, permissions).expect("fake awk is executable");

    let path = format!(
        "{}:{STANDARD_MACOS_PATH}",
        command_directory.path().display()
    );
    assert_failure_without_output(run_copy_script(&path, "ignored input\n"));
}

#[test]
fn copy_for_spreadsheetは必須command欠落時に出力を公開しない() {
    let empty_path = TemporaryDirectory::new();
    assert_failure_without_output(run_copy_script(
        empty_path.path().to_str().expect("temporary path is UTF-8"),
        "ignored input\n",
    ));
}
