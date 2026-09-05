#![cfg(unix)]

use std::io::Write;
use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};

use super::command::{parse_command, Command, CommandAction, ParseMode};

const TASK_NAME_ROWS: &str =
    include_str!("../../../../tests/fixtures/task_name_spreadsheet/allowed-task-names.tsv");

fn repository_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn generate_commands(input: &str) -> String {
    let mut child = ProcessCommand::new("zsh")
        .arg(repository_path(
            "shell/generate_command_from_spreadsheet.sh",
        ))
        .arg("--stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Spreadsheet generator starts");
    child
        .stdin
        .take()
        .expect("generator stdin")
        .write_all(input.as_bytes())
        .expect("fixture is written to generator stdin");

    let output = child.wait_with_output().expect("generator exits");
    assert!(
        output.status.success(),
        "generator failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("generator stdout is UTF-8")
}

fn task_name(row: &str) -> &str {
    row.split('\t').nth(9).expect("fixture row has a J column")
}

#[test]
fn spreadsheetのtask名はinteractive_cliを通って原文へround_tripする() {
    let expected_names = TASK_NAME_ROWS.lines().map(task_name).collect::<Vec<_>>();
    let output = generate_commands(TASK_NAME_ROWS);
    let commands = output
        .lines()
        .filter(|line| line.starts_with("新 "))
        .collect::<Vec<_>>();

    assert_eq!(commands.len(), expected_names.len());
    for (command, expected_name) in commands.into_iter().zip(expected_names) {
        let parsed = parse_command(command, ParseMode::Interactive)
            .unwrap_or_else(|error| panic!("generated command must parse: {command:?}: {error}"));
        let Command::Action(CommandAction::NewProject { name, .. }) = parsed else {
            panic!("generated command must create a task: {command:?}");
        };
        assert_eq!(
            name, expected_name,
            "round-trip failed for {expected_name:?}"
        );
    }
}
