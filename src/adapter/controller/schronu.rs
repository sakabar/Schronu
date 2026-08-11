use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, TimeZone, Timelike, Weekday,
};
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};
use regex::Regex;
use schronu::adapter::gateway::free_time_manager::FreeTimeManager;
use schronu::adapter::gateway::storage_lock::{LockMode, StorageLock};
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::FreeTimeManagerTrait;
#[cfg(test)]
use schronu::application::interface::TaskRepositoryOperation;
use schronu::application::interface::{TaskRepositoryError, TaskRepositoryTrait};
use schronu::application::schedule_use_case::get_schedule;
use schronu::application::task_use_case::{
    breakdown_task, complete_task, create_task, defer_task, estimated_work_seconds_from_minutes,
    get_focus, set_category, set_deadline, set_estimate, validate_task_name, ApplicationError,
    BreakdownTaskInput, CompleteTaskInput, CreateTaskInput,
};
use schronu::entity::datetime::{get_next_morning_datetime, parse_local_datetime};
use schronu::entity::task::{
    extract_leaf_tasks_from_project, extract_leaf_tasks_from_project_with_pending,
    read_project_category, round_up_sec_as_minute, ProjectCategory, Status, Task, TaskAttr,
};
#[cfg(test)]
use std::cell::Cell;
use std::cmp::{max, min};
use std::collections::HashMap;
use std::env;
use std::io::Stdout;
use std::io::{stdout, Write};
#[cfg(test)]
use std::path::PathBuf;
use std::process;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;

mod storage_directory;
use std::time::{Duration as StdDuration, Instant};
use storage_directory::resolve_project_storage_directory;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::raw::RawTerminal;
use termion::style;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;
use url::Url;
use uuid::Uuid;

const MAX_COL: u16 = 999;

const MAX_ARRANGE_ESTIMATED_WORK_MINUTES: i64 = 1439;
const DEFAULT_LOWEST_PRIORITY_RECENT_DAYS: i64 = 0;
const FOCUS_PROGRESS_BAR_SEGMENTS: usize = 100;
const IDLE_REFRESH_INTERVAL: StdDuration = StdDuration::from_secs(60);

// パーセントエンコーディングする対象にスペースを追加する
const MY_ASCII_SET: &AsciiSet = &CONTROLS.add(b' ');

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FocusSelectionMode {
    HighestPriority,
    LowestPriority { recent_days: i64 },
}

impl FocusSelectionMode {
    fn label(&self) -> String {
        match self {
            FocusSelectionMode::HighestPriority => "高".to_string(),
            FocusSelectionMode::LowestPriority { recent_days } => {
                if *recent_days == DEFAULT_LOWEST_PRIORITY_RECENT_DAYS {
                    "低".to_string()
                } else {
                    format!("低 {}", recent_days)
                }
            }
        }
    }
}

trait SchronuWriter: Write {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error>;
}

#[derive(Debug)]
enum RunError {
    Repository(TaskRepositoryError),
    InputDisconnected {
        save_error_opt: Option<TaskRepositoryError>,
    },
    InputRead {
        input_error: std::io::Error,
        save_error_opt: Option<TaskRepositoryError>,
    },
    Interrupted,
}

impl From<TaskRepositoryError> for RunError {
    fn from(error: TaskRepositoryError) -> Self {
        Self::Repository(error)
    }
}

impl std::fmt::Display for RunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Repository(error) => error.fmt(formatter),
            Self::InputDisconnected {
                save_error_opt: Some(error),
            } => write!(
                formatter,
                "interactive input channel disconnected; additionally, {error}"
            ),
            Self::InputDisconnected {
                save_error_opt: None,
            } => write!(formatter, "interactive input channel disconnected"),
            Self::InputRead {
                input_error,
                save_error_opt: Some(error),
            } => write!(
                formatter,
                "failed to read interactive input: {input_error}; additionally, {error}"
            ),
            Self::InputRead {
                input_error,
                save_error_opt: None,
            } => write!(formatter, "failed to read interactive input: {input_error}"),
            Self::Interrupted => write!(formatter, "interactive input interrupted"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
            Self::InputDisconnected { save_error_opt } => save_error_opt
                .as_ref()
                .map(|error| error as &(dyn std::error::Error + 'static)),
            Self::InputRead { input_error, .. } => Some(input_error),
            Self::Interrupted => None,
        }
    }
}

impl SchronuWriter for RawTerminal<Stdout> {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        writeln!(self, "{}{}", termion::cursor::Left(MAX_COL), message)
    }
}

impl SchronuWriter for Stdout {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        writeln!(self, "{}", message)
    }
}

fn writeln_newline(stdout: &mut dyn SchronuWriter, message: &str) -> Result<(), std::io::Error> {
    stdout.writeln_newline(message)
}

fn backward_width(line: &str, cursor_x: usize) -> u16 {
    if line.chars().count() == 0 || cursor_x == 0 {
        return 0;
    }

    let ch_opt = line.chars().nth(cursor_x - 1);

    (match ch_opt {
        Some(ch) => UnicodeWidthChar::width(ch).unwrap_or(0),
        None => 0,
    }) as u16
}

fn get_weekday_jp(date: &NaiveDate) -> &str {
    match date.weekday() {
        Weekday::Mon => "月",
        Weekday::Tue => "火",
        Weekday::Wed => "水",
        Weekday::Thu => "木",
        Weekday::Fri => "金",
        Weekday::Sat => "土",
        Weekday::Sun => "日",
    }
}

fn resolve_upcoming_mmdd(mmdd: &str, now: DateTime<Local>) -> Option<LocalResult<DateTime<Local>>> {
    let mmdd_reg = Regex::new(r"^(\d{1,2})/(\d{1,2})$").unwrap();
    let caps = mmdd_reg.captures(mmdd)?;
    let month: u32 = caps[1].parse().ok()?;
    let day: u32 = caps[2].parse().ok()?;

    let schronu_day_start = |year| match Local.with_ymd_and_hms(year, month, day, 12, 0, 0) {
        LocalResult::Single(datetime) => {
            LocalResult::Single(get_next_morning_datetime(datetime) - Duration::days(1))
        }
        LocalResult::Ambiguous(earliest, latest) => LocalResult::Ambiguous(
            get_next_morning_datetime(earliest) - Duration::days(1),
            get_next_morning_datetime(latest) - Duration::days(1),
        ),
        LocalResult::None => LocalResult::None,
    };

    Some(match schronu_day_start(now.year()) {
        LocalResult::Single(datetime) if datetime < now => schronu_day_start(now.year() + 1),
        result => result,
    })
}

fn resolve_show_all_pattern(pattern: &str, now: DateTime<Local>) -> String {
    match resolve_upcoming_mmdd(pattern, now) {
        Some(LocalResult::Single(datetime)) => datetime.format("%Y/%m/%d").to_string(),
        _ => pattern.to_string(),
    }
}

#[test]
fn test_resolve_upcoming_mmdd_未来の日付は現在年を使う() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let target_date = Local.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap();
    let expected = get_next_morning_datetime(target_date) - Duration::days(1);

    assert_eq!(
        resolve_upcoming_mmdd("9/26", now),
        Some(LocalResult::Single(expected))
    );
}

#[test]
fn test_resolve_upcoming_mmdd_過去の日付は翌年を使う() {
    let now = Local.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
    let target_date = Local.with_ymd_and_hms(2027, 9, 26, 12, 0, 0).unwrap();
    let expected = get_next_morning_datetime(target_date) - Duration::days(1);

    assert_eq!(
        resolve_upcoming_mmdd("09/26", now),
        Some(LocalResult::Single(expected))
    );
}

#[test]
fn test_resolve_upcoming_mmdd_当日の境界時刻は現在年を使う() {
    let target_date = Local.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap();
    let now = get_next_morning_datetime(target_date) - Duration::days(1);

    assert_eq!(
        resolve_upcoming_mmdd("9/26", now),
        Some(LocalResult::Single(now))
    );
}

#[test]
fn test_resolve_show_all_pattern_年なし日付を完全日付へ変換する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    assert_eq!(resolve_show_all_pattern("9/26", now), "2026/09/26");
}

#[test]
fn test_resolve_show_all_pattern_過ぎた日付は翌年へ変換する() {
    let now = Local.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();

    assert_eq!(resolve_show_all_pattern("9/26", now), "2027/09/26");
}

#[test]
fn test_resolve_show_all_pattern_完全日付と検索語は変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    assert_eq!(resolve_show_all_pattern("2026/09/26", now), "2026/09/26");
    assert_eq!(resolve_show_all_pattern("タスク", now), "タスク");
}

fn get_adjustable_prefix_label(
    task: &Task,
    dt: DateTime<Local>,
    rank: usize,
    last_synced_time: DateTime<Local>,
) -> String {
    if rank != 0 || task.get_is_on_other_side() {
        return "".to_string();
    }

    let planned_date = (get_next_morning_datetime(dt) - Duration::days(1)).date_naive();
    let available_datetime = max(task.get_start_time(), last_synced_time);
    let available_date =
        (get_next_morning_datetime(available_datetime) - Duration::days(1)).date_naive();
    let advance_days = (planned_date - available_date).num_days();

    if advance_days > 0 {
        format!("【前{}】", advance_days)
    } else {
        "".to_string()
    }
}

fn parse_clear_or_gather_defer_to_datetime(
    cmd_str: &str,
    arg: &str,
    now: DateTime<Local>,
) -> Option<DateTime<Local>> {
    let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();
    if let Some(caps) = hhmm_reg.captures(arg) {
        let hh_orig: u32 = caps[1].parse().unwrap();
        let hh = hh_orig % 24;
        let mm: u32 = caps[2].parse().unwrap();
        let days: i64 = hh_orig as i64 / 24;
        let todays_start = get_next_morning_datetime(now) - Duration::days(1);

        return Some(
            Local
                .with_ymd_and_hms(
                    todays_start.year(),
                    todays_start.month(),
                    todays_start.day(),
                    hh,
                    mm,
                    0,
                )
                .unwrap()
                + Duration::days(days),
        );
    }

    let integer_reg = Regex::new(r"^\d+$").unwrap();
    if matches!(cmd_str, "空" | "clear" | "集" | "gather") && integer_reg.is_match(arg) {
        let minutes: i64 = arg.parse().unwrap();
        return Some(now + Duration::minutes(minutes));
    }

    None
}

fn parse_focus_selection_mode_command(line: &str) -> Option<FocusSelectionMode> {
    let tokens = line.split_whitespace().collect::<Vec<&str>>();

    match tokens.as_slice() {
        ["低" | "low" | "lo" | "lowest"] => Some(FocusSelectionMode::LowestPriority {
            recent_days: DEFAULT_LOWEST_PRIORITY_RECENT_DAYS,
        }),
        ["低" | "low" | "lo" | "lowest", recent_days_str]
            if recent_days_str.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            match recent_days_str.parse::<i64>() {
                Ok(recent_days) => Some(FocusSelectionMode::LowestPriority { recent_days }),
                Err(_) => None,
            }
        }
        ["高" | "high" | "hi" | "highest"] => Some(FocusSelectionMode::HighestPriority),
        _ => None,
    }
}

fn select_focus_task_id(
    task_repository: &mut dyn TaskRepositoryTrait,
    focus_selection_mode: FocusSelectionMode,
) -> Option<Uuid> {
    match focus_selection_mode {
        FocusSelectionMode::HighestPriority => get_focus(task_repository).map(|task| task.id),
        FocusSelectionMode::LowestPriority { recent_days } => {
            task_repository.get_defer_candidate_leaf_task_id(recent_days)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_adjustable_prefix_label_前倒し可能日数を表示する() {
        let task = Task::new("タスク");
        task.set_start_time(Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap());
        let dt = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

        let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time);

        assert_eq!(actual, "【前3】");
    }

    #[test]
    fn test_get_adjustable_prefix_label_今日より前には戻さない() {
        let task = Task::new("タスク");
        task.set_start_time(Local.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap());
        let dt = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

        let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time);

        assert_eq!(actual, "【前3】");
    }

    #[test]
    fn test_get_adjustable_prefix_label_同日着手可能なら表示しない() {
        let task = Task::new("タスク");
        task.set_start_time(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap());
        let dt = Local.with_ymd_and_hms(2026, 5, 10, 18, 0, 0).unwrap();
        let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

        let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time);

        assert_eq!(actual, "");
    }

    #[test]
    fn test_get_adjustable_prefix_label_今日と予定日が同じなら過去の着手可能日は表示しない() {
        let task = Task::new("タスク");
        task.set_start_time(Local.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap());
        let dt = Local.with_ymd_and_hms(2026, 5, 7, 18, 0, 0).unwrap();
        let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

        let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time);

        assert_eq!(actual, "");
    }

    #[test]
    fn test_get_adjustable_prefix_label_相手待ちは表示しない() {
        let task = Task::new("タスク");
        task.set_start_time(Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap());
        task.set_is_on_other_side(true);
        let dt = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

        let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time);

        assert_eq!(actual, "");
    }

    #[test]
    fn test_get_adjustable_prefix_label_葉以外は表示しない() {
        let task = Task::new("タスク");
        task.set_start_time(Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap());
        let dt = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

        let actual = get_adjustable_prefix_label(&task, dt, 1, last_synced_time);

        assert_eq!(actual, "");
    }

    #[test]
    fn test_parse_clear_or_gather_defer_to_datetime_空の分指定は現在時刻からの分として解釈する() {
        let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

        let actual = parse_clear_or_gather_defer_to_datetime("空", "120", now);

        assert_eq!(actual, Some(now + Duration::minutes(120)));
    }

    #[test]
    fn test_parse_clear_or_gather_defer_to_datetime_hhmm指定は従来通り当日の時刻として解釈する() {
        let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

        let actual = parse_clear_or_gather_defer_to_datetime("空", "10:00", now);

        assert_eq!(
            actual,
            Some(Local.with_ymd_and_hms(2026, 5, 7, 10, 0, 0).unwrap())
        );
    }

    #[test]
    fn test_parse_clear_or_gather_defer_to_datetime_集の分指定は現在時刻からの分として解釈する() {
        let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

        let actual = parse_clear_or_gather_defer_to_datetime("集", "120", now);

        assert_eq!(actual, Some(now + Duration::minutes(120)));
    }

    #[test]
    fn test_parse_focus_selection_mode_command_low() {
        assert_eq!(
            parse_focus_selection_mode_command("低"),
            Some(FocusSelectionMode::LowestPriority {
                recent_days: DEFAULT_LOWEST_PRIORITY_RECENT_DAYS
            })
        );
        assert_eq!(
            parse_focus_selection_mode_command("low"),
            Some(FocusSelectionMode::LowestPriority {
                recent_days: DEFAULT_LOWEST_PRIORITY_RECENT_DAYS
            })
        );
    }

    #[test]
    fn test_parse_focus_selection_mode_command_low_with_recent_days() {
        assert_eq!(
            parse_focus_selection_mode_command("低 0"),
            Some(FocusSelectionMode::LowestPriority { recent_days: 0 })
        );
        assert_eq!(
            parse_focus_selection_mode_command("low 0"),
            Some(FocusSelectionMode::LowestPriority { recent_days: 0 })
        );
        assert_eq!(
            parse_focus_selection_mode_command("lo 3"),
            Some(FocusSelectionMode::LowestPriority { recent_days: 3 })
        );
        assert_eq!(
            parse_focus_selection_mode_command("lowest 12"),
            Some(FocusSelectionMode::LowestPriority { recent_days: 12 })
        );
    }

    #[test]
    fn test_parse_focus_selection_mode_command_high() {
        assert_eq!(
            parse_focus_selection_mode_command("高"),
            Some(FocusSelectionMode::HighestPriority)
        );
        assert_eq!(
            parse_focus_selection_mode_command("high"),
            Some(FocusSelectionMode::HighestPriority)
        );
    }

    #[test]
    fn test_parse_focus_selection_mode_command_trims_spaces() {
        assert_eq!(
            parse_focus_selection_mode_command("  low  "),
            Some(FocusSelectionMode::LowestPriority {
                recent_days: DEFAULT_LOWEST_PRIORITY_RECENT_DAYS
            })
        );
        assert_eq!(
            parse_focus_selection_mode_command("  高  "),
            Some(FocusSelectionMode::HighestPriority)
        );
    }

    #[test]
    fn test_parse_focus_selection_mode_command_unknown() {
        assert_eq!(parse_focus_selection_mode_command("後 7日"), None);
        assert_eq!(parse_focus_selection_mode_command("低 abc"), None);
        assert_eq!(parse_focus_selection_mode_command("低 -1"), None);
        assert_eq!(parse_focus_selection_mode_command("低 1 2"), None);
    }

    #[test]
    fn test_execute_set_priority_優先度を変更する() {
        let task = Task::new("タスク");
        let focused_task_opt = Some(task.clone());

        execute_set_priority(&focused_task_opt, "8");

        assert_eq!(task.get_priority(), 8);
    }

    #[test]
    fn test_execute_set_priority_不正値なら変更しない() {
        let task = Task::new("タスク");
        task.set_priority(5);
        let focused_task_opt = Some(task.clone());

        execute_set_priority(&focused_task_opt, "invalid");

        assert_eq!(task.get_priority(), 5);
    }

    #[test]
    fn test_execute_set_priority_フォーカスなしなら何もしない() {
        let focused_task_opt = None;

        execute_set_priority(&focused_task_opt, "8");
    }

    #[test]
    fn test_advance_display_datetime_cursor_過去の終了時刻では巻き戻さない() {
        let current_datetime_cursor = Local.with_ymd_and_hms(2026, 5, 10, 14, 54, 0).unwrap();
        let end_datetime = Local.with_ymd_and_hms(2026, 5, 10, 14, 2, 0).unwrap();

        let actual = advance_display_datetime_cursor(current_datetime_cursor, end_datetime);

        assert_eq!(actual, current_datetime_cursor);
    }

    #[test]
    fn test_advance_display_datetime_cursor_未来の終了時刻には進める() {
        let current_datetime_cursor = Local.with_ymd_and_hms(2026, 5, 10, 14, 2, 0).unwrap();
        let end_datetime = Local.with_ymd_and_hms(2026, 5, 10, 14, 54, 0).unwrap();

        let actual = advance_display_datetime_cursor(current_datetime_cursor, end_datetime);

        assert_eq!(actual, end_datetime);
    }

    #[test]
    fn test_sort_task_list_display_rows_通常表示は予定時刻の逆順にする() {
        let early_id = Uuid::new_v4();
        let late_id = Uuid::new_v4();
        let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
        let mut rows = vec![
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
                target_date,
                0,
                early_id,
                10,
                60,
                None,
                "".to_string(),
                "early".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
                target_date,
                0,
                late_id,
                1,
                60,
                None,
                "".to_string(),
                "late".to_string(),
            ),
        ];

        sort_task_list_display_rows(&mut rows, TaskListDisplayOrder::ScheduledStartDesc);

        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![late_id, early_id]
        );
    }

    #[test]
    fn test_sort_task_list_display_rows_尾表示は低優先度を下側にする() {
        let high_priority_id = Uuid::new_v4();
        let low_priority_id = Uuid::new_v4();
        let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
        let mut rows = vec![
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
                target_date,
                0,
                high_priority_id,
                10,
                60,
                None,
                "".to_string(),
                "high".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
                target_date,
                0,
                low_priority_id,
                1,
                60,
                None,
                "".to_string(),
                "low".to_string(),
            ),
        ];

        sort_task_list_display_rows(&mut rows, TaskListDisplayOrder::LowPriorityTail);

        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![high_priority_id, low_priority_id]
        );
    }

    #[test]
    fn test_sort_task_list_display_rows_尾表示で同じ優先度なら予定時刻が遅いものを下側にする() {
        let early_id = Uuid::new_v4();
        let late_id = Uuid::new_v4();
        let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
        let mut rows = vec![
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
                target_date,
                0,
                early_id,
                1,
                60,
                None,
                "".to_string(),
                "early".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
                target_date,
                0,
                late_id,
                1,
                60,
                None,
                "".to_string(),
                "late".to_string(),
            ),
        ];

        sort_task_list_display_rows(&mut rows, TaskListDisplayOrder::LowPriorityTail);

        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![early_id, late_id]
        );
    }

    #[test]
    fn test_mark_give_up_candidate_rows_低優先度側から不足時間を満たすまで印を付ける() {
        let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
        let high_id = Uuid::new_v4();
        let nineteen_min_id = Uuid::new_v4();
        let twenty_min_id = Uuid::new_v4();
        let fifteen_min_id = Uuid::new_v4();
        let six_min_id = Uuid::new_v4();
        let thirteen_min_id = Uuid::new_v4();
        let eighteen_min_id = Uuid::new_v4();
        let mut rows = vec![
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 21, 0, 0).unwrap(),
                target_date,
                0,
                high_id,
                89,
                120 * 60,
                None,
                "prefix ".to_string(),
                "high".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 23, 11, 0).unwrap(),
                target_date,
                0,
                nineteen_min_id,
                5,
                19 * 60,
                None,
                "0001 00000000-0000-0000-0000-000000000000 / ____/__/__ 05/10(日)-23:11~23:30 0 19 05 ".to_string(),
                "<19/60>レビュー".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 22, 36, 0).unwrap(),
                target_date,
                1,
                twenty_min_id,
                5,
                20 * 60,
                None,
                "prefix ".to_string(),
                "回収する".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 22, 21, 0).unwrap(),
                target_date,
                0,
                fifteen_min_id,
                5,
                15 * 60,
                None,
                "prefix ".to_string(),
                "心当たりがある店に電話して確認".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 22, 16, 0).unwrap(),
                target_date,
                0,
                six_min_id,
                5,
                6 * 60,
                None,
                "prefix ".to_string(),
                "日から土までの実績を確認する".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 22, 3, 0).unwrap(),
                target_date,
                0,
                thirteen_min_id,
                5,
                13 * 60,
                None,
                "prefix ".to_string(),
                "<13/30>一次レビュー".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 21, 42, 0).unwrap(),
                target_date,
                0,
                eighteen_min_id,
                5,
                18 * 60,
                None,
                "prefix ".to_string(),
                "<18/30>一次レビュー".to_string(),
            ),
        ];

        mark_give_up_candidate_rows(&mut rows, 83 * 60, target_date);

        let give_up_ids = rows
            .iter()
            .filter_map(|row| {
                if row.give_up_candidate {
                    Some(row.id)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        assert_eq!(
            give_up_ids,
            vec![
                nineteen_min_id,
                twenty_min_id,
                fifteen_min_id,
                six_min_id,
                thirteen_min_id,
                eighteen_min_id
            ]
        );
        let rendered = rows
            .iter()
            .find(|row| row.id == nineteen_min_id)
            .unwrap()
            .render_message();
        assert!(rendered.contains(" A "));
        assert!(rendered.ends_with("<19/60>レビュー"));
        assert!(
            !rows
                .iter()
                .find(|row| row.id == high_id)
                .unwrap()
                .give_up_candidate
        );
    }

    #[test]
    fn test_mark_give_up_candidate_rows_空き時間行と別日は候補にしない() {
        let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
        let other_date = NaiveDate::from_ymd_opt(2026, 5, 11).unwrap();
        let target_id = Uuid::new_v4();
        let other_date_id = Uuid::new_v4();
        let blank_id = Uuid::new_v4();
        let mut rows = vec![
            TaskListDisplayRow::new_message(
                Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
                0,
                blank_id,
                0,
                "空き時間".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 11, 12, 0, 0).unwrap(),
                other_date,
                0,
                other_date_id,
                1,
                60 * 60,
                None,
                "".to_string(),
                "tomorrow".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 11, 0, 0).unwrap(),
                target_date,
                0,
                target_id,
                10,
                30 * 60,
                None,
                "".to_string(),
                "today".to_string(),
            ),
        ];

        mark_give_up_candidate_rows(&mut rows, 10 * 60, target_date);

        assert!(
            !rows
                .iter()
                .find(|row| row.id == blank_id)
                .unwrap()
                .give_up_candidate
        );
        assert!(
            !rows
                .iter()
                .find(|row| row.id == other_date_id)
                .unwrap()
                .give_up_candidate
        );
        assert!(
            rows.iter()
                .find(|row| row.id == target_id)
                .unwrap()
                .give_up_candidate
        );
    }

    #[test]
    fn test_mark_give_up_candidate_rows_不足なしなら印を付けない() {
        let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
        let id = Uuid::new_v4();
        let mut rows = vec![TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
            target_date,
            0,
            id,
            1,
            60 * 60,
            None,
            "".to_string(),
            "task".to_string(),
        )];

        mark_give_up_candidate_rows(&mut rows, 0, target_date);

        assert!(!rows[0].give_up_candidate);
    }

    #[test]
    fn test_mark_give_up_candidate_rows_by_date_未来日にも空差累に応じて印を付ける() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
        let tomorrow = NaiveDate::from_ymd_opt(2026, 5, 11).unwrap();
        let today_id = Uuid::new_v4();
        let tomorrow_high_id = Uuid::new_v4();
        let tomorrow_low_late_id = Uuid::new_v4();
        let tomorrow_low_early_id = Uuid::new_v4();
        let mut rows = vec![
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
                today,
                0,
                today_id,
                1,
                60 * 60,
                None,
                "prefix ".to_string(),
                "today".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 11, 10, 0, 0).unwrap(),
                tomorrow,
                0,
                tomorrow_high_id,
                10,
                60 * 60,
                None,
                "prefix ".to_string(),
                "tomorrow high".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 11, 18, 0, 0).unwrap(),
                tomorrow,
                0,
                tomorrow_low_late_id,
                1,
                45 * 60,
                None,
                "prefix ".to_string(),
                "tomorrow low late".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 11, 13, 0, 0).unwrap(),
                tomorrow,
                0,
                tomorrow_low_early_id,
                1,
                30 * 60,
                None,
                "prefix ".to_string(),
                "tomorrow low early".to_string(),
            ),
        ];
        let mut shortage_duration_by_date = HashMap::new();
        shortage_duration_by_date.insert(today, Duration::seconds(0));
        shortage_duration_by_date.insert(tomorrow, Duration::minutes(50));

        mark_give_up_candidate_rows_by_date(&mut rows, &shortage_duration_by_date);

        let give_up_ids = rows
            .iter()
            .filter_map(|row| {
                if row.give_up_candidate {
                    Some(row.id)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        assert_eq!(
            give_up_ids,
            vec![tomorrow_low_late_id, tomorrow_low_early_id]
        );
        assert!(
            !rows
                .iter()
                .find(|row| row.id == today_id)
                .unwrap()
                .give_up_candidate
        );
        assert!(
            !rows
                .iter()
                .find(|row| row.id == tomorrow_high_id)
                .unwrap()
                .give_up_candidate
        );
    }

    #[test]
    fn test_replace_task_list_icon_アイコン列だけを置き換える() {
        let message_prefix =
            "0028 task-id / ____/__/__ 06/28(日)-23:11~23:30 0 19 05 資 ".to_string();

        let actual = replace_task_list_icon(&message_prefix, "A");

        assert_eq!(
            actual,
            "0028 task-id A ____/__/__ 06/28(日)-23:11~23:30 0 19 05 資 "
        );
    }

    #[test]
    fn test_project_category_symbol_カテゴリ表示記号を返す() {
        assert_eq!(
            project_category_symbol(Some(ProjectCategory::Earning)),
            "獲"
        );
        assert_eq!(
            project_category_symbol(Some(ProjectCategory::Sustaining)),
            "維"
        );
        assert_eq!(
            project_category_symbol(Some(ProjectCategory::Recovery)),
            "回"
        );
        assert_eq!(
            project_category_symbol(Some(ProjectCategory::Investment)),
            "資"
        );
        assert_eq!(
            project_category_symbol(Some(ProjectCategory::Consumption)),
            "消"
        );
        assert_eq!(project_category_symbol(None), "_");
    }

    #[test]
    fn test_format_focused_task_header_project_categoryを表示する() {
        assert_eq!(
            format_focused_task_header(Some(ProjectCategory::Investment)),
            "focused task is: project_category=資"
        );
        assert_eq!(
            format_focused_task_header(None),
            "focused task is: project_category=_"
        );
    }

    #[test]
    fn test_summarize_scheduled_work_seconds_by_project_category_実タスクだけをカテゴリ別に集計する(
    ) {
        let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
        let rows = vec![
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
                target_date,
                0,
                Uuid::new_v4(),
                1,
                60 * 60,
                Some(ProjectCategory::Earning),
                "".to_string(),
                "earning".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
                target_date,
                0,
                Uuid::new_v4(),
                1,
                30 * 60,
                Some(ProjectCategory::Investment),
                "".to_string(),
                "investment".to_string(),
            ),
            TaskListDisplayRow::new_task(
                Local.with_ymd_and_hms(2026, 5, 10, 14, 0, 0).unwrap(),
                target_date,
                0,
                Uuid::new_v4(),
                1,
                30 * 60,
                None,
                "".to_string(),
                "uncategorized".to_string(),
            ),
            TaskListDisplayRow::new_message(
                Local.with_ymd_and_hms(2026, 5, 10, 15, 0, 0).unwrap(),
                0,
                Uuid::new_v4(),
                1,
                "message".to_string(),
            ),
        ];

        let summary = summarize_scheduled_work_seconds_by_project_category(&rows);

        assert_eq!(summary[0], 60 * 60);
        assert_eq!(summary[3], 30 * 60);
        assert_eq!(summary[5], 30 * 60);
    }

    #[test]
    fn test_format_scheduled_work_seconds_by_project_category_比率を表示する() {
        let summary = [60 * 60, 0, 0, 30 * 60, 0, 30 * 60];

        let actual = format_scheduled_work_seconds_by_project_category(&summary, 2 * 60 * 60);

        assert_eq!(
            actual,
            "予定カテゴリ: 獲得 1.0時間(50% | 50%) / 維持 0.0時間(0% | 50%) / 回復 0.0時間(0% | 50%) / 投資 0.5時間(25% | 75%) / 消費 0.0時間(0% | 75%) / 未分類 0.5時間(25% | 100%)"
        );
    }

    #[test]
    fn test_format_scheduled_work_seconds_by_project_category_空き時間超過を表示する() {
        let summary = [60 * 60, 0, 0, 30 * 60, 0, 30 * 60];

        let actual = format_scheduled_work_seconds_by_project_category(&summary, 60 * 60);

        assert_eq!(
            actual,
            "予定カテゴリ: 獲得 1.0時間(100% | 100%) / 維持 0.0時間(0% | 100%) / 回復 0.0時間(0% | 100%) / 投資 0.5時間(50% | 150%) / 消費 0.0時間(0% | 150%) / 未分類 0.5時間(50% | 200%)"
        );
    }

    #[test]
    fn test_format_scheduled_work_seconds_by_project_category_空き時間なし() {
        let summary = [60 * 60, 0, 0, 0, 0, 0];

        let actual = format_scheduled_work_seconds_by_project_category(&summary, 0);

        assert_eq!(
            actual,
            "予定カテゴリ: 獲得 1.0時間(inf% | inf%) / 維持 0.0時間(0% | inf%) / 回復 0.0時間(0% | inf%) / 投資 0.0時間(0% | inf%) / 消費 0.0時間(0% | inf%) / 未分類 0.0時間(0% | inf%)"
        );
    }

    #[test]
    fn test_format_scheduled_work_seconds_by_project_category_予定なし() {
        let summary = [0; PROJECT_CATEGORY_SUMMARY_LEN];

        let actual = format_scheduled_work_seconds_by_project_category(&summary, 0);

        assert_eq!(actual, "予定カテゴリ: 予定なし");
    }
}

