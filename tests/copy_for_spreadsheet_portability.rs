#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const STANDARD_MACOS_PATH: &str = "/usr/bin:/bin";

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
