#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug)]
struct SpreadsheetColumn {
    letter: String,
    index: usize,
    name: String,
    sync: bool,
    time_format: Option<String>,
}

fn repository_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn parse_columns() -> Vec<SpreadsheetColumn> {
    fs::read_to_string(repository_path("spreadsheet_columns.tsv"))
        .expect("spreadsheet column manifest exists")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 7, "manifest row has seven fields: {line}");
            SpreadsheetColumn {
                letter: fields[0].to_owned(),
                index: fields[1].parse().expect("column index is numeric"),
                name: fields[2].to_owned(),
                sync: fields[4].parse().expect("sync flag is boolean"),
                time_format: (!fields[5].is_empty()).then(|| fields[5].to_owned()),
            }
        })
        .collect()
}

fn column<'a>(columns: &'a [SpreadsheetColumn], name: &str) -> &'a SpreadsheetColumn {
    columns
        .iter()
        .find(|column| column.name == name)
        .unwrap_or_else(|| panic!("manifest column exists: {name}"))
}

fn time_format_ranges(columns: &[SpreadsheetColumn]) -> Vec<String> {
    let mut time_columns: Vec<_> = columns
        .iter()
        .filter(|column| column.time_format.as_deref() == Some("hh:mm"))
        .collect();
    time_columns.sort_by_key(|column| column.index);
    let mut ranges = Vec::new();
    let mut start = time_columns[0];
    let mut end = time_columns[0];
    for column in time_columns.into_iter().skip(1) {
        if column.index == end.index + 1 {
            end = column;
            continue;
        }
        ranges.push(format!("'{}3:{}500'", start.letter, end.letter));
        start = column;
        end = column;
    }
    ranges.push(format!("'{}3:{}500'", start.letter, end.letter));
    ranges
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
    let columns = parse_columns();
    assert_eq!(columns.len(), 19);
    assert_eq!(column(&columns, "task_id").index, 2);
    assert_eq!(column(&columns, "task_name").index, 10);
    assert_eq!(column(&columns, "start_time").index, 12);
    assert_eq!(column(&columns, "finish_flag").index, 14);
    assert_eq!(column(&columns, "finish_time").index, 16);
    assert_eq!(column(&columns, "should_extract").index, 17);
    assert_eq!(column(&columns, "defer_command").index, 18);
    assert_eq!(column(&columns, "actual_work_time").index, 19);
    assert_eq!(
        columns
            .iter()
            .filter(|column| column.sync)
            .map(|column| column.index)
            .collect::<Vec<_>>(),
        [12, 14, 16, 18]
    );
    assert_eq!(time_format_ranges(&columns), ["'L3:M500'", "'O3:P500'"]);
}

#[test]
fn cli出力からspreadsheetを経由してコマンド生成まで列契約を維持する() {
    let cli_output =
        fs::read_to_string(repository_path("tests/fixtures/spreadsheet/cli-output.txt"))
            .expect("CLI output fixture exists");
    let copied = run_script("shell/copy_for_spreadsheet.sh", &[], &cli_output);
    let copied_rows: Vec<_> = copied
        .lines()
        .filter(|line| line.chars().any(|character| character != '\t'))
        .collect();
    let expected_copied = fs::read_to_string(repository_path(
        "tests/fixtures/spreadsheet/copied-rows.tsv",
    ))
    .expect("copied rows fixture exists");
    assert_eq!(copied_rows.join("\n") + "\n", expected_copied);

    let spreadsheet = fs::read_to_string(repository_path(
        "tests/fixtures/spreadsheet/sheet-values.tsv",
    ))
    .expect("spreadsheet values fixture exists");
    let copied_task_names: Vec<_> = copied_rows
        .iter()
        .map(|row| {
            let fields: Vec<_> = row.split('\t').collect();
            (fields[1], fields[9])
        })
        .collect();
    for row in spreadsheet.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<_> = row.split('\t').collect();
        if fields[1].is_empty() {
            continue;
        }
        assert!(
            copied_task_names
                .iter()
                .any(|(task_id, task_name)| *task_id == fields[1] && *task_name == fields[9]),
            "spreadsheet fixture must retain copied B/J values: {row}"
        );
    }

    let commands = run_script(
        "shell/generate_command_from_spreadsheet.sh",
        &["--stdin"],
        &spreadsheet,
    );
    let expected_commands = fs::read_to_string(repository_path(
        "tests/fixtures/spreadsheet/generated-commands.txt",
    ))
    .expect("generated commands fixture exists");
    assert_eq!(commands, format!("{expected_commands}\n"));
}

#[test]
fn implementation_and_documentationはmanifestの重要な列契約を明記する() {
    let columns = parse_columns();
    let copy_script = fs::read_to_string(repository_path("shell/copy_for_spreadsheet.sh")).unwrap();
    let generate_script = fs::read_to_string(repository_path(
        "shell/generate_command_from_spreadsheet.sh",
    ))
    .unwrap();
    let apps_script = fs::read_to_string(repository_path("apps_script/main.js")).unwrap();
    let readme = fs::read_to_string(repository_path("README.md")).unwrap();
    let apps_readme = fs::read_to_string(repository_path("apps_script/README.md")).unwrap();

    assert!(copy_script.contains("for (i = 1; i <= 9; i++)"));
    assert!(copy_script.contains("gsub(/[[:space:]]+/, \" \", line)"));
    for (variable, name) in [
        ("task_id", "task_id"),
        ("task_name", "task_name"),
        ("finish_flag", "finish_flag"),
        ("finish_datetime", "finish_time"),
        ("should_extract", "should_extract"),
        ("defer_command", "defer_command"),
        ("actual_work_minutes", "actual_work_time"),
    ] {
        let column = column(&columns, name);
        assert!(generate_script.contains(&format!("{variable} = trim(${})", column.index)));
    }
    assert!(apps_script.contains(&format!("taskIdCol: {}", column(&columns, "task_id").index)));
    let sync_columns = columns
        .iter()
        .filter(|column| column.sync)
        .map(|column| column.index.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    assert!(apps_script.contains(&format!("syncCols: [{sync_columns}]")));
    assert!(apps_script.contains(&format!(
        "timeFormatRanges: [{}]",
        time_format_ranges(&columns).join(", ")
    )));
    assert!(readme.contains("spreadsheet_columns.tsv"));
    assert!(apps_readme.contains("spreadsheet_columns.tsv"));
}