struct RhoMetrics {
    _total_work_hours: f64,
    repetitive_work_hours: f64,
    non_repetitive_work_hours: f64,
    _available_hours: f64,
    free_hours: f64,
    rho: f64,
    non_repetitive_rho: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TaskListDisplayOrder {
    ScheduledStartDesc,
    LowPriorityTail,
}

const DAILY_BAND_SECONDS_PER_SEGMENT: i64 = 15 * 60;
const DAILY_BAND_SEGMENTS: usize = 24 * 4;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

struct DailySummaryRow {
    date: NaiveDate,
    calendar_message: String,
    band_message: String,
}

struct DailyBandDurations {
    fixed_seconds: i64,
    elapsed_seconds: i64,
    repetitive_seconds: i64,
    non_repetitive_seconds: i64,
    rho_leeway_seconds: i64,
}

fn calculate_daily_band_durations(
    is_today: bool,
    full_day_free_minutes: i64,
    remaining_free_minutes: i64,
    total_work_seconds: i64,
    repetitive_work_seconds: i64,
    diff_to_goal_hours: f64,
) -> DailyBandDurations {
    DailyBandDurations {
        fixed_seconds: (SECONDS_PER_DAY - full_day_free_minutes.max(0) * 60).max(0),
        elapsed_seconds: if is_today {
            (full_day_free_minutes - remaining_free_minutes).max(0) * 60
        } else {
            0
        },
        repetitive_seconds: repetitive_work_seconds.max(0),
        non_repetitive_seconds: (total_work_seconds - repetitive_work_seconds).max(0),
        rho_leeway_seconds: (-diff_to_goal_hours * 3600.0).max(0.0).round() as i64,
    }
}

fn round_daily_band_segment_count(seconds: i64) -> usize {
    let non_negative_seconds = seconds.max(0);
    ((non_negative_seconds.saturating_add(DAILY_BAND_SECONDS_PER_SEGMENT / 2))
        / DAILY_BAND_SECONDS_PER_SEGMENT) as usize
}

fn format_signed_hours_minutes(duration: Duration) -> String {
    let sign = if duration >= Duration::zero() {
        '+'
    } else {
        '-'
    };
    let absolute_minutes = duration.num_seconds().unsigned_abs() / 60;

    format!(
        "{}{:02}:{:02}",
        sign,
        absolute_minutes / 60,
        absolute_minutes % 60
    )
}

fn format_daily_band(
    date: NaiveDate,
    weekday_jp: &str,
    accumulated_free_diff: Duration,
    durations: &DailyBandDurations,
) -> String {
    let categories = [
        ('#', durations.fixed_seconds.max(0)),
        ('x', durations.elapsed_seconds.max(0)),
        ('=', durations.repetitive_seconds.max(0)),
        ('-', durations.non_repetitive_seconds.max(0)),
        (':', durations.rho_leeway_seconds.max(0)),
    ];
    let used_seconds = categories
        .iter()
        .fold(0_i64, |sum, (_, seconds)| sum.saturating_add(*seconds));
    let empty_seconds = SECONDS_PER_DAY.saturating_sub(used_seconds);
    let overflow_seconds = used_seconds.saturating_sub(SECONDS_PER_DAY);

    let mut bar = String::with_capacity(DAILY_BAND_SEGMENTS);
    let mut cumulative_seconds = 0_i64;
    let mut previous_boundary = 0_usize;

    for (symbol, seconds) in categories.into_iter().chain([('.', empty_seconds)]) {
        cumulative_seconds = cumulative_seconds.saturating_add(seconds);
        let boundary = round_daily_band_segment_count(cumulative_seconds.min(SECONDS_PER_DAY))
            .min(DAILY_BAND_SEGMENTS);
        bar.extend(std::iter::repeat(symbol).take(boundary - previous_boundary));
        previous_boundary = boundary;
    }

    let overflow = ">".repeat(round_daily_band_segment_count(overflow_seconds));
    format!(
        "{}({}) {} [{}]{}",
        date,
        weekday_jp,
        format_signed_hours_minutes(accumulated_free_diff),
        bar,
        overflow
    )
}

#[derive(Clone)]
struct TaskListDisplayRow {
    scheduled_start: DateTime<Local>,
    subjective_naive_date_opt: Option<NaiveDate>,
    rank: usize,
    id: Uuid,
    priority: i64,
    work_seconds: i64,
    project_category_opt: Option<ProjectCategory>,
    is_real_task: bool,
    give_up_candidate: bool,
    message_prefix: String,
    task_name: String,
    message: String,
}

impl TaskListDisplayRow {
    // 表示行の全属性を呼び出し側で確定させるため、引数を個別に受け取る。
    #[allow(clippy::too_many_arguments)]
    fn new_task(
        scheduled_start: DateTime<Local>,
        subjective_naive_date: NaiveDate,
        rank: usize,
        id: Uuid,
        priority: i64,
        work_seconds: i64,
        project_category_opt: Option<ProjectCategory>,
        message_prefix: String,
        task_name: String,
    ) -> Self {
        TaskListDisplayRow {
            scheduled_start,
            subjective_naive_date_opt: Some(subjective_naive_date),
            rank,
            id,
            priority,
            work_seconds,
            project_category_opt,
            is_real_task: true,
            give_up_candidate: false,
            message_prefix,
            task_name,
            message: String::new(),
        }
    }

    fn new_message(
        scheduled_start: DateTime<Local>,
        rank: usize,
        id: Uuid,
        priority: i64,
        message: String,
    ) -> Self {
        TaskListDisplayRow {
            scheduled_start,
            subjective_naive_date_opt: None,
            rank,
            id,
            priority,
            work_seconds: 0,
            project_category_opt: None,
            is_real_task: false,
            give_up_candidate: false,
            message_prefix: String::new(),
            task_name: String::new(),
            message,
        }
    }

    fn render_message(&self) -> String {
        if self.is_real_task {
            let message_prefix = if self.give_up_candidate {
                // A means Abandon candidate.
                replace_task_list_icon(&self.message_prefix, "A")
            } else {
                self.message_prefix.clone()
            };

            format!("{}{}", message_prefix, self.task_name)
        } else {
            self.message.clone()
        }
    }
}

fn replace_task_list_icon(message_prefix: &str, icon: &str) -> String {
    let mut parts = message_prefix.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 8 {
        return message_prefix.to_string();
    }

    parts[2] = icon;
    format!("{} ", parts.join(" "))
}

const PROJECT_CATEGORY_SUMMARY_LEN: usize = 6;

fn project_category_symbol(project_category_opt: Option<ProjectCategory>) -> &'static str {
    match project_category_opt {
        Some(ProjectCategory::Earning) => "獲",
        Some(ProjectCategory::Sustaining) => "維",
        Some(ProjectCategory::Recovery) => "回",
        Some(ProjectCategory::Investment) => "資",
        Some(ProjectCategory::Consumption) => "消",
        None => "_",
    }
}

fn format_focused_task_header(project_category_opt: Option<ProjectCategory>) -> String {
    format!(
        "focused task is: project_category={}",
        project_category_symbol(project_category_opt)
    )
}

fn project_category_summary_index(project_category_opt: Option<ProjectCategory>) -> usize {
    match project_category_opt {
        Some(ProjectCategory::Earning) => 0,
        Some(ProjectCategory::Sustaining) => 1,
        Some(ProjectCategory::Recovery) => 2,
        Some(ProjectCategory::Investment) => 3,
        Some(ProjectCategory::Consumption) => 4,
        None => 5,
    }
}

fn project_category_summary_label(index: usize) -> &'static str {
    match index {
        0 => "獲得",
        1 => "維持",
        2 => "回復",
        3 => "投資",
        4 => "消費",
        _ => "未分類",
    }
}

fn summarize_scheduled_work_seconds_by_project_category(
    rows: &[TaskListDisplayRow],
) -> [i64; PROJECT_CATEGORY_SUMMARY_LEN] {
    let mut summary = [0; PROJECT_CATEGORY_SUMMARY_LEN];

    for row in rows.iter().filter(|row| row.is_real_task) {
        let index = project_category_summary_index(row.project_category_opt);
        summary[index] += row.work_seconds;
    }

    summary
}

fn format_scheduled_work_seconds_by_project_category(
    summary: &[i64; PROJECT_CATEGORY_SUMMARY_LEN],
    denominator_seconds: i64,
) -> String {
    let total_seconds: i64 = summary.iter().sum();

    if total_seconds == 0 {
        return "予定カテゴリ: 予定なし".to_string();
    }

    let mut cumulative_seconds = 0;
    let parts = summary
        .iter()
        .enumerate()
        .map(|(index, seconds)| {
            cumulative_seconds += seconds;
            format!(
                "{} {:.1}時間({} | {})",
                project_category_summary_label(index),
                *seconds as f64 / 3600.0,
                format_project_category_percentage(*seconds, denominator_seconds),
                format_project_category_percentage(cumulative_seconds, denominator_seconds)
            )
        })
        .collect::<Vec<_>>();

    format!("予定カテゴリ: {}", parts.join(" / "))
}

fn format_project_category_percentage(seconds: i64, denominator_seconds: i64) -> String {
    if denominator_seconds > 0 {
        format!(
            "{:.0}%",
            seconds as f64 / denominator_seconds as f64 * 100.0
        )
    } else if seconds > 0 {
        "inf%".to_string()
    } else {
        "0%".to_string()
    }
}

