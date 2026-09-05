use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const COLUMN_COUNT: usize = 19;

pub(super) fn spreadsheet_row(task_name: &str) -> String {
    let mut columns = vec![""; COLUMN_COUNT];
    columns[9] = task_name;
    columns[18] = "0:00:00";
    columns.join("\t")
}

pub(super) fn run_generator(input: &[u8]) -> Output {
    let mut child = Command::new("zsh")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("shell/generate_command_from_spreadsheet.sh"),
        )
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
        .write_all(input)
        .expect("Spreadsheet input is written");
    child.wait_with_output().expect("generator exits")
}
