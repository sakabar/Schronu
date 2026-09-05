#![cfg(unix)]

#[path = "task_name_contract_support/spreadsheet.rs"]
mod spreadsheet_support;

use spreadsheet_support::{run_generator, spreadsheet_row};

const COLUMN_COUNT: usize = 19;

fn assert_rejected(input: &[u8], line: usize, diagnostic: &str) {
    let output = run_generator(input);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "input must be rejected: {stderr}");
    assert_eq!(output.stdout, b"", "stdout must remain atomic: {stderr}");
    assert!(
        stderr.contains(&format!("line {line}: {diagnostic}")),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn nonempty_physical_rowはa_sの19列だけを受理する() {
    let twenty_columns = format!("{}\textra\n", spreadsheet_row("tabbed task"));
    assert_rejected(
        twenty_columns.as_bytes(),
        1,
        "列数が不正です: 20列 (A-Sの19列が必要です)",
    );

    let split_row = format!("{}\n", spreadsheet_row("line one\nline two"));
    assert_rejected(
        split_row.as_bytes(),
        1,
        "列数が不正です: 10列 (A-Sの19列が必要です)",
    );
}

#[test]
fn exact_empty_rowだけをskipしてwhitespace_task名は拒否する() {
    let all_empty_columns = vec![""; COLUMN_COUNT].join("\t");
    let valid_row = spreadsheet_row("valid task");
    let input = format!("\n{all_empty_columns}\n{valid_row}\n");
    let output = run_generator(input.as_bytes());

    assert!(
        output.status.success(),
        "empty rows must be skipped: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "新 \"valid task\"\n下 スプレッドシートで仮登録したタスクを見積もる\n予 3\n\n"
    );

    let whitespace_name = format!("{}\n", spreadsheet_row(" \u{3000} "));
    assert_rejected(whitespace_name.as_bytes(), 1, "J列のtask名が空です");
}

#[test]
fn j列のc0_del_全c1_controlを拒否する() {
    let mut controls = (0_u32..=0x1f)
        .filter(|code_point| !matches!(code_point, 9 | 10))
        .collect::<Vec<_>>();
    controls.push(0x7f);
    controls.extend(0x80..=0x9f);

    for code_point in controls {
        let control = char::from_u32(code_point).expect("control code point");
        let name = format!("before{control}after");
        let input = format!("{}\n", spreadsheet_row(&name));
        assert_rejected(
            input.as_bytes(),
            1,
            "J列のtask名にcontrol characterが含まれています",
        );
    }

    let control_only = format!("{}\n", spreadsheet_row("\u{1b}"));
    assert_rejected(
        control_only.as_bytes(),
        1,
        "J列のtask名にcontrol characterが含まれています",
    );
}

#[test]
fn blankと符号付きascii整数を拒否する() {
    for invalid_name in ["", "   ", "42", "+42", "-42", "  -42  "] {
        let input = format!("{}\n", spreadsheet_row(invalid_name));
        let diagnostic = if invalid_name.trim().is_empty() {
            "J列のtask名が空です"
        } else {
            "J列のtask名に整数だけは指定できません"
        };
        assert_rejected(input.as_bytes(), 1, diagnostic);
    }
}

#[test]
fn terminal_crlfだけをline_endingとして許可する() {
    let row = spreadsheet_row("CRLF task");
    let lf_output = run_generator(format!("{row}\n").as_bytes());
    let crlf_output = run_generator(format!("{row}\r\n").as_bytes());

    assert!(lf_output.status.success());
    assert!(
        crlf_output.status.success(),
        "terminal CR must be accepted: {}",
        String::from_utf8_lossy(&crlf_output.stderr)
    );
    assert_eq!(crlf_output.stdout, lf_output.stdout);

    let embedded_cr = format!("{}\n", spreadsheet_row("before\rafter"));
    assert_rejected(
        embedded_cr.as_bytes(),
        1,
        "J列のtask名にcontrol characterが含まれています",
    );
}

#[test]
fn 後続rowのerrorでも先行commandを出力しない() {
    let input = format!(
        "{}\n{}\n",
        spreadsheet_row("valid first task"),
        spreadsheet_row("invalid\u{1b}second task")
    );
    assert_rejected(
        input.as_bytes(),
        2,
        "J列のtask名にcontrol characterが含まれています",
    );
}