fn calculate_free_time_minutes_for_subjective_date(
    date: &NaiveDate,
    last_synced_time: DateTime<Local>,
    eod: DateTime<Local>,
    eod_duration: Duration,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> i64 {
    let local_datetime_base = get_next_morning_datetime(
        Local::now()
            .timezone()
            .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
            .unwrap(),
    );

    if local_datetime_base < last_synced_time
        && last_synced_time < get_next_morning_datetime(local_datetime_base)
    {
        if last_synced_time.hour() < get_next_morning_datetime(last_synced_time).hour() {
            if last_synced_time < eod {
                (eod - last_synced_time).num_minutes()
            } else {
                0
            }
        } else {
            free_time_manager.get_free_minutes(&last_synced_time, &eod)
        }
    } else {
        calculate_full_day_free_time_minutes_for_subjective_date(
            date,
            eod_duration,
            free_time_manager,
        )
    }
}

fn calculate_full_day_free_time_minutes_for_subjective_date(
    date: &NaiveDate,
    eod_duration: Duration,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> i64 {
    let local_tz = Local::now().timezone();
    let start = get_next_morning_datetime(
        local_tz
            .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
            .unwrap(),
    );
    let end = local_tz
        .from_local_datetime(&date.and_hms_opt(23, 59, 59).unwrap())
        .unwrap()
        + eod_duration;
    free_time_manager.get_free_minutes(&start, &end)
}

fn calculate_project_category_denominator_seconds(
    rows: &[TaskListDisplayRow],
    last_synced_time: DateTime<Local>,
    eod: DateTime<Local>,
    eod_duration: Duration,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> i64 {
    let mut dates = rows
        .iter()
        .filter(|row| row.is_real_task)
        .filter_map(|row| row.subjective_naive_date_opt)
        .collect::<Vec<_>>();
    dates.sort();
    dates.dedup();

    dates
        .iter()
        .map(|date| {
            calculate_free_time_minutes_for_subjective_date(
                date,
                last_synced_time,
                eod,
                eod_duration,
                free_time_manager,
            ) * 60
        })
        .sum()
}

fn advance_display_datetime_cursor(
    current_datetime_cursor: DateTime<Local>,
    end_datetime: DateTime<Local>,
) -> DateTime<Local> {
    max(current_datetime_cursor, end_datetime)
}

fn sort_task_list_display_rows(
    rows: &mut [TaskListDisplayRow],
    display_order: TaskListDisplayOrder,
) {
    match display_order {
        TaskListDisplayOrder::ScheduledStartDesc => {
            rows.reverse();
        }
        TaskListDisplayOrder::LowPriorityTail => {
            rows.sort_by(|a, b| {
                b.priority
                    .cmp(&a.priority)
                    .then_with(|| a.scheduled_start.cmp(&b.scheduled_start))
                    .then_with(|| a.rank.cmp(&b.rank))
                    .then_with(|| a.id.cmp(&b.id))
            });
        }
    }
}

fn mark_give_up_candidate_rows(
    rows: &mut [TaskListDisplayRow],
    shortage_seconds: i64,
    target_date: NaiveDate,
) {
    if shortage_seconds <= 0 {
        return;
    }

    let mut candidate_indices: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            if row.is_real_task
                && row.work_seconds > 0
                && row.subjective_naive_date_opt == Some(target_date)
            {
                Some(index)
            } else {
                None
            }
        })
        .collect();

    candidate_indices.sort_by(|a, b| {
        rows[*a]
            .priority
            .cmp(&rows[*b].priority)
            .then_with(|| rows[*b].scheduled_start.cmp(&rows[*a].scheduled_start))
            .then_with(|| rows[*b].rank.cmp(&rows[*a].rank))
            .then_with(|| rows[*b].id.cmp(&rows[*a].id))
    });

    let mut accumulated_seconds = 0;
    for index in candidate_indices {
        rows[index].give_up_candidate = true;
        accumulated_seconds += rows[index].work_seconds;

        if accumulated_seconds >= shortage_seconds {
            break;
        }
    }
}

fn mark_give_up_candidate_rows_by_date(
    rows: &mut [TaskListDisplayRow],
    shortage_duration_by_date: &HashMap<NaiveDate, Duration>,
) {
    let mut dates_and_shortages = shortage_duration_by_date
        .iter()
        .filter_map(|(date, shortage_duration)| {
            if *shortage_duration > Duration::seconds(0) {
                Some((*date, shortage_duration.num_seconds()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    dates_and_shortages.sort_by(|a, b| a.0.cmp(&b.0));

    for (date, shortage_seconds) in dates_and_shortages {
        mark_give_up_candidate_rows(rows, shortage_seconds, date);
    }
}

fn calculate_rho_metrics(
    total_work_seconds: i64,
    repetitive_work_seconds: i64,
    available_minutes: i64,
) -> RhoMetrics {
    let total_work_hours = total_work_seconds as f64 / 3600.0;
    let repetitive_work_hours = repetitive_work_seconds as f64 / 3600.0;
    let non_repetitive_work_hours = (total_work_seconds - repetitive_work_seconds) as f64 / 3600.0;
    let available_hours = available_minutes as f64 / 60.0;
    let free_hours = available_hours - total_work_hours;

    let rho = if available_minutes > 0 {
        total_work_seconds as f64 / (available_minutes * 60) as f64
    } else {
        f64::INFINITY
    };

    let non_repetitive_available_hours = available_hours - repetitive_work_hours;
    let non_repetitive_rho = if non_repetitive_available_hours > 0.0 {
        non_repetitive_work_hours / non_repetitive_available_hours
    } else {
        f64::INFINITY
    };

    RhoMetrics {
        _total_work_hours: total_work_hours,
        repetitive_work_hours,
        non_repetitive_work_hours,
        _available_hours: available_hours,
        free_hours,
        rho,
        non_repetitive_rho,
    }
}

fn calculate_lq_opt(rho: f64) -> Option<f64> {
    if rho < 1.0 {
        Some(rho / (1.0 - rho))
    } else {
        None
    }
}

#[test]
fn test_backward_width_正常系1() {
    let s = String::from("あ");
    let cursor_x = 1;
    let actual = backward_width(&s, cursor_x);
    let expected = 2;
    assert_eq!(actual, expected);
}

#[test]
fn test_backward_width_異常系1() {
    let s = String::from("");
    let cursor_x = 10;
    let actual = backward_width(&s, cursor_x);
    let expected = 0;
    assert_eq!(actual, expected);
}

#[test]
fn test_backward_width_異常系2() {
    let s = String::from("テスト");
    let cursor_x = 0;
    let actual = backward_width(&s, cursor_x);
    let expected = 0;
    assert_eq!(actual, expected);
}

#[test]
fn test_calculate_rho_metrics_単発作業量に端数が漏れないこと() {
    let actual = calculate_rho_metrics(61, 61, 120);

    assert_eq!(actual.non_repetitive_work_hours, 0.0);
    assert_eq!(actual.non_repetitive_rho, 0.0);
}

#[test]
fn test_calculate_rho_metrics_混在ケースでも整合すること() {
    let actual = calculate_rho_metrics(5400, 1800, 120);

    assert!((actual._total_work_hours - 1.5).abs() < 1e-9);
    assert!((actual.repetitive_work_hours - 0.5).abs() < 1e-9);
    assert!((actual.non_repetitive_work_hours - 1.0).abs() < 1e-9);
    assert!((actual._available_hours - 2.0).abs() < 1e-9);
    assert!((actual.free_hours - 0.5).abs() < 1e-9);
    assert!((actual.rho - 0.75).abs() < 1e-9);
    assert!((actual.non_repetitive_rho - (1.0 / 1.5)).abs() < 1e-9);
}

#[test]
fn test_calculate_lq_opt_負荷率が1以上ならinf扱いになること() {
    assert_eq!(calculate_lq_opt(1.0), None);
    assert_eq!(calculate_lq_opt(f64::INFINITY), None);
}

fn get_byte_offset_for_insert(line: &str, cursor_x: usize) -> usize {
    let char_indices_vec = line.char_indices().collect::<Vec<_>>();

    if !line.is_empty() && cursor_x < char_indices_vec.len() {
        char_indices_vec[cursor_x].0
    } else {
        line.len()
    }
}

#[test]
fn test_get_byte_offset_for_insert_正常系1() {
    // "|"
    let line = String::from("");
    let cursor_x: usize = 0;
    let actual = get_byte_offset_for_insert(&line, cursor_x);
    let expected = 0;
    assert_eq!(actual, expected);
}

#[test]
fn test_get_byte_offset_for_insert_正常系2() {
    // |例1の文字列
    let line = String::from("例1の文字列");
    let cursor_x: usize = 0;
    let actual = get_byte_offset_for_insert(&line, cursor_x);
    let expected = 0;
    assert_eq!(actual, expected);
}

#[test]
fn test_get_byte_offset_for_insert_正常系3() {
    // 例1の|文字列
    let line = String::from("例1の文字列");
    let cursor_x: usize = 3;
    let actual = get_byte_offset_for_insert(&line, cursor_x);
    let expected = String::from("例1の").len(); // 3+1+3=7
    assert_eq!(actual, expected);
}

#[test]
fn test_get_byte_offset_for_insert_正常系4() {
    // あ|
    let line = String::from("あ");
    let cursor_x: usize = 1;
    let actual = get_byte_offset_for_insert(&line, cursor_x);
    let expected = String::from("あ").len(); // 3
    assert_eq!(actual, expected);
}

fn get_width_for_rerender(header: &str, line: &str, cursor_x: usize) -> u16 {
    let mut width = UnicodeWidthStr::width(header);

    for ch in line.chars().take(cursor_x) {
        width += UnicodeWidthChar::width(ch).unwrap_or(0);
    }

    width as u16
}

#[test]
fn test_get_width_for_rerender_正常系_アスキー() {
    let header = String::from("schronu>");
    let line = String::from("project new");
    let cursor_x = 3;

    let actual = get_width_for_rerender(&header, &line, cursor_x);
    let expected = 11; // "schronu>pro"
    assert_eq!(actual, expected);
}

#[test]
fn test_get_width_for_rerender_正常系_多バイト1() {
    let header = String::from("schronu>");
    let line = String::from("breakdown タク1"); // 「ス」を入れたい
    let cursor_x = 11;

    let actual = get_width_for_rerender(&header, &line, cursor_x);
    let expected = 20; // "schronu>breakdown タ"
    assert_eq!(actual, expected);
}

#[test]
fn test_get_width_for_rerender_正常系_多バイト2() {
    let header = String::from("schronu>");
    let line = String::from("あい");
    let cursor_x = 2;

    let actual = get_width_for_rerender(&header, &line, cursor_x);
    let expected = 12; // "schronu>あい"
    assert_eq!(actual, expected);
}

fn get_forward_width(line: &str, cursor_x: usize) -> u16 {
    if !line.is_empty() && cursor_x < line.chars().count() {
        let ch_opt = line.chars().nth(cursor_x);
        let n = match ch_opt {
            Some(ch) => UnicodeWidthChar::width(ch).unwrap_or(0),
            None => 0,
        } as u16;

        return n;
    }

    0
}

#[test]
fn test_get_forward_width_正常系1() {
    let line = String::from("あ");
    let cursor_x = 0;

    let actual = get_forward_width(&line, cursor_x);
    let expected = 2;
    assert_eq!(actual, expected);
}

fn execute_show_tree(stdout: &mut dyn SchronuWriter, focused_task_opt: &Option<Task>) {
    writeln!(stdout).unwrap();
    if let Some(focused_task) = focused_task_opt.as_ref() {
        let s: String = focused_task.tree_debug_pretty_print();
        let lines: Vec<_> = s.split('\n').collect();
        for line in lines.iter() {
            // Done([+])のタスクは表示しない
            // 恒久的には、tree_debug_pretty_print()に似た関数を自分で実装してカスタマイズする
            if line.contains("[ ]") || line.contains("[-]") {
                writeln_newline(stdout, line).unwrap()
            }
        }
    }
    writeln!(stdout).unwrap();
}

fn execute_start_new_project(
    _stdout: &mut dyn SchronuWriter,
    focused_task_id_opt: &mut Option<Uuid>,
    task_repository: &mut dyn TaskRepositoryTrait,
    new_project_name_str: &str,
    defer_days_opt: Option<i64>,
    estimated_work_minutes_opt: Option<i64>,
) {
    let pending_until = defer_days_opt.map(|defer_days| {
        get_next_morning_datetime(task_repository.get_last_synced_time())
            + Duration::days(defer_days - 1)
    });
    if let Ok(task_id) = create_task(
        task_repository,
        CreateTaskInput {
            name: new_project_name_str.to_string(),
            estimated_work_minutes: estimated_work_minutes_opt,
            pending_until,
        },
    ) {
        *focused_task_id_opt = Some(task_id);
    }
}

fn execute_make_appointment(focused_task_opt: &Option<Task>, start_time: DateTime<Local>) {
    if let Some(task) = focused_task_opt {
        task.make_appointment(start_time);
    }
}

fn execute_show_ancestor(stdout: &mut dyn SchronuWriter, focused_task_opt: &Option<Task>) {
    writeln!(stdout).unwrap();

    // まずは葉タスクから根に向かいながら後ろに追加していき、
    // 最後に逆順にして表示する
    let mut ancestors: Vec<(DateTime<Local>, Task)> = vec![];

    if let Some(task) = focused_task_opt {
        ancestors = task.list_all_parent_tasks_with_first_available_time();
    }

    ancestors.reverse();

    for (level, (first_available_datetime, task)) in ancestors.iter().enumerate() {
        let header = if level == 0 {
            String::from("")
        } else {
            let indent = ' '.to_string().repeat(4 * (level - 1));
            format!("{}`-- ", &indent)
        };

        let id = task.get_id();
        let name = task.get_name();
        let estimated_work_minutes =
            (task.get_estimated_work_seconds() as f64 / 60.0).ceil() as i64;
        let first_available_date_str = first_available_datetime.format("%Y/%m/%d").to_string();

        let msg = format!(
            "{}{} [{}] {}m {}",
            &header, &id, &first_available_date_str, &estimated_work_minutes, &name
        );
        writeln_newline(stdout, &msg).unwrap();
    }

    writeln_newline(stdout, "").unwrap();
}

fn execute_show_leaf_tasks(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    _free_time_manager: &mut dyn FreeTimeManagerTrait,
) {
    let mut ans_tpls = vec![];

    for project_root_task in task_repository.get_all_projects().iter() {
        let project_name = project_root_task.get_name();

        // 優先度が高いタスクほど下に表示されるようにし、フォーカスが当たるタスクは末尾に表示されるようにする。
        let leaf_tasks = extract_leaf_tasks_from_project(project_root_task);
        for leaf_task in leaf_tasks.iter() {
            let deadline_time_opt = leaf_task.get_deadline_time_opt();
            let neg_priority = -leaf_task.get_priority();
            let id = leaf_task.get_id();
            let message = format!("{}\t{:?}", project_name, leaf_task.get_attr());

            let tpl = (
                deadline_time_opt.is_none(),
                neg_priority,
                deadline_time_opt,
                id,
                message,
            );
            ans_tpls.push(tpl);
        }
    }

    ans_tpls.sort();
    ans_tpls.reverse();

    for (ind, ans_tpl) in ans_tpls.iter().enumerate() {
        let task_cnt = ans_tpls.len() - ind;
        let message = format!("{}\t{}", task_cnt, ans_tpl.4);
        writeln_newline(stdout, &message).unwrap();
    }
    writeln_newline(stdout, "").unwrap();
}

// 集計用タプルはこの関数内だけで使用し、意味を持つ公開型を増やさない。
#[allow(clippy::type_complexity)]
fn execute_show_all_tasks(
    stdout: &mut dyn SchronuWriter,
    focused_task_id_opt: &mut Option<Uuid>,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    pattern_opt: &Option<String>,
    display_order: TaskListDisplayOrder,
) {
    let scheduled_tasks = get_schedule(task_repository);
    let mut dt_id_tpl_arr = scheduled_tasks
        .iter()
        .map(|scheduled| {
            let deadline_time_opt = scheduled.task.deadline_time;
            (
                (get_next_morning_datetime(scheduled.first_available_time) - Duration::days(1))
                    .date_naive(),
                deadline_time_opt.is_none(),
                scheduled.first_available_time,
                -scheduled.task.priority,
                scheduled.rank,
                deadline_time_opt,
                scheduled.task.id,
            )
        })
        .collect::<Vec<_>>();
    dt_id_tpl_arr.sort();
    dt_id_tpl_arr.dedup_by_key(|entry| entry.6);

    let mut task_list_display_rows: Vec<TaskListDisplayRow> = vec![];
    let mut available_biggest_row_opt: Option<TaskListDisplayRow> = None;
    let mut available_biggest_task_estimate_work_seconds = 0;

    // ここからρ計算用
    let last_synced_time = task_repository.get_last_synced_time();

    // FIXME 外部設定ファイルで設定できるようにする
    let eod_duration = Duration::hours(0) + Duration::minutes(30);
    let eod = (get_next_morning_datetime(last_synced_time) + Duration::days(0))
        .with_hour(0)
        .expect("invalid hour")
        .with_minute(0)
        .expect("invalid minute")
        + eod_duration;
    // ここまでρ計算用

    let is_calendar_func = pattern_opt.as_ref().map_or(false, |pattern| {
        pattern == "暦" || pattern == "calendar" || pattern == "cal"
    });

    let is_band_func = pattern_opt
        .as_ref()
        .map_or(false, |pattern| pattern == "帯" || pattern == "band");

    let is_daily_summary_func = is_calendar_func || is_band_func;

    let is_flatten_func = pattern_opt.as_ref().map_or(false, |pattern| {
        pattern == "平" || pattern == "flatten" || pattern == "flat"
    });

    // 日付ごとのタスク数を集計する
    let mut counter: HashMap<NaiveDate, usize> = HashMap::new();
    let mut total_estimated_work_seconds_of_the_date_counter: HashMap<NaiveDate, i64> =
        HashMap::new();
    let mut deadline_estimated_work_seconds_map: HashMap<NaiveDate, i64> = HashMap::new();

    let mut repetitive_task_estimated_work_seconds_map: HashMap<NaiveDate, i64> = HashMap::new();

    // 日ごとの、前倒し可能なタスクの見積もりの和
    // 前倒し可能という決め方だと、何日まで前倒しできるのか曖昧性が発生する?
    let mut adjustable_estimated_work_seconds_map: HashMap<NaiveDate, i64> = HashMap::new();

    // 「暦」コマンドで、未来のサマリは見ても仕方ないので、直近の28日ぶん(配列の末尾)に絞る
    const SUMMARY_DAYS: usize = 28;

    // タスク一覧で、どのタスクをいつやる見込みかを表示するために、「現在時刻」をズラして見ていく
    let mut current_datetime_cursor = task_repository.get_last_synced_time();
    let yyyymmdd_reg = Regex::new(r"^(\d{4})/(\d{2})/(\d{2})$").unwrap();
    let integer_reg = Regex::new(r"^\d+$").unwrap();
    let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];

    for (ind, scheduled_task) in scheduled_tasks.iter().enumerate() {
        let dt = &scheduled_task.first_available_time;
        let scheduled_start = &scheduled_task.scheduled_start;
        let scheduled_end = &scheduled_task.scheduled_end;
        let scheduled_work_seconds = scheduled_task.scheduled_work_seconds;
        let total_work_seconds = scheduled_task.total_work_seconds;
        let rank = &scheduled_task.rank;
        let deadline_time_opt = &scheduled_task.task.deadline_time;
        let id = &scheduled_task.task.id;
        let subjective_naive_date =
            (get_next_morning_datetime(*scheduled_start) - Duration::days(1)).date_naive();

        // 「今」「明」コマンドの場合は未来の情報には興味がないので、スキップする
        if let Some(pattern) = pattern_opt {
            if pattern == "今"
                || pattern == "明"
                || pattern == "近"
                || pattern == "暦"
                || pattern == "帯"
            {
                let valid_days = if pattern == "今" {
                    0
                } else if pattern == "明" || pattern == "近" {
                    1
                } else if pattern == "暦" || pattern == "帯" {
                    SUMMARY_DAYS as i64
                } else {
                    // 事前にif文で囲ってあるので、通常はこのケースに入ることはない
                    9999
                };

                if (get_next_morning_datetime(*scheduled_start)
                    - get_next_morning_datetime(task_repository.get_last_synced_time()))
                    > Duration::days(valid_days)
                {
                    break;
                }
            }
        }

        counter
            .entry(subjective_naive_date)
            .and_modify(|cnt| *cnt += 1)
            .or_insert(1);

        let task_opt = task_repository.get_by_id(*id);
        if let Some(task) = task_opt {
            let inherited_repetition_interval_days_opt =
                task.get_inherited_repetition_interval_days_opt();
            let mut repetition_prefix_label = "".to_string();

            if let Some(repetition_interval_days) = inherited_repetition_interval_days_opt {
                // FIXME 【繰】というマジックナンバーが2ヶ所に登場していて危ない
                repetition_prefix_label = format!(
                    "{}【繰】({})",
                    repetition_prefix_label, repetition_interval_days
                );
            }

            if task.get_is_on_other_side() {
                repetition_prefix_label = format!("{}【待ち】", repetition_prefix_label);
            }

            // 前倒し可能なタスクの見積もり時間をカウントする
            let adjustable_prefix_label =
                get_adjustable_prefix_label(&task, *dt, *rank, last_synced_time);

            if !adjustable_prefix_label.is_empty() {
                adjustable_estimated_work_seconds_map
                    .entry(subjective_naive_date)
                    .and_modify(|estimated_work_seconds_val| {
                        *estimated_work_seconds_val += task.get_estimated_work_seconds()
                    })
                    .or_insert(task.get_estimated_work_seconds());
            }

            let name = format!(
                "{}{}{}",
                adjustable_prefix_label,
                repetition_prefix_label,
                task.get_name()
            );
            let chars_vec: Vec<char> = name.chars().collect();
            let max_len: usize = 70;

            let chars_width_acc: Vec<usize> = chars_vec
                .iter()
                .map(|&ch| UnicodeWidthChar::width(ch).unwrap_or(0))
                .scan(0, |acc, x| {
                    *acc += x;
                    Some(*acc)
                })
                .collect();

            let latest_index_opt =
                chars_width_acc
                    .iter()
                    .enumerate()
                    .find_map(
                        |(index, &value)| {
                            if value > max_len {
                                Some(index)
                            } else {
                                None
                            }
                        },
                    );

            let mut shorten_name: String = if let Some(latest_index) = latest_index_opt {
                format!(
                    "{}...",
                    chars_vec.iter().take(latest_index + 1).collect::<String>()
                )
            } else {
                name.to_string()
            };
            if total_work_seconds > scheduled_work_seconds {
                shorten_name = format!(
                    "<{}/{}>{}",
                    round_up_sec_as_minute(scheduled_work_seconds),
                    round_up_sec_as_minute(total_work_seconds),
                    shorten_name
                );
            }

            // 元々見積もり時間から作業済時間を引いたのが残りの見積もり時間
            // ただし、作業時間が元々の見積もり時間をオーバーしている時には既に想定外の事態になっているため、
            // 残りの見積もりを0とはせず、安全に倒して元々の見積もりの2倍として扱う
            let estimated_work_seconds = scheduled_work_seconds;
            if let Some(deadline_time) = deadline_time_opt {
                let deadline_naive_date =
                    (get_next_morning_datetime(*deadline_time) - Duration::days(1)).date_naive();

                deadline_estimated_work_seconds_map
                    .entry(deadline_naive_date)
                    .and_modify(|deadline_estimated_work_seconds| {
                        *deadline_estimated_work_seconds += estimated_work_seconds
                    })
                    .or_insert(estimated_work_seconds);
            }

            if inherited_repetition_interval_days_opt.is_some() {
                repetitive_task_estimated_work_seconds_map
                    .entry(subjective_naive_date)
                    .and_modify(|repetitive_task_estimated_work_seconds| {
                        *repetitive_task_estimated_work_seconds += estimated_work_seconds
                    })
                    .or_insert(estimated_work_seconds);
            }

            let current_datetime_cursor_clone = &current_datetime_cursor.clone();
            let start_datetime = scheduled_start;

            // 「今」か「明」か「近」の時のみ、日時カーソルが飛んだ場合には、その間の時間を表示する
            if (*scheduled_start - current_datetime_cursor_clone).num_minutes() > 0 {
                let blank_duration = *scheduled_start - current_datetime_cursor_clone;
                let tmp_id = Uuid::new_v4();

                let skip_msg = format!(
                    "---- ------------------------------------ - ---------- --------------------- - -- -- {}分間の空き時間",
                    blank_duration.num_minutes()
                );

                if let Some(pattern) = pattern_opt {
                    if (pattern == "今"
                        && *scheduled_start
                            < get_next_morning_datetime(task_repository.get_last_synced_time()))
                        || (pattern == "明"
                            && *current_datetime_cursor_clone
                                >= get_next_morning_datetime(
                                    task_repository.get_last_synced_time(),
                                )
                            && *scheduled_start
                                < get_next_morning_datetime(task_repository.get_last_synced_time())
                                    + Duration::days(1))
                        || (pattern == "近"
                            && *scheduled_start
                                < get_next_morning_datetime(task_repository.get_last_synced_time())
                                    + Duration::days(1))
                    {
                        task_list_display_rows.push(TaskListDisplayRow::new_message(
                            *current_datetime_cursor_clone,
                            0,
                            tmp_id,
                            0,
                            skip_msg,
                        ));
                    }
                }
            }

            let end_datetime = *scheduled_end;
            current_datetime_cursor =
                advance_display_datetime_cursor(current_datetime_cursor, end_datetime);

            total_estimated_work_seconds_of_the_date_counter
                .entry(subjective_naive_date)
                .and_modify(|estimated_work_seconds_val| {
                    *estimated_work_seconds_val += estimated_work_seconds
                })
                .or_insert(estimated_work_seconds);

            // ! : 今日中が締切。締切注意の意
            let deadline_icon: String = "!".to_string();

            // v : もっと着手を手前(下)にせよの意
            let breaking_deadline_icon: String = "v".to_string();

            // / : 今日着手する予定の葉タスク。/という記号自体に強い意味合いはない。
            let today_leaf_icon: String = "/".to_string();

            let icon = if task.get_deadline_time_opt().is_some()
                && task.get_deadline_time_opt().unwrap()
                    < get_next_morning_datetime(last_synced_time)
                && task.get_deadline_time_opt().unwrap() < end_datetime
            {
                &breaking_deadline_icon
            } else if task.get_deadline_time_opt().is_some()
                && task.get_deadline_time_opt().unwrap()
                    < get_next_morning_datetime(last_synced_time)
            {
                &deadline_icon
            } else if rank == &0 && scheduled_start < &eod {
                &today_leaf_icon
            } else {
                // - : 特に無しだが、空白にすると列数が乱れるので目立たない記号を入れる
                "-"
            };

            let deadline_string = if let Some(deadline_time) = deadline_time_opt {
                if *deadline_time < get_next_morning_datetime(last_synced_time) {
                    let breaking_minutes = (end_datetime - deadline_time).num_minutes().abs();
                    let breaking_hh = breaking_minutes / 60;
                    let breaking_mm = breaking_minutes % 60;

                    if *deadline_time < last_synced_time {
                        format!("+{:02}:{:02}ASAP", breaking_hh, breaking_mm)
                    } else if *deadline_time < end_datetime {
                        format!("+{:02}:{:02}____", breaking_hh, breaking_mm)
                    } else {
                        format!("____-{:02}:{:02}", breaking_hh, breaking_mm)
                    }
                } else {
                    let deadline_leeway_days = (*deadline_time - end_datetime).num_days().abs();

                    if deadline_leeway_days == 0 {
                        "________0D".to_string()
                    } else if *deadline_time > end_datetime {
                        format!("_____-{:03}D", deadline_leeway_days)
                    } else {
                        format!("_____+{:03}D", deadline_leeway_days)
                    }
                }
            } else {
                "____/__/__".to_string()
            };

            let message_prefix: String = format!(
                "{:04} {} {} {} {}({})-{}~{} {} {:02.0} {:02} {} ",
                ind,
                id,
                icon,
                deadline_string,
                start_datetime.format("%m/%d"),
                get_weekday_jp(&start_datetime.date_naive()),
                start_datetime.format("%H:%M"),
                end_datetime.format("%H:%M"),
                rank,
                round_up_sec_as_minute(estimated_work_seconds),
                task.get_priority(),
                project_category_symbol(task.get_project_category_opt())
            );
            let msg = format!("{}{}", message_prefix, shorten_name);
            let task_list_display_row = TaskListDisplayRow::new_task(
                *scheduled_start,
                subjective_naive_date,
                *rank,
                *id,
                task.get_priority(),
                estimated_work_seconds,
                task.get_project_category_opt(),
                message_prefix,
                shorten_name,
            );

            match pattern_opt {
                Some(pattern) => {
                    // FIXME 文字列マッチの絞り込み機能とその他の属性による絞り込みを機能を分ける
                    if pattern == "葉" {
                        if rank == &0
                            || task.get_deadline_time_opt().is_some()
                                && task.get_deadline_time_opt().unwrap()
                                    < get_next_morning_datetime(last_synced_time)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "枝" {
                        if rank > &0 {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "印" {
                        if msg.contains(&format!(" {} ", &deadline_icon))
                            || msg.contains(&format!(" {} ", &breaking_deadline_icon))
                            || msg.contains(&format!(" {} ", &today_leaf_icon))
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "〆" {
                        if msg.contains(&format!(" {} ", &deadline_icon))
                            || msg.contains(&format!(" {} ", &breaking_deadline_icon))
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if is_daily_summary_func || is_flatten_func {
                        // カレンダー表示機能を使う時には、タスク一覧は表示しない。
                    } else if pattern == "今" {
                        if get_next_morning_datetime(*scheduled_start)
                            == get_next_morning_datetime(last_synced_time)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "明" {
                        if get_next_morning_datetime(*scheduled_start)
                            == get_next_morning_datetime(last_synced_time) + Duration::days(1)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "近" {
                        if get_next_morning_datetime(*scheduled_start)
                            == get_next_morning_datetime(last_synced_time)
                            || get_next_morning_datetime(*scheduled_start)
                                == get_next_morning_datetime(last_synced_time) + Duration::days(1)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "単" {
                        // non_repetitive (単発) のタスクのみを表示する
                        // FIXME 【繰】が2ヶ所に登場していて危ない
                        if !msg.contains("【繰】") {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if days_of_week.contains(&pattern.as_str()) {
                        // 月 火 水 木 金 土 日 が指定された時は、明日以降で、直近のその曜日のタスクを表示する
                        let todays_morning_datetime =
                            get_next_morning_datetime(last_synced_time) - Duration::days(1);
                        let dn = todays_morning_datetime.date_naive();
                        let now_weekday_jp = get_weekday_jp(&dn);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == pattern.as_str())
                            .unwrap();

                        let ind_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                        // 今日のデータについては「全 今」で表示できるので、その代わりに、1週間後の同じ曜日の情報を表示するようにする
                        let days: i64 = if ind_diff == 0 { 7 } else { ind_diff as i64 };

                        if get_next_morning_datetime(last_synced_time) + Duration::days(days)
                            == get_next_morning_datetime(*scheduled_start)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "週" {
                        // 今日を含む直近1週間のタスクを表示する
                        if get_next_morning_datetime(*scheduled_start)
                            - get_next_morning_datetime(last_synced_time)
                            < Duration::days(7)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "末" {
                        // 週末までのタスクを表示する
                        let todays_morning_datetime =
                            get_next_morning_datetime(last_synced_time) - Duration::days(1);
                        let dn = todays_morning_datetime.date_naive();
                        let now_weekday_jp = get_weekday_jp(&dn);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind =
                            days_of_week.iter().position(|&x| x == "日").unwrap();

                        let days_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                        if get_next_morning_datetime(*scheduled_start)
                            - get_next_morning_datetime(last_synced_time)
                            <= Duration::days(days_diff as i64)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "翌" {
                        // 翌週末までのタスクを表示する
                        let todays_morning_datetime =
                            get_next_morning_datetime(last_synced_time) - Duration::days(1);
                        let dn = todays_morning_datetime.date_naive();
                        let now_weekday_jp = get_weekday_jp(&dn);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind =
                            days_of_week.iter().position(|&x| x == "日").unwrap();

                        let days_diff =
                            ((7 + target_days_of_week_ind - now_days_of_week_ind) % 7) as i64;

                        let diff = get_next_morning_datetime(*scheduled_start)
                            - get_next_morning_datetime(last_synced_time);
                        if Duration::days(days_diff) < diff && diff <= Duration::days(days_diff + 7)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if yyyymmdd_reg.is_match(pattern) {
                        let caps = yyyymmdd_reg.captures(pattern).unwrap();
                        let yyyy: i32 = caps[1].parse().unwrap();
                        let mm: u32 = caps[2].parse().unwrap();
                        let dd: u32 = caps[3].parse().unwrap();

                        let yyyymmdd = Local.with_ymd_and_hms(yyyy, mm, dd, 0, 0, 0).unwrap();

                        if get_next_morning_datetime(*scheduled_start) - Duration::days(1)
                            == get_next_morning_datetime(yyyymmdd)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if integer_reg.is_match(pattern) {
                        let caps = integer_reg.captures(pattern).unwrap();
                        let input_minute: i64 = caps[0].parse().unwrap();
                        let target_free_time_seconds = input_minute * 60;

                        if *scheduled_start > get_next_morning_datetime(last_synced_time)
                            || last_synced_time < task.get_start_time()
                        {
                            continue;
                        }

                        // 【待ち】がマジックナンバーなのがちょっとよくない
                        if *rank == 0
                            && !msg.contains("【待ち】")
                            && estimated_work_seconds < target_free_time_seconds
                            && estimated_work_seconds > available_biggest_task_estimate_work_seconds
                        {
                            available_biggest_task_estimate_work_seconds = estimated_work_seconds;

                            available_biggest_row_opt = Some(task_list_display_row.clone());
                        }
                    } else if name.to_lowercase().contains(&pattern.to_lowercase())
                        || msg.contains(pattern)
                    {
                        task_list_display_rows.push(task_list_display_row.clone());
                    }
                }
                None => {
                    task_list_display_rows.push(task_list_display_row.clone());
                }
            }
        }
    }

    // 着手可能な最大のタスクを実施するモード
    if let Some(row) = available_biggest_row_opt {
        task_list_display_rows.push(row);
    }

    // 1日の残りの時間から稼働率ρを計算する
    let busy_minutes = max(
        0,
        free_time_manager.get_busy_minutes(&last_synced_time, &eod),
    );
    let busy_hours = busy_minutes as f64 / 60.0;
    let busy_s = format!("残り拘束時間は{:.1}時間です", busy_hours);

    let naive_dt_today =
        (get_next_morning_datetime(last_synced_time) - Duration::days(1)).date_naive();
    let today_total_deadline_estimated_work_seconds =
        *total_estimated_work_seconds_of_the_date_counter
            .get(&naive_dt_today)
            .unwrap_or(&0);
    let today_total_deadline_estimated_work_minutes =
        (today_total_deadline_estimated_work_seconds as f64 / 60.0).ceil() as i64;
    let lambda_minutes = today_total_deadline_estimated_work_minutes + busy_minutes;
    let lambda_hours = lambda_minutes as f64 / 60.0;

    let estimated_finish_dt = last_synced_time + Duration::minutes(lambda_minutes);
    let s = format!(
        "完了見込み日時は{:.1}時間後の{}です",
        lambda_hours,
        estimated_finish_dt.format("%Y/%m/%d %H:%M:%S")
    );

    let mu_minutes = max(0, (eod - last_synced_time).num_minutes());
    let today_total_repetitive_estimated_work_seconds = *repetitive_task_estimated_work_seconds_map
        .get(&naive_dt_today)
        .unwrap_or(&0);
    let available_minutes = mu_minutes - busy_minutes;
    let rho_metrics = calculate_rho_metrics(
        today_total_deadline_estimated_work_seconds,
        today_total_repetitive_estimated_work_seconds,
        available_minutes,
    );
    let lq1_opt = calculate_lq_opt(rho_metrics.rho);
    let non_repetitive_lq_opt = calculate_lq_opt(rho_metrics.non_repetitive_rho);

    let free_hours = rho_metrics.free_hours;
    let free_hours_sign = if free_hours >= 0.0 { '+' } else { '-' };
    let free_hours_hour: i64 = free_hours.abs().floor() as i64;
    let free_hours_minute: i64 = ((free_hours.abs() - free_hours_hour as f64) * 60.0) as i64;

    let non_repetitive_rho_msg = format!(
        "one ρ = ({:.2} + 0.00) / ({:.2} + 0.00 {} {} {} {}/60) = {:4.2}",
        rho_metrics.non_repetitive_work_hours,
        rho_metrics.non_repetitive_work_hours,
        free_hours_sign,
        free_hours_hour,
        free_hours_sign,
        free_hours_minute,
        rho_metrics.non_repetitive_rho,
    );
    let non_repetitive_lq_msg = match non_repetitive_lq_opt {
        Some(non_repetitive_lq) => format!("Lq = {:.1}", non_repetitive_lq),
        None => "Lq = inf".to_string(),
    };

    let s_for_non_repetitive_rho = format!("{}, {}", non_repetitive_rho_msg, non_repetitive_lq_msg);

    let rho1_msg = format!(
        "rep ρ = ({:.2} + {:.2}) / ({:.2} + {:.2} {} {} {} {}/60) = {:4.2}",
        rho_metrics.non_repetitive_work_hours,
        rho_metrics.repetitive_work_hours,
        rho_metrics.non_repetitive_work_hours,
        rho_metrics.repetitive_work_hours,
        free_hours_sign,
        free_hours_hour,
        free_hours_sign,
        free_hours_minute,
        rho_metrics.rho,
    );

    let lq_msg = match lq1_opt {
        Some(lq1) => format!("Lq = {:.1}", lq1),
        None => "Lq = inf".to_string(),
    };

    let s_for_rho1 = format!("{}, {}", rho1_msg, lq_msg);

    // 日付の小さい順にソートする
    let mut counter_arr: Vec<(&NaiveDate, &usize)> = counter.iter().collect();
    counter_arr.sort_by(|a, b| a.0.cmp(b.0));

    let mut daily_summary_rows: Vec<DailySummaryRow> = vec![];
    let mut shortage_duration_by_date: HashMap<NaiveDate, Duration> = HashMap::new();

    // 順調フラグ
    let mut has_today_deadline_leeway = true;
    let mut has_today_freetime_leeway = true;
    let mut has_today_new_task_leeway = true;
    let mut has_tomorrow_deadline_leeway = true;
    let mut has_tomorrow_freetime_leeway = true;
    let mut has_weekly_deadline_leeway = true;
    let mut has_weekly_freetime_leeway = true;

    // 「それぞれの日の rho (0.7) との差」の累積和。
    // どれくらい突発を吸収できるかの指標となる。
    // 元々は単に0.7との差で計算していたが、それだと0.7<rho<1.0でその日のタスクがなんとかなっているのに
    // 0.7との差の累積和が肥大化して使いものにならなかったため、以下の定義で計算するようにした。
    // ただし、特定の日にタスクを寄せて無理矢理rho<0.7の日を作るほうが良く見えてしまうので注意が必要。
    // rho < 0.7 : 累積和はそのぶん減る
    // 0.7<= rho <=1.0 : ノーカウント。その日のうちに吸収できる
    // 1.0 < rho : 累積和はそのぶん増える
    let mut accumulate_duration_diff_to_goal_rho = Duration::minutes(0);

    // 「それぞれの日の自由時間との差」の累積和
    let mut accumulate_duration_diff_to_limit = Duration::minutes(0);

    // 平坦化可能ポイント
    let mut flattenable_date_opt: Option<NaiveDate> = None;
    let mut overload_day_is_found = false;
    let mut flattenable_duration = Duration::seconds(0);

    let mut first_caught_up_date = NaiveDate::from_ymd_opt(2037, 12, 31).unwrap();

    let mut first_leeway_date = NaiveDate::from_ymd_opt(2037, 12, 31).unwrap();
    let mut first_leeway_duration = Duration::seconds(0);

    let mut max_accumulate_duration_diff_to_limit = -Duration::hours(24);
    let mut max_accumulate_duration_diff_to_limit_date =
        NaiveDate::from_ymd_opt(1900, 1, 1).unwrap();

    let mut max_accumulated_rho_diff: f64 = -1.0;
    let mut max_accumulated_rho_diff_date = NaiveDate::from_ymd_opt(1900, 1, 1).unwrap();

    let max_counter_days = min(counter_arr.len(), SUMMARY_DAYS);

    for (date, _cnt) in &counter_arr[0..max_counter_days] {
        let total_estimated_work_seconds_of_the_date: i64 =
            *total_estimated_work_seconds_of_the_date_counter
                .get(date)
                .unwrap_or(&0);
        let total_estimated_work_hours_of_the_date =
            total_estimated_work_seconds_of_the_date as f64 / 3600.0;

        let total_repetitive_task_work_seconds_of_the_date =
            *repetitive_task_estimated_work_seconds_map
                .get(date)
                .unwrap_or(&0);
        let total_repetitive_task_work_hours_of_the_date =
            total_repetitive_task_work_seconds_of_the_date as f64 / 3600.0;

        let cnt_of_the_date = *counter.get(date).unwrap_or(&0);

        let weekday_jp = get_weekday_jp(date);

        let free_time_minutes = calculate_free_time_minutes_for_subjective_date(
            date,
            last_synced_time,
            eod,
            eod_duration,
            free_time_manager,
        );
        let full_day_free_time_minutes_opt = if is_band_func {
            Some(calculate_full_day_free_time_minutes_for_subjective_date(
                date,
                eod_duration,
                free_time_manager,
            ))
        } else {
            None
        };

        let free_time_hours = free_time_minutes as f64 / 60.0;
        let rho_in_date = total_estimated_work_hours_of_the_date / free_time_hours;
        let non_repetitive_rho_in_date =
            if free_time_hours - total_repetitive_task_work_hours_of_the_date > 0.0 {
                (total_estimated_work_hours_of_the_date
                    - total_repetitive_task_work_hours_of_the_date)
                    / (free_time_hours - total_repetitive_task_work_hours_of_the_date)
            } else {
                f64::INFINITY
            };

        const RHO_GOAL: f64 = 0.7;

        let diff_to_goal = if free_time_hours - total_repetitive_task_work_hours_of_the_date > 0.0 {
            (total_estimated_work_hours_of_the_date - total_repetitive_task_work_hours_of_the_date)
                - (free_time_hours - total_repetitive_task_work_hours_of_the_date) * RHO_GOAL
        } else {
            0.0
        };
        let diff_to_goal_sign: char = if diff_to_goal > 0.0 { ' ' } else { '-' };
        let diff_to_goal_hour = diff_to_goal.abs().floor();
        let diff_to_goal_minute = (diff_to_goal.abs() - diff_to_goal_hour) * 60.0;

        let over_time_hours_f = total_estimated_work_hours_of_the_date - free_time_hours;
        let over_time_hours = over_time_hours_f.abs().floor() as i64;
        let over_time_minutes = (over_time_hours_f.abs() * 60.0) as i64 % 60;

        let adjustable_estimated_work_seconds: i64 = *adjustable_estimated_work_seconds_map
            .get(date)
            .unwrap_or(&0);
        let adjustable_estimated_work_duration =
            Duration::seconds(adjustable_estimated_work_seconds);

        // これまでにどれだけ累積でマイナス(余裕)だったとしても、前倒しできるタスクの量でキャップされる
        if accumulate_duration_diff_to_limit < -adjustable_estimated_work_duration {
            accumulate_duration_diff_to_limit = -adjustable_estimated_work_duration
        }

        let over_time_duration = if over_time_hours_f > 0.0 {
            Duration::hours(over_time_hours) + Duration::minutes(over_time_minutes)
        } else {
            -Duration::hours(over_time_hours) - Duration::minutes(over_time_minutes)
        };
        accumulate_duration_diff_to_limit += over_time_duration;

        if accumulate_duration_diff_to_limit > max_accumulate_duration_diff_to_limit {
            max_accumulate_duration_diff_to_limit = accumulate_duration_diff_to_limit;
            max_accumulate_duration_diff_to_limit_date = **date;
        }
        shortage_duration_by_date.insert(**date, accumulate_duration_diff_to_limit);

        if !daily_summary_rows.is_empty()
            && accumulate_duration_diff_to_limit < Duration::seconds(0)
            && **date < first_caught_up_date
        {
            first_caught_up_date = **date;
        }

        if !overload_day_is_found && accumulate_duration_diff_to_limit > Duration::seconds(0) {
            overload_day_is_found = true;
        } else if accumulate_duration_diff_to_limit <= Duration::seconds(300) {
            let flattenable_duration_cand = Duration::seconds(
                free_time_minutes * 60 - total_estimated_work_seconds_of_the_date,
            );
            if flattenable_date_opt.is_none()
                && overload_day_is_found
                && flattenable_duration_cand >= Duration::seconds(900)
            {
                flattenable_date_opt = Some(**date);
                flattenable_duration = flattenable_duration_cand;
            }
        }

        let diff_to_limit_sign: char = if accumulate_duration_diff_to_limit > Duration::minutes(0) {
            ' '
        } else {
            '-'
        };

        let repetitive_task_estimated_work_seconds = *repetitive_task_estimated_work_seconds_map
            .get(date)
            .unwrap_or(&0);
        let repetitive_task_estimated_work_hours =
            repetitive_task_estimated_work_seconds as f64 / 3600.0;

        let non_repetitive_free_time_hours = free_time_hours - repetitive_task_estimated_work_hours;
        let accumulated_rho_diff = if free_time_hours - repetitive_task_estimated_work_hours > 0.0 {
            accumulate_duration_diff_to_limit.num_minutes() as f64
                / 60.0
                / non_repetitive_free_time_hours
        } else {
            f64::INFINITY
        };

        accumulate_duration_diff_to_goal_rho = if accumulated_rho_diff >= 0.0 {
            // タスクが捌けていない場合はそれがそのまま積み残される
            accumulate_duration_diff_to_limit
        } else if accumulated_rho_diff < RHO_GOAL - 1.0 && non_repetitive_rho_in_date < RHO_GOAL {
            // タスクが捌けてかなり余裕がある場合
            accumulate_duration_diff_to_goal_rho
                - Duration::hours(diff_to_goal_hour as i64)
                - Duration::minutes(diff_to_goal_minute as i64)
        } else if accumulated_rho_diff < 0.0 {
            // なんとかその日のうちに捌けている状態。積む余裕は無い
            Duration::minutes(0)
        } else {
            accumulate_duration_diff_to_goal_rho
        };

        if accumulate_duration_diff_to_goal_rho < Duration::minutes(0) && **date < first_leeway_date
        {
            first_leeway_date = **date;
            first_leeway_duration = accumulate_duration_diff_to_goal_rho;
        }

        let acc_diff_to_goal_sign: char =
            if accumulate_duration_diff_to_goal_rho > Duration::minutes(0) {
                ' '
            } else {
                '-'
            };

        let diff_to_limit_in_day_sign: char =
            if total_estimated_work_hours_of_the_date > free_time_hours {
                ' '
            } else {
                '-'
            };
        let diff_to_limit_hours_in_day: i64 = (total_estimated_work_hours_of_the_date
            - free_time_hours)
            .abs()
            .floor() as i64;
        let diff_to_limit_minutes_in_day: i64 =
            (((total_estimated_work_hours_of_the_date - free_time_hours).abs()
                - diff_to_limit_hours_in_day as f64)
                * 60.0)
                .floor() as i64;

        if !daily_summary_rows.is_empty()
            && accumulated_rho_diff.is_finite()
            && accumulated_rho_diff > max_accumulated_rho_diff
        {
            max_accumulated_rho_diff = accumulated_rho_diff;
            max_accumulated_rho_diff_date = **date;
        }

        let deadline_rest_duration_seconds: i64 =
            deadline_estimated_work_seconds_map.get(date).unwrap_or(&0)
                - (free_time_hours * 3600.0).floor() as i64;
        let deadline_rest_hours = deadline_rest_duration_seconds.abs() / 3600;
        let deadline_rest_minutes =
            deadline_rest_duration_seconds.abs() / 60 - deadline_rest_hours * 60;
        let deadline_rest_sign: char = if deadline_rest_duration_seconds > 0 {
            ' '
        } else {
            '-'
        };

        let indicator_about_deadline = format!(
            "{}{:.0}時間{:02.0}分\t{:5.2}",
            deadline_rest_sign,
            deadline_rest_hours,
            deadline_rest_minutes,
            deadline_rest_duration_seconds as f64 / (free_time_hours * 60.0 * 60.0),
        );

        let non_repetitive_free_time_sign = if non_repetitive_free_time_hours >= 0.0 {
            ' '
        } else {
            '-'
        };
        let indicator_about_diff_to_limit = format!(
            "{}{:02}時間{:02}分\t{}{:02}時間{:02}分\t{:5.2}",
            diff_to_limit_sign,
            accumulate_duration_diff_to_limit.num_hours().abs(),
            accumulate_duration_diff_to_limit.num_minutes().abs() % 60,
            non_repetitive_free_time_sign,
            non_repetitive_free_time_hours.abs().floor(),
            (non_repetitive_free_time_hours.abs() * 60.0) as i64 % 60,
            accumulated_rho_diff,
        );

        // 順調フラグ確認
        if daily_summary_rows.is_empty() {
            has_today_deadline_leeway = deadline_rest_sign == '-';
            has_today_freetime_leeway = diff_to_limit_in_day_sign == '-';
            has_today_new_task_leeway = diff_to_goal_sign == '-';
        }

        if daily_summary_rows.len() == 1 {
            has_tomorrow_deadline_leeway = deadline_rest_sign == '-';
            has_tomorrow_freetime_leeway = diff_to_limit_in_day_sign == '-';
        }

        // 一度フラグが折れていたら復活させない
        // 今日と明日については個別にアラートを出すので、判定はそれ以降について行う。
        if 2 <= daily_summary_rows.len()
            && daily_summary_rows.len() < 7
            && has_weekly_deadline_leeway
        {
            has_weekly_deadline_leeway = deadline_rest_sign == '-';
        }

        if 2 <= daily_summary_rows.len()
            && daily_summary_rows.len() < 7
            && has_weekly_freetime_leeway
        {
            has_weekly_freetime_leeway = diff_to_limit_sign == '-';
        }

        // 今日より前には前倒せないため
        let adjustable_estimated_work_hours = if daily_summary_rows.is_empty() {
            0.0
        } else {
            *adjustable_estimated_work_seconds_map
                .get(date)
                .unwrap_or(&0) as f64
                / 3600.0
        };

        let adjustable_estimated_work_rate = adjustable_estimated_work_hours / free_time_hours;

        let adjustable_estimated_work_hours_str = if adjustable_estimated_work_hours == 0.0 {
            // "({:02.0}%)"と同じ幅になるようにする
            "     ".to_string()
        } else {
            format!("({:02.0}%)", adjustable_estimated_work_rate * 100.0)
        };

        let s = format!(
            "{}({})\t{:4.1}時間\t{}{:.0}時間{:02.0}分{}\t{:5.2}\t{}{:.0}時間{:02.0}分\t{}{:02}時間{:02}分\t{}\t{}\t{:02}[タスク]",
            date,
            weekday_jp,

            free_time_hours,

            diff_to_limit_in_day_sign,
            diff_to_limit_hours_in_day,
            diff_to_limit_minutes_in_day,
            adjustable_estimated_work_hours_str,

            rho_in_date - 1.0,

            diff_to_goal_sign,
            diff_to_goal_hour,
            diff_to_goal_minute,

            acc_diff_to_goal_sign,
            accumulate_duration_diff_to_goal_rho.num_hours().abs(),
            accumulate_duration_diff_to_goal_rho.num_minutes().abs() % 60,

            indicator_about_deadline,
            indicator_about_diff_to_limit,

            cnt_of_the_date,
        );

        let band_message =
            full_day_free_time_minutes_opt.map_or_else(String::new, |full_minutes| {
                format_daily_band(
                    **date,
                    weekday_jp,
                    accumulate_duration_diff_to_limit,
                    &calculate_daily_band_durations(
                        **date == naive_dt_today,
                        full_minutes,
                        free_time_minutes,
                        total_estimated_work_seconds_of_the_date,
                        total_repetitive_task_work_seconds_of_the_date,
                        diff_to_goal,
                    ),
                )
            });

        daily_summary_rows.push(DailySummaryRow {
            date: **date,
            calendar_message: s,
            band_message,
        });
    }

    if !is_daily_summary_func && !is_flatten_func {
        mark_give_up_candidate_rows_by_date(
            &mut task_list_display_rows,
            &shortage_duration_by_date,
        );
    }

    sort_task_list_display_rows(&mut task_list_display_rows, display_order);

    if !is_daily_summary_func && !is_flatten_func {
        for row in task_list_display_rows.iter() {
            *focused_task_id_opt = Some(row.id);
            writeln_newline(stdout, &row.render_message()).unwrap();
        }

        writeln_newline(stdout, "").unwrap();
        let project_category_summary =
            summarize_scheduled_work_seconds_by_project_category(&task_list_display_rows);
        let project_category_denominator_seconds = calculate_project_category_denominator_seconds(
            &task_list_display_rows,
            last_synced_time,
            eod,
            eod_duration,
            free_time_manager,
        );
        writeln_newline(
            stdout,
            &format_scheduled_work_seconds_by_project_category(
                &project_category_summary,
                project_category_denominator_seconds,
            ),
        )
        .unwrap();
        writeln_newline(stdout, "").unwrap();
    }

    // 逆順にして、下側に直近の日付があるようにする
    daily_summary_rows.reverse();

    if is_calendar_func && !is_flatten_func {
        for (cal_ind, row) in daily_summary_rows.iter().enumerate() {
            writeln_newline(stdout, &row.calendar_message).unwrap();

            if row.calendar_message.contains("(月)") && cal_ind != daily_summary_rows.len() - 1 {
                writeln_newline(stdout, "").unwrap();
            }
        }
        // フッター
        let footer: String = [
            "日          ",
            "空          ",
            "空差      ",
            "空差比",
            "余差    ",
            "余差累    ",
            "〆差      ",
            "〆差比",
            "空差累    ",
            "単発余暇",
            "空差累比",
            "タスク数",
        ]
        .join("\t");
        writeln_newline(stdout, &footer).unwrap();
        writeln_newline(stdout, "").unwrap();

        let clear_date_info = format!(
            "今のタスクが片付く日付: {}日後の{}",
            (first_caught_up_date - last_synced_time.date_naive()).num_days(),
            first_caught_up_date
        );

        let first_leeway_date_info = format!(
            "次にタスクを積める日付: {}日後の{} (-{}時間{:02}分)",
            (first_leeway_date - last_synced_time.date_naive()).num_days(),
            first_leeway_date,
            first_leeway_duration.num_hours().abs(),
            first_leeway_duration.num_minutes().abs() % 60,
        );

        let max_hours_sign = if max_accumulate_duration_diff_to_limit >= Duration::seconds(0) {
            ' '
        } else {
            '-'
        };
        let max_hours = max_accumulate_duration_diff_to_limit.num_hours().abs();
        let max_minutes = max_accumulate_duration_diff_to_limit.num_minutes().abs() % 60;
        let max_info = format!(
            "最大の累積時間: {}{:02}時間{:02}分 ({}), 最大のrhoの差: {:.2} ({}), {}",
            max_hours_sign,
            max_hours,
            max_minutes,
            max_accumulate_duration_diff_to_limit_date,
            max_accumulated_rho_diff,
            max_accumulated_rho_diff_date,
            first_leeway_date_info,
        );

        writeln_newline(stdout, &clear_date_info).unwrap();
        writeln_newline(stdout, &max_info).unwrap();
        writeln_newline(stdout, "").unwrap();

        let mut is_all_favorable = true;

        // 順調フラグが折れている時にアラート表示
        if !has_today_deadline_leeway {
            writeln_newline(stdout, "[Crit] 【今日の】〆切に間に合いません。【ただちに】〆切をリスケする調整をしてください。").unwrap();
            is_all_favorable = false;
        }

        if has_today_freetime_leeway {
            if !has_today_new_task_leeway {
                writeln_newline(stdout, "[Warn] 脇道に逸れずに予定の遂行をしてください。見積もりを間違えたり突発タスクが発生したりした場合に終了予定時刻に間に合わなくなる可能性があります。").unwrap();
                is_all_favorable = false;
            }
        } else {
            writeln_newline(stdout, "[Crit] 【今日の】終了予定時刻に間に合いません。【ただちに】どれかの予定を諦めて明日以降に延期してください。").unwrap();
            is_all_favorable = false;
        }

        if !has_tomorrow_deadline_leeway {
            writeln_newline(stdout, "[Warn] 【明日の】〆切に間に合いません。〆切をあさって以降にリスケする調整を【今日中に】してください。").unwrap();
            is_all_favorable = false;
        }

        if !has_tomorrow_freetime_leeway {
            writeln_newline(stdout, "[Warn] 【明日の】終了予定時刻に間に合いません。【今日中に】どれかの予定を諦めてあさって以降に延期してください。").unwrap();
            is_all_favorable = false;
        }

        if !has_weekly_deadline_leeway {
            writeln_newline(stdout, "[Warn] 【1週間以内の】〆切に間に合いません。【近々】どれかの予定を諦めて来週以降に延期してください。").unwrap();
            is_all_favorable = false;
        }

        if !has_weekly_freetime_leeway {
            writeln_newline(stdout, "[Warn] 【1週間以内の】終了予定時刻に間に合いません。【近々】どれかの予定を諦めて来週以降に延期してください。").unwrap();
            is_all_favorable = false;
        }

        if is_all_favorable {
            writeln_newline(
                stdout,
                "[Info] 順調です。突発タスクに対応したり1日の終わり際にタスクを新しく積んだりする余裕があります。ひとまずは脇道に逸れずに予定の遂行をしてください。",
            )
            .unwrap();
        }

        writeln_newline(stdout, "").unwrap();
    } else if is_band_func && !is_flatten_func {
        writeln_newline(
            stdout,
            "凡例: # 固定  x 経過済み  = 繰返  - 単発  : 余差  . 空き  > 超過  (1文字=15分)",
        )
        .unwrap();
        writeln_newline(stdout, "").unwrap();

        for (band_ind, row) in daily_summary_rows.iter().enumerate() {
            writeln_newline(stdout, &row.band_message).unwrap();

            if row.date.weekday() == Weekday::Mon && band_ind != daily_summary_rows.len() - 1 {
                writeln_newline(stdout, "").unwrap();
            }
        }
    }

    if !is_flatten_func && !is_band_func {
        writeln_newline(stdout, &busy_s).unwrap();
        writeln_newline(stdout, &s).unwrap();
        writeln_newline(stdout, &s_for_rho1).unwrap();
        writeln_newline(stdout, &s_for_non_repetitive_rho).unwrap();
    }

    writeln_newline(stdout, "").unwrap();

    // flatten
    if pattern_opt == &Some("平".to_string()) {
        writeln_newline(
            stdout,
            &format!(
                "flatten dst date : {:?} for {:?}",
                flattenable_date_opt, flattenable_duration
            ),
        )
        .unwrap();

        if let Some(flattenable_date) = flattenable_date_opt {
            let mut any_was_flattened = false;
            let mut src_date = flattenable_date - Duration::days(1);

            while !any_was_flattened && src_date >= naive_dt_today {
                writeln_newline(stdout, &format!("src_date: {:?}", src_date)).unwrap();

                // dt_dictを未来から見ていき、〆切に違反しない範囲で、翌日に飛ばしていく
                for (
                    _ind,
                    (_naive_date, _has_no_deadline, dt, _neg_priority, rank, deadline_time_opt, id),
                ) in dt_id_tpl_arr.iter().enumerate().rev()
                {
                    let days_until_deadline = match deadline_time_opt {
                        Some(deadline_time) => (*deadline_time - *dt).num_days(),
                        None => 100,
                    };

                    if dt.date_naive() == src_date && days_until_deadline > 0 {
                        if let Some(task) = task_repository.get_by_id(*id) {
                            if !task.get_is_on_other_side()
                                && task.get_estimated_work_seconds() > 0
                                && flattenable_duration.num_seconds()
                                    > task.get_estimated_work_seconds()
                            // && rank != &0
                            {
                                flattenable_duration -=
                                    Duration::seconds(task.get_estimated_work_seconds());
                                let dst_dt = get_next_morning_datetime(*dt);
                                task.set_pending_until(dst_dt);
                                task.set_orig_status(Status::Pending);

                                writeln_newline(
                                    stdout,
                                    &format!(
                                        "{}\t{}\t{}\t{}",
                                        // dt,
                                        // dst_dt,
                                        rank,
                                        task.get_id(),
                                        task.get_estimated_work_seconds(),
                                        task.get_name(),
                                    ),
                                )
                                .unwrap();

                                any_was_flattened = true;
                            }
                        }
                    }
                }

                src_date -= Duration::days(1);
            }
        }
    }
}

fn execute_focus(focused_task_id_opt: &mut Option<Uuid>, new_task_id_str: &str) {
    if let Ok(id) = Uuid::parse_str(new_task_id_str) {
        *focused_task_id_opt = Some(id)
    }
}

fn execute_pick(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    new_task_id_str: &str,
) {
    match Uuid::parse_str(new_task_id_str) {
        Ok(id) => {
            *focused_task_id_opt = Some(id);

            // Statusをtodoに戻す
            if let Some(task) = task_repository.get_by_id(id) {
                task.set_orig_status(Status::Todo);
            }
        }
        Err(_) => {
            // 今フォーカスが当たっているタスクをtodoに戻す
            match focused_task_id_opt {
                Some(focused_task_id) => {
                    if let Some(task) = task_repository.get_by_id(*focused_task_id) {
                        task.set_orig_status(Status::Todo);
                    }
                }
                None => {}
            }
        }
    }
}

fn execute_unfocus(focused_task_id_opt: &mut Option<Uuid>) {
    *focused_task_id_opt = None;
}

// 文字列の中からhttpから始まる部分文字列でURLとして解釈できる一番長い文字列を抽出する
fn extract_url(s: &str) -> Option<String> {
    // "http"が始まるインデックスを探す
    if let Some(start) = s.find("http") {
        // "http"から始まる部分文字列を取得する
        let (_, http_str) = s.split_at(start);

        // 末尾の文字を必ずNGにするために、番兵として日本語の文字を置く
        let chars: Vec<char> = (http_str.to_owned() + "あ").chars().collect();

        // その中で二分探索する
        let mut ok: usize = 0;
        let mut ng: usize = chars.len();

        let mut mid = (ok + ng) / 2;

        while ng - ok > 1 {
            let cand_str: String = chars[0..mid].iter().collect();
            let encoded_cand_str: String =
                percent_encode(cand_str.as_bytes(), MY_ASCII_SET).to_string();

            // Url::parse()は未パーセントエンコーディングの文字列(日本語)も受け付けてしまう。
            // もし cand_str == encoded_cand_str なら、日本語が混ざっていないということ
            if Url::parse(&cand_str).is_ok() && cand_str == encoded_cand_str {
                ok = mid;
            } else {
                ng = mid;
            }

            mid = (ok + ng) / 2;
        }

        let ans: String = chars[0..ok].iter().collect();
        Some(ans)
    } else {
        None
    }
}

#[test]
fn test_extract_url_正常系() {
    let input = "これはhttps://example.com?param1=hoge&param2=barというURLです。";
    let actual = extract_url(input);
    let expected = Some(String::from("https://example.com?param1=hoge&param2=bar"));

    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_extract_url_正常系_URLが2つ() {
    let input = "これはhttps://example.com?param1=hoge&param2=barとhttps://example.com";
    let actual = extract_url(input);
    let expected = Some(String::from("https://example.com?param1=hoge&param2=bar"));

    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_extract_url_正常系_2つのURLがスペース区切り() {
    let input = "これはhttps://example.com?param1=hoge&param2=bar https://example.com";
    let actual = extract_url(input);
    let expected = Some(String::from("https://example.com?param1=hoge&param2=bar"));

    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_extract_url_正常系_正しいURLのまま文字列が終わるケース() {
    let input = "正しいURLのまま文字列が終わるケースhttps://example.com/hoge";
    let actual = extract_url(input);
    let expected = Some(String::from("https://example.com/hoge"));

    assert_eq!(actual, expected);
}

//親に辿っていって見つかった最初のリンクを開く
fn execute_open_link(focused_task_opt: &Option<Task>) {
    let mut t_opt: Option<Task> = focused_task_opt.clone();

    while let Some(t) = &t_opt {
        if let Some(url) = extract_url(&t.get_name()) {
            // エラーは無視する
            let _ = webbrowser::open(&url);
            return;
        }

        t_opt = t.parent();
    }
}

fn make_obsidian_search_url(query: &str) -> String {
    format!(
        "obsidian://search?vault=Obsidian-Moica&query={}",
        percent_encode(query.as_bytes(), MY_ASCII_SET)
    )
}

fn make_obsidian_root_task_search_url(focused_task: &Task) -> String {
    let root_task_id = focused_task.root().get_id();
    make_obsidian_search_url(&root_task_id.hyphenated().to_string())
}

fn open_obsidian_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = process::Command::new("open")
            .arg(url)
            .status()
            .map_err(|err| err.to_string())?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("open exited with status {}", status))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        webbrowser::open(url).map_err(|err| err.to_string())
    }
}

fn execute_open_obsidian_root_task_search(focused_task_opt: &Option<Task>) {
    if let Some(focused_task) = focused_task_opt {
        let url = make_obsidian_root_task_search_url(focused_task);
        // エラーは無視する
        let _ = open_obsidian_url(&url);
    }
}

#[test]
fn test_make_obsidian_search_url_task_idをqueryにする() {
    let query = "11111111-1111-1111-1111-111111111111";
    let actual = make_obsidian_search_url(query);
    let expected =
        "obsidian://search?vault=Obsidian-Moica&query=11111111-1111-1111-1111-111111111111";

    assert_eq!(actual, expected);
}

#[test]
fn test_make_obsidian_root_task_search_url_子タスクからrootのtask_idをqueryにする() {
    let mut root_task = Task::new("root");
    let root_task_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    root_task.set_id(root_task_id);
    let child_task = root_task.create_as_last_child(TaskAttr::new("child"));

    let actual = make_obsidian_root_task_search_url(&child_task);
    let expected =
        "obsidian://search?vault=Obsidian-Moica&query=11111111-1111-1111-1111-111111111111";

    assert_eq!(actual, expected);
}

#[allow(unused_must_use)]
fn execute_next_up(
    _stdout: &mut dyn SchronuWriter,
    focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    new_task_name_str: &str,
    estimated_work_minutes_opt: &Option<i64>,
) -> Result<Option<Uuid>, ApplicationError> {
    validate_task_name(new_task_name_str, "name")?;
    let estimated_work_seconds_opt = estimated_work_minutes_opt
        .map(estimated_work_seconds_from_minutes)
        .transpose()?;

    let Some(mut focused_task) = focused_task_opt.clone() else {
        return Ok(None);
    };
    let mut new_task_attr = TaskAttr::new(new_task_name_str);

    // 親タスクの〆切を引き継ぐ
    if let Some(parent_task) = focused_task.parent() {
        new_task_attr.set_deadline_time_opt(parent_task.get_deadline_time_opt());
    }

    if let Some(new_task_estimated_work_seconds) = estimated_work_seconds_opt {
        new_task_attr.set_estimated_work_seconds(new_task_estimated_work_seconds);

        // 親タスクの見積もりをそのぶん減らす
        if let Some(parent_task) = focused_task.parent() {
            let parent_task_estimated_work_seconds = parent_task.get_estimated_work_seconds();
            parent_task.set_estimated_work_seconds(
                if parent_task_estimated_work_seconds > new_task_estimated_work_seconds {
                    parent_task_estimated_work_seconds - new_task_estimated_work_seconds
                } else {
                    0
                },
            );
        }
    }

    let new_task_id = *new_task_attr.get_id();

    if focused_task.create_as_parent(new_task_attr).is_ok() {
        *focused_task_id_opt = Some(new_task_id);
        Ok(Some(new_task_id))
    } else {
        Ok(None)
    }
}

fn execute_breakdown(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    new_task_names: &[&str],
    pending_until_opt: &Option<DateTime<Local>>,
) -> Result<Option<Vec<Uuid>>, ApplicationError> {
    let Some(parent_id) = *focused_task_id_opt else {
        return Ok(None);
    };
    let names = new_task_names
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();

    let input = BreakdownTaskInput {
        parent_id,
        names,
        pending_until: *pending_until_opt,
    };
    let child_ids = breakdown_task(task_repository, input)?;
    for (child_id, child_name) in child_ids.iter().zip(new_task_names.iter()) {
        writeln_newline(stdout, &format!("{child_id} {child_name}")).unwrap();
    }
    *focused_task_id_opt = child_ids.first().copied();
    Ok(Some(child_ids))
}

// コマンド引数を変換せず、そのままドメイン操作へ渡す境界関数である。
#[allow(clippy::too_many_arguments)]
fn execute_breakdown_sequentially(
    _stdout: &mut dyn SchronuWriter,
    focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    new_task_name_str: &str,
    estimated_work_minutes: i64,
    begin_index: u64,
    end_index: u64,
    new_task_name_suffix_str: &str,
) -> Result<Option<Uuid>, ApplicationError> {
    validate_task_name(new_task_name_str, "name")?;
    let estimated_work_seconds = estimated_work_seconds_from_minutes(estimated_work_minutes)?;

    if let Some(focused_task) = focused_task_opt {
        let grand_child_task_result = focused_task.create_sequential_children(
            new_task_name_str,
            estimated_work_seconds,
            begin_index,
            end_index,
            new_task_name_suffix_str,
        );

        if let Ok(grand_child_task) = grand_child_task_result {
            // フォーカスを移す
            *focused_task_id_opt = Some(grand_child_task.get_id());
            return Ok(Some(grand_child_task.get_id()));
        }
    }
    Ok(None)
}

// 繰り返しタスク作成コマンドの全入力を明示的に受け取る境界関数である。
#[allow(clippy::too_many_arguments)]
fn execute_create_repetition_task(
    _stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    new_task_name_str: &str,
    exec_day_str: &str,
    estimated_work_minutes: i64,
    _start_time_str: &str,
    _deadline_time_str: &str,
) -> Result<Option<Uuid>, ApplicationError> {
    estimated_work_seconds_from_minutes(estimated_work_minutes)?;

    // まず繰り返しタスクの親タスクを作る。
    let Some(_) = execute_breakdown(
        _stdout,
        task_repository,
        focused_task_id_opt,
        &[new_task_name_str],
        &None,
    )?
    else {
        return Ok(None);
    };
    let repetition_parent_task_opt =
        focused_task_id_opt.and_then(|id| task_repository.get_by_id(id));
    execute_set_estimated_work_minutes(
        task_repository,
        repetition_parent_task_opt.map(|task| task.get_id()),
        &format!("{}", estimated_work_minutes),
    );

    let task_num = if exec_day_str == "毎" { 7 } else { 4 };

    if let Some(focused_task_id) = focused_task_id_opt {
        let repetition_parent_task_id = *focused_task_id;

        // ループを回して子タスクを作る
        for _ in 0..task_num {
            let Some(_) = execute_breakdown(
                _stdout,
                task_repository,
                focused_task_id_opt,
                &[new_task_name_str],
                &None,
            )?
            else {
                return Ok(None);
            };
            let child_task_opt = focused_task_id_opt.and_then(|id| task_repository.get_by_id(id));
            execute_set_estimated_work_minutes(
                task_repository,
                child_task_opt.map(|task| task.get_id()),
                &format!("{}", estimated_work_minutes),
            );

            // 次ここから作業再開する。start_timeを作るために、「毎」か「月~日」でそれぞれ日付をループさせたい
            // focused_task.set_start_time(start_dst_time);

            execute_focus(
                focused_task_id_opt,
                &repetition_parent_task_id.hyphenated().to_string(),
            );
        }
        Ok(Some(repetition_parent_task_id))
    } else {
        Ok(None)
    }
}

fn execute_split(
    stdout: &mut dyn SchronuWriter,
    focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    new_task_name: &str,
    splitted_work_minutes_str: &str,
) -> Result<Option<Uuid>, ApplicationError> {
    validate_task_name(new_task_name, "name")?;

    match focused_task_opt {
        None => Ok(None),
        Some(focused_task) => {
            // 今のタスクの予時間をn減らす
            // 下 <new_task_name>
            // 予 n

            let focused_estimated_work_seconds = focused_task.get_estimated_work_seconds();

            // もしsplitted_work_minutes_strがマイナスの場合は、親タスクにその値だけ残すようにする
            // 割 -30 <新タスク> なら、(親タスク-30)を見積もりとして<新タスク>を作るよ、という意味合い
            let splitted_work_minutes = splitted_work_minutes_str.parse::<i64>().map_err(|_| {
                ApplicationError::InvalidInput {
                    field: "splitted_work_minutes",
                    reason: "must be an integer",
                }
            })?;

            let splitted_work_seconds: i64 = if splitted_work_minutes > 0 {
                min(
                    estimated_work_seconds_from_minutes(splitted_work_minutes)?,
                    focused_estimated_work_seconds,
                )
            } else {
                // このif分岐では負の場合splitted_work_minutesは負だが、
                // 分かりやすいようにabs()して引き算している
                let retained_work_minutes =
                    splitted_work_minutes
                        .checked_abs()
                        .ok_or(ApplicationError::InvalidInput {
                            field: "splitted_work_minutes",
                            reason: "absolute value is too large",
                        })?;
                let retained_work_seconds =
                    estimated_work_seconds_from_minutes(retained_work_minutes)?;
                if focused_estimated_work_seconds > retained_work_seconds {
                    focused_estimated_work_seconds - retained_work_seconds
                } else {
                    0
                }
            };

            focused_task
                .set_estimated_work_seconds(focused_estimated_work_seconds - splitted_work_seconds);

            let mut new_task_attr = TaskAttr::new(new_task_name);
            new_task_attr.set_estimated_work_seconds(splitted_work_seconds);

            // 親タスクに〆切がある場合には、それを引き継ぐ
            match focused_task.get_deadline_time_opt() {
                Some(deadline_time) => new_task_attr.set_deadline_time_opt(Some(deadline_time)),
                None => {
                    // pass
                }
            }

            let new_task = focused_task.create_as_last_child(new_task_attr);

            let msg: String = format!("{} {}", new_task.get_id(), &new_task_name);
            writeln_newline(stdout, msg.as_str()).unwrap();

            // 新しい子タスクにフォーカス(id)を移す
            *focused_task_id_opt = Some(new_task.get_id());
            Ok(Some(new_task.get_id()))
        }
    }
}

fn split_amount_and_unit(input: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut buffer = String::new();

    for c in input.chars() {
        if c.is_numeric() {
            buffer.push(c);
        } else {
            break;
        }
    }

    result.push(buffer);
    result.push(input[result[0].len()..].to_string());

    result
}

#[test]
fn test_split_amount_and_unit() {
    let input = "暦";
    let actual = split_amount_and_unit(input);

    assert_eq!(actual, vec!["".to_string(), "暦".to_string()]);
}

#[test]
fn test_split_amount_and_unit_err() {
    let input = "6543abc123def456gh789";
    let actual = split_amount_and_unit(input);

    assert_eq!(
        actual,
        vec!["6543".to_string(), "abc123def456gh789".to_string()]
    );
}

fn execute_wait_for_others(focused_task_opt: &Option<Task>) {
    if let Some(focused_task) = focused_task_opt.as_ref() {
        focused_task.set_is_on_other_side(true)
    }
}

fn execute_defer(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    amount_str: &str,
    unit_str: &str,
) {
    let amount: i64 = amount_str.parse().unwrap();
    let duration = match unit_str.chars().next() {
        // 24時間単位ではなく、next_monring単位とする
        Some('日') | Some('d') => {
            let mut dt = task_repository.get_last_synced_time();

            for _ in 0..amount {
                dt = get_next_morning_datetime(dt);
            }

            dt - task_repository.get_last_synced_time()
        }
        Some('時') | Some('h') => Duration::hours(amount),
        Some('分') | Some('m') => Duration::minutes(amount),
        // 誤入力した時に傷が浅いように、デフォルトは秒としておく
        _ => Duration::seconds(amount),
    };

    if let Some(task_id) = *focused_task_id_opt {
        let pending_until = task_repository.get_last_synced_time() + duration;
        let _ = defer_task(task_repository, task_id, pending_until);
    }

    *focused_task_id_opt = None;
}

// 指定の日付から、step_days間隔でdeferしていく
fn execute_extrude(
    _focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    first_datetime: &DateTime<Local>,
    step_days: u16,
) {
    if let Some(focused_task) = focused_task_opt {
        let mut pending_until_datetime = *first_datetime;

        for (_, task) in focused_task
            .list_all_parent_tasks_with_first_available_time()
            .iter()
        {
            if focused_task.get_status() != Status::Done {
                task.set_orig_status(Status::Pending);
                task.set_pending_until(pending_until_datetime);

                pending_until_datetime += Duration::days(step_days as i64);

                // 平日の仕事用: 土日にはextrudeせずにスキップする
                // match pending_until_datetime.weekday() {
                //     Weekday::Sat => {
                //         pending_until_datetime = pending_until_datetime + Duration::days(2);
                //     }
                //     Weekday::Sun => {
                //         pending_until_datetime = pending_until_datetime + Duration::days(1);
                //     }
                //     _ => {}
                // }
            }
        }
    }
}

// 〆切をrepetition_interval_daysのぶん伸ばし、pendingにする
// start_timeも伸ばすが、時刻は元のstart_timeを維持する
fn execute_defer_routine(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
) {
    if let Some(focused_task_id) = focused_task_id_opt {
        if let Some(ref focused_task) = task_repository.get_by_id(*focused_task_id) {
            if let Some(orig_deadline_time) = focused_task.get_deadline_time_opt() {
                if let Some(parent_task) = focused_task.parent() {
                    if let Some(repetition_interval_days) =
                        parent_task.get_repetition_interval_days_opt()
                    {
                        let new_deadline_time = if let Some(parent_deadline_time) =
                            parent_task.get_deadline_time_opt()
                        {
                            (get_next_morning_datetime(orig_deadline_time)
                                + Duration::days(repetition_interval_days - 1))
                            .with_hour(parent_deadline_time.hour())
                            .expect("invalid hour")
                            .with_minute(parent_deadline_time.minute())
                            .expect("invalid minute")
                            .with_second(parent_deadline_time.second())
                            .expect("invalid second")
                        } else {
                            orig_deadline_time + Duration::days(repetition_interval_days)
                        };

                        focused_task.unset_deadline_time_opt();
                        focused_task.set_deadline_time_opt(Some(new_deadline_time));

                        focused_task.set_orig_status(Status::Todo);

                        // 〆切の日に合わせる
                        let new_start_time = focused_task.get_start_time()
                            + Duration::days((new_deadline_time - orig_deadline_time).num_days());

                        focused_task.set_start_time(new_start_time);

                        *focused_task_id_opt = None;
                    }
                }
            }
        }
    }
}

// 何日もSchronuを開いていなくてあまりにもTODOがたまってしまった場合に、repetition_intervalが7日以内のルーチンタスクを自動的に先送りする
// 7日よりも大きい場合は、1年に1回のような重要なタスクである可能性があるため、何もしない
fn execute_defer_all_frequent_routines(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    _focused_task_opt: &Option<Task>,
) {
    const MAX_REPETITION_INTERVAL_DAYS: i64 = 7;
    const MIN_OVERDUE_HOURS: i64 = 24;
    let now = task_repository.get_last_synced_time();
    // let mut cnt = 0;

    loop {
        let mut any_is_changed = false;

        // まず対象のタスクIDを収集して所有権のあるベクタに保持し、
        // その後でmut借用が必要な処理を行う (借用の競合を避ける)
        let candidate_task_ids: Vec<Uuid> = {
            let mut ids = Vec::new();
            for project_root_task in task_repository.get_all_projects().iter() {
                let leaf_tasks = extract_leaf_tasks_from_project(project_root_task);
                for leaf_task in leaf_tasks.iter() {
                    if let Some(parent_task) = leaf_task.parent() {
                        if let Some(repetition_interval_days) =
                            parent_task.get_repetition_interval_days_opt()
                        {
                            if let Some(deadline_time) = leaf_task.get_deadline_time_opt() {
                                if repetition_interval_days <= MAX_REPETITION_INTERVAL_DAYS
                                    && now - deadline_time >= Duration::hours(MIN_OVERDUE_HOURS)
                                {
                                    ids.push(leaf_task.get_id());
                                }
                            }
                        }
                    }
                }
            }
            ids
        };

        // TODOの葉タスクについて、条件を満たす限りexecute_defer_routine()を適用し続ける
        for task_id in candidate_task_ids.into_iter() {
            *focused_task_id_opt = Some(task_id);
            let orig_focused_task_id_opt = *focused_task_id_opt;
            execute_defer_routine(task_repository, focused_task_id_opt);

            // deferが成功してフォーカスが移ったら記録しておく
            if orig_focused_task_id_opt != *focused_task_id_opt {
                any_is_changed = true;
                // cnt +=  1;
            }
        }

        if !any_is_changed {
            break;
        }
    }

    // println!("{:?}", cnt );
}

fn execute_set_deadline(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: Option<Uuid>,
    deadline_date_str: &str,
) {
    if deadline_date_str == "消" {
        if let Some(task_id) = focused_task_id_opt {
            let _ = set_deadline(task_repository, task_id, None);
        }
        return;
    }

    let mut deadline_time_str = format!("{} 23:59:59", deadline_date_str);
    let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();

    // 時刻のみを指定した場合は、日付は今日にする
    if hhmm_reg.is_match(deadline_date_str) {
        let caps = hhmm_reg.captures(deadline_date_str).unwrap();
        let hh: u32 = caps[1].parse().unwrap();
        let mm: u32 = caps[2].parse().unwrap();

        let now = task_repository.get_last_synced_time();
        deadline_time_str = format!("{} {:02}:{:02}:00", now.format("%Y/%m/%d"), hh, mm);
    }

    let deadline_time_opt_result = parse_local_datetime(&deadline_time_str, "%Y/%m/%d %H:%M:%S");

    if let Ok(LocalResult::Single(deadline_time)) = deadline_time_opt_result {
        if let Some(task_id) = focused_task_id_opt {
            let _ = set_deadline(task_repository, task_id, Some(deadline_time));
        }
    }
}

fn execute_set_estimated_work_minutes(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: Option<Uuid>,
    estimated_work_minutes_str: &str,
) {
    if let (Some(task_id), Ok(estimated_work_minutes)) = (
        focused_task_id_opt,
        estimated_work_minutes_str.parse::<i64>(),
    ) {
        let _ = set_estimate(task_repository, task_id, estimated_work_minutes);
    }
}

fn execute_set_arrange_children_work_minutes(
    focused_task_opt: &Option<Task>,
    estimated_work_minutes_str: &str,
    includes_zero_estimate: bool,
) {
    let estimated_minutes_result = estimated_work_minutes_str.parse::<i64>();

    // 繰り返しタスクについて、その子タスクでDoneでないものの時間を一律変更する。
    if let Ok(estimated_minutes) = estimated_minutes_result {
        if !(0..=MAX_ARRANGE_ESTIMATED_WORK_MINUTES).contains(&estimated_minutes) {
            return;
        }

        if let Some(focused_task) = focused_task_opt {
            if focused_task.get_repetition_interval_days_opt().is_some() {
                let children = focused_task.get_children();
                for child_task in children.iter() {
                    if child_task.get_status() != Status::Done
                        && (includes_zero_estimate || child_task.get_estimated_work_seconds() != 0)
                    {
                        child_task.set_estimated_work_seconds(estimated_minutes * 60);
                    }
                }
            }
        }
    }
}

#[allow(unused_must_use)]
fn execute_set_actual_work_minutes(focused_task_opt: &Option<Task>, actual_work_minutes_str: &str) {
    let actual_minutes_result = actual_work_minutes_str.parse::<i64>();

    if let Ok(actual_work_minutes) = actual_minutes_result {
        let actual_work_seconds = actual_work_minutes * 60;
        if let Some(focused_task) = focused_task_opt.as_ref() {
            focused_task.set_actual_work_seconds(actual_work_seconds)
        }
    }
}

#[allow(unused_must_use)]
fn execute_set_priority(focused_task_opt: &Option<Task>, priority_str: &str) {
    let priority_result = priority_str.parse::<i64>();

    if let Ok(priority) = priority_result {
        if let Some(focused_task) = focused_task_opt.as_ref() {
            focused_task.set_priority(priority)
        }
    }
}

fn read_project_category_command_arg(s: &str) -> Option<Option<ProjectCategory>> {
    match s.to_lowercase().as_str() {
        "_" | "none" | "clear" => Some(None),
        _ => read_project_category(s).map(Some),
    }
}

fn execute_set_project_category(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: Option<Uuid>,
    project_category_str: &str,
) {
    if let (Some(task_id), Some(project_category_opt)) = (
        focused_task_id_opt,
        read_project_category_command_arg(project_category_str),
    ) {
        let _ = set_category(task_repository, task_id, project_category_opt);
    }
}

fn decide_time(tokens: &[&str], now: &DateTime<Local>) -> Option<DateTime<Local>> {
    let mut start_time = None;

    if tokens.len() >= 2 {
        let start_hhmm_str = &tokens[1];

        // 日付はオプショナル引数。入力されなかった場合は今日の日付とする。
        let start_date_str = if tokens.len() >= 3 {
            tokens[2]
        } else {
            "dummy"
        };

        let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();
        let (hh, mm) = if hhmm_reg.is_match(start_hhmm_str) {
            let caps = hhmm_reg.captures(start_hhmm_str).unwrap();
            let hh: u32 = caps[1].parse().unwrap();
            let mm: u32 = caps[2].parse().unwrap();

            (hh, mm)
        } else {
            (12, 00)
        };

        let yyyymmdd_reg = Regex::new(r"^(\d{2,4})/(\d{1,2})/(\d{1,2})$").unwrap();
        let mmdd_reg = Regex::new(r"^(\d{1,2})/(\d{1,2})$").unwrap();

        let start_time_tmp = if yyyymmdd_reg.is_match(start_date_str) {
            let caps = yyyymmdd_reg.captures(start_date_str).unwrap();
            let tmp_yyyy: i32 = caps[1].parse().unwrap();
            let yyyy = if tmp_yyyy < 100 {
                tmp_yyyy + 2000
            } else {
                tmp_yyyy
            };
            let mm_month: u32 = caps[2].parse().unwrap();
            let dd: u32 = caps[3].parse().unwrap();

            Local
                .with_ymd_and_hms(yyyy, mm_month, dd, hh, mm, 0)
                .unwrap()
        } else if mmdd_reg.is_match(start_date_str) {
            // 年なしの日付が指定された場合は未来方向でその日付に合致する日付に送る
            let caps = mmdd_reg.captures(start_date_str).unwrap();
            let mm_month: u32 = caps[1].parse().unwrap();
            let dd: u32 = caps[2].parse().unwrap();

            let mut ans_datetime = Local
                .with_ymd_and_hms(now.year(), mm_month, dd, hh, mm, 0)
                .unwrap();

            if ans_datetime < *now {
                ans_datetime = Local
                    .with_ymd_and_hms(now.year() + 1, mm_month, dd, hh, mm, 0)
                    .unwrap()
            }

            ans_datetime
        } else if start_date_str.starts_with('明') {
            let next_schronu_day = get_next_morning_datetime(*now);
            Local
                .with_ymd_and_hms(
                    next_schronu_day.year(),
                    next_schronu_day.month(),
                    next_schronu_day.day(),
                    hh,
                    mm,
                    0,
                )
                .unwrap()
        } else if tokens.len() >= 3
            && ["月", "火", "水", "木", "金", "土", "日"].contains(&tokens[2])
        {
            // 月 火 水 木 金 土 日 が指定された時は、明日以降で、直近のその曜日とする。
            // (show_all_tasksとロジック重複...)
            let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];

            let todays_morning_datetime = get_next_morning_datetime(*now) - Duration::days(1);

            let dn = todays_morning_datetime.date_naive();
            let now_weekday_jp = get_weekday_jp(&dn);

            let now_days_of_week_ind = days_of_week
                .iter()
                .position(|&x| x == now_weekday_jp)
                .unwrap();
            let target_days_of_week_ind =
                days_of_week.iter().position(|&x| x == tokens[2]).unwrap();

            let ind_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

            // 今日の6:00にdeferする味意はないので、その代わりに、1週間後の同じ曜日にdeferできるようにする
            let days: i64 = if ind_diff == 0 { 7 } else { ind_diff as i64 };
            let n_days_after_datetime = get_next_morning_datetime(*now) + Duration::days(days - 1);

            Local
                .with_ymd_and_hms(
                    n_days_after_datetime.year(),
                    n_days_after_datetime.month(),
                    n_days_after_datetime.day(),
                    hh,
                    mm,
                    0,
                )
                .unwrap()
        } else {
            Local
                .with_ymd_and_hms(now.year(), now.month(), now.day(), hh, mm, 0)
                .unwrap()
        };

        start_time = Some(start_time_tmp);
    }

    start_time
}

fn decide_finish_time(tokens: &Vec<&str>, now: &DateTime<Local>) -> Option<DateTime<Local>> {
    let hhmmss_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})(?::(\d{1,2}))?$").unwrap();
    let yyyymmdd_reg = Regex::new(r"^\d{2,4}/\d{1,2}/\d{1,2}$").unwrap();
    let mmdd_reg = Regex::new(r"^\d{1,2}/\d{1,2}$").unwrap();
    let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];

    let build_finish_time = |hhmmss: &str| -> Option<DateTime<Local>> {
        let caps = hhmmss_reg.captures(hhmmss)?;
        let hh: u32 = caps[1].parse().ok()?;
        let mm: u32 = caps[2].parse().ok()?;
        let ss: u32 = caps
            .get(3)
            .map(|sec| sec.as_str().parse().ok())
            .unwrap_or(Some(0))?;

        if hh > 23 || mm > 59 || ss > 59 {
            return None;
        }

        let hhmm = format!("{}:{}", hh, mm);
        let finish_time = match tokens.as_slice() {
            [cmd, _] => decide_time(&[*cmd, hhmm.as_str()], now),
            [cmd, _, date] => decide_time(&[*cmd, hhmm.as_str(), *date], now),
            _ => None,
        }?;

        finish_time.with_second(ss)
    };

    match tokens.as_slice() {
        [_] => Some(*now),
        [_, "今"] | [_, "now"] => Some(*now),
        [_, hhmmss] if hhmmss_reg.is_match(hhmmss) => build_finish_time(hhmmss),
        [_, hhmmss, date]
            if hhmmss_reg.is_match(hhmmss)
                && (yyyymmdd_reg.is_match(date)
                    || mmdd_reg.is_match(date)
                    || date.starts_with('明')
                    || days_of_week.contains(date)) =>
        {
            build_finish_time(hhmmss)
        }
        _ => None,
    }
}

#[test]
fn test_decide_time_明_6時以降は次のschronu日付にする() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["始", "7:00", "明"];

    let actual = decide_time(&tokens, &now);
    let expected = Some(Local.with_ymd_and_hms(2026, 5, 18, 7, 0, 0).unwrap());

    assert_eq!(actual, expected);
}

#[test]
fn test_decide_time_明_24時過ぎは直近6時を使う() {
    let now = Local.with_ymd_and_hms(2026, 5, 18, 0, 15, 0).unwrap();
    let tokens = vec!["始", "7:00", "明"];

    let actual = decide_time(&tokens, &now);
    let expected = Some(Local.with_ymd_and_hms(2026, 5, 18, 7, 0, 0).unwrap());

    assert_eq!(actual, expected);
}

#[test]
fn test_decide_finish_time_今は現在時刻を返す() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["終", "今"];

    let actual = decide_finish_time(&tokens, &now);

    assert_eq!(actual, Some(now));
}

#[test]
fn test_decide_finish_time_時刻指定はdecide_timeと同じ形式で解釈する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["終", "7:00", "明"];

    let actual = decide_finish_time(&tokens, &now);
    let expected = Some(Local.with_ymd_and_hms(2026, 5, 18, 7, 0, 0).unwrap());

    assert_eq!(actual, expected);
}

#[test]
fn test_decide_finish_time_秒つき時刻を解釈する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["終", "9:23:45", "2026/7/4"];

    let actual = decide_finish_time(&tokens, &now);
    let expected = Some(Local.with_ymd_and_hms(2026, 7, 4, 9, 23, 45).unwrap());

    assert_eq!(actual, expected);
}

#[test]
fn test_decide_finish_time_不正な時刻は完了時刻にしない() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["終", "xxx"];

    let actual = decide_finish_time(&tokens, &now);

    assert_eq!(actual, None);
}

#[test]
fn test_decide_finish_time_不正な秒は完了時刻にしない() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["終", "9:23:60", "2026/7/4"];

    let actual = decide_finish_time(&tokens, &now);

    assert_eq!(actual, None);
}

#[test]
fn test_decide_finish_time_不正な日付は完了時刻にしない() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["終", "14:30", "xxx"];

    let actual = decide_finish_time(&tokens, &now);

    assert_eq!(actual, None);
}

#[cfg(test)]
struct TestWriter {
    buffer: Vec<u8>,
}

#[cfg(test)]
struct TestStorageDir {
    path: PathBuf,
}

#[cfg(test)]
impl TestStorageDir {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!("schronu-controller-{}", Uuid::new_v4())),
        }
    }
}

#[cfg(test)]
impl Drop for TestStorageDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
impl TestWriter {
    fn new() -> Self {
        Self { buffer: vec![] }
    }

    fn into_string(self) -> String {
        String::from_utf8(self.buffer).expect("test output must be UTF-8")
    }
}

#[cfg(test)]
impl Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
impl SchronuWriter for TestWriter {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        writeln!(self, "{}", message)
    }
}

#[cfg(test)]
struct TestTaskRepository {
    task: Task,
    last_synced_time: DateTime<Local>,
    highest_priority_leaf_task_id_opt: Option<Uuid>,
    defer_candidate_leaf_task_id_opt: Option<Uuid>,
    last_defer_candidate_recent_days_opt: Option<i64>,
    load_should_fail: bool,
    save_failures_remaining: Cell<usize>,
    save_attempt_count: Cell<usize>,
}

#[cfg(test)]
struct CommandTestResult {
    task: Task,
    focused_task_id_opt: Option<Uuid>,
    output: String,
}

#[cfg(test)]
fn execute_command_for_test(
    task: Task,
    now: DateTime<Local>,
    focused_task_id_opt: Option<Uuid>,
    command: &str,
) -> CommandTestResult {
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = focused_task_id_opt;
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    );

    CommandTestResult {
        task: task_repository.task,
        focused_task_id_opt,
        output: stdout.into_string(),
    }
}

#[cfg(test)]
impl TestTaskRepository {
    fn new(task: Task, last_synced_time: DateTime<Local>) -> Self {
        let task_id = task.get_id();
        Self {
            task,
            last_synced_time,
            highest_priority_leaf_task_id_opt: Some(task_id),
            defer_candidate_leaf_task_id_opt: Some(task_id),
            last_defer_candidate_recent_days_opt: None,
            load_should_fail: false,
            save_failures_remaining: Cell::new(0),
            save_attempt_count: Cell::new(0),
        }
    }
}

#[cfg(test)]
impl TaskRepositoryTrait for TestTaskRepository {
    fn get_project_storage_dir_name(&self) -> &str {
        ""
    }

    fn get_all_projects(&self) -> Vec<&Task> {
        vec![&self.task]
    }

    fn load(&mut self) -> Result<(), schronu::application::interface::TaskRepositoryError> {
        if self.load_should_fail {
            Err(TaskRepositoryError::new(
                TaskRepositoryOperation::Load,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "ParseProject failed for /test/project.yaml: test load failure",
                ),
            ))
        } else {
            Ok(())
        }
    }

    fn save(&self) -> Result<(), schronu::application::interface::TaskRepositoryError> {
        self.save_attempt_count
            .set(self.save_attempt_count.get() + 1);
        let failures_remaining = self.save_failures_remaining.get();
        if failures_remaining > 0 {
            self.save_failures_remaining.set(failures_remaining - 1);
            Err(TaskRepositoryError::new(
                TaskRepositoryOperation::Save,
                std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "WriteFile failed for /test/project.yaml: test save failure",
                ),
            ))
        } else {
            Ok(())
        }
    }

    fn sync_clock(&mut self, now: DateTime<Local>) {
        self.last_synced_time = now;
    }

    fn get_last_synced_time(&self) -> DateTime<Local> {
        self.last_synced_time
    }

    fn get_highest_priority_project(&mut self) -> Option<&Task> {
        Some(&self.task)
    }

    fn get_highest_priority_leaf_task_id(&mut self) -> Option<Uuid> {
        self.highest_priority_leaf_task_id_opt
    }

    fn get_defer_candidate_leaf_task_id(&mut self, recent_days: i64) -> Option<Uuid> {
        self.last_defer_candidate_recent_days_opt = Some(recent_days);
        self.defer_candidate_leaf_task_id_opt
    }

    fn get_by_id(&self, id: Uuid) -> Option<Task> {
        self.task.get_by_id(id)
    }

    fn start_new_project(&mut self, root_task: Task) {
        self.task = root_task;
    }
}

#[cfg(test)]
struct TestFreeTimeManager;

#[cfg(test)]
impl FreeTimeManagerTrait for TestFreeTimeManager {
    fn get_free_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        0
    }

    fn get_busy_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        0
    }

    fn register_busy_time_slot(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) {}

    fn load_busy_time_slots_from_file(
        &mut self,
        _busy_time_slots_file_path: &str,
        _now: &DateTime<Local>,
    ) {
    }
}

