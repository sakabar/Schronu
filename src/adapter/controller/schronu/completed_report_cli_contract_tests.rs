use super::command::{Command, CommandKind, ParseMode};
use super::command_context::resolve_completed_logical_date;
use super::command_test_support::parse_command;
use super::renderer::{
    render_display_model, CompletedTaskReportDisplay, DisplayModel, SchronuWriter,
};
use crate::application::completed_task_report::CompletedTaskReportRow;
use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone};
use std::io::Write;
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

#[test]
fn completed_command_aliases_and_arity_are_typed_in_both_modes() {
    for mode in [ParseMode::Interactive, ParseMode::NonInteractive] {
        for alias in ["済", "completed"] {
            assert_eq!(
                parse_command(alias, mode).unwrap(),
                Command::Completed { date: None }
            );
            assert_eq!(
                parse_command(&format!("{alias} 8/11"), mode).unwrap(),
                Command::Completed {
                    date: Some("8/11".to_string())
                }
            );
            let error = parse_command(&format!("{alias} 8/11 extra"), mode).unwrap_err();
            assert_eq!(error.command(), "済");
            assert_eq!(error.field(), "arguments");
            assert_eq!(error.usage(), "済 [日付]");
        }
    }
    assert_eq!(
        parse_command("済", ParseMode::Interactive).unwrap().kind(),
        CommandKind::Completed
    );
}

#[test]
fn completed_date_resolves_omission_past_mmdd_today_leap_day_and_explicit_year() {
    let now = Local.with_ymd_and_hms(2026, 3, 1, 5, 59, 59).unwrap();
    assert_eq!(
        resolve_completed_logical_date(None, now).unwrap(),
        NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()
    );
    assert_eq!(
        resolve_completed_logical_date(Some("2/28"), now).unwrap(),
        NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()
    );
    assert_eq!(
        resolve_completed_logical_date(Some("3/1"), now).unwrap(),
        NaiveDate::from_ymd_opt(2025, 3, 1).unwrap()
    );
    assert_eq!(
        resolve_completed_logical_date(Some("2/29"), now).unwrap(),
        NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()
    );
    assert_eq!(
        resolve_completed_logical_date(Some("2028/2/29"), now).unwrap(),
        NaiveDate::from_ymd_opt(2028, 2, 29).unwrap()
    );
}

#[test]
fn completed_date_rejects_empty_invalid_calendar_and_overflow_values() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    for value in ["", "2026-8-11", "13/1", "2025/2/29", "999999999999/1/1"] {
        let error = resolve_completed_logical_date(Some(value), now).unwrap_err();
        assert!(
            error.to_string().starts_with("入力エラー: date:"),
            "value={value}: {error}"
        );
    }
}

#[test]
fn completed_date_rejects_future_month_day_at_chrono_lower_bound() {
    let now = DateTime::<Local>::from_naive_utc_and_offset(
        NaiveDate::MIN.and_hms_opt(12, 0, 0).unwrap(),
        FixedOffset::east_opt(0).unwrap(),
    );

    let error = resolve_completed_logical_date(Some("1/2"), now).unwrap_err();

    assert!(error.to_string().starts_with("入力エラー: date:"));
}

struct TestWriter(Vec<u8>);

impl Write for TestWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SchronuWriter for TestWriter {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        writeln!(self, "{message}")
    }
}

fn report_row(
    id: u128,
    hour: u32,
    actual: i64,
    estimated: i64,
    project: &str,
    task: &str,
) -> CompletedTaskReportRow {
    CompletedTaskReportRow {
        task_id: Uuid::from_u128(id),
        task_name: task.to_string(),
        project_name: project.to_string(),
        completed_at: Local.with_ymd_and_hms(2026, 8, 11, hour, 2, 3).unwrap(),
        actual_work_seconds: actual,
        estimated_work_seconds: estimated,
    }
}

fn render(rows: Vec<CompletedTaskReportRow>) -> String {
    let mut writer = TestWriter(Vec::new());
    render_display_model(
        &mut writer,
        &DisplayModel::CompletedTaskReport(CompletedTaskReportDisplay { rows }),
    )
    .unwrap();
    String::from_utf8(writer.0).unwrap()
}

#[test]
fn completed_report_renderer_uses_unicode_fixed_widths_signed_differences_and_unabridged_tasks() {
    let output = render(vec![
        report_row(1, 8, 360_000, 1, "日本語", "末尾まで省略しない長いタスク名"),
        report_row(2, 9, 0, 1, "abcdefghijklmnopqrs界tail", "second"),
        report_row(3, 10, 1, 1, "e\u{301}", "zero"),
    ]);
    assert!(!output.contains('\t'));
    assert!(output.contains("100:00:00"));
    assert!(output.contains("+99:59:59"));
    assert!(output.contains("-00:00:01"));
    assert!(output.contains("+00:00:00"));
    assert!(output.contains("abcdefghijklmnopqrs…"));
    assert!(!output.contains("tail"));
    assert!(output.contains("末尾まで省略しない長いタスク名"));

    fn suffix_at_width(line: &str, target: usize) -> &str {
        let mut width = 0;
        for (byte, character) in line.char_indices() {
            if width == target {
                return &line[byte..];
            }
            width += unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        }
        assert_eq!(width, target);
        ""
    }

    let lines = output.lines().collect::<Vec<_>>();
    for (line, task) in
        lines
            .iter()
            .zip(["タスク", "末尾まで省略しない長いタスク名", "second", "zero"])
    {
        assert_eq!(suffix_at_width(line, 64), task);
        assert_eq!(UnicodeWidthStr::width(&line[..line.len() - task.len()]), 64);
    }
}

#[test]
fn completed_report_renderer_explicitly_reports_empty_results() {
    assert_eq!(
        render(Vec::new()),
        "完了時刻  実績  見積  差  Project  タスク\n完了したタスクはありません。\n"
    );
}
