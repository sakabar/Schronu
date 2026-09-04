#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn repository_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn run_copy_script(input: &str) -> std::process::Output {
    let mut child = Command::new("/bin/zsh")
        .arg(repository_path("shell/copy_for_spreadsheet.sh"))
        .env("PATH", "/usr/bin:/bin")
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
fn copy_for_spreadsheetはrank上限境界のtask行だけを列順どおり保持する() {
    let fixture_directory = repository_path("tests/fixtures/copy_for_spreadsheet_rank_boundary");
    let input = fs::read_to_string(fixture_directory.join("cli-output.txt"))
        .expect("CLI output fixture exists");
    let expected_task_rows = fs::read_to_string(fixture_directory.join("task-rows.tsv"))
        .expect("task rows fixture exists");

    let output = run_copy_script(&input);
    assert!(
        output.status.success(),
        "script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("script output is UTF-8");
    let lines: Vec<_> = stdout.lines().collect();
    let task_lines: Vec<_> = lines
        .iter()
        .filter(|line| line.chars().any(|character| character != '\t'))
        .collect();
    let actual_task_rows = task_lines
        .iter()
        .map(|line| line.split('\t').take(10).collect::<Vec<_>>().join("\t"))
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(actual_task_rows + "\n", expected_task_rows);
    assert!(task_lines.iter().all(|line| line.split('\t').count() == 18));
    assert_eq!(lines.len(), task_lines.len() + 50);
}

#[test]
fn copy_for_spreadsheetは不完全な数値rank候補を元入力line番号付きerrorにする() {
    let uuid = "44444444-4444-4444-4444-444444444444";
    let cases = [
        ("A-I途中token不足", format!("1000 {uuid} !")),
        (
            "J空",
            format!("1000 {uuid} ! ____-00:40 06/21(土)-18:00~18:40 0 40 01 獲"),
        ),
        (
            "short rank",
            format!("999 {uuid} ! ____-00:40 06/21(土)-18:00~18:40 0 40 01 獲 task"),
        ),
        (
            "UUID",
            "1000 invalid-uuid ! ____-00:40 06/21(土)-18:00~18:40 0 40 01 獲 task".to_string(),
        ),
        (
            "scheduled time",
            format!("1000 {uuid} ! ____-00:40 06/21(土)-18:00-18:40 0 40 01 獲 task"),
        ),
        (
            "F priority",
            format!("1000 {uuid} ! ____-00:40 06/21(土)-18:00~18:40 high 40 01 獲 task"),
        ),
        (
            "G estimated",
            format!("1000 {uuid} ! ____-00:40 06/21(土)-18:00~18:40 0 forty 01 獲 task"),
        ),
        (
            "H signed project priority",
            format!("1000 {uuid} ! ____-00:40 06/21(土)-18:00~18:40 0 40 --1 獲 task"),
        ),
        (
            "I category",
            format!("1000 {uuid} ! ____-00:40 06/21(土)-18:00~18:40 0 40 01 未 task"),
        ),
    ];

    for (case_name, malformed_row) in cases {
        let input = format!(
            "rank task_id icon remaining_time scheduled_time priority estimated_minutes project_number category task_name\n[Warn] この行はtask候補ではありません。\n{malformed_row}\n"
        );
        let output = run_copy_script(&input);

        assert!(
            !output.status.success(),
            "{case_name}: incomplete task row must fail instead of being skipped"
        );
        assert!(
            output.stdout.is_empty(),
            "{case_name}: stdout must stay empty on failure: {}",
            String::from_utf8_lossy(&output.stdout)
        );

        let stderr = String::from_utf8(output.stderr).expect("script error is UTF-8");
        assert!(
            stderr.contains("line 3:"),
            "{case_name}: error must report the original input line: {stderr}"
        );
    }
}