#[cfg(test)]
struct TestFreeTimeManagerWithFreeMinutes {
    free_minutes: i64,
}

#[cfg(test)]
impl FreeTimeManagerTrait for TestFreeTimeManagerWithFreeMinutes {
    fn get_free_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        self.free_minutes
    }

    fn get_busy_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        0
    }

    fn register_busy_time_slot(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) {}

    fn load_busy_time_slots_from_file(
        &mut self,
        _busy_time_slots_file_path: &str,
        _now: &DateTime<Local>,
    ) {
    }
}

#[cfg(test)]
struct TestFreeTimeManagerForBand;

#[cfg(test)]
impl FreeTimeManagerTrait for TestFreeTimeManagerForBand {
    fn get_free_minutes(&mut self, start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        if start.hour() == 6 {
            990
        } else {
            190
        }
    }

    fn get_busy_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        0
    }

    fn register_busy_time_slot(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) {}

    fn load_busy_time_slots_from_file(
        &mut self,
        _busy_time_slots_file_path: &str,
        _now: &DateTime<Local>,
    ) {
    }
}

#[cfg(test)]
fn execute_sequential_command(command: &str) -> (Task, Option<Uuid>) {
    let now = Local.with_ymd_and_hms(2026, 7, 26, 12, 0, 0).unwrap();
    let task = Task::new("親タスク");
    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    );

    (task, focused_task_id_opt)
}

