#![cfg(unix)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const COLUMN_COUNT: usize = 19;

fn repository_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn spreadsheet_row(
    task_id: &str,
    task_name: &str,
    finish_flag: &str,
    finish_datetime: &str,
    actual_work_time: &str,
) -> String {
    let mut columns = vec![""; COLUMN_COUNT];
    columns[1] = task_id;
    columns[9] = task_name;
    columns[13] = finish_flag;
    columns[15] = finish_datetime;
    columns[16] = "TRUE";
    columns[18] = actual_work_time;
    columns.join("\t")
}

fn run_import(input: &str) -> Output {
    let mut child = Command::new("zsh")
        .arg(repository_path(
            "shell/generate_command_from_spreadsheet.sh",
        ))
        .arg("--stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Spreadsheet import script starts");
    child
        .stdin
        .take()
        .expect("script stdin")
        .write_all(input.as_bytes())
        .expect("Spreadsheet rows are written to stdin");
    child
        .wait_with_output()
        .expect("Spreadsheet import script exits")
}

#[test]
fn s列のminuteとsecondが範囲外なら全commandを出力せず拒否する() {
    for invalid_time in ["0:60:00", "0:00:60", "0:99:99"] {
        let input = format!(
            "{}\n{}\n",
            spreadsheet_row("valid-id", "valid task", "F", "", "0:01:00"),
            spreadsheet_row("invalid-id", "invalid task", "F", "", invalid_time),
        );
        let output = run_import(&input);
        let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");

        assert!(!output.status.success(), "{invalid_time} must be rejected");
        assert_eq!(output.stdout, b"", "commands must be emitted atomically");
        assert!(
            stderr.contains(&format!("line 2: S列の形式が不正です: {invalid_time}")),
            "unexpected stderr for {invalid_time}: {stderr}"
        );
    }
}

#[test]
fn s列は境界内の時分秒と複数桁hourを受理する() {
    let input = format!(
        "{}\n{}\n",
        spreadsheet_row("boundary-id", "boundary task", "F", "", "0:59:59"),
        spreadsheet_row("multi-hour-id", "multi hour task", "F", "", "12:34:56"),
    );
    let output = run_import(&input);

    assert!(
        output.status.success(),
        "valid durations must be accepted: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("働 59\n"));
    assert!(stdout.contains("働 754\n"));
}

#[test]
fn s列の24時間表現は形式ではなく既存の合計上限として拒否する() {
    let input = format!(
        "{}\n",
        spreadsheet_row("over-limit-id", "over limit task", "F", "", "24:00:00")
    );
    let output = run_import(&input);
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");

    assert!(!output.status.success());
    assert_eq!(output.stdout, b"");
    assert!(
        stderr.contains("line 1: 働の分数が1380(23時間)を超えています: over-limit-id (1440分)"),
        "unexpected stderr: {stderr}"
    );
    assert!(!stderr.contains("S列の形式が不正です"));
}

#[test]
fn p列の存在しない暦日と範囲外時刻は全commandを出力せず拒否する() {
    for invalid_datetime in [
        "2026/02/31 9:10:00",
        "2025/02/29 9:10:00",
        "1900/02/29 9:10:00",
        "2026/04/31 9:10:00",
        "2026/06/31 9:10:00",
        "2026/09/31 9:10:00",
        "2026/11/31 9:10:00",
        "2026/12/31 24:00:00",
    ] {
        let input = format!(
            "{}\n{}\n",
            spreadsheet_row(
                "valid-id",
                "valid task",
                "",
                "2026/01/01 9:00:00",
                "0:01:00",
            ),
            spreadsheet_row(
                "invalid-id",
                "invalid task",
                "",
                invalid_datetime,
                "0:01:00",
            ),
        );
        let output = run_import(&input);
        let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");

        assert!(
            !output.status.success(),
            "{invalid_datetime} must be rejected"
        );
        assert_eq!(output.stdout, b"", "commands must be emitted atomically");
        assert!(
            stderr.contains(&format!("line 2: P列の形式が不正です: {invalid_datetime}")),
            "unexpected stderr for {invalid_datetime}: {stderr}"
        );
    }
}

#[test]
fn p列は通常の閏日と400年ごとの閏日を受理する() {
    for valid_datetime in ["2024/02/29 23:59:59", "2000/02/29 9:10:00"] {
        let input = format!(
            "{}\n",
            spreadsheet_row(
                "leap-day-id",
                "leap day task",
                "",
                valid_datetime,
                "0:01:00",
            )
        );
        let output = run_import(&input);

        assert!(
            output.status.success(),
            "{valid_datetime} must be accepted: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
        let expected_datetime = valid_datetime.split_once(' ').unwrap();
        assert!(stdout.contains(&format!(
            "終 {} {}\n",
            expected_datetime.1, expected_datetime.0
        )));
    }
}
