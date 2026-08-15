#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn repository_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn run_script(path: &str, arguments: &[&str], input: &str) -> String {
    let mut child = Command::new("zsh")
        .arg(repository_path(path))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spreadsheet script starts");

    child
        .stdin
        .take()
        .expect("script stdin")
        .write_all(input.as_bytes())
        .expect("fixture is written to script stdin");

    let output = child.wait_with_output().expect("spreadsheet script exits");
    assert!(
        output.status.success(),
        "script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("script output is UTF-8")
}

#[test]
fn spreadsheet_column_manifestはaからs列の既存契約を定義する() {
    let manifest = fs::read_to_string(repository_path("spreadsheet_columns.tsv"))
        .expect("spreadsheet column manifest exists");

    let rows: Vec<_> = manifest
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert_eq!(rows.len(), 19);
    assert!(rows.iter().any(|row| row.starts_with("B\t2\ttask_id\t")));
    assert!(rows.iter().any(|row| row.starts_with("J\t10\ttask_name\t")));
    assert!(rows.iter().any(|row| row.starts_with("L\t12\tstart_time\t")));
    assert!(rows.iter().any(|row| row.starts_with("N\t14\tfinish_flag\t")));
    assert!(rows.iter().any(|row| row.starts_with("P\t16\tfinish_time\t")));
    assert!(rows.iter().any(|row| row.starts_with("Q\t17\tshould_extract\t")));
    assert!(rows.iter().any(|row| row.starts_with("R\t18\tdefer_command\t")));
    assert!(rows.iter().any(|row| row.starts_with("S\t19\tactual_work_time\t")));
}

#[test]
fn cli出力からspreadsheetを経由してコマンド生成まで列契約を維持する() {
    let cli_output = fs::read_to_string(repository_path("tests/fixtures/spreadsheet/cli-output.txt"))
        .expect("CLI output fixture exists");
    let copied = run_script("shell/copy_for_spreadsheet.sh", &[], &cli_output);
    let copied_rows: Vec<_> = copied.lines().filter(|line| !line.is_empty()).collect();
    let expected_copied = fs::read_to_string(repository_path("tests/fixtures/spreadsheet/copied-rows.tsv"))
        .expect("copied rows fixture exists");
    assert_eq!(copied_rows.join("\n") + "\n", expected_copied);

    let spreadsheet = fs::read_to_string(repository_path("tests/fixtures/spreadsheet/sheet-values.tsv"))
        .expect("spreadsheet values fixture exists");
    let commands = run_script(
        "shell/generate_command_from_spreadsheet.sh",
        &["--stdin"],
        &spreadsheet,
    );
    let expected_commands =
        fs::read_to_string(repository_path("tests/fixtures/spreadsheet/generated-commands.txt"))
            .expect("generated commands fixture exists");
    assert_eq!(commands, expected_commands);
}

#[test]
fn implementation_and_documentationはmanifestの重要な列契約を明記する() {
    let manifest = fs::read_to_string(repository_path("spreadsheet_columns.tsv"))
        .expect("spreadsheet column manifest exists");
    let copy_script = fs::read_to_string(repository_path("shell/copy_for_spreadsheet.sh")).unwrap();
    let generate_script =
        fs::read_to_string(repository_path("shell/generate_command_from_spreadsheet.sh")).unwrap();
    let apps_script = fs::read_to_string(repository_path("apps_script/main.js")).unwrap();
    let readme = fs::read_to_string(repository_path("README.md")).unwrap();
    let apps_readme = fs::read_to_string(repository_path("apps_script/README.md")).unwrap();

    for expected in [
        "B\t2\ttask_id\t",
        "J\t10\ttask_name\t",
        "L\t12\tstart_time\t",
        "N\t14\tfinish_flag\t",
        "P\t16\tfinish_time\t",
        "Q\t17\tshould_extract\t",
        "R\t18\tdefer_command\t",
        "S\t19\tactual_work_time\t",
    ] {
        assert!(manifest.contains(expected), "manifest entry missing: {expected}");
    }

    assert!(copy_script.contains("cut -f1-10"));
    assert!(generate_script.contains("task_id = trim($2)"));
    assert!(generate_script.contains("task_name = trim($10)"));
    assert!(generate_script.contains("finish_flag = trim($14)"));
    assert!(generate_script.contains("finish_datetime = trim($16)"));
    assert!(generate_script.contains("should_extract = trim($17)"));
    assert!(generate_script.contains("defer_command = trim($18)"));
    assert!(generate_script.contains("actual_work_minutes = trim($19)"));
    assert!(apps_script.contains("taskIdCol: 2"));
    assert!(apps_script.contains("syncCols: [12, 14, 16, 18]"));
    assert!(apps_script.contains("timeFormatRanges: ['L3:M500', 'O3:P500']"));
    assert!(readme.contains("R列"));
    assert!(readme.contains("P列"));
    assert!(readme.contains("S列"));
    assert!(apps_readme.contains("| B | `task_id`"));
    assert!(apps_readme.contains("| L | 同期対象 |"));
    assert!(apps_readme.contains("| N | 同期対象 |"));
    assert!(apps_readme.contains("| P | 同期対象 |"));
    assert!(apps_readme.contains("| Q | 抽出対象。"));
    assert!(apps_readme.contains("| R | 同期対象。"));
}