#[cfg(test)]
fn execute_arrange_command(command: &str) -> Task {
    let now = Local.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
    let task = Task::new("ルーチン");
    task.set_repetition_interval_days_opt(Some(7));

    let mut estimated_child_attr = TaskAttr::new("見積もりあり");
    estimated_child_attr.set_estimated_work_seconds(5 * 60);
    task.create_as_last_child(estimated_child_attr);

    let mut zero_estimate_child_attr = TaskAttr::new("見積もり0");
    zero_estimate_child_attr.set_estimated_work_seconds(0);
    task.create_as_last_child(zero_estimate_child_attr);

    let mut done_child_attr = TaskAttr::new("完了済み");
    done_child_attr.set_estimated_work_seconds(10 * 60);
    done_child_attr.set_orig_status(Status::Done);
    task.create_as_last_child(done_child_attr);

    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    );

    task
}

#[test]
fn test_select_focus_task_id_高優先度modeでは最優先leafを返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("タスク");
    let expected_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task, now);
    task_repository.highest_priority_leaf_task_id_opt = Some(expected_id);

    let actual = select_focus_task_id(&mut task_repository, FocusSelectionMode::HighestPriority);

    assert_eq!(actual, Some(expected_id));
}

#[test]
fn test_select_focus_task_id_低優先度modeでは指定日数の延期候補を返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("タスク");
    let expected_id = Uuid::new_v4();
    let mut task_repository = TestTaskRepository::new(task, now);
    task_repository.defer_candidate_leaf_task_id_opt = Some(expected_id);

    let actual = select_focus_task_id(
        &mut task_repository,
        FocusSelectionMode::LowestPriority { recent_days: 3 },
    );

    assert_eq!(actual, Some(expected_id));
    assert_eq!(
        task_repository.last_defer_candidate_recent_days_opt,
        Some(3)
    );
}

#[test]
fn test_execute_all_pendingタスクを予定時刻に含め_doneタスクを除外する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    let pending_task = Task::new("延期中タスク");
    pending_task.set_start_time(now);
    pending_task.sync_clock(now);
    pending_task.set_pending_until(now + Duration::hours(2));
    pending_task.set_orig_status(Status::Pending);
    let pending_result =
        execute_command_for_test(pending_task.clone(), now, Some(pending_task.get_id()), "全");

    let done_task = Task::new("完了済みタスク");
    done_task.set_start_time(now);
    done_task.sync_clock(now);
    done_task.set_orig_status(Status::Done);
    let done_result =
        execute_command_for_test(done_task.clone(), now, Some(done_task.get_id()), "全");

    assert!(pending_result.output.contains("延期中タスク"));
    assert!(pending_result.output.contains("14:00~14:15"));
    assert!(!done_result.output.contains("完了済みタスク"));
}

#[test]
fn test_execute_all_project_categoryで絞り込む() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("カテゴリ対象タスク");
    task.sync_clock(now);
    task.set_project_category_opt(Some(ProjectCategory::Investment));

    let matched = execute_command_for_test(task.clone(), now, Some(task.get_id()), "全 資");
    let unmatched = execute_command_for_test(task.clone(), now, Some(task.get_id()), "全 獲");

    assert!(matched.output.contains("カテゴリ対象タスク"));
    assert!(!unmatched.output.contains("カテゴリ対象タスク"));
}

#[test]
fn test_execute_all_締切順の予定時刻を表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root_task = Task::new("親タスク");
    root_task.sync_clock(now);
    root_task.set_estimated_work_seconds(0);

    let mut late_deadline_attr = TaskAttr::new("締切が遅いタスク");
    late_deadline_attr.set_estimated_work_seconds(30 * 60);
    late_deadline_attr.set_start_time(now);
    late_deadline_attr.set_deadline_time_opt(Some(now + Duration::hours(3)));
    let late_deadline_task = root_task.create_as_last_child(late_deadline_attr);
    late_deadline_task.sync_clock(now);

    let mut early_deadline_attr = TaskAttr::new("締切が早いタスク");
    early_deadline_attr.set_estimated_work_seconds(15 * 60);
    early_deadline_attr.set_start_time(now);
    early_deadline_attr.set_deadline_time_opt(Some(now + Duration::hours(2)));
    let early_deadline_task = root_task.create_as_last_child(early_deadline_attr);
    early_deadline_task.sync_clock(now);

    let result = execute_command_for_test(root_task.clone(), now, Some(root_task.get_id()), "全");
    let early_deadline_line = result
        .output
        .lines()
        .find(|line| line.contains("締切が早いタスク"))
        .expect("early-deadline task line");
    let late_deadline_line = result
        .output
        .lines()
        .find(|line| line.contains("締切が遅いタスク"))
        .expect("late-deadline task line");

    assert!(
        early_deadline_line.contains("12:00~12:15"),
        "unexpected schedule output: {}",
        result.output
    );
    assert!(
        late_deadline_line.contains("12:15~12:45"),
        "unexpected schedule output: {}",
        result.output
    );
}

#[test]
fn test_execute_new_新規projectを翌朝までpendingで作成する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let original_task = Task::new("既存タスク");
    let result = execute_command_for_test(
        original_task.clone(),
        now,
        Some(original_task.get_id()),
        "新 新規project 30",
    );

    assert_eq!(result.task.get_name(), "新規project");
    assert_eq!(result.task.get_priority(), 5);
    assert_eq!(result.task.get_estimated_work_seconds(), 30 * 60);
    assert_eq!(result.task.get_orig_status(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until(),
        get_next_morning_datetime(now)
    );
    assert_eq!(result.focused_task_id_opt, Some(result.task.get_id()));
}

#[test]
fn test_execute_unplanned_延期と見積もりを省略して即時着手可能で作成する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let original_task = Task::new("既存タスク");
    let result = execute_command_for_test(
        original_task.clone(),
        now,
        Some(original_task.get_id()),
        "突 割り込みproject",
    );

    assert_eq!(result.task.get_name(), "割り込みproject");
    assert_eq!(result.task.get_orig_status(), Status::Todo);
    assert_eq!(result.task.get_estimated_work_seconds(), 15 * 60);
    assert_eq!(result.focused_task_id_opt, Some(result.task.get_id()));
}

#[test]
fn test_execute_breakdown_子を順に作り締切を継承して最初の子へfocusする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
    let parent_task = Task::new("親タスク");
    parent_task.sync_clock(now);
    parent_task.set_deadline_time_opt(Some(deadline));

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(parent_task.get_id()),
        "下 子A 子B",
    );
    let children = result.task.get_children();

    assert_eq!(
        children
            .iter()
            .map(|task| task.get_name())
            .collect::<Vec<_>>(),
        vec!["子A", "子B"]
    );
    assert!(children
        .iter()
        .all(|task| task.get_deadline_time_opt() == Some(deadline)));
    assert_eq!(result.focused_task_id_opt, Some(children[0].get_id()));
    assert!(result.output.contains("子A"));
    assert!(result.output.contains("子B"));
}

#[test]
fn test_execute_breakdown_数値を含む引数では子を作らない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = Task::new("親タスク");
    let parent_task_id = parent_task.get_id();

    let result = execute_command_for_test(parent_task, now, Some(parent_task_id), "下 子タスク 15");

    assert!(result.task.get_children().is_empty());
    assert_eq!(result.focused_task_id_opt, Some(parent_task_id));
}

#[test]
fn test_execute_breakdown_親に締切がなければ子も締切なしで作る() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = Task::new("親タスク");

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(parent_task.get_id()),
        "下 子タスク",
    );
    let children = result.task.get_children();

    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_deadline_time_opt(), None);
}

#[test]
fn test_execute_next_up_数値名と負の見積もりでは変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in [
        "上 123 10",
        "上 新しい親 -1",
        "上 新しい親 abc",
        "上 新しい親 9223372036854775808",
    ] {
        let root = Task::new("root");
        let focused = root.create_as_last_child(TaskAttr::new("focus"));
        let result = execute_command_for_test(root, now, Some(focused.get_id()), command);

        assert_eq!(result.task.get_children().len(), 1);
        assert_eq!(result.task.get_children()[0].get_name(), "focus");
        assert_eq!(result.focused_task_id_opt, Some(focused.get_id()));
    }
}

#[test]
fn test_execute_sequential_数値名と負の見積もりでは変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in ["連 123 10 1 2", "連 子 -1 1 2"] {
        let root = Task::new("root");
        let result = execute_command_for_test(root.clone(), now, Some(root.get_id()), command);

        assert!(result.task.get_children().is_empty());
        assert_eq!(result.focused_task_id_opt, Some(root.get_id()));
    }
}

#[test]
fn test_execute_split_負数は親に残す時間として扱う() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root = Task::new("root");
    root.set_estimated_work_seconds(100 * 60);

    let result = execute_command_for_test(root.clone(), now, Some(root.get_id()), "割 -15 子");
    let child = &result.task.get_children()[0];

    assert_eq!(result.task.get_estimated_work_seconds(), 15 * 60);
    assert_eq!(child.get_name(), "子");
    assert_eq!(child.get_estimated_work_seconds(), 85 * 60);
    assert_eq!(result.focused_task_id_opt, Some(child.get_id()));
}

#[test]
fn test_execute_split_数値名とoverflowでは変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in [
        "割 -15 123",
        "割 -9223372036854775808 子",
        "割 9223372036854775807 子",
    ] {
        let root = Task::new("root");
        root.set_estimated_work_seconds(100 * 60);
        let result = execute_command_for_test(root.clone(), now, Some(root.get_id()), command);

        assert_eq!(result.task.get_estimated_work_seconds(), 100 * 60);
        assert!(result.task.get_children().is_empty());
        assert_eq!(result.focused_task_id_opt, Some(root.get_id()));
    }
}

#[test]
fn test_execute_defer_指定時間までpendingにしてfocusを外す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("延期対象");

    let result = execute_command_for_test(task.clone(), now, Some(task.get_id()), "後 5 分");

    assert_eq!(result.task.get_orig_status(), Status::Pending);
    assert_eq!(result.task.get_pending_until(), now + Duration::minutes(5));
    assert_eq!(result.focused_task_id_opt, None);
}

#[test]
fn test_execute_defer_日付指定はその日の朝までpendingにする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("延期対象");

    let result = execute_command_for_test(task.clone(), now, Some(task.get_id()), "後 2026/08/13");

    assert_eq!(result.task.get_orig_status(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until(),
        Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 1).unwrap()
    );
    assert_eq!(result.focused_task_id_opt, None);
}

#[test]
fn test_execute_finish_未完了の子があれば完了しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = Task::new("親タスク");
    parent_task.create_as_last_child(TaskAttr::new("未完了の子"));

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(parent_task.get_id()),
        "終 今",
    );

    assert_ne!(result.task.get_status(), Status::Done);
    assert_eq!(result.task.get_end_time_opt(), None);
    assert_eq!(result.focused_task_id_opt, Some(parent_task.get_id()));
    assert!(result.output.contains("未完了の子"));
}

#[test]
fn test_execute_finish_未完了の子があれば不正引数でもtreeを表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = Task::new("親タスク");
    parent_task.create_as_last_child(TaskAttr::new("未完了の子"));

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(parent_task.get_id()),
        "終 invalid",
    );

    assert_ne!(result.task.get_status(), Status::Done);
    assert_eq!(result.task.get_end_time_opt(), None);
    assert_eq!(result.focused_task_id_opt, Some(parent_task.get_id()));
    assert!(result.output.contains("未完了の子"));
}

#[test]
fn test_execute_finish_唯一の子を完了すると親へfocusする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = Task::new("親タスク");
    let child_task = parent_task.create_as_last_child(TaskAttr::new("子タスク"));

    let result =
        execute_command_for_test(parent_task.clone(), now, Some(child_task.get_id()), "終 今");
    let finished_child = result.task.get_by_id(child_task.get_id()).unwrap();

    assert_eq!(finished_child.get_status(), Status::Done);
    assert_eq!(finished_child.get_end_time_opt(), Some(now));
    assert_eq!(result.focused_task_id_opt, Some(parent_task.get_id()));
}

#[test]
fn test_execute_finish_繰り返しtaskの見積もりを実績との差に応じて補正する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let cases = [(1_000, 900), (200, 500), (600, 600)];

    for (actual_work_seconds, expected_estimated_work_seconds) in cases {
        let parent_task = Task::new("繰り返しtask");
        parent_task.set_repetition_interval_days_opt(Some(7));
        parent_task.set_estimated_work_seconds(600);
        let mut child_attr = TaskAttr::new("今回分");
        child_attr.set_actual_work_seconds(actual_work_seconds);
        let child_task = parent_task.create_as_last_child(child_attr);

        let result = execute_command_for_test(parent_task, now, Some(child_task.get_id()), "終 今");

        assert_eq!(
            result.task.get_estimated_work_seconds(),
            expected_estimated_work_seconds
        );
    }
}

#[test]
fn test_execute_repetition_数値だけの名前は拒否して元taskを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("既存タスク");
    task.set_estimated_work_seconds(45 * 60);

    let result = execute_command_for_test(
        task.clone(),
        now,
        Some(task.get_id()),
        "繰 123 10 毎 09:00 10:00",
    );

    assert_eq!(result.task.get_estimated_work_seconds(), 45 * 60);
    assert!(result.task.get_children().is_empty());
    assert_eq!(result.focused_task_id_opt, Some(task.get_id()));
}

#[test]
fn test_execute_new_数値だけの名前は拒否して元taskを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("既存タスク");

    let result = execute_command_for_test(task.clone(), now, Some(task.get_id()), "新 123 10");

    assert_eq!(result.task.get_id(), task.get_id());
    assert_eq!(result.task.get_name(), "既存タスク");
    assert_eq!(result.focused_task_id_opt, Some(task.get_id()));
}

#[test]
fn test_execute_repetition_不正な見積もりでは元taskを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for estimated_work_minutes in ["-1", "9223372036854775807"] {
        let task = Task::new("既存タスク");
        task.set_estimated_work_seconds(45 * 60);
        let command = format!("繰 反復 {estimated_work_minutes} 毎 09:00 10:00");

        let result = execute_command_for_test(task.clone(), now, Some(task.get_id()), &command);

        assert_eq!(result.task.get_estimated_work_seconds(), 45 * 60);
        assert!(result.task.get_children().is_empty());
        assert_eq!(result.focused_task_id_opt, Some(task.get_id()));
    }
}

#[test]
fn test_execute_estimate_見積もりを更新し不正値では維持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("更新対象");
    let task_id = task.get_id();

    let updated = execute_command_for_test(task, now, Some(task_id), "予 45");
    assert_eq!(updated.task.get_estimated_work_seconds(), 45 * 60);

    let unchanged = execute_command_for_test(updated.task, now, Some(task_id), "予 invalid");
    assert_eq!(unchanged.task.get_estimated_work_seconds(), 45 * 60);
}

#[test]
fn test_execute_deadline_締切を設定して解除する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("更新対象");
    let task_id = task.get_id();

    let updated = execute_command_for_test(task, now, Some(task_id), "〆 2026/08/20");
    assert_eq!(
        updated.task.get_deadline_time_opt(),
        Some(Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap())
    );

    let cleared = execute_command_for_test(updated.task, now, Some(task_id), "〆 消");
    assert_eq!(cleared.task.get_deadline_time_opt(), None);

    let time_updated = execute_command_for_test(cleared.task, now, Some(task_id), "〆 14:30");
    assert_eq!(
        time_updated.task.get_deadline_time_opt(),
        Some(Local.with_ymd_and_hms(2026, 8, 11, 14, 30, 0).unwrap())
    );

    let invalid = execute_command_for_test(time_updated.task, now, Some(task_id), "〆 invalid");
    assert_eq!(
        invalid.task.get_deadline_time_opt(),
        Some(Local.with_ymd_and_hms(2026, 8, 11, 14, 30, 0).unwrap())
    );
}

#[test]
fn test_execute_arrange_デフォルトで見積もり0と完了済みを維持する() {
    let task = execute_arrange_command("揃 15");
    let children = task.get_children();

    assert_eq!(children[0].get_estimated_work_seconds(), 15 * 60);
    assert_eq!(children[1].get_estimated_work_seconds(), 0);
    assert_eq!(children[2].get_estimated_work_seconds(), 10 * 60);
}

#[test]
fn test_execute_arrange_全指定で見積もり0も変更し完了済みは維持する() {
    let task = execute_arrange_command("揃 15 全");
    let children = task.get_children();

    assert_eq!(children[0].get_estimated_work_seconds(), 15 * 60);
    assert_eq!(children[1].get_estimated_work_seconds(), 15 * 60);
    assert_eq!(children[2].get_estimated_work_seconds(), 10 * 60);
}

#[test]
fn test_execute_arrange_all指定は全指定と同じ挙動になる() {
    let task = execute_arrange_command("arr 15 all");
    let children = task.get_children();

    assert_eq!(children[0].get_estimated_work_seconds(), 15 * 60);
    assert_eq!(children[1].get_estimated_work_seconds(), 15 * 60);
    assert_eq!(children[2].get_estimated_work_seconds(), 10 * 60);
}

#[test]
fn test_execute_arrange_未知の第3引数で見積もり0を維持する() {
    let task = execute_arrange_command("揃 15 unknown");
    let children = task.get_children();

    assert_eq!(children[0].get_estimated_work_seconds(), 15 * 60);
    assert_eq!(children[1].get_estimated_work_seconds(), 0);
    assert_eq!(children[2].get_estimated_work_seconds(), 10 * 60);
}

#[test]
fn test_execute_arrange_見積もり0分を受理する() {
    let task = execute_arrange_command("揃 0");
    let children = task.get_children();

    assert_eq!(children[0].get_estimated_work_seconds(), 0);
    assert_eq!(children[1].get_estimated_work_seconds(), 0);
    assert_eq!(children[2].get_estimated_work_seconds(), 10 * 60);
}

#[test]
fn test_execute_arrange_見積もり1439分を受理する() {
    let task = execute_arrange_command("揃 1439");
    let children = task.get_children();

    assert_eq!(children[0].get_estimated_work_seconds(), 1439 * 60);
    assert_eq!(children[1].get_estimated_work_seconds(), 0);
    assert_eq!(children[2].get_estimated_work_seconds(), 10 * 60);
}

#[test]
fn test_execute_arrange_見積もり1440分では変更しない() {
    let task = execute_arrange_command("揃 1440");
    let children = task.get_children();

    assert_eq!(children[0].get_estimated_work_seconds(), 5 * 60);
    assert_eq!(children[1].get_estimated_work_seconds(), 0);
    assert_eq!(children[2].get_estimated_work_seconds(), 10 * 60);
}

#[test]
fn test_execute_arrange_負の見積もりでは変更しない() {
    let task = execute_arrange_command("揃 -1");
    let children = task.get_children();

    assert_eq!(children[0].get_estimated_work_seconds(), 5 * 60);
    assert_eq!(children[1].get_estimated_work_seconds(), 0);
    assert_eq!(children[2].get_estimated_work_seconds(), 10 * 60);
}

#[test]
fn test_execute_sequential_接尾辞の前にハイフンを付ける() {
    let (task, focused_task_id_opt) = execute_sequential_command("連 鎖タスク 10 1 2 話");

    let children = task.get_children();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_name(), "鎖タスク 2-話");

    let grand_children = children[0].get_children();
    assert_eq!(grand_children.len(), 1);
    assert_eq!(grand_children[0].get_name(), "鎖タスク 1-話");
    assert_eq!(focused_task_id_opt, Some(grand_children[0].get_id()));
}

#[test]
fn test_execute_sequential_接尾辞なしではハイフンを付けない() {
    let (task, focused_task_id_opt) = execute_sequential_command("連 鎖タスク 10 1 2");

    let children = task.get_children();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_name(), "鎖タスク 2");

    let grand_children = children[0].get_children();
    assert_eq!(grand_children.len(), 1);
    assert_eq!(grand_children[0].get_name(), "鎖タスク 1");
    assert_eq!(focused_task_id_opt, Some(grand_children[0].get_id()));
}

#[test]
fn test_execute_finish_引数なしは実作業時間を自動加算して現在時刻で完了する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = Task::new("タスク");
    task.set_actual_work_seconds(60);
    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "終",
    );

    let actual = task_repository.get_by_id(task_id).unwrap();
    assert_eq!(actual.get_status(), Status::Done);
    assert_eq!(actual.get_actual_work_seconds(), 360);
    assert_eq!(actual.get_end_time_opt(), Some(now));
}

#[test]
fn test_execute_finish_今は実作業時間を自動加算せず現在時刻で完了する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = Task::new("タスク");
    task.set_actual_work_seconds(60);
    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "終 今",
    );

    let actual = task_repository.get_by_id(task_id).unwrap();
    assert_eq!(actual.get_status(), Status::Done);
    assert_eq!(actual.get_actual_work_seconds(), 60);
    assert_eq!(actual.get_end_time_opt(), Some(now));
}

#[test]
fn test_execute_finish_時刻指定は実作業時間を自動加算せず指定時刻で完了する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = Task::new("タスク");
    task.set_actual_work_seconds(60);
    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "終 14:30",
    );

    let actual = task_repository.get_by_id(task_id).unwrap();
    assert_eq!(actual.get_status(), Status::Done);
    assert_eq!(actual.get_actual_work_seconds(), 60);
    assert_eq!(
        actual.get_end_time_opt(),
        Some(Local.with_ymd_and_hms(2026, 5, 17, 14, 30, 0).unwrap())
    );
}

#[test]
fn test_execute_finish_秒つき時刻指定は指定秒で完了する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = Task::new("タスク");
    task.set_actual_work_seconds(60);
    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "終 9:23:45 2026/7/4",
    );

    let actual = task_repository.get_by_id(task_id).unwrap();
    assert_eq!(actual.get_status(), Status::Done);
    assert_eq!(actual.get_actual_work_seconds(), 60);
    assert_eq!(
        actual.get_end_time_opt(),
        Some(Local.with_ymd_and_hms(2026, 7, 4, 9, 23, 45).unwrap())
    );
}

#[test]
fn test_execute_finish_不正な引数では完了しない() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = Task::new("タスク");
    task.set_actual_work_seconds(60);
    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "終 xxx",
    );

    let actual = task_repository.get_by_id(task_id).unwrap();
    assert_eq!(actual.get_status(), Status::Todo);
    assert_eq!(actual.get_actual_work_seconds(), 60);
    assert_eq!(actual.get_end_time_opt(), None);
}

#[test]
fn test_execute_today_カテゴリ別の予定時間集計を表示する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = Task::new("投資タスク");
    task.set_project_category_opt(Some(ProjectCategory::Investment));
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManagerWithFreeMinutes { free_minutes: 30 };
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "今",
    );

    let actual = String::from_utf8(stdout.buffer).unwrap();
    assert!(actual.contains(" 00 資 投資タスク"));
    assert!(actual.contains(
        "予定カテゴリ: 獲得 0.0時間(0% | 0%) / 維持 0.0時間(0% | 0%) / 回復 0.0時間(0% | 0%) / 投資 1.0時間(200% | 200%) / 消費 0.0時間(0% | 200%) / 未分類 0.0時間(0% | 200%)"
    ));
}

#[test]
fn test_execute_set_project_category_表示記号でカテゴリを設定する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = Task::new("タスク");
    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "類 資",
    );

    let actual = task_repository.get_by_id(task_id).unwrap();
    assert_eq!(
        actual.get_project_category_opt(),
        Some(ProjectCategory::Investment)
    );
}

#[test]
fn test_execute_set_project_category_英語aliasでカテゴリを設定する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = Task::new("タスク");
    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "category earning",
    );

    let actual = task_repository.get_by_id(task_id).unwrap();
    assert_eq!(
        actual.get_project_category_opt(),
        Some(ProjectCategory::Earning)
    );

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "cat 消",
    );

    let actual = task_repository.get_by_id(task_id).unwrap();
    assert_eq!(
        actual.get_project_category_opt(),
        Some(ProjectCategory::Consumption)
    );
}

#[test]
fn test_execute_set_project_category_未分類に戻す() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = Task::new("タスク");
    task.set_project_category_opt(Some(ProjectCategory::Investment));
    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    for cmd in ["類 _", "類 none", "類 clear"] {
        task.set_project_category_opt(Some(ProjectCategory::Investment));

        execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &focus_started_datetime,
            cmd,
        );

        let actual = task_repository.get_by_id(task_id).unwrap();
        assert_eq!(actual.get_project_category_opt(), None);
    }
}

#[test]
fn test_execute_set_project_category_不正カテゴリでは変更しない() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = Task::new("タスク");
    task.set_project_category_opt(Some(ProjectCategory::Investment));
    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "類 invalid",
    );

    let actual = task_repository.get_by_id(task_id).unwrap();
    assert_eq!(
        actual.get_project_category_opt(),
        Some(ProjectCategory::Investment)
    );
}

fn execute(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &DateTime<Local>,
    untrimmed_line: &str,
) {
    // 整形
    let re = Regex::new(r"\s+").unwrap();
    let line: String = re
        .replace_all(untrimmed_line, " ")
        .to_string()
        .trim()
        .to_string();

    let focused_task_opt: Option<Task> =
        focused_task_id_opt.and_then(|id| task_repository.get_by_id(id));

    let tokens: Vec<&str> = line.split(' ').collect();

    if tokens.is_empty() {
        return;
    }

    match tokens[0] {
        "新" | "遊" | "new" | "hobby" => {
            if tokens.len() >= 2 {
                let new_project_name_str = &tokens[1];

                let estimated_work_minutes_opt: Option<i64> = if tokens.len() >= 3 {
                    match tokens[2].parse() {
                        Ok(m) => Some(m),
                        Err(_) => None,
                    }
                } else {
                    None
                };

                let defer_days_opt = if tokens[0] == "新" || tokens[0] == "new" {
                    Some(1)
                } else {
                    Some(1400)
                };
                execute_start_new_project(
                    stdout,
                    focused_task_id_opt,
                    task_repository,
                    new_project_name_str,
                    defer_days_opt,
                    estimated_work_minutes_opt,
                );
            }
        }
        "突" | "unplanned" => {
            if tokens.len() >= 2 {
                let new_project_name_str = &tokens[1];

                let estimated_work_minutes_opt: Option<i64> = if tokens.len() >= 3 {
                    match tokens[2].parse() {
                        Ok(m) => Some(m),
                        Err(_) => None,
                    }
                } else {
                    None
                };

                let defer_days_opt = None;
                execute_start_new_project(
                    stdout,
                    focused_task_id_opt,
                    task_repository,
                    new_project_name_str,
                    defer_days_opt,
                    estimated_work_minutes_opt,
                );
            }
        }
        "連" | "sequential" | "seq" => {
            if tokens.len() >= 5 {
                let new_task_name_str = &tokens[1];
                let estimated_work_minutes_result = &tokens[2].parse();
                let begin_index_result = &tokens[3].parse();
                let end_index_result = &tokens[4].parse();
                let new_task_name_suffix = tokens
                    .get(5)
                    .map_or_else(String::new, |suffix| format!("-{suffix}"));

                if let Ok(estimated_work_minutes) = estimated_work_minutes_result {
                    if let Ok(begin_index) = begin_index_result {
                        if let Ok(end_index) = end_index_result {
                            if begin_index <= end_index {
                                let _ = execute_breakdown_sequentially(
                                    stdout,
                                    focused_task_id_opt,
                                    &focused_task_opt,
                                    new_task_name_str,
                                    *estimated_work_minutes,
                                    *begin_index,
                                    *end_index,
                                    &new_task_name_suffix,
                                );
                            }
                        }
                    }
                }
            }
        }
        "繰" | "repeat" => {
            if tokens.len() == 6 {
                let new_task_name_str = &tokens[1];
                let estimated_work_minutes_result = &tokens[2].parse();
                let day = &tokens[3];
                let start_time_str = &tokens[4];
                let deadline_time_str = &tokens[5];

                if let Ok(estimated_work_minutes) = estimated_work_minutes_result {
                    let _ = execute_create_repetition_task(
                        stdout,
                        task_repository,
                        focused_task_id_opt,
                        new_task_name_str,
                        day,
                        *estimated_work_minutes,
                        start_time_str,
                        deadline_time_str,
                    );
                }
            }
        }
        "約" | "appointment" => {
            let now = task_repository.get_last_synced_time();
            let start_time_opt = decide_time(&tokens, &now);

            if let Some(start_time) = start_time_opt {
                execute_make_appointment(&focused_task_opt, start_time);
            }
        }
        "始" | "start" => {
            let now: DateTime<Local> = task_repository.get_last_synced_time();
            let start_dst_time_opt = decide_time(&tokens, &now);

            if let Some(start_dst_time) = start_dst_time_opt {
                if let Some(focused_task) =
                    focused_task_id_opt.and_then(|id| task_repository.get_by_id(id))
                {
                    focused_task.set_start_time(start_dst_time);
                }
            }
        }
        // 最初は「木」コマンドだったが、曜日だけを指定して直近のその曜日について「全」コマンドを動かすコマンドとコンフリクトしてしまったためリネームした。
        "樹" | "tree" => {
            execute_show_tree(stdout, &focused_task_opt);
        }
        "条" | "祖" | "ancestor" | "anc" => {
            execute_show_ancestor(stdout, &focused_task_opt);
        }
        "根" | "root" => {
            if let Some(focused_task) = focused_task_opt {
                let root_task = focused_task.root();
                let root_task_id = root_task.get_id();
                execute_focus(focused_task_id_opt, &root_task_id.hyphenated().to_string());
            }
        }
        "葉" | "leaves" | "leaf" | "lf" => {
            execute_show_leaf_tasks(stdout, task_repository, free_time_manager);
        }
        "全" | "all" => {
            let pattern_opt = if tokens.len() >= 2 {
                Some(resolve_show_all_pattern(
                    tokens[1],
                    task_repository.get_last_synced_time(),
                ))
            } else {
                None
            };

            execute_show_all_tasks(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
                TaskListDisplayOrder::ScheduledStartDesc,
            );
        }
        "尾" => {
            let pattern_opt = if tokens.len() >= 2 {
                Some(tokens[1].to_string())
            } else {
                Some("今".to_string())
            };

            execute_show_all_tasks(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
                TaskListDisplayOrder::LowPriorityTail,
            );
        }
        "今" | "today" => {
            let pattern_opt = Some("今".to_string());
            execute_show_all_tasks(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
                TaskListDisplayOrder::ScheduledStartDesc,
            );
        }
        "単" | "non_repetitive" => {
            let pattern_opt = Some("単".to_string());
            execute_show_all_tasks(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
                TaskListDisplayOrder::ScheduledStartDesc,
            );
        }
        "暦" | "cal" => {
            let pattern_opt = Some("暦".to_string());
            execute_show_all_tasks(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
                TaskListDisplayOrder::ScheduledStartDesc,
            );
        }
        "帯" | "band" => {
            let pattern_opt = Some("帯".to_string());
            execute_show_all_tasks(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
                TaskListDisplayOrder::ScheduledStartDesc,
            );
        }
        "見" | "focus" | "fc" => {
            if tokens.len() >= 2 {
                let new_task_id_str = &tokens[1];
                execute_focus(focused_task_id_opt, new_task_id_str);
            }
        }
        "選" | "pick" => {
            let new_task_id_str = if tokens.len() >= 2 { tokens[1] } else { "" };
            execute_pick(task_repository, focused_task_id_opt, new_task_id_str);
        }
        "開" | "open" | "op" => {
            execute_open_link(&focused_task_opt);
        }
        "黒" | "obs" => {
            execute_open_obsidian_root_task_search(&focused_task_opt);
        }
        "外" | "unfocus" | "ufc" => {
            execute_unfocus(focused_task_id_opt);
        }
        "親" | "parent" => {
            if let Some(focused_task) = focused_task_opt {
                if let Some(parent_task) = focused_task.parent() {
                    let parent_task_id = parent_task.get_id();
                    execute_focus(
                        focused_task_id_opt,
                        &parent_task_id.hyphenated().to_string(),
                    );
                }
            }
        }
        "子" | "children" | "ch" => {
            // 今見ているノードの子タスクが1つだけの時、その子に移動する
            // 2つ以上ある時には、「木」コマンドを実行してツリーの様子を表示する

            if let Some(ref focused_task) = focused_task_opt {
                let tmp_children = focused_task.get_children();
                let children: Vec<_> = tmp_children
                    .iter()
                    .filter(|child| child.get_status() != Status::Done)
                    .collect();

                match children.len() {
                    0 => {
                        // Do nothing
                    }
                    1 => {
                        *focused_task_id_opt = Some(children[0].get_id());
                    }
                    _ => {
                        execute_show_tree(stdout, &focused_task_opt);
                    }
                }
            }
        }
        "深" | "deep" | "deepest" => {
            // 今見ているノードの子タスクが1つだけである限り、その子に移動して同じことを繰り返す
            // 2つ以上ある時には、「木」コマンドを実行してツリーの様子を表示する

            if let Some(ref focused_task) = focused_task_opt {
                let mut tmp_focused_task_opt: Option<Task> = Some(focused_task.clone());

                while let Some(ref tmp_focused_task) = tmp_focused_task_opt {
                    let tmp_children = tmp_focused_task.get_children();
                    let children: Vec<_> = tmp_children
                        .iter()
                        .filter(|child| child.get_status() != Status::Done)
                        .collect();

                    if children.len() != 1 {
                        break;
                    }

                    tmp_focused_task_opt = Some(children[0].clone());
                }

                if let Some(ref tmp_focused_task) = tmp_focused_task_opt {
                    *focused_task_id_opt = Some(tmp_focused_task.get_id());

                    if tmp_focused_task.get_children().len() > 1 {
                        execute_show_tree(stdout, &tmp_focused_task_opt);
                    }
                }
            }
        }
        "上" | "nextup" | "nu" => {
            if tokens.len() >= 2 {
                let new_task_name_str = &tokens[1];
                let estimated_work_minutes_result =
                    tokens.get(2).map(|token| token.parse::<i64>()).transpose();

                if let Ok(estimated_work_minutes_opt) = estimated_work_minutes_result {
                    let _ = execute_next_up(
                        stdout,
                        focused_task_id_opt,
                        &focused_task_opt,
                        new_task_name_str,
                        &estimated_work_minutes_opt,
                    );
                }
            }
        }
        "下" | "breakdown" | "bd" => {
            if tokens.len() >= 2 {
                let new_task_names = &tokens[1..];

                // 「割」コマンドと間違えて数値を引数に取った場合は何もしない
                if !tokens.iter().any(|token| token.parse::<i64>().is_ok()) {
                    let _ = execute_breakdown(
                        stdout,
                        task_repository,
                        focused_task_id_opt,
                        new_task_names,
                        &None,
                    );
                }
            }
        }
        "割" | "split" | "sp" => {
            if tokens.len() == 3 {
                let splitted_work_minutes_str = &tokens[1];
                let new_task_name = &tokens[2];

                let _ = execute_split(
                    stdout,
                    focused_task_id_opt,
                    &focused_task_opt,
                    new_task_name,
                    splitted_work_minutes_str,
                );
            }
        }
        // "詳" | "description" | "desc" => {}
        "待" | "wait" => {
            // フラグを立てるだけか、deferコマンドを自動実行するかは迷う。
            execute_wait_for_others(&focused_task_opt);
        }
        "〆" | "締" | "deadline" => {
            if tokens.len() >= 2 {
                // "2023/05/23"とか。簡単のため、時刻は指定不要とし、自動的に23:59を〆切と設定する
                // 5/23のようにhh/mmで指定した場合は、年の情報を補完してその日の23:59を〆切と設定する
                // 月~日と指定した場合、明日以降で直近のその曜日の23:59を〆切と設定する

                let deadline_date_str = &tokens[1];

                let now: DateTime<Local> = task_repository.get_last_synced_time();

                let mmdd_reg = Regex::new(r"^(\d{1,2})/(\d{1,2})$").unwrap();

                if tokens[1].starts_with('今') {
                    let s = (get_next_morning_datetime(now) - Duration::days(1))
                        .format("%Y/%m/%d")
                        .to_string();
                    execute_set_deadline(task_repository, *focused_task_id_opt, &s);
                } else if tokens[1].starts_with('明') {
                    let s = get_next_morning_datetime(now)
                        .format("%Y/%m/%d")
                        .to_string();
                    execute_set_deadline(task_repository, *focused_task_id_opt, &s);
                } else if ["月", "火", "水", "木", "金", "土", "日"].contains(&tokens[1]) {
                    // 月 火 水 木 金 土 日 が指定された時は、明日以降で、直近のその曜日の23:59を〆切とする
                    // (show_all_tasksとロジック重複...)

                    let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];

                    let todays_morning_datetime =
                        get_next_morning_datetime(now) - Duration::days(1);

                    let dn = todays_morning_datetime.date_naive();
                    let now_weekday_jp = get_weekday_jp(&dn);

                    let now_days_of_week_ind = days_of_week
                        .iter()
                        .position(|&x| x == now_weekday_jp)
                        .unwrap();
                    let target_days_of_week_ind =
                        days_of_week.iter().position(|&x| x == tokens[1]).unwrap();

                    let ind_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                    // 今日の〆切については「〆 今」で設定できるので、その代わりに、1週間後の同じ曜日の情報を設定するようにする
                    let days: i64 = if ind_diff == 0 { 7 } else { ind_diff as i64 };

                    let s = (get_next_morning_datetime(now) + Duration::days(days - 1))
                        .format("%Y/%m/%d")
                        .to_string();

                    execute_set_deadline(task_repository, *focused_task_id_opt, &s);
                } else if mmdd_reg.is_match(tokens[1]) {
                    // FIXME 「後」コマンドとロジック重複

                    let caps = mmdd_reg.captures(tokens[1]).unwrap();
                    let mm: u32 = caps[1].parse().unwrap();
                    let dd: u32 = caps[2].parse().unwrap();

                    // この時点では12:00にしているが、後で時刻を無視するので問題ない
                    let mut deadline_dst_time = Local
                        .with_ymd_and_hms(now.year(), mm, dd, 12, 0, 0)
                        .unwrap();

                    if deadline_dst_time < now {
                        deadline_dst_time = get_next_morning_datetime(
                            Local
                                .with_ymd_and_hms(now.year() + 1, mm, dd, 12, 0, 0)
                                .unwrap(),
                        ) - Duration::days(1);
                    }

                    let s = deadline_dst_time.format("%Y/%m/%d").to_string();

                    execute_set_deadline(task_repository, *focused_task_id_opt, &s);
                } else {
                    execute_set_deadline(task_repository, *focused_task_id_opt, deadline_date_str);
                }
            }
        }
        "予" | "estimate" | "es" => {
            if tokens.len() >= 2 {
                let estimated_work_minutes_str = &tokens[1];
                execute_set_estimated_work_minutes(
                    task_repository,
                    *focused_task_id_opt,
                    estimated_work_minutes_str,
                );
            }
        }
        "揃" | "arrange" | "arr" => {
            if tokens.len() >= 2 {
                let estimated_work_minutes_str = &tokens[1];
                let includes_zero_estimate = tokens
                    .get(2)
                    .is_some_and(|token| matches!(*token, "全" | "all"));
                execute_set_arrange_children_work_minutes(
                    &focused_task_opt,
                    estimated_work_minutes_str,
                    includes_zero_estimate,
                );
            }
        }
        "実" | "actual" | "ac" => {
            if tokens.len() >= 2 {
                let actual_work_minutes_str = &tokens[1];
                execute_set_actual_work_minutes(&focused_task_opt, actual_work_minutes_str);
            }
        }
        "重" | "priority" | "pr" => {
            if tokens.len() >= 2 {
                let priority_str = &tokens[1];
                execute_set_priority(&focused_task_opt, priority_str);
            }
        }
        "類" | "category" | "cat" => {
            if tokens.len() >= 2 {
                let project_category_str = &tokens[1];
                execute_set_project_category(
                    task_repository,
                    *focused_task_id_opt,
                    project_category_str,
                );
            }
        }
        "働" | "work" | "wk" => {
            let additional_actual_work_minutes: i64 = if tokens.len() >= 2 {
                tokens[1].parse().unwrap()
            } else {
                (Local::now() - *focus_started_datetime).num_minutes() + 1
            };

            if let Some(ref focused_task) = focused_task_opt {
                let original_actual_work_minutes = focused_task.get_actual_work_seconds() / 60;
                let actual_work_minutes_str = format!(
                    "{}",
                    original_actual_work_minutes + additional_actual_work_minutes
                );
                execute_set_actual_work_minutes(&focused_task_opt, &actual_work_minutes_str);
                *focused_task_id_opt = None;
            }
        }
        "後" | "defer" => {
            if tokens.len() >= 3 {
                let amount_str = &tokens[1];
                let unit_str = &tokens[2].to_lowercase();

                execute_defer(task_repository, focused_task_id_opt, amount_str, unit_str);
            } else if tokens.len() == 2 {
                let yyyymmdd_reg = Regex::new(r"^\d{4}/\d{2}/\d{2}$").unwrap();
                let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();

                if yyyymmdd_reg.is_match(tokens[1]) {
                    let defer_dst_str = format!("{} 12:00:00", tokens[1]);
                    let defer_dst_date_result =
                        parse_local_datetime(&defer_dst_str, "%Y/%m/%d %H:%M:%S");

                    match defer_dst_date_result {
                        Ok(LocalResult::Single(defer_dst_date)) => {
                            let defer_dst_time =
                                get_next_morning_datetime(defer_dst_date) - Duration::days(1);

                            let now: DateTime<Local> = task_repository.get_last_synced_time();
                            let seconds = (defer_dst_time - now).num_seconds() + 1;

                            execute_defer(
                                task_repository,
                                focused_task_id_opt,
                                &format!("{}", seconds),
                                "秒",
                            );
                        }
                        _ => {
                            // pass
                        }
                    }
                } else if let Some(LocalResult::Single(defer_dst_time)) =
                    resolve_upcoming_mmdd(tokens[1], task_repository.get_last_synced_time())
                {
                    let now: DateTime<Local> = task_repository.get_last_synced_time();
                    let seconds = (defer_dst_time - now).num_seconds() + 1;

                    if seconds > 0 {
                        execute_defer(
                            task_repository,
                            focused_task_id_opt,
                            &format!("{}", seconds),
                            "秒",
                        );
                    }
                } else if hhmm_reg.is_match(tokens[1]) {
                    // 時刻が指定された時は今日のその時刻まで送る。25:00のような指定も可能
                    let now: DateTime<Local> = task_repository.get_last_synced_time();

                    let caps = hhmm_reg.captures(tokens[1]).unwrap();
                    let hh_i64: i64 = caps[1].parse().unwrap();
                    let mm: u32 = caps[2].parse().unwrap();

                    let hh = (hh_i64 % 24) as u32;

                    let defer_dst_time = now
                        .with_hour(hh % 24)
                        .expect("invalid hour")
                        .with_minute(mm)
                        .expect("invalid minute")
                        + Duration::days(hh_i64 / 24);

                    let seconds = (defer_dst_time - now).num_seconds() + 1;

                    if seconds > 0 {
                        execute_defer(
                            task_repository,
                            focused_task_id_opt,
                            &format!("{}", seconds),
                            "秒",
                        );
                    }
                } else if ["月", "火", "水", "木", "金", "土", "日"].contains(&tokens[1]) {
                    // 月 火 水 木 金 土 日 が指定された時は、明日以降で、直近のその曜日の06:00にpendingする
                    // (show_all_tasksとロジック重複...)

                    let now: DateTime<Local> = task_repository.get_last_synced_time();
                    let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];

                    let todays_morning_datetime =
                        get_next_morning_datetime(now) - Duration::days(1);

                    let dn = todays_morning_datetime.date_naive();
                    let now_weekday_jp = get_weekday_jp(&dn);

                    let now_days_of_week_ind = days_of_week
                        .iter()
                        .position(|&x| x == now_weekday_jp)
                        .unwrap();
                    let target_days_of_week_ind =
                        days_of_week.iter().position(|&x| x == tokens[1]).unwrap();

                    let ind_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                    // 今日の6:00にdeferする味意はないので、その代わりに、1週間後の同じ曜日にdeferできるようにする
                    let days: i64 = if ind_diff == 0 { 7 } else { ind_diff as i64 };

                    let seconds = (get_next_morning_datetime(now) + Duration::days(days - 1) - now)
                        .num_seconds()
                        + 1;

                    if seconds > 0 {
                        execute_defer(
                            task_repository,
                            focused_task_id_opt,
                            &format!("{}", seconds),
                            "秒",
                        );
                    }
                } else {
                    // "defer 5days" のように引数が1つしか与えられなかった場合は、数字部分とそれ以降に分割する
                    let splitted = split_amount_and_unit(tokens[1]);
                    if splitted.len() == 2 && !splitted[0].is_empty() {
                        let amount_str = &splitted[0];
                        let unit_str = &splitted[1].to_lowercase();

                        execute_defer(task_repository, focused_task_id_opt, amount_str, unit_str);
                    }
                }
            }
        }
        "清" | "defer_all_frequent_routines" => {
            execute_defer_all_frequent_routines(
                task_repository,
                focused_task_id_opt,
                &focused_task_opt,
            );
        }
        "逃" | "escape" | "esc" => {
            // 先延ばしにしてしまう時。要求している見積もりが小さすぎる可能性があるので、2倍にする
            if let Some(focused_task) = focused_task_opt {
                let estimated_work_seconds = focused_task.get_estimated_work_seconds();
                focused_task.set_estimated_work_seconds(estimated_work_seconds * 2);

                // 引数が与えられた時はそのままdeferする
                if tokens.len() >= 2 {
                    let s = format!("後 {}", tokens[1..].join(" "));

                    execute(
                        stdout,
                        task_repository,
                        free_time_manager,
                        focused_task_id_opt,
                        focus_started_datetime,
                        &s,
                    );
                }
            }
        }
        "平" | "flatten" | "flat" => {
            for _ in 0..7 {
                let pattern_opt = Some("平".to_string());
                execute_show_all_tasks(
                    stdout,
                    focused_task_id_opt,
                    task_repository,
                    free_time_manager,
                    &pattern_opt,
                    TaskListDisplayOrder::ScheduledStartDesc,
                );
            }
        }
        "押" | "extrude" => {
            if tokens.len() >= 2 {
                if let Some(ref focused_task) = focused_task_opt {
                    let first_datetime =
                        focused_task.list_all_parent_tasks_with_first_available_time()[0].0;
                    let step_days: u16 = tokens[1].parse().unwrap_or(1);

                    execute_extrude(
                        focused_task_id_opt,
                        &focused_task_opt,
                        &first_datetime,
                        step_days,
                    );
                }
            }
        }
        "空" | "clear" | "集" | "gather" => {
            // 空 13:00
            // 今着手可能なタスクについてactiveなものを、指定したタイミングまでpendingする

            // 空 13:00 10:00
            // 10:00以降に着手可能なタスクについてactiveなものを、指定したタイミングまでpendingする
            // 第3引数を任意とするので、順番が to → from の順になっているのはちょっと気になる

            // 集 13:00
            // 指定したタイミングまでに着手する予定のタスクを全てTodoに直す
            if tokens.len() >= 2 {
                let cmd_str = tokens[0];
                let defer_to_datetime_opt = parse_clear_or_gather_defer_to_datetime(
                    cmd_str,
                    tokens[1],
                    task_repository.get_last_synced_time(),
                );

                if let Some(defer_to_datetime) = defer_to_datetime_opt {
                    for project_root_task in task_repository.get_all_projects().iter() {
                        let leaf_tasks =
                            extract_leaf_tasks_from_project_with_pending(project_root_task);
                        for leaf_task in leaf_tasks.iter() {
                            match cmd_str {
                                "空" | "clear" => {
                                    if leaf_task.get_start_time() < defer_to_datetime
                                        && (leaf_task.get_orig_status() == Status::Todo
                                            || (leaf_task.get_orig_status() == Status::Pending
                                                && leaf_task.get_pending_until()
                                                    < defer_to_datetime))
                                    {
                                        leaf_task.set_orig_status(Status::Pending);
                                        leaf_task.set_pending_until(defer_to_datetime);
                                    }
                                }
                                "集" | "gather" => {
                                    if leaf_task.get_status() == Status::Pending
                                        && leaf_task.get_start_time() < defer_to_datetime
                                        && leaf_task.get_pending_until() < defer_to_datetime
                                    {
                                        leaf_task.set_orig_status(Status::Todo);
                                    }
                                }
                                _ => {
                                    // Skip
                                }
                            }
                        }
                    }
                }
            }
        }
        "終" | "finish" | "fin" => {
            if let Some(ref focused_task) = focused_task_opt {
                if focused_task.has_undone_children() {
                    execute_show_tree(stdout, &focused_task_opt);
                } else {
                    let now = task_repository.get_last_synced_time();
                    if let Some(finished_at) = decide_finish_time(&tokens, &now) {
                        let additional_actual_work_seconds = if tokens.len() == 1 {
                            let focus_duration_seconds =
                                (now - *focus_started_datetime).num_seconds();
                            if focus_duration_seconds >= 60 {
                                focus_duration_seconds
                            } else {
                                0
                            }
                        } else {
                            0
                        };

                        match complete_task(
                            task_repository,
                            CompleteTaskInput {
                                task_id: focused_task.get_id(),
                                finished_at,
                                additional_actual_work_seconds,
                            },
                        ) {
                            Ok(output) => {
                                *focused_task_id_opt = output.next_focus_task_id;
                            }
                            Err(ApplicationError::HasUndoneChildren(_)) => {
                                execute_show_tree(stdout, &focused_task_opt);
                            }
                            Err(_) => {}
                        }
                    }
                }
            }
        }
        "" | "#" => {}
        &_ => {
            // 何も該当するコマンドが無い場合には「全」コマンドとして実行する
            // ただし、最初が数字の0から始まる場合は無視する
            // show_all_commandの結果をコピーしたものを誤って貼り付けた場合に迅速に停止させるため。
            // 精緻に書こうと思えば条件を変えられる。

            if let Some(first_char) = untrimmed_line.chars().next() {
                if first_char != '0' {
                    let cmd_of_show_all = String::from("全 ") + untrimmed_line;

                    execute(
                        stdout,
                        task_repository,
                        free_time_manager,
                        focused_task_id_opt,
                        focus_started_datetime,
                        &cmd_of_show_all,
                    );
                }
            }
        }
    }

    stdout.flush().unwrap();
}

#[cfg(test)]
fn execute_show_all_command_for_test(command: &str, now: DateTime<Local>, task: Task) -> String {
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = None;
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    );

    String::from_utf8(stdout.buffer).unwrap()
}

#[cfg(test)]
fn execute_calendar_command_for_test(
    command: &str,
    now: DateTime<Local>,
    task: Task,
    free_minutes: i64,
) -> String {
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerWithFreeMinutes { free_minutes };
    let mut focused_task_id_opt = None;
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    );

    String::from_utf8(stdout.buffer).unwrap()
}

#[cfg(test)]
fn execute_band_command_with_elapsed_for_test(
    command: &str,
    now: DateTime<Local>,
    task: Task,
) -> String {
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerForBand;
    let mut focused_task_id_opt = None;
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    );

    String::from_utf8(stdout.buffer).unwrap()
}

#[cfg(test)]
fn add_scheduled_child_for_test(
    root: &Task,
    name: &str,
    start_time: DateTime<Local>,
    estimated_work_minutes: i64,
) -> Task {
    let child = root.create_as_last_child(TaskAttr::new(name));
    child.set_estimated_work_seconds(estimated_work_minutes * 60);
    child.set_start_time(start_time);
    child.set_pending_until(start_time);
    child.set_orig_status(Status::Pending);
    child
}

#[test]
fn test_execute_calendar_現行出力を固定する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("暦出力固定用タスク");
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);

    let actual = execute_calendar_command_for_test("暦", now, task.clone(), 10 * 60);
    let expected = concat!(
        "2026-08-11(火)\t10.0時間\t-9時間00分     \t-0.90\t-6時間00分\t-06時間00分\t-10時間00分\t-1.00\t-09時間00分\t 10時間00分\t-0.90\t01[タスク]\n",
        "日          \t空          \t空差      \t空差比\t余差    \t余差累    \t〆差      \t〆差比\t空差累    \t単発余暇\t空差累比\tタスク数\n",
        "\n",
        "今のタスクが片付く日付: 4160日後の2037-12-31\n",
        "最大の累積時間: -09時間00分 (2026-08-11), 最大のrhoの差: -1.00 (1900-01-01), 次にタスクを積める日付: 0日後の2026-08-11 (-6時間00分)\n",
        "\n",
        "[Info] 順調です。突発タスクに対応したり1日の終わり際にタスクを新しく積んだりする余裕があります。ひとまずは脇道に逸れずに予定の遂行をしてください。\n",
        "\n",
        "残り拘束時間は0.0時間です\n",
        "完了見込み日時は1.0時間後の2026/08/11 13:00:00です\n",
        "rep ρ = (1.00 + 0.00) / (1.00 + 0.00 + 11 + 30/60) = 0.08, Lq = 0.1\n",
        "one ρ = (1.00 + 0.00) / (1.00 + 0.00 + 11 + 30/60) = 0.08, Lq = 0.1\n",
        "\n",
    );

    assert_eq!(actual, expected);

    let english_alias = execute_calendar_command_for_test("cal", now, task, 10 * 60);
    assert_eq!(english_alias, expected);
}

#[test]
fn test_execute_calendar_日付逆順と週区切りと28日境界を固定する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root = Task::new("暦複数日fixture");
    root.set_estimated_work_seconds(0);
    add_scheduled_child_for_test(&root, "当日", now, 15);
    add_scheduled_child_for_test(
        &root,
        "月曜日",
        Local.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap(),
        15,
    );
    add_scheduled_child_for_test(
        &root,
        "28日境界",
        Local.with_ymd_and_hms(2026, 9, 8, 12, 0, 0).unwrap(),
        15,
    );
    add_scheduled_child_for_test(
        &root,
        "29日目",
        Local.with_ymd_and_hms(2026, 9, 9, 12, 0, 0).unwrap(),
        15,
    );

    let actual = execute_calendar_command_for_test("暦", now, root, 10 * 60);
    let lines = actual.lines().collect::<Vec<_>>();
    let boundary_index = lines
        .iter()
        .position(|line| line.starts_with("2026-09-08(火)"))
        .unwrap();
    let monday_index = lines
        .iter()
        .position(|line| line.starts_with("2026-08-17(月)"))
        .unwrap();
    let today_index = lines
        .iter()
        .position(|line| line.starts_with("2026-08-11(火)"))
        .unwrap();

    assert!(boundary_index < monday_index);
    assert!(monday_index < today_index);
    assert_eq!(lines[monday_index + 1], "");
    assert!(!actual.contains("2026-09-09(水)"));
}

#[test]
fn test_format_daily_band_累積境界で端数を丸めて96文字にする() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 15).unwrap();
    let actual = format_daily_band(
        date,
        "土",
        Duration::hours(46) + Duration::minutes(9),
        &DailyBandDurations {
            fixed_seconds: 450 * 60,
            elapsed_seconds: 0,
            repetitive_seconds: 855 * 60,
            non_repetitive_seconds: 71 * 60,
            rho_leeway_seconds: 24 * 60,
        },
    );
    let expected = format!(
        "2026-08-15(土) +46:09 [{}{}{}{}{}]",
        "#".repeat(30),
        "=".repeat(57),
        "-".repeat(5),
        ":",
        ".".repeat(3),
    );

    assert_eq!(actual, expected);
}

#[test]
fn test_calculate_daily_band_durations_経過した空き時間を当日だけ計上する() {
    let today = calculate_daily_band_durations(true, 990, 190, 60 * 60, 40 * 60, -1.0);
    let future = calculate_daily_band_durations(false, 990, 990, 60 * 60, 40 * 60, -1.0);

    assert_eq!(today.fixed_seconds, 450 * 60);
    assert_eq!(today.elapsed_seconds, 800 * 60);
    assert_eq!(today.repetitive_seconds, 40 * 60);
    assert_eq!(today.non_repetitive_seconds, 20 * 60);
    assert_eq!(today.rho_leeway_seconds, 60 * 60);
    assert_eq!(future.elapsed_seconds, 0);
}

#[test]
fn test_format_signed_hours_minutes_符号付きで時分を2桁ゼロ埋めする() {
    assert_eq!(format_signed_hours_minutes(Duration::zero()), "+00:00");
    assert_eq!(
        format_signed_hours_minutes(Duration::hours(6) + Duration::minutes(5)),
        "+06:05"
    );
    assert_eq!(
        format_signed_hours_minutes(-Duration::hours(6) - Duration::minutes(5)),
        "-06:05"
    );
}

#[test]
fn test_format_daily_band_当日経過と24時間超過を表示する() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
    let actual = format_daily_band(
        date,
        "火",
        -Duration::hours(3) - Duration::minutes(4),
        &DailyBandDurations {
            fixed_seconds: 450 * 60,
            elapsed_seconds: 800 * 60,
            repetitive_seconds: 476 * 60,
            non_repetitive_seconds: 40 * 60,
            rho_leeway_seconds: 0,
        },
    );
    let expected = format!(
        "2026-08-11(火) -03:04 [{}{}{}]{}",
        "#".repeat(30),
        "x".repeat(53),
        "=".repeat(13),
        ">".repeat(22),
    );

    assert_eq!(actual, expected);
}

#[test]
fn test_execute_band_日本語と英語で凡例と棒だけを表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("帯出力固定用タスク");
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);

    let japanese = execute_calendar_command_for_test("帯", now, task.clone(), 10 * 60);
    let english = execute_calendar_command_for_test("band", now, task, 10 * 60);
    let expected = format!(
        concat!(
            "凡例: # 固定  x 経過済み  = 繰返  - 単発  : 余差  . 空き  > 超過  (1文字=15分)\n",
            "\n",
            "2026-08-11(火) -09:00 [{}{}{}{}]\n",
            "\n",
        ),
        "#".repeat(56),
        "-".repeat(4),
        ":".repeat(24),
        ".".repeat(12),
    );

    assert_eq!(japanese, expected);
    assert_eq!(english, expected);
    assert!(!japanese.contains("日          "));
    assert!(!japanese.contains("残り拘束時間"));
    assert!(!japanese.contains("帯出力固定用タスク"));
}

#[test]
fn test_execute_band_全日空き差分と繰り返し判定を帯へ反映する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root = Task::new("帯データフローfixture");
    root.set_estimated_work_seconds(0);
    let repetitive_group = root.create_as_last_child(TaskAttr::new("繰り返しグループ"));
    repetitive_group.set_estimated_work_seconds(0);
    repetitive_group.set_repetition_interval_days_opt(Some(7));
    add_scheduled_child_for_test(&repetitive_group, "繰り返しタスク", now, 40);

    let actual = execute_band_command_with_elapsed_for_test("帯", now, root);
    let expected_row = format!(
        "2026-08-11(火) -02:30 [{}{}{}{}{}]",
        "#".repeat(30),
        "x".repeat(53),
        "=".repeat(3),
        ":".repeat(7),
        ".".repeat(3),
    );

    assert!(actual.contains(&expected_row), "{actual}");
}

#[test]
fn test_should_suppress_leaf_tasks_after_command_帯とbandでは葉を追加表示しない() {
    assert!(should_suppress_leaf_tasks_after_command("帯"));
    assert!(should_suppress_leaf_tasks_after_command("band"));
    assert!(!should_suppress_leaf_tasks_after_command("見"));
}

#[test]
fn test_execute_show_all_年なし日付は完全日付と同じ予定を表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let scheduled_start = Local.with_ymd_and_hms(2026, 9, 26, 6, 0, 0).unwrap();
    let task = Task::new("TARGET_DATE_TASK");
    task.set_start_time(scheduled_start);
    task.set_pending_until(scheduled_start);
    task.set_orig_status(Status::Pending);

    let abbreviated = execute_show_all_command_for_test("全 9/26", now, task.clone());
    let full = execute_show_all_command_for_test("全 2026/09/26", now, task.clone());
    let other_date = execute_show_all_command_for_test("全 9/27", now, task);

    assert_eq!(abbreviated, full);
    assert!(abbreviated.contains("TARGET_DATE_TASK"));
    assert!(!other_date.contains("TARGET_DATE_TASK"));
}

#[test]
fn test_execute_show_all_過ぎた年なし日付は翌年の予定を表示する() {
    let now = Local.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
    let scheduled_start = Local.with_ymd_and_hms(2027, 9, 26, 6, 0, 0).unwrap();
    let task = Task::new("TARGET_DATE_TASK");
    task.set_start_time(scheduled_start);
    task.set_pending_until(scheduled_start);
    task.set_orig_status(Status::Pending);

    let abbreviated = execute_show_all_command_for_test("all 9/26", now, task.clone());
    let full = execute_show_all_command_for_test("all 2027/09/26", now, task);

    assert_eq!(abbreviated, full);
    assert!(abbreviated.contains("TARGET_DATE_TASK"));
}

// 削除できない時はNoneを返す。例えば、文字列が空の時
fn get_byte_offset_for_deletion(line: &str, cursor_x: usize) -> Option<usize> {
    let byte_offset_opt = if line.is_empty() || cursor_x == 0 {
        None
    } else {
        let char_indices_vec = line.char_indices().collect::<Vec<_>>();

        Some(char_indices_vec[cursor_x - 1].0)
    };

    byte_offset_opt
}

#[test]
fn get_byte_offset_for_deletion_noneを返す場合() {
    let line = "あ";
    let cursor_x = 0;
    let actual = get_byte_offset_for_deletion(&line, cursor_x);
    let expected = None;
    assert_eq!(actual, expected);
}

#[test]
fn get_byte_offset_for_deletion_正常系() {
    let line = "あ";
    let cursor_x = 1;
    let actual = get_byte_offset_for_deletion(&line, cursor_x);
    let expected = Some(0);
    assert_eq!(actual, expected);
}

fn main() {
    let command_opt = parse_non_interactive_command(env::args().skip(1).collect());
    let project_storage_directory =
        match resolve_project_storage_directory(env::var_os("SCHRONU_STORAGE_DIR")) {
            Ok(directory) => directory,
            Err(error) => {
                eprintln!("[Error] {error}");
                process::exit(1);
            }
        };
    let mut task_repository = TaskRepository::new(
        project_storage_directory
            .to_str()
            .expect("storage path was validated"),
    );
    let mut free_time_manager = FreeTimeManager::new();
    let _storage_lock = match StorageLock::acquire(
        task_repository.get_project_storage_dir_name().as_ref(),
        LockMode::Cli,
    ) {
        Ok(storage_lock) => storage_lock,
        Err(error) => {
            eprintln!("[Error] {error}");
            process::exit(1);
        }
    };

    // controllerで実体を見るのを避けるために、1つ関数を切る
    let result = match command_opt {
        Some(command) => {
            execute_non_interactive_command(&mut task_repository, &mut free_time_manager, &command)
        }
        None => application(&mut task_repository, &mut free_time_manager),
    };
    if !report_run_result(&mut std::io::stderr(), result) {
        process::exit(1);
    }
}

fn report_run_result(stderr: &mut dyn Write, result: Result<(), RunError>) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            writeln!(stderr, "[Error] {error}").unwrap();
            false
        }
    }
}

#[test]
fn test_report_run_result_load_errorを表示して失敗を返す() {
    let mut stderr = Vec::new();
    let error = TaskRepositoryError::new(
        TaskRepositoryOperation::Load,
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "ParseProject failed for /test/project.yaml: broken YAML",
        ),
    );

    let actual = report_run_result(&mut stderr, Err(RunError::Repository(error)));

    assert!(!actual);
    let output = String::from_utf8(stderr).unwrap();
    assert!(output.contains("[Error]"));
    assert!(output.contains("Load"));
    assert!(output.contains("/test/project.yaml"));
    assert!(output.contains("broken YAML"));
}

#[test]
fn test_report_run_result_input切断を表示して失敗を返す() {
    let mut stderr = Vec::new();

    let actual = report_run_result(
        &mut stderr,
        Err(RunError::InputDisconnected {
            save_error_opt: None,
        }),
    );

    assert!(!actual);
    let output = String::from_utf8(stderr).unwrap();
    assert!(output.contains("[Error]"));
    assert!(output.contains("interactive input channel disconnected"));
}

#[test]
fn test_report_run_result_ctrl_cを表示して失敗を返す() {
    let mut stderr = Vec::new();

    let actual = report_run_result(&mut stderr, Err(RunError::Interrupted));

    assert!(!actual);
    let output = String::from_utf8(stderr).unwrap();
    assert!(output.contains("[Error]"));
    assert!(output.contains("interactive input interrupted"));
}

fn parse_non_interactive_command(args: Vec<String>) -> Option<String> {
    if args.is_empty() {
        return None;
    }

    Some(args.join(" "))
}

#[test]
fn test_parse_non_interactive_command_引数なしは_none() {
    let actual = parse_non_interactive_command(vec![]);
    let expected = None;

    assert_eq!(actual, expected);
}

#[test]
fn test_parse_non_interactive_command_単一引数をコマンドにする() {
    let actual = parse_non_interactive_command(vec!["今".to_string()]);
    let expected = Some("今".to_string());

    assert_eq!(actual, expected);
}

#[test]
fn test_parse_non_interactive_command_複数引数を1コマンドにする() {
    let actual = parse_non_interactive_command(vec!["尾".to_string(), "週".to_string()]);
    let expected = Some("尾 週".to_string());

    assert_eq!(actual, expected);
}

fn execute_non_interactive_command(
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    command: &str,
) -> Result<(), RunError> {
    let now = Local::now();
    task_repository.sync_clock(now);
    task_repository.load()?;
    free_time_manager
        .load_busy_time_slots_from_file("../Schronu-private/busy_time_slots.yaml", &now);

    let mut focused_task_id_opt: Option<Uuid> =
        select_focus_task_id(task_repository, FocusSelectionMode::HighestPriority);
    let focus_started_datetime: DateTime<Local> = now;
    let mut stdout = stdout();

    execute(
        &mut stdout,
        task_repository,
        free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        command,
    );
    Ok(())
}

#[test]
fn test_execute_non_interactive_command_load失敗時はcommandを実行しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("変更しないtask");
    let task_id = task.get_id();
    let original_estimated_work_seconds = task.get_estimated_work_seconds();
    let mut task_repository = TestTaskRepository::new(task, now);
    task_repository.load_should_fail = true;
    let mut free_time_manager = TestFreeTimeManager;

    let actual =
        execute_non_interactive_command(&mut task_repository, &mut free_time_manager, "予 45");

    assert!(matches!(
        actual,
        Err(RunError::Repository(ref error))
            if error.operation() == TaskRepositoryOperation::Load
    ));
    assert_eq!(
        task_repository
            .get_by_id(task_id)
            .unwrap()
            .get_estimated_work_seconds(),
        original_estimated_work_seconds
    );
}

#[test]
fn test_execute_non_interactive_command_gatewayの変換errorをstderrへ表示する() {
    let storage_dir = TestStorageDir::new();
    let project_dir = storage_dir.path.join("broken-project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let project_yaml_path = project_dir.join("project.yaml");
    std::fs::write(
        &project_yaml_path,
        "project:\n  name: broken\n  children: not-an-array\n",
    )
    .unwrap();
    let mut task_repository = TaskRepository::new(storage_dir.path.to_str().unwrap());
    let mut free_time_manager = TestFreeTimeManager;

    let result =
        execute_non_interactive_command(&mut task_repository, &mut free_time_manager, "予 45");
    let mut stderr = Vec::new();
    let succeeded = report_run_result(&mut stderr, result);

    assert!(!succeeded);
    let output = String::from_utf8(stderr).unwrap();
    assert!(output.contains("repository Load failed"));
    assert!(output.contains(project_yaml_path.to_str().unwrap()));
    assert!(output.contains("children must be an array or null"));
}

fn make_messages_about_focus(
    focused_task: &Task,
    focus_started_datetime: &DateTime<Local>,
    now: &DateTime<Local>,
) -> [String; 2] {
    let estimated_finish_datetime = *focus_started_datetime
        + Duration::seconds(
            focused_task.get_estimated_work_seconds() - focused_task.get_actual_work_seconds(),
        );

    let left_duration = estimated_finish_datetime - *now;
    let for_duration = *now - *focus_started_datetime;
    let focusing_minutes = for_duration.num_minutes() + 1;
    let progress = format_focus_progress(
        focused_task.get_estimated_work_seconds(),
        focused_task.get_actual_work_seconds(),
        for_duration.num_seconds(),
    );

    let summary = format!(
        "{} (since {} until {}) focusing for {} minutes",
        if left_duration >= Duration::minutes(1) {
            format!("{} minutes left", left_duration.num_minutes())
        } else if left_duration >= Duration::seconds(0) {
            format!("{} seconds left", left_duration.num_seconds())
        } else {
            format!("{} minutes over", -left_duration.num_minutes() + 1)
        },
        focus_started_datetime.format("%H:%M:%S"),
        estimated_finish_datetime.format("%H:%M:%S"),
        focusing_minutes,
    );

    [summary, progress]
}

fn format_focus_progress(
    estimated_work_seconds: i64,
    actual_work_seconds: i64,
    focusing_seconds: i64,
) -> String {
    if estimated_work_seconds <= 0 {
        return format!("[{}] --%", "-".repeat(FOCUS_PROGRESS_BAR_SEGMENTS));
    }

    let total_work_seconds =
        (i128::from(actual_work_seconds) + i128::from(focusing_seconds)).max(0);
    let percentage = total_work_seconds * 100 / i128::from(estimated_work_seconds);
    let filled_segments = percentage.min(FOCUS_PROGRESS_BAR_SEGMENTS as i128) as usize;
    let overflow_segments = (percentage - 100).max(0) as usize;

    format!(
        "[{}{}]{} {}%",
        // U+2588 FULL BLOCK
        "█".repeat(filled_segments),
        // U+2591 LIGHT SHADE
        "░".repeat(FOCUS_PROGRESS_BAR_SEGMENTS - filled_segments),
        ">".repeat(overflow_segments),
        percentage
    )
}

#[test]
fn test_format_focus_progress_100パーセントで全区画を塗る() {
    let actual = format_focus_progress(60 * 60, 59 * 60, 60);

    assert_eq!(actual, format!("[{}] 100%", "█".repeat(100)));
}

#[test]
fn test_format_focus_progress_101パーセントで超過記号を表示する() {
    let actual = format_focus_progress(100, 101, 0);

    assert_eq!(actual, format!("[{}]> 101%", "█".repeat(100)));
}

#[test]
fn test_format_focus_progress_114パーセントで超過分の記号を表示する() {
    let actual = format_focus_progress(100, 114, 0);

    assert_eq!(
        actual,
        format!("[{}]{} 114%", "█".repeat(100), ">".repeat(14))
    );
}

#[test]
fn test_format_focus_progress_開始直後は実経過0秒として扱う() {
    let actual = format_focus_progress(100 * 60, 0, 0);

    assert_eq!(actual, format!("[{}] 0%", "░".repeat(100)));
}

#[test]
fn test_format_focus_progress_見積と作業時間を秒数基準で計算する() {
    let actual = format_focus_progress(4 * 60 + 33, 0, 2 * 60);

    assert_eq!(
        actual,
        format!("[{}{}] 43%", "█".repeat(43), "░".repeat(57))
    );
}

#[test]
fn test_format_focus_progress_表示が2分でも実経過秒数を使う() {
    let actual = format_focus_progress(4 * 60 + 33, 0, 60);

    assert_eq!(
        actual,
        format!("[{}{}] 21%", "█".repeat(21), "░".repeat(79))
    );
}

#[test]
fn test_format_focus_progress_99パーセントでは1区画を未達として残す() {
    let actual = format_focus_progress(100, 99, 0);

    assert_eq!(actual, format!("[{}░] 99%", "█".repeat(99)));
}

#[test]
fn test_make_messages_about_focus_既存実績と表示中の作業時間から進捗を表示する() {
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 7, 25, 12, 0, 0).unwrap();
    let now = Local.with_ymd_and_hms(2026, 7, 25, 12, 19, 0).unwrap();
    let task = Task::new("タスク");
    task.set_estimated_work_seconds(60 * 60);
    task.set_actual_work_seconds(10 * 60);

    let actual = make_messages_about_focus(&task, &focus_started_datetime, &now);

    assert!(actual[0].ends_with("focusing for 20 minutes"));
    assert_eq!(
        actual[1],
        format!("[{}{}] 48%", "█".repeat(48), "░".repeat(52))
    );
}

#[test]
fn test_make_messages_about_focus_バーを1パーセント単位で表示する() {
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 7, 25, 12, 0, 0).unwrap();
    let now = Local.with_ymd_and_hms(2026, 7, 25, 12, 19, 0).unwrap();
    let task = Task::new("タスク");
    task.set_estimated_work_seconds(100 * 60);
    task.set_actual_work_seconds(39 * 60);

    let actual = make_messages_about_focus(&task, &focus_started_datetime, &now);

    assert!(actual[0].ends_with("focusing for 20 minutes"));
    assert_eq!(
        actual[1],
        format!("[{}{}] 58%", "█".repeat(58), "░".repeat(42))
    );
}

#[test]
fn test_make_messages_about_focus_見積時間超過時はバーだけ100パーセントを上限にする() {
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 7, 25, 12, 0, 0).unwrap();
    let now = Local.with_ymd_and_hms(2026, 7, 25, 12, 59, 0).unwrap();
    let task = Task::new("タスク");
    task.set_estimated_work_seconds(100 * 60);
    task.set_actual_work_seconds(57 * 60);

    let actual = make_messages_about_focus(&task, &focus_started_datetime, &now);

    assert!(actual[0].ends_with("focusing for 60 minutes"));
    assert_eq!(
        actual[1],
        format!("[{}]{} 116%", "█".repeat(100), ">".repeat(16))
    );
}

#[test]
fn test_make_messages_about_focus_見積時間が0なら進捗を未算定として表示する() {
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 7, 25, 12, 0, 0).unwrap();
    let now = Local.with_ymd_and_hms(2026, 7, 25, 12, 19, 0).unwrap();
    let task = Task::new("タスク");
    task.set_estimated_work_seconds(0);
    task.set_actual_work_seconds(10 * 60);

    let actual = make_messages_about_focus(&task, &focus_started_datetime, &now);

    assert!(actual[0].ends_with("focusing for 20 minutes"));
    assert_eq!(actual[1], format!("[{}] --%", "-".repeat(100)));
}

fn idle_refresh_deadline(now: Instant) -> Instant {
    now + IDLE_REFRESH_INTERVAL
}

fn idle_wait_duration(deadline: Instant, now: Instant) -> StdDuration {
    deadline.saturating_duration_since(now)
}

fn render_prompt(stdout: &mut dyn SchronuWriter, header: &str, line: &str, cursor_x: usize) {
    write!(
        stdout,
        "{}{}",
        termion::cursor::Left(MAX_COL),
        termion::clear::CurrentLine,
    )
    .unwrap();

    let width = get_width_for_rerender(header, line, cursor_x);
    write!(stdout, "{}{}", header, line).unwrap();
    write!(
        stdout,
        "{}{}",
        termion::cursor::Left(MAX_COL),
        termion::cursor::Right(width)
    )
    .unwrap();
    stdout.flush().unwrap();
}

fn try_save_before_exit(
    stdout: &mut dyn SchronuWriter,
    task_repository: &dyn TaskRepositoryTrait,
) -> bool {
    match task_repository.save() {
        Ok(()) => true,
        Err(error) => {
            writeln_newline(stdout, &format!("[Error] {error}")).unwrap();
            stdout.flush().unwrap();
            false
        }
    }
}

fn handle_input_disconnected(task_repository: &dyn TaskRepositoryTrait) -> RunError {
    RunError::InputDisconnected {
        save_error_opt: task_repository.save().err(),
    }
}

fn handle_input_read_error(
    task_repository: &dyn TaskRepositoryTrait,
    input_error: std::io::Error,
) -> RunError {
    RunError::InputRead {
        input_error,
        save_error_opt: task_repository.save().err(),
    }
}

#[allow(clippy::too_many_arguments)]
fn try_exit_interactive(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    header: &str,
    line: &str,
    cursor_x: usize,
    now: DateTime<Local>,
) -> bool {
    if !line.is_empty() {
        return false;
    }
    if !try_save_before_exit(stdout, task_repository) {
        render_prompt(stdout, header, line, cursor_x);
        return false;
    }

    task_repository.sync_clock(now);
    execute_show_all_tasks(
        stdout,
        focused_task_id_opt,
        task_repository,
        free_time_manager,
        &Some("暦".to_string()),
        TaskListDisplayOrder::ScheduledStartDesc,
    );
    true
}

fn render_focused_task(
    stdout: &mut dyn SchronuWriter,
    task_repository: &dyn TaskRepositoryTrait,
    focused_task_id_opt: Option<Uuid>,
    last_focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &mut DateTime<Local>,
    now: DateTime<Local>,
) {
    let Some(focused_task_id) = focused_task_id_opt else {
        return;
    };
    let focused_task_opt = task_repository.get_by_id(focused_task_id);

    if focused_task_id_opt != *last_focused_task_id_opt {
        *focus_started_datetime = now;
        *last_focused_task_id_opt = focused_task_id_opt;
    }

    execute_show_ancestor(stdout, &focused_task_opt);

    if let Some(focused_task) = focused_task_opt {
        writeln_newline(
            stdout,
            &format_focused_task_header(focused_task.get_project_category_opt()),
        )
        .unwrap();
        writeln_newline(stdout, &format!("{:?}", focused_task.get_attr())).unwrap();

        let messages = make_messages_about_focus(&focused_task, focus_started_datetime, &now);
        for message in messages {
            writeln_newline(stdout, &message).unwrap();
        }
        stdout.flush().unwrap();
    }
}

struct FocusRenderState<'a> {
    focused_task_id_opt: &'a mut Option<Uuid>,
    last_focused_task_id_opt: &'a mut Option<Uuid>,
    focus_started_datetime: &'a mut DateTime<Local>,
}

struct PromptRenderState<'a> {
    header: &'a str,
    line: &'a str,
    cursor_x: usize,
}

fn render_interactive_screen(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focus_state: FocusRenderState,
    prompt_state: PromptRenderState,
    now: DateTime<Local>,
) {
    task_repository.sync_clock(now);

    write!(
        stdout,
        "{}{}",
        termion::clear::All,
        termion::cursor::Goto(1, 1)
    )
    .unwrap();

    execute_show_all_tasks(
        stdout,
        focus_state.focused_task_id_opt,
        task_repository,
        free_time_manager,
        &Some("暦".to_string()),
        TaskListDisplayOrder::ScheduledStartDesc,
    );
    render_focused_task(
        stdout,
        task_repository,
        *focus_state.focused_task_id_opt,
        focus_state.last_focused_task_id_opt,
        focus_state.focus_started_datetime,
        now,
    );
    render_prompt(
        stdout,
        prompt_state.header,
        prompt_state.line,
        prompt_state.cursor_x,
    );
}

fn should_suppress_leaf_tasks_after_command(line: &str) -> bool {
    matches!(
        line.chars().next(),
        Some('新')
            | Some('突')
            | Some('全')
            | Some('尾')
            | Some('今')
            | Some('明')
            | Some('近')
            | Some('週')
            | Some('末')
            | Some('翌')
            | Some('暦')
            | Some('帯')
            | Some('平')
            | Some('葉')
            | Some('樹')
            | Some('清')
    ) || line.split_whitespace().next() == Some("band")
}

#[test]
fn test_idle_refresh_deadline_現在時刻の60秒後を返す() {
    let now = Instant::now();

    assert_eq!(
        idle_refresh_deadline(now).duration_since(now),
        StdDuration::from_secs(60)
    );
}

#[test]
fn test_idle_wait_duration_期限までの残り時間を返す() {
    let now = Instant::now();
    let deadline = now + StdDuration::from_secs(15);

    assert_eq!(
        idle_wait_duration(deadline, now + StdDuration::from_secs(5)),
        StdDuration::from_secs(10)
    );
}

#[test]
fn test_idle_wait_duration_期限を過ぎていれば0秒を返す() {
    let now = Instant::now();
    let deadline = now + StdDuration::from_secs(15);

    assert_eq!(
        idle_wait_duration(deadline, now + StdDuration::from_secs(16)),
        StdDuration::ZERO
    );
}

#[test]
fn test_render_prompt_日本語入力中のカーソル位置を復元する() {
    let mut stdout = TestWriter::new();

    render_prompt(&mut stdout, "schronu> ", "あいう", 1);

    let actual = String::from_utf8(stdout.buffer).unwrap();
    let expected = format!(
        "{}{}schronu> あいう{}{}",
        termion::cursor::Left(MAX_COL),
        termion::clear::CurrentLine,
        termion::cursor::Left(MAX_COL),
        termion::cursor::Right(11),
    );
    assert_eq!(actual, expected);
}

#[test]
fn test_try_save_before_exit_保存成功なら終了可能にする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task_repository = TestTaskRepository::new(Task::new("保存対象"), now);
    let mut stdout = TestWriter::new();

    let actual = try_save_before_exit(&mut stdout, &task_repository);

    assert!(actual);
    assert_eq!(stdout.into_string(), "");
}

#[test]
fn test_try_save_before_exit_保存失敗ならerrorを表示して終了を止める() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("memoryに残すtask");
    let task_id = task.get_id();
    let task_repository = TestTaskRepository::new(task, now);
    task_repository.save_failures_remaining.set(1);
    let mut stdout = TestWriter::new();

    let actual = try_save_before_exit(&mut stdout, &task_repository);

    assert!(!actual);
    assert_eq!(
        task_repository.get_by_id(task_id).unwrap().get_name(),
        "memoryに残すtask"
    );
    let output = stdout.into_string();
    assert!(output.contains("[Error]"));
    assert!(output.contains("WriteFile"));
    assert!(output.contains("/test/project.yaml"));
    assert!(output.contains("test save failure"));
}

#[test]
fn test_handle_input_disconnected_保存を1回試して入力異常を返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for save_failures in [0, 1] {
        let task_repository = TestTaskRepository::new(Task::new("保存対象"), now);
        task_repository.save_failures_remaining.set(save_failures);

        let actual = handle_input_disconnected(&task_repository);

        assert!(matches!(
            &actual,
            RunError::InputDisconnected {
                save_error_opt
            } if save_error_opt.is_some() == (save_failures == 1)
        ));
        assert_eq!(task_repository.save_attempt_count.get(), 1);
        let message = actual.to_string();
        assert!(message.contains("interactive input channel disconnected"));
        if save_failures == 1 {
            assert!(message.contains("repository Save failed"));
            assert!(message.contains("test save failure"));
        }
    }
}

#[test]
fn test_handle_input_read_error_保存を1回試して両方のerrorを保持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for save_failures in [0, 1] {
        let task_repository = TestTaskRepository::new(Task::new("保存対象"), now);
        task_repository.save_failures_remaining.set(save_failures);

        let actual = handle_input_read_error(
            &task_repository,
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin read failure"),
        );

        assert!(matches!(
            &actual,
            RunError::InputRead {
                input_error,
                save_error_opt,
            } if input_error.kind() == std::io::ErrorKind::BrokenPipe
                && save_error_opt.is_some() == (save_failures == 1)
        ));
        assert_eq!(task_repository.save_attempt_count.get(), 1);
        let message = actual.to_string();
        assert!(message.contains("stdin read failure"));
        if save_failures == 1 {
            assert!(message.contains("repository Save failed"));
            assert!(message.contains("test save failure"));
        }
    }
}

#[test]
fn test_try_exit_interactive_保存失敗後の再試行で成功する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = Task::new("再試行中もmemoryに残すtask");
    let task_id = task.get_id();
    let mut task_repository = TestTaskRepository::new(task, now);
    task_repository.save_failures_remaining.set(1);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();
    let keys = [Key::Ctrl('d'), Key::Ctrl('d')];
    let mut exited = false;

    for key in keys {
        if key == Key::Ctrl('d')
            && try_exit_interactive(
                &mut stdout,
                &mut task_repository,
                &mut free_time_manager,
                &mut focused_task_id_opt,
                "schronu> ",
                "",
                0,
                now,
            )
        {
            exited = true;
            break;
        }
    }

    assert!(exited);
    assert_eq!(task_repository.save_attempt_count.get(), 2);
    assert_eq!(
        task_repository.get_by_id(task_id).unwrap().get_name(),
        "再試行中もmemoryに残すtask"
    );
    let output = stdout.into_string();
    assert_eq!(output.matches("[Error]").count(), 1);
    assert!(output.contains("schronu> "));
}

fn application(
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<(), RunError> {
    // 時計を合わせる
    let now = Local::now();
    task_repository.sync_clock(now);

    // let next_morning = get_next_morning_datetime(now)
    //     .with_hour(6)
    //     .expect("invalid hour")
    //     .with_minute(0)
    //     .expect("invalid minute");
    // task_repository.sync_clock(next_morning);

    task_repository.load()?;

    free_time_manager
        .load_busy_time_slots_from_file("../Schronu-private/busy_time_slots.yaml", &now);

    // RawModeを有効にする
    let mut stdout = stdout().into_raw_mode().unwrap();

    write!(stdout, "{}", termion::cursor::BlinkingBar).unwrap();
    stdout.flush().unwrap();

    // 起動直後はrhoの値を見たいので葉は出力しない
    // execute_show_leaf_tasks(&mut stdout, task_repository, free_time_manager);

    // 優先度の最も高いPJを一つ選ぶ
    // 一番下のタスクにフォーカスが自動的に当たる

    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;
    let mut focused_task_id_opt: Option<Uuid> =
        select_focus_task_id(task_repository, focus_selection_mode);

    let mut last_focused_task_id_opt: Option<Uuid> = None;
    let mut focus_started_datetime: DateTime<Local> = now;

    let header: &str = "schronu> ";
    let mut line = String::from("");

    // 画面に表示されている「文字」単位でのカーソル。
    let mut cursor_x: usize = 0;

    render_interactive_screen(
        &mut stdout,
        task_repository,
        free_time_manager,
        FocusRenderState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
        },
        PromptRenderState {
            header,
            line: &line,
            cursor_x,
        },
        now,
    );

    let (key_sender, key_receiver) = mpsc::channel();
    thread::spawn(move || {
        for key_result in std::io::stdin().keys() {
            if key_sender.send(key_result).is_err() {
                break;
            }
        }
    });

    let mut next_refresh_at = idle_refresh_deadline(Instant::now());
    let mut loop_error_opt = None;

    // キー入力を受け付け、無操作が60秒続いたら画面を再描画する
    loop {
        let wait_duration = idle_wait_duration(next_refresh_at, Instant::now());
        let key = match key_receiver.recv_timeout(wait_duration) {
            Ok(Ok(key)) => {
                next_refresh_at = idle_refresh_deadline(Instant::now());
                key
            }
            Ok(Err(input_error)) => {
                loop_error_opt = Some(handle_input_read_error(task_repository, input_error));
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                render_interactive_screen(
                    &mut stdout,
                    task_repository,
                    free_time_manager,
                    FocusRenderState {
                        focused_task_id_opt: &mut focused_task_id_opt,
                        last_focused_task_id_opt: &mut last_focused_task_id_opt,
                        focus_started_datetime: &mut focus_started_datetime,
                    },
                    PromptRenderState {
                        header,
                        line: &line,
                        cursor_x,
                    },
                    Local::now(),
                );
                next_refresh_at = idle_refresh_deadline(Instant::now());
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                loop_error_opt = Some(handle_input_disconnected(task_repository));
                break;
            }
        };

        match key {
            Key::Ctrl('d') => {
                if try_exit_interactive(
                    &mut stdout,
                    task_repository,
                    free_time_manager,
                    &mut focused_task_id_opt,
                    header,
                    &line,
                    cursor_x,
                    Local::now(),
                ) {
                    break;
                }
            }
            Key::Ctrl('c') => {
                // 保存せず、terminalを後始末してから異常終了する
                loop_error_opt = Some(RunError::Interrupted);
                break;
            }
            // Key::Up => write!(stdout, "{}", termion::cursor::Up(1)).unwrap(),
            // Key::Down => write!(stdout, "{}", termion::cursor::Down(1)).unwrap(),
            Key::Left | Key::Ctrl('b') => {
                let width = backward_width(&line, cursor_x);

                if width > 0 {
                    cursor_x -= 1;
                    write!(stdout, "{}", termion::cursor::Left(width)).unwrap();
                    stdout.flush().unwrap();
                }
            }
            Key::Right | Key::Ctrl('f') => {
                let width = get_forward_width(&line, cursor_x);

                if width > 0 {
                    cursor_x += 1;
                    write!(stdout, "{}", termion::cursor::Right(width)).unwrap();
                    stdout.flush().unwrap();
                }
            }
            Key::Ctrl('a') => {
                cursor_x = 0;

                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::clear::CurrentLine
                )
                .unwrap();

                let width = get_width_for_rerender(header, &line, cursor_x);
                write!(stdout, "{}{}", header, line).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::cursor::Right(width)
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            Key::Ctrl('e') => {
                loop {
                    let width = get_forward_width(&line, cursor_x);

                    if width == 0 {
                        break;
                    }
                    cursor_x += 1;
                    write!(stdout, "{}", termion::cursor::Right(width)).unwrap();
                }
                stdout.flush().unwrap();
            }
            Key::Ctrl('u') => {
                cursor_x = 0;
                line.clear();

                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::clear::CurrentLine,
                )
                .unwrap();

                let width = get_width_for_rerender(header, &line, cursor_x);
                write!(stdout, "{}{}", header, line).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::cursor::Right(width)
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            Key::Ctrl('k') => {
                // カーソルの位置を変えずに後ろをカットする
                line = line.chars().take(cursor_x).collect();

                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::clear::CurrentLine,
                )
                .unwrap();

                let width = get_width_for_rerender(header, &line, cursor_x);
                write!(stdout, "{}{}", header, line).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::cursor::Right(width)
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            Key::Backspace | Key::Ctrl('h') => {
                let byte_offset_opt = get_byte_offset_for_deletion(&line, cursor_x);
                if let Some(byte_offset) = byte_offset_opt {
                    line.remove(byte_offset);
                    cursor_x -= 1;
                }

                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::clear::CurrentLine,
                )
                .unwrap();

                let width = get_width_for_rerender(header, &line, cursor_x);
                write!(stdout, "{}{}", header, line).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::cursor::Right(width)
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            Key::Char('\n') | Key::Ctrl('m') => {
                // 時計を合わせる
                task_repository.sync_clock(Local::now());

                line = line.trim().to_string();

                writeln_newline(&mut stdout, "").unwrap();

                println!(
                    "{}{}> {}{}",
                    style::Bold,
                    &Local::now().format("%Y/%m/%d %H:%M:%S.%f").to_string(),
                    line,
                    style::Reset
                );
                writeln_newline(&mut stdout, "").unwrap();
                stdout.flush().unwrap();

                if let Some(new_focus_selection_mode) = parse_focus_selection_mode_command(&line) {
                    focus_selection_mode = new_focus_selection_mode;
                    focused_task_id_opt = None;
                    writeln_newline(
                        &mut stdout,
                        &format!("フォーカス選択モード: {}", focus_selection_mode.label()),
                    )
                    .unwrap();
                } else if line == "t" {
                    // do it "t"oday
                    let s = "後 1秒".to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                } else if line == "h" {
                    // skip an "h"our
                    let s = "後 1時間".to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                } else if line == "d" {
                    // skip "d"aily
                    let now: DateTime<Local> = task_repository.get_last_synced_time();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds() + 1;
                    let s = format!("後 {}秒", sec).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                } else if line == "D" {
                    // skip "D"aily (24h)
                    let sec = 24 * 60 * 60;
                    let s = format!("後 {}秒", sec).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                } else if line == "w" {
                    // skip "w"eekly
                    let now: DateTime<Local> = task_repository.get_last_synced_time();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds() + 86400 * 6 + 1;

                    let s = format!("後 {}秒", sec).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                } else if line == "W" {
                    execute_defer_routine(task_repository, &mut focused_task_id_opt);
                } else if line == "y" {
                    // skip "y"early
                    let now: DateTime<Local> = task_repository.get_last_synced_time();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds() + 86400 * (7 * 52 * 5 - 1) + 1;

                    let s = format!("後 {}秒", sec).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                } else {
                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &line,
                    );
                }

                // 時計を合わせる
                task_repository.sync_clock(Local::now());

                //////////////////////////////

                // もしfocused_task_id_optがNoneの時は最も優先度が高いタスクの選出をやり直す

                if focused_task_id_opt.is_none() {
                    focused_task_id_opt =
                        select_focus_task_id(task_repository, focus_selection_mode);
                    last_focused_task_id_opt = None;
                }

                //////////////////////////////

                // スクロールするのが面倒なので、新や突のように付加情報を表示するコマンドの直後は葉を表示しない
                // Todo: "new" や  "unplanned" の場合にも対応する
                if !should_suppress_leaf_tasks_after_command(&line) {
                    execute_show_leaf_tasks(&mut stdout, task_repository, free_time_manager);
                }

                render_focused_task(
                    &mut stdout,
                    task_repository,
                    focused_task_id_opt,
                    &mut last_focused_task_id_opt,
                    &mut focus_started_datetime,
                    Local::now(),
                );

                //////////////////////////////

                // 初期化
                cursor_x = 0;
                line.clear();
                render_prompt(&mut stdout, header, &line, cursor_x);
            }
            Key::Char(c) => {
                // 多バイト文字の挿入位置を知る
                let byte_offset = get_byte_offset_for_insert(&line, cursor_x);
                line.insert(byte_offset, c);

                cursor_x += 1;
                write!(stdout, "{}", c).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::clear::CurrentLine
                )
                .unwrap();

                let width = get_width_for_rerender(header, &line, cursor_x);
                write!(stdout, "{}{}", header, line).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::cursor::Right(width)
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            _key => {
                // write!(stdout, "{:?}", x).unwrap();
                // stdout.flush().unwrap();

                // キー入力をリアルタイムで反映させる
                // write!(stdout, "{}", termion::clear::CurrentLine).unwrap();
                // write!(stdout, "{}", termion::cursor::Left(999)).unwrap();
                // stdout.flush().unwrap();
                // write!(stdout, "{}", line).unwrap();
                // stdout.flush().unwrap();
            }
        }
    }

    write!(stdout, "{}", termion::clear::CurrentLine).unwrap();
    println!("{}{}{}", style::Bold, line, style::Reset);

    // SteadyBlockに戻す
    // Todo: 本当は、元々の状態を保存しておいてそれに戻したい。
    writeln!(stdout, "{}", termion::cursor::SteadyBlock).unwrap();
    match loop_error_opt {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
