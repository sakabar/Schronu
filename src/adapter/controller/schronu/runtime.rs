#![allow(unused_must_use)]

use super::command::{
    parse_command, Command, CommandAction, CommandKind, CommandParseError, ParseMode,
};
#[cfg(test)]
use super::handler::{decide_finish_time_values, decide_time_values, write_pack_result};
use super::handler::{
    handle, handle_breakdown_split_command, handle_defer_command, handle_finish_placement_command,
    handle_project_command, handle_task_attribute_command, handle_task_tree_command,
    CommandOutcome, DeferCommandContext, DeferCommandError, ExternalRequest,
    FinishPlacementCommandContext, FocusRequest, ProjectCommandContext,
    TaskAttributeCommandContext, TaskListOrder, TaskTreeCommandContext,
};
use super::interactive;
#[cfg(test)]
use super::interactive::{
    backward_width, get_byte_offset_for_deletion, get_byte_offset_for_insert, get_forward_width,
    get_width_for_rerender, idle_refresh_deadline, idle_wait_duration,
};
use super::renderer::{
    format_spreadsheet_task_row, render_display_model, render_plain_display_model, writeln_newline,
    DisplayModel, ErrorCapturingWriter, SchronuWriter, SpreadsheetTaskRow,
};
use chrono::{
    DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday,
};
#[cfg(test)]
use chrono::{FixedOffset, Timelike};
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};
use regex::Regex;
use schronu::adapter::gateway::free_time_manager::FreeTimeManager;
use schronu::adapter::gateway::schronu_config::{load_schronu_config, SchronuConfig};
use schronu::adapter::gateway::storage_lock::{LockMode, StorageLock, StorageLockError};
use schronu::adapter::gateway::task_repository::TaskRepository;
#[cfg(test)]
use schronu::application::daily_capacity::subjective_date_start;
use schronu::application::daily_capacity::{
    calculate_daily_rho_diff_hours,
    calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes,
    calculate_full_day_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes,
    try_local_date_and_time, try_next_business_day_start, try_subjective_date,
    try_subjective_date_end, try_subjective_date_start, RHO_GOAL,
};
use schronu::application::flatten_use_case::{
    flatten_tasks_with_end_of_day_offset_minutes, FlattenResult,
};
use schronu::application::interface::{BusyTimeSlotLoadError, FreeTimeManagerTrait};
#[cfg(test)]
use schronu::application::interface::{
    BusyTimeSlotRegistrationError, RepositoryReloadOutcome, TaskRepositoryOperation,
};
use schronu::application::interface::{TaskRepositoryError, TaskRepositoryTrait};
use schronu::application::pack_use_case::{pack_tasks_with_end_of_day_offset_minutes, PackResult};
use schronu::application::repository_transaction::{
    run_repository_transaction, RepositoryTransactionError,
};
use schronu::application::schedule_use_case::get_schedule;
use schronu::application::task_use_case::{
    breakdown_task, complete_task, create_task, defer_task, estimated_work_seconds_from_minutes,
    get_focus, set_category, set_deadline, set_estimate, validate_task_name, ApplicationError,
    BreakdownTaskInput, CompleteTaskInput, CreateTaskInput, TaskFactory,
};
use schronu::entity::datetime::{get_next_morning_datetime, parse_local_datetime};
use schronu::entity::task::{
    extract_leaf_tasks_from_project, extract_leaf_tasks_from_project_with_pending,
    read_project_category, round_up_sec_as_minute, ProjectCategory, Status, TaskAttr, TaskHandle,
    TaskTreeError,
};
#[cfg(test)]
use std::cell::{Cell, RefCell};
use std::cmp::{max, min};
use std::collections::{HashMap, HashSet};
use std::env;
use std::io::{stdout, Write};
#[cfg(test)]
use std::path::PathBuf;
use std::process;
use std::sync::OnceLock;

#[path = "../storage_directory.rs"]
mod storage_directory;
use std::time::Duration as StdDuration;
#[cfg(test)]
use std::time::Instant;
use storage_directory::resolve_project_storage_directory;
use termion::color;
use termion::style;
use unicode_width::UnicodeWidthChar;
use url::Url;
use uuid::Uuid;

const MAX_ARRANGE_ESTIMATED_WORK_MINUTES: i64 = 1439;
#[cfg(test)]
const DEFAULT_LOWEST_PRIORITY_RECENT_DAYS: i64 = 0;
const FOCUS_PROGRESS_BAR_SEGMENTS: usize = 100;
const CLI_LOCK_TIMEOUT: StdDuration = StdDuration::from_secs(1);

static ACTIVE_CONFIG: OnceLock<SchronuConfig> = OnceLock::new();

#[cfg(test)]
trait TaskHandleTestExt {
    fn create_as_last_child(&self, task_attr: TaskAttr) -> TaskHandle;
}

#[cfg(test)]
impl TaskHandleTestExt for TaskHandle {
    fn create_as_last_child(&self, task_attr: TaskAttr) -> TaskHandle {
        self.create_child(task_attr)
            .expect("test hierarchy child creation must succeed")
    }
}

#[cfg(test)]
fn next_test_task_id() -> Uuid {
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    Uuid::from_u128(u128::from(SEQUENCE.fetch_add(1, Ordering::Relaxed)))
}

#[cfg(test)]
fn test_task_time() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 8, 19, 0, 0, 0).unwrap()
}

#[cfg(test)]
fn maximum_local_datetime() -> DateTime<Local> {
    DateTime::<Local>::from_naive_utc_and_offset(
        NaiveDate::MAX.and_hms_opt(12, 0, 0).unwrap(),
        FixedOffset::east_opt(0).unwrap(),
    )
}

#[cfg(test)]
fn new_test_task_attr(name: &str) -> TaskAttr {
    TaskAttr::with_identity(name, next_test_task_id(), test_task_time())
}

#[cfg(test)]
fn new_test_task_handle(name: &str) -> Result<TaskHandle, TaskTreeError> {
    TaskHandle::with_identity(name, next_test_task_id(), test_task_time())
}

fn active_config() -> &'static SchronuConfig {
    ACTIVE_CONFIG.get_or_init(SchronuConfig::default)
}

// パーセントエンコーディングする対象にスペースを追加する
const MY_ASCII_SET: &AsciiSet = &CONTROLS.add(b' ');
const OBSIDIAN_VAULT_ASCII_SET: &AsciiSet = &MY_ASCII_SET.add(b'&').add(b'=');

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FocusSelectionMode {
    HighestPriority,
    LowestPriority { recent_days: i64 },
    Explicit,
}

#[derive(Debug)]
enum RunError {
    Command(CommandError),
    BusyTimeSlots(BusyTimeSlotLoadError),
    Repository(TaskRepositoryError),
    CliRepositoryTransaction(CliRepositoryTransactionError),
    InputDisconnected {
        save_error_opt: Option<TaskRepositoryError>,
    },
    InputRead {
        input_error: std::io::Error,
        save_error_opt: Option<TaskRepositoryError>,
    },
    InputDisconnectedWithRepository {
        repository_error: CliRepositoryTransactionError,
    },
    InputReadWithRepository {
        input_error: std::io::Error,
        repository_error: CliRepositoryTransactionError,
    },
    Interrupted,
}

#[derive(Debug)]
enum CommandError {
    Parse(CommandParseError),
    Application(ApplicationError),
    Output(std::io::Error),
    ExternalOpen {
        target: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::Application(error) => write!(formatter, "操作エラー: {error}"),
            Self::Output(error) => write!(formatter, "出力エラー: {error}"),
            Self::ExternalOpen { target, source } => {
                write!(formatter, "外部起動エラー ({target}): {source}")
            }
        }
    }
}

impl std::error::Error for CommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Application(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::ExternalOpen { source, .. } => Some(source.as_ref()),
        }
    }
}

fn external_open_error(
    target: &'static str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> CommandError {
    CommandError::ExternalOpen {
        target,
        source: Box::new(source),
    }
}

fn validate_non_interactive_command(command: &Command) -> Result<(), CommandError> {
    match command {
        Command::Estimate { minutes } => {
            estimated_work_seconds_from_minutes(*minutes)?;
            Ok(())
        }
        Command::Action(CommandAction::StringValue {
            kind: CommandKind::Category,
            value,
            ..
        }) => {
            read_project_category_command_arg(value).ok_or_else(|| {
                command_parse_error("類", "category", "カテゴリが不正です", "類 <カテゴリ>")
            })?;
            Ok(())
        }
        Command::Action(CommandAction::StringValue {
            kind: CommandKind::Deadline,
            value,
            ..
        }) => {
            if value.starts_with('今')
                || value.starts_with('明')
                || matches!(
                    value.as_str(),
                    "消" | "月" | "火" | "水" | "木" | "金" | "土" | "日"
                )
                || Regex::new(r"^\d{1,2}/\d{1,2}$")
                    .expect("valid regex")
                    .is_match(value)
                || Regex::new(r"^\d{1,2}:\d{1,2}$")
                    .expect("valid regex")
                    .is_match(value)
                || parse_local_datetime(&format!("{} 23:59:59", value), "%Y/%m/%d %H:%M:%S").is_ok()
            {
                Ok(())
            } else {
                Err(command_parse_error(
                    "〆",
                    "deadline",
                    "日時が不正です",
                    "〆 <日付または時刻>",
                ))
            }
        }
        _ => Ok(()),
    }
}

fn validate_contextual_task_attribute_command(
    command: &Command,
    now: DateTime<Local>,
    config: &SchronuConfig,
) -> Result<(), CommandError> {
    if let Command::Action(CommandAction::StringValue {
        kind: CommandKind::Deadline,
        value,
        ..
    }) = command
    {
        resolve_deadline_time(value, now, config)?;
    }
    Ok(())
}

impl From<ApplicationError> for CommandError {
    fn from(error: ApplicationError) -> Self {
        Self::Application(error)
    }
}

fn command_parse_error(
    command: &'static str,
    field: &'static str,
    reason: &'static str,
    usage: &'static str,
) -> CommandError {
    CommandError::Parse(CommandParseError::new(command, field, reason, usage))
}

fn map_command_parse_error(error: CommandParseError) -> CommandError {
    command_parse_error(
        error.command(),
        error.field(),
        error.reason(),
        error.usage(),
    )
}

fn error_display_model(error: &impl std::fmt::Display) -> DisplayModel {
    DisplayModel::newline(format!("[Error] {error}"))
}

#[cfg(test)]
fn report_command_result(stdout: &mut dyn SchronuWriter, result: Result<(), CommandError>) {
    if let Err(error) = result {
        let _output_error = render_display_model(stdout, &error_display_model(&error))
            .map_err(CommandError::Output);
    }
}

fn report_application_result<T>(
    stdout: &mut dyn SchronuWriter,
    result: Result<T, ApplicationError>,
) {
    if let Err(error) = result {
        let error = CommandError::Application(error);
        let _output_error = render_display_model(stdout, &error_display_model(&error))
            .map_err(CommandError::Output);
    }
}

#[derive(Debug)]
enum CliRepositoryTransactionError {
    Lock(StorageLockError),
    Load(TaskRepositoryError),
    Save(TaskRepositoryError),
}

impl From<TaskRepositoryError> for RunError {
    fn from(error: TaskRepositoryError) -> Self {
        Self::Repository(error)
    }
}
impl From<CommandError> for RunError {
    fn from(error: CommandError) -> Self {
        Self::Command(error)
    }
}
impl From<BusyTimeSlotLoadError> for RunError {
    fn from(error: BusyTimeSlotLoadError) -> Self {
        Self::BusyTimeSlots(error)
    }
}

impl From<CliRepositoryTransactionError> for RunError {
    fn from(error: CliRepositoryTransactionError) -> Self {
        match error {
            CliRepositoryTransactionError::Load(error) => Self::Repository(error),
            error => Self::CliRepositoryTransaction(error),
        }
    }
}

impl std::fmt::Display for CliRepositoryTransactionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lock(error) => write!(formatter, "CLI repository Lock failed: {error}"),
            Self::Load(error) => write!(formatter, "CLI repository Load failed: {error}"),
            Self::Save(error) => write!(formatter, "CLI repository Save failed: {error}"),
        }
    }
}

impl std::error::Error for CliRepositoryTransactionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lock(error) => Some(error),
            Self::Load(error) | Self::Save(error) => Some(error),
        }
    }
}

impl std::fmt::Display for RunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command(error) => error.fmt(formatter),
            Self::BusyTimeSlots(error) => error.fmt(formatter),
            Self::Repository(error) => error.fmt(formatter),
            Self::CliRepositoryTransaction(error) => error.fmt(formatter),
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
            Self::InputDisconnectedWithRepository { repository_error } => write!(
                formatter,
                "interactive input channel disconnected; additionally, {repository_error}"
            ),
            Self::InputReadWithRepository {
                input_error,
                repository_error,
            } => write!(
                formatter,
                "failed to read interactive input: {input_error}; additionally, {repository_error}"
            ),
            Self::Interrupted => write!(formatter, "interactive input interrupted"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Command(error) => Some(error),
            Self::BusyTimeSlots(error) => Some(error),
            Self::Repository(error) => Some(error),
            Self::CliRepositoryTransaction(error) => Some(error),
            Self::InputDisconnected { save_error_opt } => save_error_opt
                .as_ref()
                .map(|error| error as &(dyn std::error::Error + 'static)),
            Self::InputRead { input_error, .. } => Some(input_error),
            Self::InputDisconnectedWithRepository { repository_error } => Some(repository_error),
            Self::InputReadWithRepository { input_error, .. } => Some(input_error),
            Self::Interrupted => None,
        }
    }
}

fn get_weekday_jp(date: &NaiveDate) -> &str {
    get_weekday_jp_from_weekday(date.weekday())
}

fn get_weekday_jp_from_weekday(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "月",
        Weekday::Tue => "火",
        Weekday::Wed => "水",
        Weekday::Thu => "木",
        Weekday::Fri => "金",
        Weekday::Sat => "土",
        Weekday::Sun => "日",
    }
}

fn resolve_upcoming_mmdd(
    mmdd: &str,
    now: DateTime<Local>,
) -> Result<Option<DateTime<Local>>, ApplicationError> {
    let mmdd_reg = Regex::new(r"^(\d{1,2})/(\d{1,2})$").unwrap();
    let Some(caps) = mmdd_reg.captures(mmdd) else {
        return Ok(None);
    };
    let (Some(month), Some(day)) = (caps[1].parse::<u32>().ok(), caps[2].parse::<u32>().ok())
    else {
        return Ok(None);
    };

    let validation_year = 2000 + now.year().rem_euclid(400);
    if NaiveDate::from_ymd_opt(validation_year, month, day).is_none() {
        return Ok(None);
    }
    let out_of_range = || ApplicationError::SubjectiveDateOutOfRange {
        operation: "upcoming_calendar_date",
        datetime: now,
    };
    let current_year_date =
        NaiveDate::from_ymd_opt(now.year(), month, day).ok_or_else(out_of_range)?;
    let current_year_start = try_subjective_date_start(current_year_date)?;
    if current_year_start >= now {
        return Ok(Some(current_year_start));
    }

    let next_year = now.year().checked_add(1).ok_or_else(out_of_range)?;
    let next_validation_year = 2000 + next_year.rem_euclid(400);
    if NaiveDate::from_ymd_opt(next_validation_year, month, day).is_none() {
        return Ok(None);
    }
    let next_year_date = NaiveDate::from_ymd_opt(next_year, month, day).ok_or_else(out_of_range)?;
    Ok(Some(try_subjective_date_start(next_year_date)?))
}

fn resolve_upcoming_clear_or_gather_day(
    date: &str,
    now: DateTime<Local>,
) -> Result<Option<DateTime<Local>>, ApplicationError> {
    if date == "明" {
        return Ok(Some(try_next_business_day_start(now)?));
    }

    let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];
    if let Some(target_days_of_week_ind) = days_of_week.iter().position(|day| *day == date) {
        let subjective_date = try_subjective_date(now)?;
        let now_days_of_week_ind = days_of_week
            .iter()
            .position(|day| *day == get_weekday_jp(&subjective_date))
            .expect("subjective weekday must be in the Japanese weekday table");
        let days_until_target =
            (7 + target_days_of_week_ind - now_days_of_week_ind) % days_of_week.len();
        let days = if days_until_target == 0 {
            7
        } else {
            days_until_target
        };

        let target_date = subjective_date
            .checked_add_signed(Duration::days(days as i64))
            .ok_or(ApplicationError::SubjectiveDateOutOfRange {
                operation: "weekday_date",
                datetime: now,
            })?;
        let target_datetime = try_subjective_date_start(target_date)?;
        return Ok(Some(target_datetime));
    }

    resolve_upcoming_mmdd(date, now)
}

fn resolve_show_all_pattern(
    pattern: &str,
    now: DateTime<Local>,
) -> Result<String, ApplicationError> {
    Ok(match resolve_upcoming_mmdd(pattern, now)? {
        Some(datetime) => datetime.format("%Y/%m/%d").to_string(),
        None => pattern.to_string(),
    })
}

#[test]
fn test_resolve_upcoming_mmdd_未来の日付は現在年を使う() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let target_date = Local.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap();
    let expected = get_next_morning_datetime(target_date) - Duration::days(1);

    assert_eq!(resolve_upcoming_mmdd("9/26", now), Ok(Some(expected)));
}

#[test]
fn test_resolve_upcoming_mmdd_過去の日付は翌年を使う() {
    let now = Local.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
    let target_date = Local.with_ymd_and_hms(2027, 9, 26, 12, 0, 0).unwrap();
    let expected = get_next_morning_datetime(target_date) - Duration::days(1);

    assert_eq!(resolve_upcoming_mmdd("09/26", now), Ok(Some(expected)));
}

#[test]
fn test_resolve_upcoming_mmdd_当日の境界時刻は現在年を使う() {
    let target_date = Local.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap();
    let now = get_next_morning_datetime(target_date) - Duration::days(1);

    assert_eq!(resolve_upcoming_mmdd("9/26", now), Ok(Some(now)));
}

#[test]
fn test_resolve_upcoming_clear_or_gather_day_明は次の業務日を返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    assert_eq!(
        resolve_upcoming_clear_or_gather_day("明", now),
        Ok(Some(Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap()))
    );
}

#[test]
fn test_resolve_upcoming_clear_or_gather_day_曜日は明日以降で最も近い日を返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    for (weekday, day) in [
        ("月", 17),
        ("火", 18),
        ("水", 19),
        ("木", 20),
        ("金", 21),
        ("土", 15),
        ("日", 16),
    ] {
        assert_eq!(
            resolve_upcoming_clear_or_gather_day(weekday, now),
            Ok(Some(Local.with_ymd_and_hms(2026, 8, day, 6, 0, 0).unwrap()))
        );
    }
}

#[test]
fn test_resolve_upcoming_clear_or_gather_day_午前6時前の明と不正値を扱う() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 2, 0, 0).unwrap();

    assert_eq!(
        resolve_upcoming_clear_or_gather_day("明", now),
        Ok(Some(Local.with_ymd_and_hms(2026, 8, 14, 6, 0, 0).unwrap()))
    );
    assert_eq!(resolve_upcoming_clear_or_gather_day("翌", now), Ok(None));
}

#[test]
fn test_resolve_upcoming_clear_or_gather_day_業務日計算不能を情報付きerrorにする() {
    let now = maximum_local_datetime();

    assert_eq!(
        resolve_upcoming_clear_or_gather_day("明", now),
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime: now,
        })
    );
}

#[test]
fn test_resolve_upcoming_clear_or_gather_day_曜日範囲外は曜日計算errorにする() {
    let now = maximum_local_datetime();

    assert_eq!(
        resolve_upcoming_clear_or_gather_day("月", now),
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "weekday_date",
            datetime: now,
        })
    );
}

#[test]
fn test_resolve_upcoming_clear_or_gather_day_mmddの翌年計算不能を情報付きerrorにする() {
    let now = maximum_local_datetime();

    assert_eq!(
        resolve_upcoming_clear_or_gather_day("12/31", now),
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "upcoming_calendar_date",
            datetime: now,
        })
    );
}

#[test]
fn test_resolve_show_all_pattern_年なし日付を完全日付へ変換する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    assert_eq!(
        resolve_show_all_pattern("9/26", now),
        Ok("2026/09/26".to_string())
    );
}

#[test]
fn test_resolve_show_all_pattern_過ぎた日付は翌年へ変換する() {
    let now = Local.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();

    assert_eq!(
        resolve_show_all_pattern("9/26", now),
        Ok("2027/09/26".to_string())
    );
}

#[test]
fn test_resolve_show_all_pattern_完全日付と検索語は変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    assert_eq!(
        resolve_show_all_pattern("2026/09/26", now),
        Ok("2026/09/26".to_string())
    );
    assert_eq!(
        resolve_show_all_pattern("タスク", now),
        Ok("タスク".to_string())
    );
}

#[test]
fn test_show_task_list_mmddの日時errorを伝搬して表示と状態を変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("show all日時範囲外対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut display = TestWriter::new();
    let mut next_id = || Uuid::nil();
    let mut task_factory = TaskFactory::new(now, &mut next_id);
    let mut context = RuntimeTaskTreeCommandContext {
        task_repository: &mut task_repository,
        free_time_manager: &mut free_time_manager,
        focused_task_id_opt: &mut focused_task_id_opt,
        task_factory: &mut task_factory,
        config: active_config(),
        supports_ansi_color: false,
    };

    let actual = context.show_task_list(
        &mut display,
        Some("12/31"),
        TaskListOrder::ScheduledStartDesc,
        true,
    );

    assert!(matches!(
        actual,
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "upcoming_calendar_date",
            datetime,
        }) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(display.into_string().is_empty());
}

fn get_adjustable_prefix_label(
    task: &TaskHandle,
    dt: DateTime<Local>,
    rank: usize,
    last_synced_time: DateTime<Local>,
) -> Result<String, ApplicationError> {
    if rank != 0
        || task
            .get_is_on_other_side()
            .map_err(ApplicationError::TaskTree)?
    {
        return Ok("".to_string());
    }

    let planned_date = try_subjective_date(dt)?;
    let available_datetime = max(
        task.get_start_time().map_err(ApplicationError::TaskTree)?,
        last_synced_time,
    );
    let available_date = try_subjective_date(available_datetime)?;
    let advance_days = (planned_date - available_date).num_days();

    if advance_days > 0 {
        Ok(format!("【前{}】", advance_days))
    } else {
        Ok("".to_string())
    }
}

fn parse_clear_or_gather_defer_to_datetime(
    cmd_str: &str,
    arg: &str,
    now: DateTime<Local>,
) -> Result<Option<DateTime<Local>>, ApplicationError> {
    let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();
    if let Some(caps) = hhmm_reg.captures(arg) {
        let (Some(hour), Some(minute)) = (caps[1].parse::<u32>().ok(), caps[2].parse::<u32>().ok())
        else {
            return Ok(None);
        };
        let Some(calendar_time) = NaiveTime::from_hms_opt(hour % 24, minute, 0) else {
            return Ok(None);
        };
        let subjective_date = try_subjective_date(now)?;
        let target_date = subjective_date
            .checked_add_signed(Duration::days(i64::from(hour / 24)))
            .ok_or(ApplicationError::SubjectiveDateOutOfRange {
                operation: "clear_or_gather_time",
                datetime: now,
            })?;
        return Ok(Some(try_local_date_and_time(target_date, calendar_time)?));
    }

    let integer_reg = Regex::new(r"^\d+$").unwrap();
    if matches!(cmd_str, "空" | "clear" | "集" | "gather") && integer_reg.is_match(arg) {
        let Some(minutes) = arg.parse::<i64>().ok() else {
            return Ok(None);
        };
        let defer_to_datetime = Duration::try_minutes(minutes)
            .and_then(|duration| now.checked_add_signed(duration))
            .ok_or(ApplicationError::SubjectiveDateOutOfRange {
                operation: "clear_or_gather_minutes",
                datetime: now,
            })?;
        return Ok(Some(defer_to_datetime));
    }

    Ok(None)
}

type ClearOrGatherTimeRange = (DateTime<Local>, DateTime<Local>);

fn resolve_dated_clear_or_gather_end_naive(
    schronu_day_start: NaiveDateTime,
    hour: i64,
    minute: u32,
) -> Option<NaiveDateTime> {
    let calendar_hour = u32::try_from(hour % 24).ok()?;
    let calendar_time = NaiveTime::from_hms_opt(calendar_hour, minute, 0)?;
    let calendar_duration = Duration::try_days(hour / 24)?;
    let mut target_date = schronu_day_start
        .date()
        .checked_add_signed(calendar_duration)?;
    let mut target = target_date.and_time(calendar_time);
    if target < schronu_day_start {
        target_date = target_date.checked_add_signed(Duration::days(1))?;
        target = target_date.and_time(calendar_time);
    }
    Some(target)
}

fn parse_dated_clear_or_gather_time_range(
    time: &str,
    mmdd: &str,
    now: DateTime<Local>,
) -> Result<Option<ClearOrGatherTimeRange>, ApplicationError> {
    let Some(schronu_day_start) = resolve_upcoming_clear_or_gather_day(mmdd, now)? else {
        return Ok(None);
    };
    let hhmm_reg = Regex::new(r"^(\d+):(\d{1,2})$").unwrap();
    let Some(caps) = hhmm_reg.captures(time) else {
        return Ok(None);
    };
    let (Some(hour), Some(minute)) = (caps[1].parse::<i64>().ok(), caps[2].parse::<u32>().ok())
    else {
        return Ok(None);
    };
    let Some(end_naive) =
        resolve_dated_clear_or_gather_end_naive(schronu_day_start.naive_local(), hour, minute)
    else {
        return Ok(None);
    };
    let end = try_local_date_and_time(end_naive.date(), end_naive.time())?;

    Ok((schronu_day_start < end).then_some((schronu_day_start, end)))
}

fn scheduled_leaf_starts_on_schronu_day(
    task_repository: &dyn TaskRepositoryTrait,
    schronu_day_start: DateTime<Local>,
) -> Result<HashMap<Uuid, Vec<DateTime<Local>>>, ApplicationError> {
    let mut leaf_task_ids = HashSet::new();
    for project in task_repository.get_all_projects() {
        for task in extract_leaf_tasks_from_project_with_pending(project)
            .map_err(ApplicationError::TaskTree)?
        {
            leaf_task_ids.insert(task.get_id().map_err(ApplicationError::TaskTree)?);
        }
    }

    let mut starts = HashMap::new();
    for scheduled in get_schedule(task_repository)? {
        if !leaf_task_ids.contains(&scheduled.task.id) {
            continue;
        }
        let scheduled_day_start =
            try_subjective_date_start(try_subjective_date(scheduled.scheduled_start)?)?;
        if scheduled_day_start != schronu_day_start {
            continue;
        }
        starts
            .entry(scheduled.task.id)
            .or_insert_with(Vec::new)
            .push(scheduled.scheduled_start);
    }
    Ok(starts)
}

fn execute_clear_or_gather(
    task_repository: &mut dyn TaskRepositoryTrait,
    kind: CommandKind,
    values: &[String],
) -> Result<(), ApplicationError> {
    match values {
        [defer_to] => {
            let canonical_name = match kind {
                CommandKind::Clear => "空",
                CommandKind::Gather => "集",
                _ => return Ok(()),
            };
            let Some(defer_to_datetime) = parse_clear_or_gather_defer_to_datetime(
                canonical_name,
                defer_to,
                task_repository.get_last_synced_time(),
            )?
            else {
                return Ok(());
            };
            for project_root_task in task_repository.get_all_projects() {
                for leaf_task in extract_leaf_tasks_from_project_with_pending(project_root_task)
                    .map_err(ApplicationError::TaskTree)?
                {
                    let start_time = leaf_task
                        .get_start_time()
                        .map_err(ApplicationError::TaskTree)?;
                    let orig_status = leaf_task
                        .get_orig_status()
                        .map_err(ApplicationError::TaskTree)?;
                    let pending_until = leaf_task
                        .get_pending_until()
                        .map_err(ApplicationError::TaskTree)?;
                    match kind {
                        CommandKind::Clear
                            if start_time < defer_to_datetime
                                && (orig_status == Status::Todo
                                    || (orig_status == Status::Pending
                                        && pending_until < defer_to_datetime)) =>
                        {
                            leaf_task
                                .set_orig_status(Status::Pending)
                                .map_err(ApplicationError::TaskTree)?;
                            leaf_task
                                .set_pending_until(defer_to_datetime)
                                .map_err(ApplicationError::TaskTree)?;
                        }
                        CommandKind::Gather
                            if leaf_task.get_status().map_err(ApplicationError::TaskTree)?
                                == Status::Pending
                                && start_time < defer_to_datetime
                                && pending_until < defer_to_datetime =>
                        {
                            leaf_task
                                .set_orig_status(Status::Todo)
                                .map_err(ApplicationError::TaskTree)?;
                        }
                        _ => {}
                    }
                }
            }
        }
        [time, mmdd] => {
            let Some((schronu_day_start, end)) = parse_dated_clear_or_gather_time_range(
                time,
                mmdd,
                task_repository.get_last_synced_time(),
            )?
            else {
                return Ok(());
            };
            let scheduled_starts =
                scheduled_leaf_starts_on_schronu_day(task_repository, schronu_day_start)?;

            for project_root_task in task_repository.get_all_projects() {
                for leaf_task in extract_leaf_tasks_from_project_with_pending(project_root_task)
                    .map_err(ApplicationError::TaskTree)?
                {
                    let scheduled_starts_opt = scheduled_starts
                        .get(&leaf_task.get_id().map_err(ApplicationError::TaskTree)?);
                    let orig_status = leaf_task
                        .get_orig_status()
                        .map_err(ApplicationError::TaskTree)?;
                    let pending_until = leaf_task
                        .get_pending_until()
                        .map_err(ApplicationError::TaskTree)?;
                    match kind {
                        CommandKind::Clear => {
                            let todo_is_scheduled_in_range = orig_status == Status::Todo
                                && scheduled_starts_opt.is_some_and(|starts| {
                                    starts.iter().any(|scheduled_start| {
                                        schronu_day_start <= *scheduled_start
                                            && *scheduled_start < end
                                    })
                                });
                            let pending_is_in_range = orig_status == Status::Pending
                                && scheduled_starts_opt.is_some()
                                && schronu_day_start <= pending_until
                                && pending_until < end;
                            if todo_is_scheduled_in_range || pending_is_in_range {
                                leaf_task
                                    .set_orig_status(Status::Pending)
                                    .map_err(ApplicationError::TaskTree)?;
                                leaf_task
                                    .set_pending_until(end)
                                    .map_err(ApplicationError::TaskTree)?;
                            }
                        }
                        CommandKind::Gather
                            if orig_status == Status::Pending
                                && scheduled_starts_opt.is_some()
                                && pending_until <= end =>
                        {
                            leaf_task
                                .set_pending_until(schronu_day_start)
                                .map_err(ApplicationError::TaskTree)?;
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
fn focus_selection_mode_from_command(command: &Command) -> Option<FocusSelectionMode> {
    handle(command)
        .and_then(|outcome| outcome.focus_request)
        .map(focus_selection_mode_from_request)
}

fn focus_selection_mode_from_request(request: FocusRequest) -> FocusSelectionMode {
    match request {
        FocusRequest::Clear => unreachable!("clear focus does not select a focus mode"),
        FocusRequest::HighestPriority => FocusSelectionMode::HighestPriority,
        FocusRequest::LowestPriority { recent_days } => {
            FocusSelectionMode::LowestPriority { recent_days }
        }
    }
}

fn select_focus_task_id(
    task_repository: &mut dyn TaskRepositoryTrait,
    focus_selection_mode: FocusSelectionMode,
) -> Result<Option<Uuid>, ApplicationError> {
    match focus_selection_mode {
        FocusSelectionMode::HighestPriority | FocusSelectionMode::Explicit => {
            Ok(get_focus(task_repository)?.map(|task| task.id))
        }
        FocusSelectionMode::LowestPriority { recent_days } => task_repository
            .get_defer_candidate_leaf_task_id(recent_days)
            .map_err(ApplicationError::TaskTree),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_focus_selection_mode_command(line: &str) -> Option<FocusSelectionMode> {
        parse_command(line, ParseMode::Interactive)
            .ok()
            .and_then(|command| focus_selection_mode_from_command(&command))
    }

    #[test]
    fn test_get_adjustable_prefix_label_前倒し可能日数を表示する() {
        let task = new_test_task_handle("タスク").unwrap();
        task.set_start_time(Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap());
        let dt = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

        let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time).unwrap();

        assert_eq!(actual, "【前3】");
    }

    #[test]
    fn test_get_adjustable_prefix_label_今日より前には戻さない() {
        let task = new_test_task_handle("タスク").unwrap();
        task.set_start_time(Local.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap());
        let dt = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

        let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time).unwrap();

        assert_eq!(actual, "【前3】");
    }

    #[test]
    fn test_get_adjustable_prefix_label_同日着手可能なら表示しない() {
        let task = new_test_task_handle("タスク").unwrap();
        task.set_start_time(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap());
        let dt = Local.with_ymd_and_hms(2026, 5, 10, 18, 0, 0).unwrap();
        let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

        let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time).unwrap();

        assert_eq!(actual, "");
    }

    #[test]
    fn test_get_adjustable_prefix_label_今日と予定日が同じなら過去の着手可能日は表示しない() {
        let task = new_test_task_handle("タスク").unwrap();
        task.set_start_time(Local.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap());
        let dt = Local.with_ymd_and_hms(2026, 5, 7, 18, 0, 0).unwrap();
        let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

        let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time).unwrap();

        assert_eq!(actual, "");
    }

    #[test]
    fn test_get_adjustable_prefix_label_相手待ちは表示しない() {
        let task = new_test_task_handle("タスク").unwrap();
        task.set_start_time(Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap());
        task.set_is_on_other_side(true);
        let dt = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

        let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time).unwrap();

        assert_eq!(actual, "");
    }

    #[test]
    fn test_get_adjustable_prefix_label_葉以外は表示しない() {
        let task = new_test_task_handle("タスク").unwrap();
        task.set_start_time(Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap());
        let dt = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

        let actual = get_adjustable_prefix_label(&task, dt, 1, last_synced_time).unwrap();

        assert_eq!(actual, "");
    }

    #[test]
    fn test_parse_clear_or_gather_defer_to_datetime_空の分指定は現在時刻からの分として解釈する() {
        let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

        let actual = parse_clear_or_gather_defer_to_datetime("空", "120", now);

        assert_eq!(actual, Ok(Some(now + Duration::minutes(120))));
    }

    #[test]
    fn test_parse_clear_or_gather_defer_to_datetime_hhmm指定は従来通り当日の時刻として解釈する() {
        let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

        let actual = parse_clear_or_gather_defer_to_datetime("空", "10:00", now);

        assert_eq!(
            actual,
            Ok(Some(Local.with_ymd_and_hms(2026, 5, 7, 10, 0, 0).unwrap()))
        );
    }

    #[test]
    fn test_parse_clear_or_gather_defer_to_datetime_集の分指定は現在時刻からの分として解釈する() {
        let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

        let actual = parse_clear_or_gather_defer_to_datetime("集", "120", now);

        assert_eq!(actual, Ok(Some(now + Duration::minutes(120))));
    }

    #[test]
    fn test_parse_clear_or_gather_defer_to_datetime_不正なcalendar時刻を拒否する() {
        let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

        assert_eq!(
            parse_clear_or_gather_defer_to_datetime("空", "13:99", now),
            Ok(None)
        );
    }

    #[test]
    fn test_parse_clear_or_gather_defer_to_datetime_i64範囲外のminutesを拒否する() {
        let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

        assert_eq!(
            parse_clear_or_gather_defer_to_datetime("空", "9223372036854775808", now),
            Ok(None)
        );
    }

    #[test]
    fn test_parse_clear_or_gather_defer_to_datetime_minutesの日時範囲外を情報付きerrorにする() {
        let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

        assert_eq!(
            parse_clear_or_gather_defer_to_datetime("空", "9223372036854775807", now),
            Err(ApplicationError::SubjectiveDateOutOfRange {
                operation: "clear_or_gather_minutes",
                datetime: now,
            })
        );
    }

    #[test]
    fn test_parse_dated_clear_or_gather_time_range_深夜と24時以降を指定業務日へ対応付ける() {
        let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
        let start = Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap();

        assert_eq!(
            parse_dated_clear_or_gather_time_range("03:00", "8/15", now),
            Ok(Some((
                start,
                Local.with_ymd_and_hms(2026, 8, 16, 3, 0, 0).unwrap()
            )))
        );
        assert_eq!(
            parse_dated_clear_or_gather_time_range("24:30", "8/15", now),
            Ok(Some((
                start,
                Local.with_ymd_and_hms(2026, 8, 16, 0, 30, 0).unwrap()
            )))
        );
    }

    #[test]
    fn test_resolve_dated_clear_or_gather_end_naive_最終壁時計日付を変換前に確定する() {
        let day_start = NaiveDate::from_ymd_opt(2026, 3, 28)
            .unwrap()
            .and_hms_opt(6, 0, 0)
            .unwrap();

        assert_eq!(
            resolve_dated_clear_or_gather_end_naive(day_start, 24, 30),
            NaiveDate::from_ymd_opt(2026, 3, 29)
                .unwrap()
                .and_hms_opt(0, 30, 0)
        );
        assert_eq!(
            resolve_dated_clear_or_gather_end_naive(day_start, 3, 0),
            NaiveDate::from_ymd_opt(2026, 3, 29)
                .unwrap()
                .and_hms_opt(3, 0, 0)
        );
    }

    #[test]
    fn test_parse_dated_clear_or_gather_time_range_不正値と空区間を拒否する() {
        let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

        for time in ["120", "06:00", "10:60", "invalid", "9223372036854775807:00"] {
            assert_eq!(
                parse_dated_clear_or_gather_time_range(time, "8/15", now),
                Ok(None)
            );
        }
        assert_eq!(
            parse_dated_clear_or_gather_time_range("13:00", "13/40", now),
            Ok(None)
        );
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
        let task = new_test_task_handle("タスク").unwrap();
        let focused_task_opt = Some(task.clone());

        execute_set_priority(&focused_task_opt, "8");

        assert_eq!(task.get_priority().unwrap(), 8);
    }

    #[test]
    fn test_execute_set_priority_不正値なら変更しない() {
        let task = new_test_task_handle("タスク").unwrap();
        task.set_priority(5);
        let focused_task_opt = Some(task.clone());

        execute_set_priority(&focused_task_opt, "invalid");

        assert_eq!(task.get_priority().unwrap(), 5);
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
            "0028 task-id / ____/__/__ 06/28(日)-23:11~23:30 0 19 05 資 夕食  の 準備".to_string();

        let actual = replace_task_list_icon(&message_prefix, "A");

        assert_eq!(
            actual,
            "0028 task-id A ____/__/__ 06/28(日)-23:11~23:30 0 19 05 資 夕食  の 準備"
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

fn format_daily_band_segment(symbol: char, count: usize, supports_ansi_color: bool) -> String {
    if count == 0 {
        return String::new();
    }
    if !supports_ansi_color {
        return symbol.to_string().repeat(count);
    }
    let color_value = match symbol {
        '#' => 110,
        'x' => 244,
        '=' => 33,
        '-' => 208,
        ':' => 28,
        '.' => 34,
        '>' => 196,
        _ => return symbol.to_string().repeat(count),
    };
    let symbols = symbol.to_string().repeat(count);
    format!(
        "{}{}{}",
        color::Fg(color::AnsiValue(color_value)),
        symbols,
        color::Fg(color::Reset)
    )
}

fn format_daily_band_legend(supports_ansi_color: bool) -> String {
    format!(
        "凡例: {} 固定  {} 経過済み  {} 繰返  {} 単発  {} 余差  {} 空き  {} 超過  (1文字=15分)",
        format_daily_band_segment('#', 1, supports_ansi_color),
        format_daily_band_segment('x', 1, supports_ansi_color),
        format_daily_band_segment('=', 1, supports_ansi_color),
        format_daily_band_segment('-', 1, supports_ansi_color),
        format_daily_band_segment(':', 1, supports_ansi_color),
        format_daily_band_segment('.', 1, supports_ansi_color),
        format_daily_band_segment('>', 1, supports_ansi_color),
    )
}

fn format_daily_band(
    date: NaiveDate,
    weekday_jp: &str,
    accumulated_free_diff: Duration,
    accumulated_rho_diff: Duration,
    durations: &DailyBandDurations,
    supports_ansi_color: bool,
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
        bar.push_str(&format_daily_band_segment(
            symbol,
            boundary - previous_boundary,
            supports_ansi_color,
        ));
        previous_boundary = boundary;
    }

    let overflow = format_daily_band_segment(
        '>',
        round_daily_band_segment_count(overflow_seconds),
        supports_ansi_color,
    );
    format!(
        "{}({}) {} {} [{}]{}",
        date,
        weekday_jp,
        format_signed_hours_minutes(accumulated_rho_diff),
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
    #[cfg(test)]
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

    #[allow(clippy::too_many_arguments)]
    fn new_spreadsheet_task(
        scheduled_start: DateTime<Local>,
        subjective_naive_date: NaiveDate,
        rank: usize,
        id: Uuid,
        priority: i64,
        work_seconds: i64,
        project_category_opt: Option<ProjectCategory>,
        message: String,
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
            message_prefix: message,
            task_name: String::new(),
            message: String::new(),
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
    let mut token_ranges = message_prefix
        .char_indices()
        .filter(|(_, character)| !character.is_whitespace())
        .map(|(index, _)| index)
        .scan(None, |previous_index, index| {
            let starts_token = previous_index.is_none_or(|previous_index| {
                message_prefix[previous_index..index]
                    .chars()
                    .any(char::is_whitespace)
            });
            *previous_index = Some(index);
            Some(starts_token.then_some(index))
        })
        .flatten();
    let Some(icon_start) = token_ranges.nth(2) else {
        return message_prefix.to_string();
    };
    let icon_end = message_prefix[icon_start..]
        .find(char::is_whitespace)
        .map_or(message_prefix.len(), |offset| icon_start + offset);

    format!(
        "{}{}{}",
        &message_prefix[..icon_start],
        icon,
        &message_prefix[icon_end..]
    )
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

fn calculate_project_category_denominator_seconds(
    rows: &[TaskListDisplayRow],
    last_synced_time: DateTime<Local>,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    end_of_day_offset_minutes: i64,
) -> Result<i64, ApplicationError> {
    let mut dates = rows
        .iter()
        .filter(|row| row.is_real_task)
        .filter_map(|row| row.subjective_naive_date_opt)
        .collect::<Vec<_>>();
    dates.sort();
    dates.dedup();

    dates.iter().try_fold(0, |total, date| {
        calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
            date,
            last_synced_time,
            free_time_manager,
            end_of_day_offset_minutes,
        )
        .map(|minutes| total + minutes * 60)
    })
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

    dates_and_shortages.sort_by_key(|a| a.0);

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

#[test]
fn test_get_forward_width_正常系1() {
    let line = String::from("あ");
    let cursor_x = 0;

    let actual = get_forward_width(&line, cursor_x);
    let expected = 2;
    assert_eq!(actual, expected);
}

fn execute_show_tree(
    stdout: &mut dyn SchronuWriter,
    focused_task_opt: &Option<TaskHandle>,
) -> Result<(), ApplicationError> {
    writeln!(stdout).unwrap();
    if let Some(focused_task) = focused_task_opt.as_ref() {
        let s = focused_task
            .tree_debug_pretty_print()
            .map_err(ApplicationError::TaskTree)?;
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
    Ok(())
}

fn execute_show_ancestor(
    stdout: &mut dyn SchronuWriter,
    focused_task_opt: &Option<TaskHandle>,
) -> Result<(), ApplicationError> {
    writeln!(stdout).unwrap();

    // まずは葉タスクから根に向かいながら後ろに追加していき、
    // 最後に逆順にして表示する
    let mut ancestors: Vec<(DateTime<Local>, TaskHandle)> = vec![];

    if let Some(task) = focused_task_opt {
        ancestors = task
            .list_all_parent_tasks_with_first_available_time()
            .map_err(ApplicationError::TaskTree)?;
    }

    ancestors.reverse();

    for (level, (first_available_datetime, task)) in ancestors.iter().enumerate() {
        let header = if level == 0 {
            String::from("")
        } else {
            let indent = ' '.to_string().repeat(4 * (level - 1));
            format!("{}`-- ", indent)
        };

        let id = task.get_id().map_err(ApplicationError::TaskTree)?;
        let name = task.get_name().map_err(ApplicationError::TaskTree)?;
        let estimated_work_minutes = (task
            .get_estimated_work_seconds()
            .map_err(ApplicationError::TaskTree)? as f64
            / 60.0)
            .ceil() as i64;
        let first_available_date_str = first_available_datetime.format("%Y/%m/%d").to_string();

        let msg = format!(
            "{}{} [{}] {}m {}",
            header, id, first_available_date_str, estimated_work_minutes, name
        );
        writeln_newline(stdout, &msg).unwrap();
    }

    writeln_newline(stdout, "").unwrap();
    Ok(())
}

fn execute_show_leaf_tasks(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    _free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<(), ApplicationError> {
    let mut ans_tpls = vec![];

    for project_root_task in task_repository.get_all_projects().iter() {
        let project_name = project_root_task
            .get_name()
            .map_err(ApplicationError::TaskTree)?;

        // 優先度が高いタスクほど下に表示されるようにし、フォーカスが当たるタスクは末尾に表示されるようにする。
        let leaf_tasks = extract_leaf_tasks_from_project(project_root_task)
            .map_err(ApplicationError::TaskTree)?;
        for leaf_task in leaf_tasks.iter() {
            let deadline_time_opt = leaf_task
                .get_deadline_time_opt()
                .map_err(ApplicationError::TaskTree)?;
            let neg_priority = !leaf_task
                .get_priority()
                .map_err(ApplicationError::TaskTree)?;
            let id = leaf_task.get_id().map_err(ApplicationError::TaskTree)?;
            let message = format!(
                "{}\t{:?}",
                project_name,
                leaf_task.get_attr().map_err(ApplicationError::TaskTree)?
            );

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
    Ok(())
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
) -> Result<(), ApplicationError> {
    execute_show_all_tasks_with_config(
        stdout,
        focused_task_id_opt,
        task_repository,
        free_time_manager,
        pattern_opt,
        display_order,
        active_config(),
    )
}

#[allow(clippy::type_complexity)]
fn execute_show_all_tasks_with_config(
    stdout: &mut dyn SchronuWriter,
    focused_task_id_opt: &mut Option<Uuid>,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    pattern_opt: &Option<String>,
    display_order: TaskListDisplayOrder,
    config: &SchronuConfig,
) -> Result<(), ApplicationError> {
    let supports_ansi_color = stdout.supports_ansi_color();
    let scheduled_tasks = get_schedule(task_repository)?;
    let mut task_list_display_rows: Vec<TaskListDisplayRow> = vec![];
    let mut available_biggest_row_opt: Option<TaskListDisplayRow> = None;
    let mut available_biggest_task_estimate_work_seconds = 0;

    // ここからρ計算用
    let last_synced_time = task_repository.get_last_synced_time();

    let eod = try_subjective_date_end(
        try_subjective_date(last_synced_time)?,
        config.end_of_day_offset_minutes,
    )?;
    // ここまでρ計算用

    let is_calendar_func = pattern_opt
        .as_ref()
        .is_some_and(|pattern| pattern == "暦" || pattern == "calendar" || pattern == "cal");

    let is_band_func = pattern_opt
        .as_ref()
        .is_some_and(|pattern| pattern == "帯" || pattern == "band");

    let is_daily_summary_func = is_calendar_func || is_band_func;

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

        let task_opt = task_repository
            .get_by_id(*id)
            .map_err(ApplicationError::TaskTree)?;
        if let Some(task) = task_opt {
            let inherited_repetition_interval_days_opt = task
                .get_inherited_repetition_interval_days_opt()
                .map_err(ApplicationError::TaskTree)?;
            let mut repetition_prefix_label = "".to_string();

            if let Some(repetition_interval_days) = inherited_repetition_interval_days_opt {
                // FIXME 【繰】というマジックナンバーが2ヶ所に登場していて危ない
                repetition_prefix_label = format!(
                    "{}【繰】({})",
                    repetition_prefix_label, repetition_interval_days
                );
            }

            if task
                .get_is_on_other_side()
                .map_err(ApplicationError::TaskTree)?
            {
                repetition_prefix_label = format!("{}【待ち】", repetition_prefix_label);
            }

            // 前倒し可能なタスクの見積もり時間をカウントする
            let adjustable_prefix_label =
                get_adjustable_prefix_label(&task, *dt, *rank, last_synced_time)?;
            let task_estimated_work_seconds = task
                .get_estimated_work_seconds()
                .map_err(ApplicationError::TaskTree)?;
            let task_deadline_time_opt = task
                .get_deadline_time_opt()
                .map_err(ApplicationError::TaskTree)?;
            let task_priority = task.get_priority().map_err(ApplicationError::TaskTree)?;
            let task_project_category_opt = task
                .get_project_category_opt()
                .map_err(ApplicationError::TaskTree)?;

            if !adjustable_prefix_label.is_empty() {
                adjustable_estimated_work_seconds_map
                    .entry(subjective_naive_date)
                    .and_modify(|estimated_work_seconds_val| {
                        *estimated_work_seconds_val += task_estimated_work_seconds
                    })
                    .or_insert(task_estimated_work_seconds);
            }

            let name = format!(
                "{}{}{}",
                adjustable_prefix_label,
                repetition_prefix_label,
                task.get_name().map_err(ApplicationError::TaskTree)?
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
            if (*scheduled_start - *current_datetime_cursor_clone).num_minutes() > 0 {
                let blank_duration = *scheduled_start - *current_datetime_cursor_clone;
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

            let icon = if task_deadline_time_opt.is_some()
                && task_deadline_time_opt.unwrap() < get_next_morning_datetime(last_synced_time)
                && task_deadline_time_opt.unwrap() < end_datetime
            {
                &breaking_deadline_icon
            } else if task_deadline_time_opt.is_some()
                && task_deadline_time_opt.unwrap() < get_next_morning_datetime(last_synced_time)
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

            let spreadsheet_rank = format!("{ind:04}");
            let spreadsheet_task_id = id.to_string();
            let spreadsheet_scheduled_time = format!(
                "{}({})-{}~{}",
                start_datetime.format("%m/%d"),
                get_weekday_jp(&start_datetime.date_naive()),
                start_datetime.format("%H:%M"),
                end_datetime.format("%H:%M"),
            );
            let spreadsheet_priority = rank.to_string();
            let spreadsheet_estimated_minutes =
                format!("{:02.0}", round_up_sec_as_minute(estimated_work_seconds));
            let spreadsheet_project_number = format!("{task_priority:02}");
            let message = format_spreadsheet_task_row(&SpreadsheetTaskRow {
                rank: &spreadsheet_rank,
                task_id: &spreadsheet_task_id,
                icon,
                remaining_time: &deadline_string,
                scheduled_time: &spreadsheet_scheduled_time,
                priority: &spreadsheet_priority,
                estimated_minutes: &spreadsheet_estimated_minutes,
                project_number: &spreadsheet_project_number,
                category: project_category_symbol(task_project_category_opt),
                task_name: &shorten_name,
            });
            let task_list_display_row = TaskListDisplayRow::new_spreadsheet_task(
                *scheduled_start,
                subjective_naive_date,
                *rank,
                *id,
                task_priority,
                estimated_work_seconds,
                task_project_category_opt,
                message,
            );
            let msg = &task_list_display_row.message_prefix;
            let has_deadline_icon = icon == deadline_icon || icon == breaking_deadline_icon;
            let has_task_list_icon = has_deadline_icon || icon == today_leaf_icon;

            match pattern_opt {
                Some(pattern) => {
                    // FIXME 文字列マッチの絞り込み機能とその他の属性による絞り込みを機能を分ける
                    if pattern == "葉" {
                        if rank == &0
                            || task_deadline_time_opt.is_some()
                                && task_deadline_time_opt.unwrap()
                                    < get_next_morning_datetime(last_synced_time)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "枝" {
                        if rank > &0 {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "印" {
                        if has_task_list_icon {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "〆" {
                        if has_deadline_icon {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if is_daily_summary_func {
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
                            || last_synced_time
                                < task.get_start_time().map_err(ApplicationError::TaskTree)?
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

        let free_time_minutes =
            calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
                date,
                last_synced_time,
                free_time_manager,
                config.end_of_day_offset_minutes,
            )?;
        let full_day_free_time_minutes_opt = if is_band_func {
            Some(
                calculate_full_day_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
                    date,
                    free_time_manager,
                    config.end_of_day_offset_minutes,
                )?,
            )
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

        let diff_to_goal = calculate_daily_rho_diff_hours(
            free_time_minutes,
            total_repetitive_task_work_seconds_of_the_date,
            total_estimated_work_seconds_of_the_date,
        );
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
                    accumulate_duration_diff_to_goal_rho,
                    &calculate_daily_band_durations(
                        **date == naive_dt_today,
                        full_minutes,
                        free_time_minutes,
                        total_estimated_work_seconds_of_the_date,
                        total_repetitive_task_work_seconds_of_the_date,
                        diff_to_goal,
                    ),
                    supports_ansi_color,
                )
            });

        daily_summary_rows.push(DailySummaryRow {
            date: **date,
            calendar_message: s,
            band_message,
        });
    }

    if !is_daily_summary_func {
        mark_give_up_candidate_rows_by_date(
            &mut task_list_display_rows,
            &shortage_duration_by_date,
        );
    }

    sort_task_list_display_rows(&mut task_list_display_rows, display_order);

    if !is_daily_summary_func {
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
            free_time_manager,
            config.end_of_day_offset_minutes,
        )?;
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

    let write_daily_summary = |stdout: &mut dyn SchronuWriter| {
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
    };

    if is_calendar_func {
        for (cal_ind, row) in daily_summary_rows.iter().enumerate() {
            writeln_newline(stdout, &row.calendar_message).unwrap();

            if row.calendar_message.contains(&format!(
                "({})",
                get_weekday_jp_from_weekday(config.calendar_blank_line_weekday)
            )) && cal_ind != daily_summary_rows.len() - 1
            {
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
        write_daily_summary(stdout);
    } else if is_band_func {
        writeln_newline(stdout, &format_daily_band_legend(supports_ansi_color)).unwrap();
        writeln_newline(stdout, "").unwrap();

        for (band_ind, row) in daily_summary_rows.iter().enumerate() {
            writeln_newline(stdout, &row.band_message).unwrap();

            if row.date.weekday() == Weekday::Mon && band_ind != daily_summary_rows.len() - 1 {
                writeln_newline(stdout, "").unwrap();
            }
        }
        writeln_newline(stdout, "").unwrap();
        write_daily_summary(stdout);
    }

    if is_calendar_func || is_band_func {
        writeln_newline(stdout, &busy_s).unwrap();
        writeln_newline(stdout, &s).unwrap();
        writeln_newline(stdout, &s_for_rho1).unwrap();
        writeln_newline(stdout, &s_for_non_repetitive_rho).unwrap();
    }

    writeln_newline(stdout, "").unwrap();
    Ok(())
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
fn execute_open_link(focused_task_opt: &Option<TaskHandle>) -> Result<(), CommandError> {
    let mut t_opt: Option<TaskHandle> = focused_task_opt.clone();

    while let Some(t) = &t_opt {
        if let Some(url) = extract_url(&t.get_name().map_err(ApplicationError::TaskTree)?) {
            webbrowser::open(&url).map_err(|source| external_open_error("browser", source))?;
            return Ok(());
        }

        t_opt = t.parent().map_err(ApplicationError::TaskTree)?;
    }
    Ok(())
}

#[cfg(test)]
fn make_obsidian_search_url(query: &str) -> String {
    make_obsidian_search_url_with_vault(query, &active_config().obsidian_vault_name)
}

fn make_obsidian_search_url_with_vault(query: &str, vault_name: &str) -> String {
    format!(
        "obsidian://search?vault={}&query={}",
        percent_encode(vault_name.as_bytes(), OBSIDIAN_VAULT_ASCII_SET),
        percent_encode(query.as_bytes(), MY_ASCII_SET)
    )
}

#[cfg(test)]
fn make_obsidian_root_task_search_url(focused_task: &TaskHandle) -> String {
    make_obsidian_root_task_search_url_with_vault(
        focused_task,
        &active_config().obsidian_vault_name,
    )
    .expect("fixture root task must be readable")
}

fn make_obsidian_root_task_search_url_with_vault(
    focused_task: &TaskHandle,
    vault_name: &str,
) -> Result<String, ApplicationError> {
    let root_task_id = focused_task
        .root()
        .and_then(|root| root.get_id())
        .map_err(ApplicationError::TaskTree)?;
    Ok(make_obsidian_search_url_with_vault(
        &root_task_id.hyphenated().to_string(),
        vault_name,
    ))
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

fn execute_open_obsidian_root_task_search_with_config(
    focused_task_opt: &Option<TaskHandle>,
    config: &SchronuConfig,
) -> Result<(), CommandError> {
    if let Some(focused_task) = focused_task_opt {
        let url = make_obsidian_root_task_search_url_with_vault(
            focused_task,
            &config.obsidian_vault_name,
        )?;
        open_obsidian_url(&url)
            .map_err(|source| external_open_error("Obsidian", std::io::Error::other(source)))?;
    }
    Ok(())
}

#[test]
fn test_make_obsidian_search_url_task_idをqueryにする() {
    let query = "11111111-1111-1111-1111-111111111111";
    let actual = make_obsidian_search_url(query);
    let expected =
        "obsidian://search?vault=Obsidian-Work&query=11111111-1111-1111-1111-111111111111";

    assert_eq!(actual, expected);
}

#[test]
fn test_make_obsidian_search_url_vault名をpercent_encodeする() {
    let actual = make_obsidian_search_url_with_vault("task id", "Work & Personal");

    assert_eq!(
        actual,
        "obsidian://search?vault=Work%20%26%20Personal&query=task%20id"
    );
}

#[test]
fn test_make_obsidian_root_task_search_url_子タスクからrootのtask_idをqueryにする() {
    let mut root_task = new_test_task_handle("root").unwrap();
    let root_task_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    root_task.set_id(root_task_id);
    let child_task = root_task.create_as_last_child(new_test_task_attr("child"));

    let actual = make_obsidian_root_task_search_url(&child_task);
    let expected =
        "obsidian://search?vault=Obsidian-Work&query=11111111-1111-1111-1111-111111111111";

    assert_eq!(actual, expected);
}

#[allow(unused_must_use)]
fn execute_next_up(
    _stdout: &mut dyn SchronuWriter,
    focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<TaskHandle>,
    new_task_name_str: &str,
    estimated_work_minutes_opt: &Option<i64>,
    task_factory: &mut TaskFactory<'_>,
) -> Result<Option<Uuid>, ApplicationError> {
    validate_task_name(new_task_name_str, "name")?;
    let estimated_work_seconds_opt = estimated_work_minutes_opt
        .map(estimated_work_seconds_from_minutes)
        .transpose()?;

    let Some(mut focused_task) = focused_task_opt.clone() else {
        return Ok(None);
    };
    let parent_task = focused_task
        .parent()
        .map_err(ApplicationError::TaskTree)?
        .ok_or(ApplicationError::TaskTree(TaskTreeError::RootOperation))?;

    // 親タスクの〆切を引き継ぐ
    let parent_deadline_time_opt = parent_task
        .get_deadline_time_opt()
        .map_err(ApplicationError::TaskTree)?;
    let parent_estimated_work_seconds_opt = estimated_work_seconds_opt
        .map(|new_task_estimated_work_seconds| {
            parent_task
                .get_estimated_work_seconds()
                .map(|parent_task_estimated_work_seconds| {
                    (parent_task_estimated_work_seconds - new_task_estimated_work_seconds).max(0)
                })
                .map_err(ApplicationError::TaskTree)
        })
        .transpose()?;

    let mut new_task_attr = task_factory.create_task_attr(new_task_name_str);
    new_task_attr.set_deadline_time_opt(parent_deadline_time_opt);

    if let Some(new_task_estimated_work_seconds) = estimated_work_seconds_opt {
        new_task_attr.set_estimated_work_seconds(new_task_estimated_work_seconds);
    }

    let new_task_id = *new_task_attr.get_id();
    focused_task
        .create_parent(new_task_attr)
        .map_err(ApplicationError::TaskTree)?;

    if let Some(parent_estimated_work_seconds) = parent_estimated_work_seconds_opt {
        parent_task
            .set_estimated_work_seconds(parent_estimated_work_seconds)
            .map_err(ApplicationError::TaskTree)?;
    }

    *focused_task_id_opt = Some(new_task_id);
    Ok(Some(new_task_id))
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

fn execute_defer(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    amount: i64,
    unit_str: &str,
) -> Result<(), ApplicationError> {
    let now = task_repository.get_last_synced_time();
    let duration_out_of_range = || ApplicationError::SubjectiveDateOutOfRange {
        operation: "defer_pending_until",
        datetime: now,
    };
    let duration = match unit_str.chars().next() {
        // 24時間単位ではなく、next_monring単位とする
        Some('日') | Some('d') => {
            let target = defer_business_day_target(now, amount)?;
            target - now
        }
        Some('時') | Some('h') => Duration::try_hours(amount).ok_or_else(duration_out_of_range)?,
        Some('分') | Some('m') => {
            Duration::try_minutes(amount).ok_or_else(duration_out_of_range)?
        }
        // 誤入力した時に傷が浅いように、デフォルトは秒としておく
        _ => Duration::try_seconds(amount).ok_or_else(duration_out_of_range)?,
    };

    if let Some(task_id) = *focused_task_id_opt {
        let pending_until = now
            .checked_add_signed(duration)
            .ok_or_else(duration_out_of_range)?;
        defer_task(task_repository, task_id, pending_until)?;
    }

    *focused_task_id_opt = None;
    Ok(())
}

fn defer_business_day_target(
    now: DateTime<Local>,
    amount: i64,
) -> Result<DateTime<Local>, ApplicationError> {
    if amount <= 0 {
        return Ok(now);
    }

    let first_business_day_start = try_next_business_day_start(now)?;
    let out_of_range = || ApplicationError::SubjectiveDateOutOfRange {
        operation: "defer_business_days",
        datetime: now,
    };
    let additional_days = amount.checked_sub(1).ok_or_else(out_of_range)?;
    let additional_duration = Duration::try_days(additional_days).ok_or_else(out_of_range)?;
    let target_date = first_business_day_start
        .date_naive()
        .checked_add_signed(additional_duration)
        .ok_or_else(out_of_range)?;
    try_subjective_date_start(target_date)
}

fn seconds_until_next_business_day_start_with_offset(
    now: DateTime<Local>,
    offset_seconds: i64,
) -> Result<i64, ApplicationError> {
    let next_business_day_start = try_next_business_day_start(now)?;
    (next_business_day_start - now)
        .num_seconds()
        .checked_add(offset_seconds)
        .ok_or(ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime: now,
        })
}

fn execute_defer_expression(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    values: &[String],
) -> Result<(), DeferCommandError> {
    match values {
        [amount, unit, ..] => {
            let amount = amount.parse::<i64>().map_err(|_| {
                DeferCommandError::Parse(CommandParseError::new(
                    "後",
                    "amount",
                    "整数で指定してください",
                    "後 <数値> <単位>",
                ))
            })?;
            execute_defer(
                task_repository,
                focused_task_id_opt,
                amount,
                &unit.to_lowercase(),
            )
            .map_err(DeferCommandError::from)
        }
        [value] => {
            let yyyymmdd_reg = Regex::new(r"^\d{4}/\d{2}/\d{2}$").unwrap();
            let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();
            let now = task_repository.get_last_synced_time();
            let seconds = if yyyymmdd_reg.is_match(value) {
                match NaiveDate::parse_from_str(value, "%Y/%m/%d") {
                    Ok(date) => {
                        let defer_dst_time = try_subjective_date_start(date)?;
                        Some((defer_dst_time - now).num_seconds().checked_add(1).ok_or(
                            ApplicationError::SubjectiveDateOutOfRange {
                                operation: "defer_date",
                                datetime: now,
                            },
                        )?)
                    }
                    Err(_) => None,
                }
            } else if let Some(defer_dst_time) = resolve_upcoming_mmdd(value, now)? {
                let seconds = (defer_dst_time - now).num_seconds() + 1;
                (seconds > 0).then_some(seconds)
            } else if let Some(captures) = hhmm_reg.captures(value) {
                let (Some(hour), Some(minute)) = (
                    captures[1].parse::<i64>().ok(),
                    captures[2].parse::<u32>().ok(),
                ) else {
                    return Ok(());
                };
                let Some(calendar_hour) = u32::try_from(hour % 24).ok() else {
                    return Ok(());
                };
                let Some(calendar_time) = NaiveTime::from_hms_opt(calendar_hour, minute, 0) else {
                    return Ok(());
                };
                let out_of_range = || ApplicationError::SubjectiveDateOutOfRange {
                    operation: "defer_time",
                    datetime: now,
                };
                let day_offset = Duration::try_days(hour / 24).ok_or_else(out_of_range)?;
                let target_date = now
                    .date_naive()
                    .checked_add_signed(day_offset)
                    .ok_or_else(out_of_range)?;
                let defer_dst_time = try_local_date_and_time(target_date, calendar_time)?;
                let seconds = (defer_dst_time - now)
                    .num_seconds()
                    .checked_add(1)
                    .ok_or_else(out_of_range)?;
                (seconds > 0).then_some(seconds)
            } else if ["月", "火", "水", "木", "金", "土", "日"].contains(&value.as_str()) {
                let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];
                let today = try_subjective_date(now)?;
                let current_index = days_of_week
                    .iter()
                    .position(|day| *day == get_weekday_jp(&today))
                    .unwrap();
                let target_index = days_of_week.iter().position(|day| *day == value).unwrap();
                let difference = (7 + target_index - current_index) % 7;
                let days = if difference == 0 {
                    7
                } else {
                    difference as i64
                };
                let offset_seconds = days
                    .checked_sub(1)
                    .and_then(|days| days.checked_mul(86_400))
                    .and_then(|seconds| seconds.checked_add(1))
                    .ok_or(ApplicationError::SubjectiveDateOutOfRange {
                        operation: "next_business_day_start",
                        datetime: now,
                    })?;
                Some(seconds_until_next_business_day_start_with_offset(
                    now,
                    offset_seconds,
                )?)
            } else {
                let split = split_amount_and_unit(value);
                if split.len() == 2 && !split[0].is_empty() {
                    split[0]
                        .parse::<i64>()
                        .ok()
                        .map(|amount| (amount, split[1].clone()))
                        .map_or(Ok(()), |(amount, unit)| {
                            execute_defer(
                                task_repository,
                                focused_task_id_opt,
                                amount,
                                &unit.to_lowercase(),
                            )
                        })?;
                }
                return Ok(());
            };

            if let Some(seconds) = seconds {
                execute_defer(task_repository, focused_task_id_opt, seconds, "秒")?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// 指定の日付から、step_days間隔でdeferしていく
fn execute_extrude_with_config(
    _focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<TaskHandle>,
    first_datetime: &DateTime<Local>,
    step_days: u16,
    config: &SchronuConfig,
) -> Result<(), ApplicationError> {
    if let Some(focused_task) = focused_task_opt {
        let mut pending_until_datetime = *first_datetime;

        for (_, task) in focused_task
            .list_all_parent_tasks_with_first_available_time()
            .map_err(ApplicationError::TaskTree)?
        {
            if focused_task
                .get_status()
                .map_err(ApplicationError::TaskTree)?
                != Status::Done
            {
                task.set_orig_status(Status::Pending)
                    .map_err(ApplicationError::TaskTree)?;
                task.set_pending_until(pending_until_datetime)
                    .map_err(ApplicationError::TaskTree)?;

                pending_until_datetime += Duration::days(step_days as i64);
                while config
                    .extrude_skip_weekdays
                    .contains(&pending_until_datetime.weekday())
                {
                    pending_until_datetime += Duration::days(1);
                }
            }
        }
    }
    Ok(())
}

// 〆切をrepetition_interval_daysのぶん伸ばし、pendingにする
// start_timeも伸ばすが、時刻は元のstart_timeを維持する
fn execute_defer_routine(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
) -> Result<(), ApplicationError> {
    let Some(focused_task_id) = *focused_task_id_opt else {
        return Ok(());
    };
    let Some(focused_task) = task_repository
        .get_by_id(focused_task_id)
        .map_err(ApplicationError::TaskTree)?
    else {
        return Ok(());
    };
    let Some(orig_deadline_time) = focused_task
        .get_deadline_time_opt()
        .map_err(ApplicationError::TaskTree)?
    else {
        return Ok(());
    };
    let Some(parent_task) = focused_task.parent().map_err(ApplicationError::TaskTree)? else {
        return Ok(());
    };
    let Some(repetition_interval_days) = parent_task
        .get_repetition_interval_days_opt()
        .map_err(ApplicationError::TaskTree)?
    else {
        return Ok(());
    };
    let parent_deadline_time_opt = parent_task
        .get_deadline_time_opt()
        .map_err(ApplicationError::TaskTree)?;
    let orig_start_time = focused_task
        .get_start_time()
        .map_err(ApplicationError::TaskTree)?;

    let deadline_out_of_range = || ApplicationError::SubjectiveDateOutOfRange {
        operation: "defer_routine_deadline",
        datetime: orig_deadline_time,
    };
    let new_deadline_time = if let Some(parent_deadline_time) = parent_deadline_time_opt {
        let first_business_day_start = try_next_business_day_start(orig_deadline_time)?;
        let additional_days = repetition_interval_days
            .checked_sub(1)
            .ok_or_else(deadline_out_of_range)?;
        let additional_duration =
            Duration::try_days(additional_days).ok_or_else(deadline_out_of_range)?;
        let target_date = first_business_day_start
            .date_naive()
            .checked_add_signed(additional_duration)
            .ok_or_else(deadline_out_of_range)?;
        try_local_date_and_time(target_date, parent_deadline_time.time())?
    } else {
        let duration =
            Duration::try_days(repetition_interval_days).ok_or_else(deadline_out_of_range)?;
        orig_deadline_time
            .checked_add_signed(duration)
            .ok_or_else(deadline_out_of_range)?
    };
    let start_out_of_range = || ApplicationError::SubjectiveDateOutOfRange {
        operation: "defer_routine_start",
        datetime: orig_start_time,
    };
    let start_offset_days = (new_deadline_time - orig_deadline_time).num_days();
    let start_offset = Duration::try_days(start_offset_days).ok_or_else(start_out_of_range)?;
    let new_start_time = orig_start_time
        .checked_add_signed(start_offset)
        .ok_or_else(start_out_of_range)?;

    focused_task
        .unset_deadline_time_opt()
        .map_err(ApplicationError::TaskTree)?;
    focused_task
        .set_deadline_time_opt(Some(new_deadline_time))
        .map_err(ApplicationError::TaskTree)?;
    focused_task
        .set_orig_status(Status::Todo)
        .map_err(ApplicationError::TaskTree)?;
    focused_task
        .set_start_time(new_start_time)
        .map_err(ApplicationError::TaskTree)?;
    *focused_task_id_opt = None;
    Ok(())
}

// 何日もSchronuを開いていなくてあまりにもTODOがたまってしまった場合に、repetition_intervalが7日以内のルーチンタスクを自動的に先送りする
// 7日よりも大きい場合は、1年に1回のような重要なタスクである可能性があるため、何もしない
fn execute_defer_all_frequent_routines(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    _focused_task_opt: &Option<TaskHandle>,
) -> Result<(), ApplicationError> {
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
                let leaf_tasks = extract_leaf_tasks_from_project(project_root_task)
                    .map_err(ApplicationError::TaskTree)?;
                for leaf_task in leaf_tasks.iter() {
                    if let Some(parent_task) =
                        leaf_task.parent().map_err(ApplicationError::TaskTree)?
                    {
                        if let Some(repetition_interval_days) = parent_task
                            .get_repetition_interval_days_opt()
                            .map_err(ApplicationError::TaskTree)?
                        {
                            if let Some(deadline_time) = leaf_task
                                .get_deadline_time_opt()
                                .map_err(ApplicationError::TaskTree)?
                            {
                                if repetition_interval_days <= MAX_REPETITION_INTERVAL_DAYS
                                    && now - deadline_time >= Duration::hours(MIN_OVERDUE_HOURS)
                                {
                                    ids.push(
                                        leaf_task.get_id().map_err(ApplicationError::TaskTree)?,
                                    );
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
            execute_defer_routine(task_repository, focused_task_id_opt)?;

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
    Ok(())
}

fn resolve_deadline_date(value: &str, now: DateTime<Local>) -> Result<String, CommandError> {
    if value == "消" {
        return Ok(value.to_string());
    }
    if value.starts_with('今') {
        return Ok(try_subjective_date(now)?.format("%Y/%m/%d").to_string());
    }
    if value.starts_with('明') {
        return Ok(try_next_business_day_start(now)?
            .format("%Y/%m/%d")
            .to_string());
    }

    let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];
    if days_of_week.contains(&value) {
        let today = try_subjective_date(now)?;
        let current_index = days_of_week
            .iter()
            .position(|day| *day == get_weekday_jp(&today))
            .expect("current weekday must be in the Japanese weekday table");
        let target_index = days_of_week
            .iter()
            .position(|day| *day == value)
            .expect("matched deadline weekday must be in the weekday table");
        let difference = (7 + target_index - current_index) % 7;
        let days = if difference == 0 {
            7
        } else {
            difference as i64
        };
        let deadline_date = today.checked_add_signed(Duration::days(days)).ok_or(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "deadline_weekday_date",
                datetime: now,
            },
        )?;
        return Ok(deadline_date.format("%Y/%m/%d").to_string());
    }

    let mmdd = Regex::new(r"^(\d{1,2})/(\d{1,2})$").expect("valid deadline regex");
    if let Some(captures) = mmdd.captures(value) {
        let invalid_deadline =
            || command_parse_error("〆", "deadline", "日時が不正です", "〆 <日付または時刻>");
        let month = captures[1].parse::<u32>().map_err(|_| invalid_deadline())?;
        let day = captures[2].parse::<u32>().map_err(|_| invalid_deadline())?;
        let validation_year = 2000 + now.year().rem_euclid(400);
        if NaiveDate::from_ymd_opt(validation_year, month, day).is_none() {
            return Err(invalid_deadline());
        }
        let out_of_range = || ApplicationError::SubjectiveDateOutOfRange {
            operation: "deadline_calendar_date",
            datetime: now,
        };
        let mut deadline_date =
            NaiveDate::from_ymd_opt(now.year(), month, day).ok_or_else(out_of_range)?;
        let deadline_noon = deadline_date
            .and_hms_opt(12, 0, 0)
            .ok_or_else(out_of_range)?;
        if deadline_noon < now.naive_local() {
            let next_year = now.year().checked_add(1).ok_or_else(out_of_range)?;
            let next_validation_year = 2000 + next_year.rem_euclid(400);
            if NaiveDate::from_ymd_opt(next_validation_year, month, day).is_none() {
                return Err(invalid_deadline());
            }
            deadline_date =
                NaiveDate::from_ymd_opt(next_year, month, day).ok_or_else(out_of_range)?;
        }
        return Ok(deadline_date.format("%Y/%m/%d").to_string());
    }

    Ok(value.to_string())
}

fn resolve_deadline_time(
    deadline_value: &str,
    now: DateTime<Local>,
    config: &SchronuConfig,
) -> Result<Option<DateTime<Local>>, CommandError> {
    let deadline_date_str = resolve_deadline_date(deadline_value, now)?;
    if deadline_date_str == "消" {
        return Ok(None);
    }

    let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();
    let invalid_datetime =
        || command_parse_error("〆", "deadline", "日時が不正です", "〆 <日付または時刻>");
    let (date, time) = if hhmm_reg.is_match(&deadline_date_str) {
        let caps = hhmm_reg
            .captures(&deadline_date_str)
            .expect("matched deadline time must have captures");
        let hh: u32 = caps[1].parse().map_err(|_| {
            command_parse_error("〆", "deadline", "時刻が不正です", "〆 <日付または時刻>")
        })?;
        let mm: u32 = caps[2].parse().map_err(|_| {
            command_parse_error("〆", "deadline", "時刻が不正です", "〆 <日付または時刻>")
        })?;
        let time = NaiveTime::from_hms_opt(hh, mm, 0).ok_or_else(invalid_datetime)?;
        (now.date_naive(), time)
    } else {
        let date = NaiveDate::parse_from_str(&deadline_date_str, "%Y/%m/%d")
            .map_err(|_| invalid_datetime())?;
        (date, config.default_deadline_time)
    };
    Ok(Some(try_local_date_and_time(date, time)?))
}

fn execute_set_arrange_children_work_minutes(
    focused_task_opt: &Option<TaskHandle>,
    estimated_minutes: i64,
    includes_zero_estimate: bool,
) -> Result<(), ApplicationError> {
    // 繰り返しタスクについて、その子タスクでDoneでないものの時間を一律変更する。
    if !(0..=MAX_ARRANGE_ESTIMATED_WORK_MINUTES).contains(&estimated_minutes) {
        return Ok(());
    }

    if let Some(focused_task) = focused_task_opt {
        if focused_task
            .get_repetition_interval_days_opt()
            .map_err(ApplicationError::TaskTree)?
            .is_some()
        {
            let children = focused_task
                .get_children()
                .map_err(ApplicationError::TaskTree)?;
            for child_task in children.iter() {
                if child_task
                    .get_status()
                    .map_err(ApplicationError::TaskTree)?
                    != Status::Done
                    && (includes_zero_estimate
                        || child_task
                            .get_estimated_work_seconds()
                            .map_err(ApplicationError::TaskTree)?
                            != 0)
                {
                    child_task
                        .set_estimated_work_seconds(estimated_minutes * 60)
                        .map_err(ApplicationError::TaskTree)?;
                }
            }
        }
    }
    Ok(())
}

fn set_focused_task_actual_work_minutes(
    focused_task_opt: &Option<TaskHandle>,
    actual_work_minutes: i64,
) -> Result<(), ApplicationError> {
    let actual_work_seconds =
        actual_work_minutes
            .checked_mul(60)
            .ok_or(ApplicationError::InvalidInput {
                field: "actual_work_minutes",
                reason: "seconds conversion overflow",
            })?;
    if let Some(focused_task) = focused_task_opt.as_ref() {
        focused_task
            .set_actual_work_seconds(actual_work_seconds)
            .map_err(ApplicationError::TaskTree)?;
    }
    Ok(())
}

fn set_focused_task_priority(
    focused_task_opt: &Option<TaskHandle>,
    priority: i64,
) -> Result<(), ApplicationError> {
    if let Some(focused_task) = focused_task_opt.as_ref() {
        focused_task
            .set_priority(priority)
            .map_err(ApplicationError::TaskTree)?;
    }
    Ok(())
}

#[cfg(test)]
fn execute_set_priority(
    focused_task_opt: &Option<TaskHandle>,
    priority_str: &str,
) -> Result<(), ApplicationError> {
    if let Ok(priority) = priority_str.parse::<i64>() {
        set_focused_task_priority(focused_task_opt, priority)?;
    }
    Ok(())
}

fn read_project_category_command_arg(s: &str) -> Option<Option<ProjectCategory>> {
    match s.to_lowercase().as_str() {
        "_" | "none" | "clear" => Some(None),
        _ => read_project_category(s).map(Some),
    }
}

#[cfg(test)]
fn decide_time(tokens: &[&str], now: &DateTime<Local>) -> Option<DateTime<Local>> {
    let values = tokens
        .iter()
        .skip(1)
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    decide_time_values(&values, now).expect("test datetime input must resolve")
}

#[cfg(test)]
fn decide_finish_time(tokens: &Vec<&str>, now: &DateTime<Local>) -> Option<DateTime<Local>> {
    let values = tokens
        .iter()
        .skip(1)
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    decide_finish_time_values(&values, now).expect("test finish datetime input must resolve")
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
    supports_ansi_color: bool,
    newline_prefix: &'static str,
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
        Self {
            buffer: vec![],
            supports_ansi_color: true,
            newline_prefix: "",
        }
    }

    fn new_for_pipe() -> Self {
        Self {
            buffer: vec![],
            supports_ansi_color: false,
            newline_prefix: "",
        }
    }

    fn new_with_newline_prefix(newline_prefix: &'static str) -> Self {
        Self {
            buffer: vec![],
            supports_ansi_color: true,
            newline_prefix,
        }
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
        let newline_prefix = self.newline_prefix;
        writeln!(self, "{newline_prefix}{message}")
    }

    fn supports_ansi_color(&self) -> bool {
        self.supports_ansi_color
    }
}

#[cfg(test)]
struct FailingNewlineWriter {
    buffer: Vec<u8>,
    failures_remaining: usize,
    newline_call_count: usize,
}

#[cfg(test)]
impl FailingNewlineWriter {
    fn fail_once() -> Self {
        Self {
            buffer: vec![],
            failures_remaining: 1,
            newline_call_count: 0,
        }
    }
}

#[cfg(test)]
impl Write for FailingNewlineWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
impl SchronuWriter for FailingNewlineWriter {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        self.newline_call_count += 1;
        if self.failures_remaining > 0 {
            self.failures_remaining -= 1;
            return Err(std::io::Error::other("newline write failure"));
        }
        writeln!(self, "<reset>{message}")
    }
}

#[cfg(test)]
struct FlushTrackingWriter {
    buffer: Vec<u8>,
    flush_count: usize,
    flush_error_kind: Option<std::io::ErrorKind>,
    supports_ansi_color: bool,
}

#[cfg(test)]
impl FlushTrackingWriter {
    fn successful(supports_ansi_color: bool) -> Self {
        Self {
            buffer: vec![],
            flush_count: 0,
            flush_error_kind: None,
            supports_ansi_color,
        }
    }

    fn failing(error_kind: std::io::ErrorKind) -> Self {
        Self {
            buffer: vec![],
            flush_count: 0,
            flush_error_kind: Some(error_kind),
            supports_ansi_color: true,
        }
    }
}

#[cfg(test)]
impl Write for FlushTrackingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flush_count += 1;
        match self.flush_error_kind {
            Some(kind) => Err(std::io::Error::new(kind, "flush failure")),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
impl SchronuWriter for FlushTrackingWriter {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        writeln!(self, "{message}")
    }

    fn supports_ansi_color(&self) -> bool {
        self.supports_ansi_color
    }
}

#[cfg(test)]
fn strip_ansi_escape_sequences(value: &str) -> String {
    Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]")
        .unwrap()
        .replace_all(value, "")
        .into_owned()
}

#[cfg(test)]
struct TestTaskRepository {
    task: TaskHandle,
    storage_directory: String,
    last_synced_time: DateTime<Local>,
    highest_priority_leaf_task_id_opt: Option<Uuid>,
    defer_candidate_leaf_task_id_opt: Option<Uuid>,
    last_defer_candidate_recent_days_opt: Option<i64>,
    load_should_fail: bool,
    load_attempt_count: Cell<usize>,
    reload_if_changed_attempt_count: Cell<usize>,
    save_failures_remaining: Cell<usize>,
    save_attempt_count: Cell<usize>,
    has_pending_changes: Cell<bool>,
}

#[cfg(test)]
struct CommandTestResult {
    task: TaskHandle,
    focused_task_id_opt: Option<Uuid>,
    output: String,
}

#[cfg(test)]
fn execute_command_for_test(
    task: TaskHandle,
    now: DateTime<Local>,
    focused_task_id_opt: Option<Uuid>,
    command: &str,
) -> CommandTestResult {
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = focused_task_id_opt;
    let mut stdout = TestWriter::new();

    if let Err(error) = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    ) {
        let _output_error = render_display_model(&mut stdout, &error_display_model(&error))
            .map_err(CommandError::Output);
    }

    CommandTestResult {
        task: task_repository.task,
        focused_task_id_opt,
        output: stdout.into_string(),
    }
}

#[test]
fn test_report_command_resultはtask_tree_errorを既存の操作エラー形式で表示する() {
    let mut stdout = TestWriter::new();

    report_command_result(
        &mut stdout,
        Err(CommandError::Application(ApplicationError::TaskTree(
            TaskTreeError::Borrow,
        ))),
    );

    assert_eq!(
        stdout.into_string(),
        "[Error] 操作エラー: task tree operation failed: cannot borrow task tree data\n"
    );
}

#[test]
fn test_execute_空_日付指定は指定日の予定開始時刻でtodoをpendingにする() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let schronu_day_start = Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap();
    let task = new_test_task_handle("日付指定の空対象").unwrap();
    task.set_start_time(schronu_day_start + Duration::hours(4));
    task.set_estimated_work_seconds(30 * 60);
    let task_id = task.get_id().unwrap();
    let original_start_time = task.get_start_time().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "空 13:00 8/15");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        schronu_day_start + Duration::hours(7)
    );
    assert_eq!(result.task.get_start_time().unwrap(), original_start_time);
}

#[test]
fn test_execute_空_明指定は次の業務日の予定をpendingにする() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let schronu_day_start = Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap();
    let task = new_test_task_handle("明指定の空対象").unwrap();
    task.set_start_time(schronu_day_start + Duration::hours(4));
    task.set_estimated_work_seconds(30 * 60);
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "空 13:00 明");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        schronu_day_start + Duration::hours(7)
    );
}

#[test]
fn test_execute_空_日付selectorの業務日計算不能を情報付きerrorにして変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外の空対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    let actual = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "空 13:00 明",
    );

    assert!(matches!(
        actual,
        Err(CommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "next_business_day_start",
                datetime,
            }
        )) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(stdout.into_string().is_empty());
}

#[test]
fn test_execute_空_mmddの翌年計算不能を情報付きerrorにして変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外のMMDD空対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    let actual = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "空 13:00 12/31",
    );

    assert!(matches!(
        actual,
        Err(CommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "upcoming_calendar_date",
                datetime,
            }
        )) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(stdout.into_string().is_empty());
}

#[test]
fn test_execute_集_日付指定はpendingを業務日開始へ集める() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let schronu_day_start = Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap();
    let task = new_test_task_handle("日付指定の集対象").unwrap();
    task.set_start_time(schronu_day_start + Duration::hours(4));
    task.set_estimated_work_seconds(30 * 60);
    task.set_orig_status(Status::Pending);
    task.set_pending_until(schronu_day_start + Duration::hours(6));
    let task_id = task.get_id().unwrap();
    let original_start_time = task.get_start_time().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "集 13:00 8/15");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(result.task.get_pending_until().unwrap(), schronu_day_start);
    assert_eq!(result.task.get_start_time().unwrap(), original_start_time);
}

#[test]
fn test_execute_集_曜日指定は次に来る曜日の業務日開始へ集める() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let schronu_day_start = Local.with_ymd_and_hms(2026, 8, 17, 6, 0, 0).unwrap();
    let task = new_test_task_handle("曜日指定の集対象").unwrap();
    task.set_start_time(schronu_day_start + Duration::hours(4));
    task.set_estimated_work_seconds(30 * 60);
    task.set_orig_status(Status::Pending);
    task.set_pending_until(schronu_day_start + Duration::hours(6));
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "集 24:00 月");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(result.task.get_pending_until().unwrap(), schronu_day_start);
}

#[test]
fn test_execute_集_曜日selector範囲外を情報付きerrorにして変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外の曜日集約対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    let actual = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "集 13:00 月",
    );

    assert!(matches!(
        actual,
        Err(CommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "weekday_date",
                datetime,
            }
        )) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(stdout.into_string().is_empty());
}

#[test]
fn test_execute_空_日付指定はpending_untilの半開区間だけを変更する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let schronu_day_start = Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap();
    let task = new_test_task_handle("日付指定のpending対象").unwrap();
    task.set_start_time(schronu_day_start + Duration::hours(4));
    task.set_estimated_work_seconds(30 * 60);
    task.set_orig_status(Status::Pending);
    task.set_pending_until(schronu_day_start + Duration::hours(5));
    let task_id = task.get_id().unwrap();
    let original_start_time = task.get_start_time().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "clear 13:00 8/15");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        schronu_day_start + Duration::hours(7)
    );
    assert_eq!(result.task.get_start_time().unwrap(), original_start_time);
}

#[test]
fn test_execute_空_日付指定は予定候補外のpendingを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let schronu_day_start = Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap();
    let task = new_test_task_handle("予定候補外のpending").unwrap();
    task.set_start_time(schronu_day_start + Duration::days(1));
    task.set_estimated_work_seconds(30 * 60);
    task.set_orig_status(Status::Pending);
    let original_pending_until = schronu_day_start + Duration::hours(5);
    task.set_pending_until(original_pending_until);
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "空 13:00 8/15");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        original_pending_until
    );
}

#[test]
fn test_execute_日付指定の不正入力は状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let task = new_test_task_handle("不正入力対象").unwrap();
    task.set_start_time(now);
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "空 06:00 8/15");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Todo);
    assert_eq!(result.task.get_start_time().unwrap(), now);
}

#[test]
fn test_execute_始と約の不正時刻はtask日時を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let original_start = Local.with_ymd_and_hms(2026, 8, 15, 8, 0, 0).unwrap();
    let original_deadline = Local.with_ymd_and_hms(2026, 8, 16, 18, 0, 0).unwrap();

    for command in ["始 invalid", "約 invalid"] {
        let task = new_test_task_handle("不正時刻対象").unwrap();
        task.set_start_time(original_start);
        task.set_deadline_time_opt(Some(original_deadline));
        let task_id = task.get_id().unwrap();

        let result = execute_command_for_test(task, now, Some(task_id), command);

        assert_eq!(result.task.get_start_time().unwrap(), original_start);
        assert_eq!(
            result.task.get_deadline_time_opt().unwrap(),
            Some(original_deadline)
        );
        assert_eq!(result.focused_task_id_opt, Some(task_id));
    }
}

#[test]
fn test_execute_空_2引数は従来通り現在時刻基準で処理する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let task = new_test_task_handle("従来の空対象").unwrap();
    task.set_start_time(now);
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "空 120");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        now + Duration::minutes(120)
    );
}

#[test]
fn test_execute_空と集_2引数の不正なcalendar時刻は変更せず拒否する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    for command in ["空 13:99", "集 13:99"] {
        let task = new_test_task_handle("不正calendar時刻対象").unwrap();
        let task_id = task.get_id().unwrap();
        let original_snapshot = task.snapshot().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager;
        let mut focused_task_id_opt = Some(task_id);
        let mut stdout = TestWriter::new();

        let actual = execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
        );

        assert!(actual.is_ok());
        assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
        assert_eq!(focused_task_id_opt, Some(task_id));
        assert!(stdout.into_string().is_empty());
    }
}

#[test]
fn test_execute_空と集_i64範囲外のminutesは変更せず拒否する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    for command in ["空 9223372036854775808", "集 9223372036854775808"] {
        let task = new_test_task_handle("i64範囲外minutes対象").unwrap();
        let task_id = task.get_id().unwrap();
        let original_snapshot = task.snapshot().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager;
        let mut focused_task_id_opt = Some(task_id);
        let mut stdout = TestWriter::new();

        let actual = execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
        );

        assert!(actual.is_ok());
        assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
        assert_eq!(focused_task_id_opt, Some(task_id));
        assert!(stdout.into_string().is_empty());
    }
}

#[test]
fn test_execute_空と集_minutesの日時範囲外を情報付きerrorにして変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    for command in ["空 9223372036854775807", "集 9223372036854775807"] {
        let task = new_test_task_handle("minutes日時範囲外対象").unwrap();
        let task_id = task.get_id().unwrap();
        let original_snapshot = task.snapshot().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager;
        let mut focused_task_id_opt = Some(task_id);
        let mut stdout = TestWriter::new();

        let actual = execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
        );

        assert!(matches!(
            actual,
            Err(CommandError::Application(
                ApplicationError::SubjectiveDateOutOfRange {
                    operation: "clear_or_gather_minutes",
                    datetime,
                }
            )) if datetime == now
        ));
        assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
        assert_eq!(focused_task_id_opt, Some(task_id));
        assert!(stdout.into_string().is_empty());
    }
}

#[test]
fn test_execute_集_2引数は従来通りtodoへ戻す() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let task = new_test_task_handle("従来の集対象").unwrap();
    task.set_start_time(now);
    task.set_orig_status(Status::Pending);
    task.set_pending_until(now + Duration::minutes(60));
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "集 120");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Todo);
}

#[test]
fn test_execute_pack_前倒し内容と集計を表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("前倒し対象").unwrap();
    task.sync_clock(now);
    task.set_start_time(now);
    task.set_estimated_work_seconds(30 * 60);
    task.set_priority(9);
    task.set_pending_until(now + Duration::days(10));
    task.set_orig_status(Status::Pending);
    let task_id = task.get_id().unwrap();
    let repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerWithFreeMinutes { free_minutes: 120 };
    let mut stdout = TestWriter::new();

    execute_pack(&mut stdout, &repository, &mut free_time_manager);

    let output = stdout.into_string();
    assert!(output.contains(&format!(
        "詰\t2026-08-21\t2026-08-11\t00:30\t優先度9\t{}\t前倒し対象",
        task_id
    )));
    assert!(output.contains("詰: 1件 00:30 (スキップ0件)"));
}

#[test]
fn test_execute_pack_候補なしを表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("対象外").unwrap();
    task.sync_clock(now);
    let repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerWithFreeMinutes { free_minutes: 120 };
    let mut stdout = TestWriter::new();

    execute_pack(&mut stdout, &repository, &mut free_time_manager);

    assert_eq!(
        stdout.into_string(),
        "[Info] 詰められるタスクはありません。\n"
    );
}

#[test]
fn test_execute_pack_収まらない候補はスキップ件数だけを表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("大きい").unwrap();
    task.sync_clock(now);
    task.set_start_time(now);
    task.set_estimated_work_seconds(60 * 60);
    task.set_priority(9);
    task.set_pending_until(now + Duration::days(10));
    task.set_orig_status(Status::Pending);
    let repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerWithFreeMinutes { free_minutes: 60 };
    let mut stdout = TestWriter::new();

    execute_pack(&mut stdout, &repository, &mut free_time_manager);

    let output = stdout.into_string();
    assert!(!output.contains("[Skip]"));
    assert!(!output.contains("大きい"));
    assert!(output.contains("詰: 0件 00:00 (スキップ1件)"));
}

#[test]
fn test_execute_詰とpackの両aliasで製品command経路を実行する() {
    for command in ["詰", "pack"] {
        let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
        let task = new_test_task_handle("対象").unwrap();
        task.sync_clock(now);
        task.set_start_time(now);
        task.set_estimated_work_seconds(30 * 60);
        task.set_pending_until(now + Duration::days(10));
        task.set_orig_status(Status::Pending);
        let mut repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManagerWithFreeMinutes { free_minutes: 120 };
        let mut stdout = TestWriter::new();
        let mut focused_task_id_opt = None;

        execute(
            &mut stdout,
            &mut repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
        );

        assert!(stdout.into_string().contains("詰: 1件 00:30 (スキップ0件)"));
        assert!(repository.task.get_pending_until().unwrap() < now + Duration::days(10));
    }
}

#[cfg(test)]
impl TestTaskRepository {
    fn new(task: TaskHandle, last_synced_time: DateTime<Local>) -> Self {
        let task_id = task.get_id().unwrap();
        Self {
            task,
            storage_directory: String::new(),
            last_synced_time,
            highest_priority_leaf_task_id_opt: Some(task_id),
            defer_candidate_leaf_task_id_opt: Some(task_id),
            last_defer_candidate_recent_days_opt: None,
            load_should_fail: false,
            load_attempt_count: Cell::new(0),
            reload_if_changed_attempt_count: Cell::new(0),
            save_failures_remaining: Cell::new(0),
            save_attempt_count: Cell::new(0),
            has_pending_changes: Cell::new(true),
        }
    }

    fn with_storage_directory(mut self, storage_directory: &std::path::Path) -> Self {
        self.storage_directory = storage_directory.to_str().unwrap().to_string();
        self
    }
}

#[cfg(test)]
impl TaskRepositoryTrait for TestTaskRepository {
    fn get_project_storage_dir_name(&self) -> &str {
        &self.storage_directory
    }

    fn get_all_projects(&self) -> Vec<&TaskHandle> {
        vec![&self.task]
    }

    fn load(&mut self) -> Result<(), schronu::application::interface::TaskRepositoryError> {
        self.load_attempt_count
            .set(self.load_attempt_count.get() + 1);
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

    fn reload_if_changed(
        &mut self,
        now: DateTime<Local>,
    ) -> Result<RepositoryReloadOutcome, TaskRepositoryError> {
        self.reload_if_changed_attempt_count
            .set(self.reload_if_changed_attempt_count.get() + 1);
        self.sync_clock(now)
            .map_err(|error| TaskRepositoryError::new(TaskRepositoryOperation::Load, error))?;
        self.load()?;
        Ok(RepositoryReloadOutcome::Reloaded)
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

    fn has_pending_changes(&self) -> Result<bool, TaskTreeError> {
        Ok(self.has_pending_changes.get())
    }

    fn sync_clock(&mut self, now: DateTime<Local>) -> Result<(), TaskTreeError> {
        self.last_synced_time = now;
        Ok(())
    }

    fn get_last_synced_time(&self) -> DateTime<Local> {
        self.last_synced_time
    }

    fn get_highest_priority_project(&mut self) -> Option<&TaskHandle> {
        Some(&self.task)
    }

    fn get_highest_priority_leaf_task_id(&mut self) -> Result<Option<Uuid>, TaskTreeError> {
        Ok(self.highest_priority_leaf_task_id_opt)
    }

    fn get_defer_candidate_leaf_task_id(
        &mut self,
        recent_days: i64,
    ) -> Result<Option<Uuid>, TaskTreeError> {
        self.last_defer_candidate_recent_days_opt = Some(recent_days);
        Ok(self.defer_candidate_leaf_task_id_opt)
    }

    fn get_by_id(&self, id: Uuid) -> Result<Option<TaskHandle>, TaskTreeError> {
        self.task.get_by_id(id)
    }

    fn start_new_project(&mut self, root_task: TaskHandle) -> Result<(), TaskTreeError> {
        self.task = root_task;
        Ok(())
    }
}

#[cfg(test)]
struct TestFreeTimeManager;

#[cfg(test)]
trait FixtureTaskOptionExt {
    fn get_pending_until(&self) -> Result<DateTime<Local>, TaskTreeError>;
    fn get_estimated_work_seconds(&self) -> Result<i64, TaskTreeError>;
    fn get_name(&self) -> Result<String, TaskTreeError>;
    fn set_estimated_work_seconds(&self, estimated_work_seconds: i64) -> Result<(), TaskTreeError>;
}

#[cfg(test)]
impl FixtureTaskOptionExt for Option<TaskHandle> {
    fn get_pending_until(&self) -> Result<DateTime<Local>, TaskTreeError> {
        self.as_ref()
            .ok_or(TaskTreeError::MissingDummyRootChild)?
            .get_pending_until()
    }

    fn get_estimated_work_seconds(&self) -> Result<i64, TaskTreeError> {
        self.as_ref()
            .ok_or(TaskTreeError::MissingDummyRootChild)?
            .get_estimated_work_seconds()
    }

    fn get_name(&self) -> Result<String, TaskTreeError> {
        self.as_ref()
            .ok_or(TaskTreeError::MissingDummyRootChild)?
            .get_name()
    }

    fn set_estimated_work_seconds(&self, estimated_work_seconds: i64) -> Result<(), TaskTreeError> {
        self.as_ref()
            .ok_or(TaskTreeError::MissingDummyRootChild)?
            .set_estimated_work_seconds(estimated_work_seconds)
    }
}

#[cfg(test)]
impl FreeTimeManagerTrait for TestFreeTimeManager {
    fn get_free_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        0
    }

    fn get_busy_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        0
    }

    fn register_busy_time_slot(
        &mut self,
        _start: &DateTime<Local>,
        _end: &DateTime<Local>,
    ) -> Result<(), BusyTimeSlotRegistrationError> {
        Ok(())
    }

    fn load_busy_time_slots_from_file(
        &mut self,
        _busy_time_slots_file_path: &str,
    ) -> Result<(), BusyTimeSlotLoadError> {
        Ok(())
    }
}

#[cfg(test)]
#[derive(Default)]
struct TestFreeTimeManagerWithLoadError {
    loaded_path: RefCell<Option<PathBuf>>,
}

#[cfg(test)]
impl TestFreeTimeManagerWithLoadError {
    fn loaded_path(&self) -> Option<PathBuf> {
        self.loaded_path.borrow().clone()
    }
}

#[cfg(test)]
impl FreeTimeManagerTrait for TestFreeTimeManagerWithLoadError {
    fn get_free_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        0
    }

    fn get_busy_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        0
    }

    fn register_busy_time_slot(
        &mut self,
        _start: &DateTime<Local>,
        _end: &DateTime<Local>,
    ) -> Result<(), BusyTimeSlotRegistrationError> {
        Ok(())
    }

    fn load_busy_time_slots_from_file(
        &mut self,
        busy_time_slots_file_path: &str,
    ) -> Result<(), BusyTimeSlotLoadError> {
        let path = PathBuf::from(busy_time_slots_file_path);
        self.loaded_path.replace(Some(path.clone()));
        Err(BusyTimeSlotLoadError::new(
            path,
            "$",
            None,
            std::io::Error::new(std::io::ErrorKind::InvalidData, "test load error"),
        ))
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

    fn register_busy_time_slot(
        &mut self,
        _start: &DateTime<Local>,
        _end: &DateTime<Local>,
    ) -> Result<(), BusyTimeSlotRegistrationError> {
        Ok(())
    }

    fn load_busy_time_slots_from_file(
        &mut self,
        _busy_time_slots_file_path: &str,
    ) -> Result<(), BusyTimeSlotLoadError> {
        Ok(())
    }
}

#[test]
fn test_execute_表示コマンドはwriter固有の改行処理を保持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in ["今", "暦", "帯"] {
        let task = new_test_task_handle("改行処理確認用タスク").unwrap();
        task.set_estimated_work_seconds(60 * 60);
        task.set_start_time(now);
        task.set_pending_until(now);
        task.set_orig_status(Status::Pending);
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManagerWithFreeMinutes {
            free_minutes: 10 * 60,
        };
        let mut focused_task_id_opt = None;
        let mut stdout = TestWriter::new_with_newline_prefix("<reset>");

        execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
        )
        .unwrap();

        let output = stdout.into_string();
        assert!(output.contains("<reset>"), "{command}: {output}");
        assert!(
            output
                .lines()
                .filter(|line| !line.is_empty())
                .all(|line| line.starts_with("<reset>")),
            "{command}: {output}"
        );
    }
}

#[test]
fn test_execute_改行出力の失敗を捕捉して後続出力を継続する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("出力失敗確認用タスク").unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = None;
    let mut stdout = FailingNewlineWriter::fail_once();

    let actual = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "今",
    );

    assert!(matches!(
        actual,
        Err(CommandError::Output(error)) if error.kind() == std::io::ErrorKind::Other
    ));
    assert!(stdout.newline_call_count > 1);
    let output = String::from_utf8(stdout.buffer).unwrap();
    assert!(output.contains("<reset>"), "{output}");
}

#[test]
fn task_tree表示commandは製品経路でtyped_fieldと表示modelを反映する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root = new_test_task_handle("ROOT").unwrap();
    root.set_estimated_work_seconds(0);
    let mut matched_attr = new_test_task_attr("BOUNDARY_MATCH");
    matched_attr.set_estimated_work_seconds(15 * 60);
    matched_attr.set_start_time(now);
    let matched = root.create_as_last_child(matched_attr);
    matched.sync_clock(now);
    let mut other_attr = new_test_task_attr("BOUNDARY_OTHER");
    other_attr.set_estimated_work_seconds(15 * 60);
    other_attr.set_start_time(now);
    let other = root.create_as_last_child(other_attr);
    other.sync_clock(now);
    let root_id = root.get_id().unwrap();
    let matched_id = matched.get_id().unwrap();
    let other_id = other.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(root, now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(root_id);

    let mut show_all_output = TestWriter::new();
    execute(
        &mut show_all_output,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "全 BOUNDARY_MATCH",
    )
    .unwrap();
    let show_all_output = show_all_output.into_string();
    assert!(show_all_output.contains("BOUNDARY_MATCH"));
    assert!(!show_all_output.contains("BOUNDARY_OTHER"));

    focused_task_id_opt = Some(root_id);
    let mut tree_output = TestWriter::new();
    execute(
        &mut tree_output,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "樹",
    )
    .unwrap();
    let tree_output = tree_output.into_string();
    assert!(tree_output.contains("BOUNDARY_MATCH"), "{tree_output}");
    assert!(tree_output.contains("BOUNDARY_OTHER"), "{tree_output}");

    let mut list_output = TestWriter::new();
    execute(
        &mut list_output,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "今",
    )
    .unwrap();
    assert!(list_output.into_string().contains("BOUNDARY_MATCH"));

    let mut focus_output = TestWriter::new();
    execute(
        &mut focus_output,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        &format!("見 {matched_id}"),
    )
    .unwrap();
    assert_eq!(focused_task_id_opt, Some(matched_id));

    other.set_orig_status(Status::Pending).unwrap();
    let mut pick_output = TestWriter::new();
    execute(
        &mut pick_output,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        &format!("選 {other_id}"),
    )
    .unwrap();
    assert_eq!(focused_task_id_opt, Some(other_id));
    assert_eq!(other.get_orig_status().unwrap(), Status::Todo);
}

#[test]
fn calendarとbandは製品経路で代表出力とansi_capabilityを維持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let make_task = || {
        let task = new_test_task_handle("BOUNDARY_DAILY").unwrap();
        task.set_estimated_work_seconds(60 * 60);
        task.set_start_time(now);
        task.set_pending_until(now);
        task.set_orig_status(Status::Pending);
        task
    };

    let calendar = execute_calendar_command_for_test("暦", now, make_task(), 10 * 60);
    assert!(calendar.contains("2026-08-11(火)"));
    assert!(calendar.contains("日          \t空"));

    let band =
        execute_calendar_command_with_ansi_color_for_test("帯", now, make_task(), 10 * 60, true);
    assert!(band.contains("凡例:"));
    assert!(band.contains("\x1b[38;5;"));

    let pipe_band =
        execute_calendar_command_with_ansi_color_for_test("帯", now, make_task(), 10 * 60, false);
    assert!(pipe_band.contains("凡例:"));
    assert!(!pipe_band.contains("\x1b["));
}

#[test]
fn task_tree表示commandは製品経路で必ず1回flushする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("flush対象").unwrap();
    task.set_estimated_work_seconds(15 * 60);
    task.set_start_time(now);
    let task_id = task.get_id().unwrap();
    let commands = [
        "樹".to_string(),
        "条".to_string(),
        "根".to_string(),
        "葉".to_string(),
        "全".to_string(),
        "尾".to_string(),
        "今".to_string(),
        "単".to_string(),
        "暦".to_string(),
        "帯".to_string(),
        format!("見 {task_id}"),
        format!("選 {task_id}"),
        "親".to_string(),
        "子".to_string(),
        "深".to_string(),
        "上 next 15".to_string(),
    ];

    for command in commands {
        let mut task_repository = TestTaskRepository::new(task.clone(), now);
        let mut free_time_manager = TestFreeTimeManager;
        let mut focused_task_id_opt = Some(task_id);
        let mut stdout = FlushTrackingWriter::successful(true);

        execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            &command,
        )
        .unwrap();

        assert_eq!(stdout.flush_count, 1, "{command}");
    }
}

#[test]
fn task_tree表示commandはflush_errorとbroken_pipeを製品経路で分類する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("flush error対象").unwrap();
    let task_id = task.get_id().unwrap();

    let execute_with_error = |kind| {
        let mut task_repository = TestTaskRepository::new(task.clone(), now);
        let mut free_time_manager = TestFreeTimeManager;
        let mut focused_task_id_opt = Some(task_id);
        let mut stdout = FlushTrackingWriter::failing(kind);
        let result = execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            "樹",
        );
        (result, stdout.flush_count)
    };

    let (output_error, output_flush_count) = execute_with_error(std::io::ErrorKind::Other);
    assert!(matches!(
        output_error,
        Err(CommandError::Output(error)) if error.kind() == std::io::ErrorKind::Other
    ));
    assert_eq!(output_flush_count, 1);

    let (broken_pipe, broken_pipe_flush_count) = execute_with_error(std::io::ErrorKind::BrokenPipe);
    assert!(broken_pipe.is_ok());
    assert_eq!(broken_pipe_flush_count, 1);
}

#[test]
fn breakdownとsplitは製品経路で必ず1回flushする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in ["下 child", "割 15 child", "待"] {
        let task = new_test_task_handle("flush対象").unwrap();
        task.set_estimated_work_seconds(30 * 60);
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager;
        let mut focused_task_id_opt = Some(task_id);
        let mut stdout = FlushTrackingWriter::successful(true);

        execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
        )
        .unwrap();

        assert_eq!(stdout.flush_count, 1, "{command}");
    }
}

#[test]
fn breakdownとsplitはflush_errorとbroken_pipeを製品経路で分類する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in ["下 child", "割 15 child", "待"] {
        let execute_with_error = |kind| {
            let task = new_test_task_handle("flush error対象").unwrap();
            task.set_estimated_work_seconds(30 * 60);
            let task_id = task.get_id().unwrap();
            let mut task_repository = TestTaskRepository::new(task, now);
            let mut free_time_manager = TestFreeTimeManager;
            let mut focused_task_id_opt = Some(task_id);
            let mut stdout = FlushTrackingWriter::failing(kind);
            let result = execute(
                &mut stdout,
                &mut task_repository,
                &mut free_time_manager,
                &mut focused_task_id_opt,
                &now,
                command,
            );
            (result, stdout.flush_count)
        };

        let (output_error, output_flush_count) = execute_with_error(std::io::ErrorKind::Other);
        assert!(matches!(
            output_error,
            Err(CommandError::Output(error)) if error.kind() == std::io::ErrorKind::Other
        ));
        assert_eq!(output_flush_count, 1, "{command}");

        let (broken_pipe, broken_pipe_flush_count) =
            execute_with_error(std::io::ErrorKind::BrokenPipe);
        assert!(broken_pipe.is_ok(), "{command}");
        assert_eq!(broken_pipe_flush_count, 1, "{command}");
    }
}

#[test]
fn task属性更新commandは製品経路で必ず1回flushする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    for command in [
        "〆 2026/08/20",
        "予 15",
        "揃 15",
        "実 20",
        "重 3",
        "類 資",
        "働 5",
    ] {
        let task = new_test_task_handle("flush対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager;
        let mut focused_task_id_opt = Some(task_id);
        let parsed = parse_command(command, ParseMode::NonInteractive).unwrap();
        let mut stdout = FlushTrackingWriter::successful(true);

        execute_parsed(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
            &parsed,
        )
        .unwrap();

        assert_eq!(stdout.flush_count, 1, "{command}");
    }
}

#[test]
fn task属性更新commandはflush_errorとbroken_pipeを製品経路で分類する() {
    let execute_with_error = |error_kind| {
        let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
        let task = new_test_task_handle("flush error対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager;
        let mut focused_task_id_opt = Some(task_id);
        let parsed = parse_command("予 15", ParseMode::NonInteractive).unwrap();
        let mut stdout = FlushTrackingWriter::failing(error_kind);

        let result = execute_parsed(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            "予 15",
            &parsed,
        );
        (result, stdout.flush_count)
    };

    let (output_error, output_flush_count) = execute_with_error(std::io::ErrorKind::Other);
    assert!(matches!(
        output_error,
        Err(CommandError::Output(error)) if error.kind() == std::io::ErrorKind::Other
    ));
    assert_eq!(output_flush_count, 1);

    let (broken_pipe, broken_pipe_flush_count) = execute_with_error(std::io::ErrorKind::BrokenPipe);
    assert!(broken_pipe.is_ok());
    assert_eq!(broken_pipe_flush_count, 1);
}

#[test]
fn defer系の通常interactive_commandはflushしshortcutはflushしない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in [
        "後 09:30",
        "後 abc 日 extra",
        "清",
        "逃",
        "押",
        "空 10:00",
        "集 10:00",
    ] {
        let task = new_test_task_handle("通常commandのflush対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager;
        let mut focused_task_id_opt = Some(task_id);
        let mut focus_selection_mode = FocusSelectionMode::Explicit;
        let mut stdout = FlushTrackingWriter::successful(true);

        execute_interactive_command(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            &mut focus_selection_mode,
            now,
            command,
        )
        .unwrap();

        assert_eq!(stdout.flush_count, 1, "{command}");
    }

    for command in ["t", "h", "D", "d", "w", "W", "y"] {
        let task = new_test_task_handle("shortcutのflush対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager;
        let mut focused_task_id_opt = Some(task_id);
        let mut focus_selection_mode = FocusSelectionMode::Explicit;
        let mut stdout = FlushTrackingWriter::successful(true);

        execute_interactive_command(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            &mut focus_selection_mode,
            now,
            command,
        )
        .unwrap();

        assert_eq!(stdout.flush_count, 0, "{command}");
    }
}

#[test]
fn interactive低優先度modeは共通outcome経路でfocusと表示を更新する() {
    let now = Local.with_ymd_and_hms(2026, 8, 18, 12, 0, 0).unwrap();
    let root = new_test_task_handle("root").unwrap();
    let high_priority_task = root.create_as_last_child(new_test_task_attr("高優先度候補"));
    let low_priority_task = root.create_as_last_child(new_test_task_attr("低優先度候補"));
    let high_priority_task_id = high_priority_task.get_id().unwrap();
    let low_priority_task_id = low_priority_task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(root, now);
    task_repository.highest_priority_leaf_task_id_opt = Some(high_priority_task_id);
    task_repository.defer_candidate_leaf_task_id_opt = Some(low_priority_task_id);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(high_priority_task_id);
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;
    let mut stdout = FlushTrackingWriter::successful(true);

    execute_interactive_command(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        &mut focus_selection_mode,
        now,
        "低 3",
    )
    .unwrap();

    assert_eq!(
        focus_selection_mode,
        FocusSelectionMode::LowestPriority { recent_days: 3 }
    );
    assert_eq!(focused_task_id_opt, Some(low_priority_task_id));
    assert_eq!(
        task_repository.last_defer_candidate_recent_days_opt,
        Some(3)
    );
    assert_eq!(stdout.flush_count, 0);
    assert!(String::from_utf8(stdout.buffer)
        .unwrap()
        .contains("フォーカス選択モード: 低 3"));
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

    fn register_busy_time_slot(
        &mut self,
        _start: &DateTime<Local>,
        _end: &DateTime<Local>,
    ) -> Result<(), BusyTimeSlotRegistrationError> {
        Ok(())
    }

    fn load_busy_time_slots_from_file(
        &mut self,
        _busy_time_slots_file_path: &str,
    ) -> Result<(), BusyTimeSlotLoadError> {
        Ok(())
    }
}

#[cfg(test)]
struct TestFreeTimeManagerByDate {
    free_minutes_by_date: HashMap<NaiveDate, i64>,
}

#[cfg(test)]
impl FreeTimeManagerTrait for TestFreeTimeManagerByDate {
    fn get_free_minutes(&mut self, start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        self.free_minutes_by_date
            .get(&start.date_naive())
            .copied()
            .unwrap_or(0)
    }

    fn get_busy_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        0
    }

    fn register_busy_time_slot(
        &mut self,
        _start: &DateTime<Local>,
        _end: &DateTime<Local>,
    ) -> Result<(), BusyTimeSlotRegistrationError> {
        Ok(())
    }

    fn load_busy_time_slots_from_file(
        &mut self,
        _busy_time_slots_file_path: &str,
    ) -> Result<(), BusyTimeSlotLoadError> {
        Ok(())
    }
}

#[cfg(test)]
fn execute_sequential_command(command: &str) -> (TaskHandle, Option<Uuid>) {
    let now = Local.with_ymd_and_hms(2026, 7, 26, 12, 0, 0).unwrap();
    let task = new_test_task_handle("親タスク").unwrap();
    let task_id = task.get_id().unwrap();
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
fn execute_arrange_command(command: &str) -> TaskHandle {
    let now = Local.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
    let task = new_test_task_handle("ルーチン").unwrap();
    task.set_repetition_interval_days_opt(Some(7));

    let mut estimated_child_attr = new_test_task_attr("見積もりあり");
    estimated_child_attr.set_estimated_work_seconds(5 * 60);
    task.create_as_last_child(estimated_child_attr);

    let mut zero_estimate_child_attr = new_test_task_attr("見積もり0");
    zero_estimate_child_attr.set_estimated_work_seconds(0);
    task.create_as_last_child(zero_estimate_child_attr);

    let mut done_child_attr = new_test_task_attr("完了済み");
    done_child_attr.set_estimated_work_seconds(10 * 60);
    done_child_attr.set_orig_status(Status::Done);
    task.create_as_last_child(done_child_attr);

    let task_id = task.get_id().unwrap();
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
    let task = new_test_task_handle("タスク").unwrap();
    let expected_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    task_repository.highest_priority_leaf_task_id_opt = Some(expected_id);

    let actual = select_focus_task_id(&mut task_repository, FocusSelectionMode::HighestPriority);

    assert_eq!(actual, Ok(Some(expected_id)));
}

#[test]
fn test_select_focus_task_id_低優先度modeでは指定日数の延期候補を返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    let expected_id = Uuid::new_v4();
    let mut task_repository = TestTaskRepository::new(task, now);
    task_repository.defer_candidate_leaf_task_id_opt = Some(expected_id);

    let actual = select_focus_task_id(
        &mut task_repository,
        FocusSelectionMode::LowestPriority { recent_days: 3 },
    );

    assert_eq!(actual, Ok(Some(expected_id)));
    assert_eq!(
        task_repository.last_defer_candidate_recent_days_opt,
        Some(3)
    );
}

#[test]
fn test_execute_all_pendingタスクを予定時刻に含め_doneタスクを除外する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    let pending_task = new_test_task_handle("延期中タスク").unwrap();
    pending_task.set_start_time(now);
    pending_task.sync_clock(now);
    pending_task.set_pending_until(now + Duration::hours(2));
    pending_task.set_orig_status(Status::Pending);
    let pending_result = execute_command_for_test(
        pending_task.clone(),
        now,
        Some(pending_task.get_id().unwrap()),
        "全",
    );

    let done_task = new_test_task_handle("完了済みタスク").unwrap();
    done_task.set_start_time(now);
    done_task.sync_clock(now);
    done_task.set_orig_status(Status::Done);
    let done_result = execute_command_for_test(
        done_task.clone(),
        now,
        Some(done_task.get_id().unwrap()),
        "全",
    );

    assert!(pending_result.output.contains("延期中タスク"));
    assert!(pending_result.output.contains("14:00~14:15"));
    assert!(!done_result.output.contains("完了済みタスク"));
}

#[test]
fn test_execute_all_project_categoryで絞り込む() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("カテゴリ対象タスク").unwrap();
    task.sync_clock(now);
    task.set_project_category_opt(Some(ProjectCategory::Investment));

    let matched =
        execute_command_for_test(task.clone(), now, Some(task.get_id().unwrap()), "全 資");
    let unmatched =
        execute_command_for_test(task.clone(), now, Some(task.get_id().unwrap()), "全 獲");

    assert!(matched.output.contains("カテゴリ対象タスク"));
    assert!(!unmatched.output.contains("カテゴリ対象タスク"));
}

#[test]
fn test_execute_allはspreadsheet_a_j列を製品formatterで出力する() {
    assert_show_all_spreadsheet_formatter_contract();
}

#[test]
fn show_allの製品経路はspreadsheet_formatterを使う() {
    assert_show_all_spreadsheet_formatter_contract();
}

#[cfg(test)]
fn assert_show_all_spreadsheet_formatter_contract() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("夕食  の 準備").unwrap();
    task.set_estimated_work_seconds(40 * 60);
    task.set_start_time(now);
    task.set_priority(1);
    task.set_project_category_opt(Some(ProjectCategory::Investment));
    task.sync_clock(now);
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "全");
    let task_row = result
        .output
        .lines()
        .find(|line| line.contains(&task_id.to_string()))
        .expect("ShowAll task row");

    assert_eq!(
        task_row,
        format!("0000 {task_id} A ____/__/__ 08/11(火)-12:00~12:40 0 40 01 資 夕食  の 準備")
    );
}

#[test]
fn test_execute_all_締切順の予定時刻を表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root_task = new_test_task_handle("親タスク").unwrap();
    root_task.sync_clock(now);
    root_task.set_estimated_work_seconds(0);

    let mut late_deadline_attr = new_test_task_attr("締切が遅いタスク");
    late_deadline_attr.set_estimated_work_seconds(30 * 60);
    late_deadline_attr.set_start_time(now);
    late_deadline_attr.set_deadline_time_opt(Some(now + Duration::hours(3)));
    let late_deadline_task = root_task.create_as_last_child(late_deadline_attr);
    late_deadline_task.sync_clock(now);

    let mut early_deadline_attr = new_test_task_attr("締切が早いタスク");
    early_deadline_attr.set_estimated_work_seconds(15 * 60);
    early_deadline_attr.set_start_time(now);
    early_deadline_attr.set_deadline_time_opt(Some(now + Duration::hours(2)));
    let early_deadline_task = root_task.create_as_last_child(early_deadline_attr);
    early_deadline_task.sync_clock(now);

    let result = execute_command_for_test(
        root_task.clone(),
        now,
        Some(root_task.get_id().unwrap()),
        "全",
    );
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
    let original_task = new_test_task_handle("既存タスク").unwrap();
    let result = execute_command_for_test(
        original_task.clone(),
        now,
        Some(original_task.get_id().unwrap()),
        "新 新規project 30",
    );

    assert_eq!(result.task.get_name().unwrap(), "新規project");
    assert_eq!(result.task.get_priority().unwrap(), 5);
    assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 30 * 60);
    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        get_next_morning_datetime(now)
    );
    assert_eq!(
        result.focused_task_id_opt,
        Some(result.task.get_id().unwrap())
    );
}

#[test]
fn test_execute_unplanned_延期と見積もりを省略して即時着手可能で作成する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let original_task = new_test_task_handle("既存タスク").unwrap();
    let result = execute_command_for_test(
        original_task.clone(),
        now,
        Some(original_task.get_id().unwrap()),
        "突 割り込みproject",
    );

    assert_eq!(result.task.get_name().unwrap(), "割り込みproject");
    assert_eq!(result.task.get_orig_status().unwrap(), Status::Todo);
    assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(
        result.focused_task_id_opt,
        Some(result.task.get_id().unwrap())
    );
}

#[test]
fn test_project作成commandの製品handler経路がtyped_fieldと表示とfocusを反映する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    let new_root = new_test_task_handle("既存").unwrap();
    let new_result = execute_command_for_test(
        new_root.clone(),
        now,
        Some(new_root.get_id().unwrap()),
        "新 新規 25",
    );
    assert_eq!(new_result.task.get_name().unwrap(), "新規");
    assert_eq!(
        new_result.task.get_estimated_work_seconds().unwrap(),
        25 * 60
    );
    assert_eq!(
        new_result.focused_task_id_opt,
        Some(new_result.task.get_id().unwrap())
    );

    let hobby_root = new_test_task_handle("既存").unwrap();
    let hobby_result = execute_command_for_test(
        hobby_root.clone(),
        now,
        Some(hobby_root.get_id().unwrap()),
        "遊 趣味 20",
    );
    assert_eq!(hobby_result.task.get_name().unwrap(), "趣味");
    assert_eq!(
        hobby_result.task.get_pending_until().unwrap(),
        get_next_morning_datetime(now) + Duration::days(1399)
    );

    let unplanned_root = new_test_task_handle("既存").unwrap();
    let unplanned_result = execute_command_for_test(
        unplanned_root.clone(),
        now,
        Some(unplanned_root.get_id().unwrap()),
        "突 割り込み 10",
    );
    assert_eq!(unplanned_result.task.get_name().unwrap(), "割り込み");
    assert_eq!(
        unplanned_result.task.get_orig_status().unwrap(),
        Status::Todo
    );

    let sequential_root = new_test_task_handle("親").unwrap();
    let sequential_result = execute_command_for_test(
        sequential_root.clone(),
        now,
        Some(sequential_root.get_id().unwrap()),
        "連 手順 15 2 3 章",
    );
    let sequential_children = sequential_result.task.get_children().unwrap();
    assert_eq!(sequential_children[0].get_name().unwrap(), "手順 3-章");
    assert_eq!(
        sequential_result.focused_task_id_opt,
        Some(
            sequential_children[0].get_children().unwrap()[0]
                .get_id()
                .unwrap()
        )
    );

    let repeat_root = new_test_task_handle("親").unwrap();
    let repeat_result = execute_command_for_test(
        repeat_root.clone(),
        now,
        Some(repeat_root.get_id().unwrap()),
        "繰 習慣 10 毎 09:00 10:00",
    );
    assert_eq!(repeat_result.task.get_children().unwrap().len(), 1);
    assert!(repeat_result.output.contains("習慣"));

    let appointment_task = new_test_task_handle("予定").unwrap();
    let appointment_id = appointment_task.get_id().unwrap();
    let appointment_result =
        execute_command_for_test(appointment_task, now, Some(appointment_id), "約 14:30 8/12");
    assert_eq!(
        appointment_result.task.get_start_time().unwrap(),
        Local.with_ymd_and_hms(2026, 8, 12, 14, 30, 0).unwrap()
    );
    assert_eq!(appointment_result.focused_task_id_opt, Some(appointment_id));

    let start_task = new_test_task_handle("開始").unwrap();
    let start_id = start_task.get_id().unwrap();
    let start_result = execute_command_for_test(start_task, now, Some(start_id), "始 16:45 8/13");
    assert_eq!(
        start_result.task.get_start_time().unwrap(),
        Local.with_ymd_and_hms(2026, 8, 13, 16, 45, 0).unwrap()
    );

    for invalid_command in ["新 123 10", "連 手順 -1 1 2", "繰 習慣 -1 毎 09:00 10:00"] {
        let task = new_test_task_handle("変更なし").unwrap();
        let result = execute_command_for_test(
            task.clone(),
            now,
            Some(task.get_id().unwrap()),
            invalid_command,
        );
        assert_eq!(result.task.get_name().unwrap(), "変更なし");
        assert!(result.task.get_children().unwrap().is_empty());
    }
}

#[test]
fn test_execute_breakdown_子を順に作り締切を継承して最初の子へfocusする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.sync_clock(now);
    parent_task.set_deadline_time_opt(Some(deadline));

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(parent_task.get_id().unwrap()),
        "下 子A 子B",
    );
    let children = result.task.get_children().unwrap();

    assert_eq!(
        children
            .iter()
            .map(|task| task.get_name().unwrap())
            .collect::<Vec<_>>(),
        vec!["子A", "子B"]
    );
    assert!(children
        .iter()
        .all(|task| task.get_deadline_time_opt().unwrap() == Some(deadline)));
    assert_eq!(
        result.focused_task_id_opt,
        Some(children[0].get_id().unwrap())
    );
    assert!(result.output.contains("子A"));
    assert!(result.output.contains("子B"));
}

#[test]
fn test_execute_breakdown_数値を含む引数では子を作らない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    let parent_task_id = parent_task.get_id().unwrap();

    let result = execute_command_for_test(parent_task, now, Some(parent_task_id), "下 子タスク 15");

    assert!(result.task.get_children().unwrap().is_empty());
    assert_eq!(result.focused_task_id_opt, Some(parent_task_id));
}

#[test]
fn test_execute_breakdown_親に締切がなければ子も締切なしで作る() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(parent_task.get_id().unwrap()),
        "下 子タスク",
    );
    let children = result.task.get_children().unwrap();

    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_deadline_time_opt().unwrap(), None);
}

#[test]
fn test_execute_wait_相手待ちにしてfocusを維持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("待機対象").unwrap();
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "待");

    assert!(result.task.get_is_on_other_side().unwrap());
    assert_eq!(result.focused_task_id_opt, Some(task_id));
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
        let root = new_test_task_handle("root").unwrap();
        let focused = root.create_as_last_child(new_test_task_attr("focus"));
        let result = execute_command_for_test(root, now, Some(focused.get_id().unwrap()), command);

        assert_eq!(result.task.get_children().unwrap().len(), 1);
        assert_eq!(
            result.task.get_children().unwrap()[0].get_name().unwrap(),
            "focus"
        );
        assert_eq!(result.focused_task_id_opt, Some(focused.get_id().unwrap()));
    }
}

#[test]
fn test_execute_next_up_入力不正とfocusなしではidentityを消費しない() {
    let assert_identity_not_consumed =
        |focused_task_opt: Option<TaskHandle>,
         name: &str,
         estimated_minutes: Option<i64>,
         expected: Result<Option<Uuid>, ApplicationError>| {
            let operation_now = Local.with_ymd_and_hms(2026, 8, 19, 14, 30, 0).unwrap();
            let id_generator_call_count = Cell::new(0);
            let mut next_id = || {
                id_generator_call_count.set(id_generator_call_count.get() + 1);
                Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap()
            };
            let mut factory = TaskFactory::new(operation_now, &mut next_id);
            let mut focused_task_id_opt =
                focused_task_opt.as_ref().map(|task| task.get_id().unwrap());
            let mut stdout = TestWriter::new();

            let actual = execute_next_up(
                &mut stdout,
                &mut focused_task_id_opt,
                &focused_task_opt,
                name,
                &estimated_minutes,
                &mut factory,
            );

            assert_eq!(actual, expected);
            assert_eq!(id_generator_call_count.get(), 0);
        };

    let root = new_test_task_handle("root").unwrap();
    let focused = root.create_as_last_child(new_test_task_attr("focused"));
    assert_identity_not_consumed(
        Some(focused.clone()),
        "123",
        Some(10),
        Err(ApplicationError::InvalidInput {
            field: "name",
            reason: "must not be an integer-only name",
        }),
    );
    assert_identity_not_consumed(
        Some(focused),
        "new parent",
        Some(-1),
        Err(ApplicationError::InvalidInput {
            field: "estimated_work_minutes",
            reason: "must not be negative",
        }),
    );
    assert_identity_not_consumed(None, "new parent", Some(10), Ok(None));
}

#[test]
fn test_execute_next_up_rootへの親追加失敗を構造化errorで返す() {
    let operation_now = Local.with_ymd_and_hms(2026, 8, 19, 14, 30, 0).unwrap();
    let id_generator_call_count = Cell::new(0);
    let mut next_id = || {
        id_generator_call_count.set(id_generator_call_count.get() + 1);
        Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap()
    };
    let mut factory = TaskFactory::new(operation_now, &mut next_id);
    let root = new_test_task_handle("root").unwrap();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(root.get_id().unwrap());
    let before_estimated_work_seconds = root.get_estimated_work_seconds().unwrap();

    let actual = execute_next_up(
        &mut stdout,
        &mut focused_task_id_opt,
        &Some(root.clone()),
        "new parent",
        &Some(10),
        &mut factory,
    );

    assert_eq!(
        actual,
        Err(ApplicationError::TaskTree(TaskTreeError::RootOperation))
    );
    assert_eq!(
        root.get_estimated_work_seconds().unwrap(),
        before_estimated_work_seconds
    );
    assert_eq!(focused_task_id_opt, Some(root.get_id().unwrap()));
    assert_eq!(id_generator_call_count.get(), 0);
}

#[test]
fn test_execute_next_up_task生成contextと既存の親挿入契約を固定する() {
    let operation_now = Local.with_ymd_and_hms(2026, 8, 19, 14, 30, 0).unwrap();
    let deadline = operation_now + Duration::days(2);
    let expected_new_parent_id = Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap();
    let mut next_id = || expected_new_parent_id;
    let mut factory = TaskFactory::new(operation_now, &mut next_id);

    let root = new_test_task_handle("root").unwrap();
    root.set_deadline_time_opt(Some(deadline)).unwrap();
    root.set_estimated_work_seconds(120 * 60).unwrap();
    let focused = root.create_as_last_child(new_test_task_attr("focused"));
    let focused_id = focused.get_id().unwrap();
    let mut focused_task_id_opt = Some(focused_id);
    let mut stdout = TestWriter::new();

    let actual = execute_next_up(
        &mut stdout,
        &mut focused_task_id_opt,
        &Some(focused),
        "new parent",
        &Some(15),
        &mut factory,
    );

    assert_eq!(actual, Ok(Some(expected_new_parent_id)));
    assert_eq!(focused_task_id_opt, Some(expected_new_parent_id));
    assert_eq!(root.get_estimated_work_seconds().unwrap(), 105 * 60);

    let root_children = root.get_children().unwrap();
    assert_eq!(root_children.len(), 1);
    let new_parent = &root_children[0];
    assert_eq!(new_parent.get_id().unwrap(), expected_new_parent_id);
    assert_eq!(new_parent.get_name().unwrap(), "new parent");
    assert_eq!(new_parent.get_start_time().unwrap(), operation_now);
    assert_eq!(new_parent.get_deadline_time_opt().unwrap(), Some(deadline));
    assert_eq!(new_parent.get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(
        new_parent.get_children().unwrap()[0].get_id().unwrap(),
        focused_id
    );
}

#[test]
fn test_execute_sequential_数値名と負の見積もりでは変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in ["連 123 10 1 2", "連 子 -1 1 2"] {
        let root = new_test_task_handle("root").unwrap();
        let result =
            execute_command_for_test(root.clone(), now, Some(root.get_id().unwrap()), command);

        assert!(result
            .task
            .get_children()
            .expect("command result tree must be readable")
            .is_empty());
        assert_eq!(result.focused_task_id_opt, Some(root.get_id().unwrap()));
    }
}

#[test]
fn test_execute_split_負数は親に残す時間として扱う() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root = new_test_task_handle("root").unwrap();
    root.set_estimated_work_seconds(100 * 60);

    let result =
        execute_command_for_test(root.clone(), now, Some(root.get_id().unwrap()), "割 -15 子");
    let children = result
        .task
        .get_children()
        .expect("split result tree must be readable");
    let child = &children[0];

    assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(child.get_name().unwrap(), "子");
    assert_eq!(child.get_estimated_work_seconds().unwrap(), 85 * 60);
    assert_eq!(result.focused_task_id_opt, Some(child.get_id().unwrap()));
}

#[test]
fn test_execute_split_数値名とoverflowでは変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in [
        "割 -15 123",
        "割 -9223372036854775808 子",
        "割 9223372036854775807 子",
    ] {
        let root = new_test_task_handle("root").unwrap();
        root.set_estimated_work_seconds(100 * 60);
        let result =
            execute_command_for_test(root.clone(), now, Some(root.get_id().unwrap()), command);

        assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 100 * 60);
        assert!(result
            .task
            .get_children()
            .expect("command result tree must be readable")
            .is_empty());
        assert_eq!(result.focused_task_id_opt, Some(root.get_id().unwrap()));
    }
}

#[test]
fn test_execute_defer_指定時間までpendingにしてfocusを外す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("延期対象").unwrap();

    let result =
        execute_command_for_test(task.clone(), now, Some(task.get_id().unwrap()), "後 5 分");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        now + Duration::minutes(5)
    );
    assert_eq!(result.focused_task_id_opt, None);
}

#[test]
fn test_execute_defer_日付指定はその日の朝までpendingにする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("延期対象").unwrap();

    let result = execute_command_for_test(
        task.clone(),
        now,
        Some(task.get_id().unwrap()),
        "後 2026/08/13",
    );

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 1).unwrap()
    );
    assert_eq!(result.focused_task_id_opt, None);
}

#[test]
fn test_execute_defer_expression_曜日指定は次の該当曜日までpendingにする() {
    let now = Local.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();

    for (weekday, expected) in [
        ("月", Local.with_ymd_and_hms(2026, 8, 24, 6, 0, 1).unwrap()),
        ("火", Local.with_ymd_and_hms(2026, 8, 18, 6, 0, 1).unwrap()),
    ] {
        let task = new_test_task_handle("曜日延期対象").unwrap();
        let task_id = task.get_id().unwrap();

        let result = execute_command_for_test(task, now, Some(task_id), &format!("後 {weekday}"));

        assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
        assert_eq!(result.task.get_pending_until().unwrap(), expected);
        assert_eq!(result.focused_task_id_opt, None);
    }
}

#[test]
fn test_execute_defer_余剰引数でも単位正規化と入力error表示を維持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("余剰引数の延期対象").unwrap();
    let task_id = task.get_id().unwrap();

    let valid = execute_command_for_test(task.clone(), now, Some(task_id), "後 2 DAYS extra");
    assert_eq!(valid.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        valid.task.get_pending_until().unwrap(),
        Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap()
    );
    assert_eq!(valid.focused_task_id_opt, None);

    task.set_orig_status(Status::Todo).unwrap();
    let invalid = execute_command_for_test(task.clone(), now, Some(task_id), "後 abc 日 extra");
    assert_eq!(invalid.task.get_orig_status().unwrap(), Status::Todo);
    assert_eq!(invalid.focused_task_id_opt, Some(task_id));
    assert!(invalid.output.contains(
        "[Error] 入力エラー: amount: 整数で指定してください (コマンド: 後, 使い方: 後 <数値> <単位>)"
    ));
}

#[test]
fn test_execute_defer_翌朝計算不能を情報付きerrorにして状態を変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外の延期対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut focused_task_id_opt = Some(task_id);

    let actual = execute_defer(&mut task_repository, &mut focused_task_id_opt, 1, "日");

    assert_eq!(
        actual,
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime: now,
        })
    );
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
}

#[test]
fn test_execute_defer_巨大な日数を即座に情報付きerrorにして状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 21, 12, 0, 0).unwrap();
    assert_eq!(defer_business_day_target(now, 0), Ok(now));
    assert_eq!(defer_business_day_target(now, -1), Ok(now));
    assert_eq!(
        defer_business_day_target(now, i64::MAX),
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "defer_business_days",
            datetime: now,
        })
    );
    let task = new_test_task_handle("巨大日数の延期対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut focused_task_id_opt = Some(task_id);

    let actual = execute_defer(
        &mut task_repository,
        &mut focused_task_id_opt,
        i64::MAX,
        "日",
    );

    assert_eq!(
        actual,
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "defer_business_days",
            datetime: now,
        })
    );
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
}

#[test]
fn test_runtime_defer_shortcut_翌朝計算不能を情報付きerrorにして状態を変更しない() {
    for shortcut in ["next_morning", "next_week", "five_years"] {
        let now = maximum_local_datetime();
        let task = new_test_task_handle("日時範囲外の延期shortcut対象").unwrap();
        let task_id = task.get_id().unwrap();
        let original_snapshot = task.snapshot().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut focused_task_id_opt = Some(task_id);
        let mut context = RuntimeDeferCommandContext {
            task_repository: &mut task_repository,
            focused_task_id_opt: &mut focused_task_id_opt,
            config: active_config(),
        };

        let actual = match shortcut {
            "next_morning" => context.defer_next_morning(),
            "next_week" => context.defer_next_week(),
            "five_years" => context.defer_five_years(),
            _ => unreachable!("test shortcut table must contain supported values"),
        };

        assert!(matches!(
            actual,
            Err(DeferCommandError::Application(
                ApplicationError::SubjectiveDateOutOfRange {
                    operation: "next_business_day_start",
                    datetime,
                }
            )) if datetime == now
        ));
        assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
        assert_eq!(focused_task_id_opt, Some(task_id));
    }
}

#[test]
fn test_execute_defer_expression_曜日の業務日計算不能を情報付きerrorにして状態を変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外の曜日延期対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut focused_task_id_opt = Some(task_id);
    let mut context = RuntimeDeferCommandContext {
        task_repository: &mut task_repository,
        focused_task_id_opt: &mut focused_task_id_opt,
        config: active_config(),
    };

    let actual = context.defer_expression(&["月".to_string()]);

    assert!(matches!(
        actual,
        Err(DeferCommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "next_business_day_start",
                datetime,
            }
        )) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
}

#[test]
fn test_execute_defer_expression_mmddの日時errorを伝搬して状態を変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外のMMDD延期対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut focused_task_id_opt = Some(task_id);
    let stdout = TestWriter::new();
    let mut context = RuntimeDeferCommandContext {
        task_repository: &mut task_repository,
        focused_task_id_opt: &mut focused_task_id_opt,
        config: active_config(),
    };

    let actual = context.defer_expression(&["12/31".to_string()]);

    assert!(matches!(
        actual,
        Err(DeferCommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "upcoming_calendar_date",
                datetime,
            }
        )) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(stdout.into_string().is_empty());
}

#[test]
fn test_execute_defer_expression_不正なcalendar時刻を変更せず拒否する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let task = new_test_task_handle("不正calendar時刻の延期対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut focused_task_id_opt = Some(task_id);
    let stdout = TestWriter::new();
    let mut context = RuntimeDeferCommandContext {
        task_repository: &mut task_repository,
        focused_task_id_opt: &mut focused_task_id_opt,
        config: active_config(),
    };

    let actual = context.defer_expression(&["13:99".to_string()]);

    assert!(actual.is_ok());
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(stdout.into_string().is_empty());
}

#[test]
fn test_execute_defer_expression_同日と24時超過を現在calendar日基準で解釈する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    for (value, expected) in [
        (
            "13:30",
            Local.with_ymd_and_hms(2026, 8, 14, 13, 30, 1).unwrap(),
        ),
        (
            "25:30",
            Local.with_ymd_and_hms(2026, 8, 15, 1, 30, 1).unwrap(),
        ),
    ] {
        let task = new_test_task_handle("時刻指定の延期対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut focused_task_id_opt = Some(task_id);
        let mut context = RuntimeDeferCommandContext {
            task_repository: &mut task_repository,
            focused_task_id_opt: &mut focused_task_id_opt,
            config: active_config(),
        };

        let actual = context.defer_expression(&[value.to_string()]);

        assert!(actual.is_ok());
        assert_eq!(task_repository.task.get_pending_until().unwrap(), expected);
        assert_eq!(focused_task_id_opt, None);
    }
}

#[test]
fn test_execute_defer_routine_翌朝計算不能を情報付きerrorにして親子とfocusを変更しない() {
    let orig_deadline = maximum_local_datetime();
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let parent = new_test_task_handle("反復routine親").unwrap();
    parent.set_repetition_interval_days_opt(Some(7)).unwrap();
    parent
        .set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 8, 20, 18, 0, 0).unwrap()))
        .unwrap();
    let mut child_attr = new_test_task_attr("延期対象routine子");
    child_attr.set_deadline_time_opt(Some(orig_deadline));
    let child = parent.create_as_last_child(child_attr);
    let child_id = child.get_id().unwrap();
    let parent_snapshot = parent.snapshot().unwrap();
    let child_snapshot = child.snapshot().unwrap();
    let child_ids = parent
        .get_children()
        .unwrap()
        .into_iter()
        .map(|task| task.get_id().unwrap())
        .collect::<Vec<_>>();
    let mut task_repository = TestTaskRepository::new(parent.clone(), now);
    let mut focused_task_id_opt = Some(child_id);
    let mut context = RuntimeDeferCommandContext {
        task_repository: &mut task_repository,
        focused_task_id_opt: &mut focused_task_id_opt,
        config: active_config(),
    };

    let actual = context.defer_routine();

    assert_eq!(
        actual,
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime: orig_deadline,
        })
    );
    assert_eq!(parent.snapshot().unwrap(), parent_snapshot);
    assert_eq!(child.snapshot().unwrap(), child_snapshot);
    assert_eq!(
        parent
            .get_children()
            .unwrap()
            .into_iter()
            .map(|task| task.get_id().unwrap())
            .collect::<Vec<_>>(),
        child_ids
    );
    assert_eq!(focused_task_id_opt, Some(child_id));
}

#[test]
fn test_execute_defer_routine_親の反復間隔と任意deadline時刻で延期する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let orig_deadline = Local.with_ymd_and_hms(2026, 8, 13, 10, 0, 0).unwrap();
    let orig_start = Local.with_ymd_and_hms(2026, 8, 10, 9, 0, 0).unwrap();
    let expected_start = Local.with_ymd_and_hms(2026, 8, 17, 9, 0, 0).unwrap();

    for (parent_deadline, expected_deadline) in [
        (
            Some(Local.with_ymd_and_hms(2026, 8, 20, 18, 0, 0).unwrap()),
            Local.with_ymd_and_hms(2026, 8, 20, 18, 0, 0).unwrap(),
        ),
        (None, Local.with_ymd_and_hms(2026, 8, 20, 10, 0, 0).unwrap()),
    ] {
        let parent = new_test_task_handle("正常反復routine親").unwrap();
        parent.set_repetition_interval_days_opt(Some(7)).unwrap();
        parent.set_deadline_time_opt(parent_deadline).unwrap();
        let mut child_attr = new_test_task_attr("正常延期routine子");
        child_attr.set_deadline_time_opt(Some(orig_deadline));
        child_attr.set_start_time(orig_start);
        child_attr.set_orig_status(Status::Pending);
        let child = parent.create_as_last_child(child_attr);
        let child_id = child.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(parent, now);
        let mut focused_task_id_opt = Some(child_id);
        let mut context = RuntimeDeferCommandContext {
            task_repository: &mut task_repository,
            focused_task_id_opt: &mut focused_task_id_opt,
            config: active_config(),
        };

        let actual = context.defer_routine();

        assert_eq!(actual, Ok(()));
        assert_eq!(
            child.get_deadline_time_opt().unwrap(),
            Some(expected_deadline)
        );
        assert_eq!(child.get_start_time().unwrap(), expected_start);
        assert_eq!(child.get_orig_status().unwrap(), Status::Todo);
        assert_eq!(focused_task_id_opt, None);
    }
}

#[test]
fn test_execute_deadline_翌朝計算不能を情報付きerrorにして状態を変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外のdeadline対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    let actual = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "〆 明",
    );

    assert!(matches!(
        actual,
        Err(CommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "next_business_day_start",
                datetime,
            }
        )) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(stdout.into_string().is_empty());
}

#[test]
fn test_resolve_deadline_date_今は最大日時でも同じcalendar日を返す() {
    let now = maximum_local_datetime();
    let expected = now.format("%Y/%m/%d").to_string();

    assert!(matches!(
        resolve_deadline_date("今", now),
        Ok(actual) if actual == expected
    ));
}

#[test]
fn test_resolve_deadline_date_曜日の範囲外を曜日計算errorにする() {
    let now = maximum_local_datetime();

    assert!(matches!(
        resolve_deadline_date("月", now),
        Err(CommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "deadline_weekday_date",
                datetime,
            }
        )) if datetime == now
    ));
}

#[test]
fn test_resolve_deadline_date_曜日は同じ曜日を7日後にする() {
    let now = Local.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();

    assert!(matches!(
        resolve_deadline_date("月", now),
        Ok(actual) if actual == "2026/08/24"
    ));
    assert!(matches!(
        resolve_deadline_date("火", now),
        Ok(actual) if actual == "2026/08/18"
    ));
}

#[test]
fn test_resolve_deadline_date_mmddは同日正午を過ぎると翌年を選ぶ() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 13, 0, 0).unwrap();

    assert!(matches!(
        resolve_deadline_date("8/14", now),
        Ok(actual) if actual == "2027/08/14"
    ));
}

#[test]
fn test_resolve_deadline_date_mmddは同日正午直前なら現在年を選ぶ() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 11, 59, 59).unwrap();

    assert!(matches!(
        resolve_deadline_date("8/14", now),
        Ok(actual) if actual == "2026/08/14"
    ));
}

#[test]
fn test_resolve_deadline_date_mmddは同日正午ちょうどなら現在年を選ぶ() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    assert!(matches!(
        resolve_deadline_date("8/14", now),
        Ok(actual) if actual == "2026/08/14"
    ));
}

#[test]
fn test_resolve_deadline_date_mmddの翌年範囲外を情報付きerrorにする() {
    let now = maximum_local_datetime()
        .checked_add_signed(Duration::hours(1))
        .unwrap();

    assert!(matches!(
        resolve_deadline_date("12/31", now),
        Err(CommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "deadline_calendar_date",
                datetime,
            }
        )) if datetime == now
    ));
}

#[test]
fn test_execute_finish_未完了の子があれば完了しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.create_as_last_child(new_test_task_attr("未完了の子"));

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(parent_task.get_id().unwrap()),
        "終 今",
    );

    assert_ne!(result.task.get_status().unwrap(), Status::Done);
    assert_eq!(result.task.get_end_time_opt().unwrap(), None);
    assert_eq!(
        result.focused_task_id_opt,
        Some(parent_task.get_id().unwrap())
    );
    assert!(result.output.contains("未完了の子"));
}

#[test]
fn test_execute_finish_未完了の子があれば不正引数でもtreeを表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.create_as_last_child(new_test_task_attr("未完了の子"));

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(parent_task.get_id().unwrap()),
        "終 invalid",
    );

    assert_ne!(result.task.get_status().unwrap(), Status::Done);
    assert_eq!(result.task.get_end_time_opt().unwrap(), None);
    assert_eq!(
        result.focused_task_id_opt,
        Some(parent_task.get_id().unwrap())
    );
    assert!(result.output.contains("未完了の子"));
}

#[test]
fn test_execute_finish_唯一の子を完了すると親へfocusする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    let child_task = parent_task.create_as_last_child(new_test_task_attr("子タスク"));

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(child_task.get_id().unwrap()),
        "終 今",
    );
    let finished_child = result
        .task
        .get_by_id(child_task.get_id().unwrap())
        .unwrap()
        .expect("finished child must remain in the fixture tree");

    assert_eq!(finished_child.get_status().unwrap(), Status::Done);
    assert_eq!(finished_child.get_end_time_opt().unwrap(), Some(now));
    assert_eq!(
        result.focused_task_id_opt,
        Some(parent_task.get_id().unwrap())
    );
}

#[test]
fn test_execute_finish_繰り返しtaskの見積もりを実績との差に応じて補正する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let cases = [(1_000, 900), (200, 500), (600, 600)];

    for (actual_work_seconds, expected_estimated_work_seconds) in cases {
        let parent_task = new_test_task_handle("繰り返しtask").unwrap();
        parent_task.set_repetition_interval_days_opt(Some(7));
        parent_task.set_estimated_work_seconds(600);
        let mut child_attr = new_test_task_attr("今回分");
        child_attr.set_actual_work_seconds(actual_work_seconds);
        let child_task = parent_task.create_as_last_child(child_attr);

        let result = execute_command_for_test(
            parent_task,
            now,
            Some(child_task.get_id().unwrap()),
            "終 今",
        );

        assert_eq!(
            result.task.get_estimated_work_seconds().unwrap(),
            expected_estimated_work_seconds
        );
    }
}

#[test]
fn test_execute_repetition_数値だけの名前は拒否して元taskを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("既存タスク").unwrap();
    task.set_estimated_work_seconds(45 * 60);

    let result = execute_command_for_test(
        task.clone(),
        now,
        Some(task.get_id().unwrap()),
        "繰 123 10 毎 09:00 10:00",
    );

    assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 45 * 60);
    assert!(result
        .task
        .get_children()
        .expect("command result tree must be readable")
        .is_empty());
    assert_eq!(result.focused_task_id_opt, Some(task.get_id().unwrap()));
}

#[test]
fn test_execute_new_数値だけの名前は拒否して元taskを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("既存タスク").unwrap();

    let result =
        execute_command_for_test(task.clone(), now, Some(task.get_id().unwrap()), "新 123 10");

    assert_eq!(result.task.get_id().unwrap(), task.get_id().unwrap());
    assert_eq!(result.task.get_name().unwrap(), "既存タスク");
    assert_eq!(result.focused_task_id_opt, Some(task.get_id().unwrap()));
}

#[test]
fn test_execute_repetition_不正な見積もりでは元taskを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for estimated_work_minutes in ["-1", "9223372036854775807"] {
        let task = new_test_task_handle("既存タスク").unwrap();
        task.set_estimated_work_seconds(45 * 60);
        let command = format!("繰 反復 {estimated_work_minutes} 毎 09:00 10:00");

        let result =
            execute_command_for_test(task.clone(), now, Some(task.get_id().unwrap()), &command);

        assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 45 * 60);
        assert!(result
            .task
            .get_children()
            .expect("command result tree must be readable")
            .is_empty());
        assert_eq!(result.focused_task_id_opt, Some(task.get_id().unwrap()));
    }
}

#[test]
fn test_execute_estimate_見積もりを更新し不正値では維持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();

    let updated = execute_command_for_test(task, now, Some(task_id), "予 45");
    assert_eq!(updated.task.get_estimated_work_seconds().unwrap(), 45 * 60);

    let unchanged = execute_command_for_test(updated.task, now, Some(task_id), "予 invalid");
    assert_eq!(
        unchanged.task.get_estimated_work_seconds().unwrap(),
        45 * 60
    );
}

#[test]
fn test_execute_estimate_不正値はfield付き入力エラーを表示して状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    task.set_estimated_work_seconds(45 * 60);
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "予 invalid");

    assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 45 * 60);
    assert_eq!(result.focused_task_id_opt, Some(task_id));
    assert!(result
        .output
        .contains("[Error] 入力エラー: estimated_work_minutes:"));
}

#[test]
fn test_execute_actual_priority_work_typed値でtaskを更新する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();

    let actual = execute_command_for_test(task, now, Some(task_id), "実 20");
    assert_eq!(actual.task.get_actual_work_seconds().unwrap(), 20 * 60);

    let prioritized = execute_command_for_test(actual.task, now, Some(task_id), "重 7");
    assert_eq!(prioritized.task.get_priority().unwrap(), 7);

    let worked = execute_command_for_test(prioritized.task, now, Some(task_id), "働 5");
    assert_eq!(worked.task.get_actual_work_seconds().unwrap(), 25 * 60);
    assert_eq!(worked.focused_task_id_opt, None);
}

#[test]
fn test_execute_actual_priority_work_不正値はfield付き入力エラーで状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    for command in ["実 invalid", "重 invalid", "働 invalid"] {
        let task = new_test_task_handle("更新対象").unwrap();
        task.set_actual_work_seconds(20 * 60);
        task.set_priority(7);
        let task_id = task.get_id().unwrap();

        let result = execute_command_for_test(task, now, Some(task_id), command);

        assert_eq!(result.task.get_actual_work_seconds().unwrap(), 20 * 60);
        assert_eq!(result.task.get_priority().unwrap(), 7);
        assert_eq!(result.focused_task_id_opt, Some(task_id));
        assert!(result.output.contains("[Error] 入力エラー:"));
    }
}

#[test]
fn test_execute_actual_work_overflowはfield付きerrorで状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    for command in [
        format!("実 {}", i64::MAX),
        format!("実 {}", i64::MIN),
        format!("働 {}", i64::MAX),
        format!("働 {}", i64::MIN),
    ] {
        let task = new_test_task_handle("更新対象").unwrap();
        task.set_actual_work_seconds(20 * 60);
        let task_id = task.get_id().unwrap();

        let result = execute_command_for_test(task, now, Some(task_id), &command);

        assert_eq!(result.task.get_actual_work_seconds().unwrap(), 20 * 60);
        assert_eq!(result.focused_task_id_opt, Some(task_id));
        assert!(result.output.contains("actual_work_minutes"));
    }
}

#[test]
fn test_execute_deadline_締切を設定して解除する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();

    let updated = execute_command_for_test(task, now, Some(task_id), "〆 2026/08/20");
    assert_eq!(
        updated.task.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap())
    );

    let cleared = execute_command_for_test(updated.task, now, Some(task_id), "〆 消");
    assert_eq!(cleared.task.get_deadline_time_opt().unwrap(), None);

    let time_updated = execute_command_for_test(cleared.task, now, Some(task_id), "〆 14:30");
    assert_eq!(
        time_updated.task.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 8, 11, 14, 30, 0).unwrap())
    );

    let invalid = execute_command_for_test(time_updated.task, now, Some(task_id), "〆 invalid");
    assert_eq!(
        invalid.task.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 8, 11, 14, 30, 0).unwrap())
    );

    let today_task = new_test_task_handle("今日締切").unwrap();
    let today_task_id = today_task.get_id().unwrap();
    let today = execute_command_for_test(today_task, now, Some(today_task_id), "〆 今日");
    assert_eq!(
        today.task.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 8, 11, 23, 59, 59).unwrap())
    );

    let tomorrow_task = new_test_task_handle("明日締切").unwrap();
    let tomorrow_task_id = tomorrow_task.get_id().unwrap();
    let tomorrow = execute_command_for_test(tomorrow_task, now, Some(tomorrow_task_id), "〆 明日");
    assert_eq!(
        tomorrow.task.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 8, 12, 23, 59, 59).unwrap())
    );
}

#[test]
fn test_execute_deadline_不正日時はfield付き入力エラーを表示して状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();
    let previous_deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
    task.set_deadline_time_opt(Some(previous_deadline));

    let result = execute_command_for_test(task, now, Some(task_id), "〆 invalid");

    assert_eq!(
        result.task.get_deadline_time_opt().unwrap(),
        Some(previous_deadline)
    );
    assert!(result.output.contains("[Error] 入力エラー: deadline:"));

    for command in ["〆 13/40", "〆 25:99"] {
        let result = execute_command_for_test(result.task.clone(), now, Some(task_id), command);
        assert_eq!(
            result.task.get_deadline_time_opt().unwrap(),
            Some(previous_deadline)
        );
        assert!(result.output.contains("コマンド: 〆"));
        assert!(result.output.contains("使い方: 〆 <日付または時刻>"));
    }
}

#[test]
fn test_execute_arrange_デフォルトで見積もり0と完了済みを維持する() {
    let task = execute_arrange_command("揃 15");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_全指定で見積もり0も変更し完了済みは維持する() {
    let task = execute_arrange_command("揃 15 全");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_all指定は全指定と同じ挙動になる() {
    let task = execute_arrange_command("arr 15 all");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_未知の第3引数で見積もり0を維持する() {
    let task = execute_arrange_command("揃 15 unknown");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_見積もり0分を受理する() {
    let task = execute_arrange_command("揃 0");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_見積もり1439分を受理する() {
    let task = execute_arrange_command("揃 1439");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 1439 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_見積もり1440分では変更しない() {
    let task = execute_arrange_command("揃 1440");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 5 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_負の見積もりでは変更しない() {
    let task = execute_arrange_command("揃 -1");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 5 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_sequential_接尾辞の前にハイフンを付ける() {
    let (task, focused_task_id_opt) = execute_sequential_command("連 鎖タスク 10 1 2 話");

    let children = task
        .get_children()
        .expect("sequential result tree must be readable");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_name().unwrap(), "鎖タスク 2-話");

    let grand_children = children[0]
        .get_children()
        .expect("sequential result subtree must be readable");
    assert_eq!(grand_children.len(), 1);
    assert_eq!(grand_children[0].get_name().unwrap(), "鎖タスク 1-話");
    assert_eq!(
        focused_task_id_opt,
        Some(grand_children[0].get_id().unwrap())
    );
}

#[test]
fn test_execute_sequential_接尾辞なしではハイフンを付けない() {
    let (task, focused_task_id_opt) = execute_sequential_command("連 鎖タスク 10 1 2");

    let children = task
        .get_children()
        .expect("sequential result tree must be readable");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_name().unwrap(), "鎖タスク 2");

    let grand_children = children[0]
        .get_children()
        .expect("sequential result subtree must be readable");
    assert_eq!(grand_children.len(), 1);
    assert_eq!(grand_children[0].get_name().unwrap(), "鎖タスク 1");
    assert_eq!(
        focused_task_id_opt,
        Some(grand_children[0].get_id().unwrap())
    );
}

#[test]
fn test_execute_finish_引数なしは実作業時間を自動加算して現在時刻で完了する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_actual_work_seconds(60);
    let task_id = task.get_id().unwrap();
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

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(actual.get_status().unwrap(), Status::Done);
    assert_eq!(actual.get_actual_work_seconds().unwrap(), 360);
    assert_eq!(actual.get_end_time_opt().unwrap(), Some(now));
}

#[test]
fn test_execute_finish_今は実作業時間を自動加算せず現在時刻で完了する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_actual_work_seconds(60);
    let task_id = task.get_id().unwrap();
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

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(actual.get_status().unwrap(), Status::Done);
    assert_eq!(actual.get_actual_work_seconds().unwrap(), 60);
    assert_eq!(actual.get_end_time_opt().unwrap(), Some(now));
}

#[test]
fn test_execute_finish_時刻指定は実作業時間を自動加算せず指定時刻で完了する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_actual_work_seconds(60);
    let task_id = task.get_id().unwrap();
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

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(actual.get_status().unwrap(), Status::Done);
    assert_eq!(actual.get_actual_work_seconds().unwrap(), 60);
    assert_eq!(
        actual.get_end_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 5, 17, 14, 30, 0).unwrap())
    );
}

#[test]
fn test_execute_finish_秒つき時刻指定は指定秒で完了する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_actual_work_seconds(60);
    let task_id = task.get_id().unwrap();
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

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(actual.get_status().unwrap(), Status::Done);
    assert_eq!(actual.get_actual_work_seconds().unwrap(), 60);
    assert_eq!(
        actual.get_end_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 7, 4, 9, 23, 45).unwrap())
    );
}

#[test]
fn test_execute_finish_不正な引数では完了しない() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_actual_work_seconds(60);
    let task_id = task.get_id().unwrap();
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

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(actual.get_status().unwrap(), Status::Todo);
    assert_eq!(actual.get_actual_work_seconds().unwrap(), 60);
    assert_eq!(actual.get_end_time_opt().unwrap(), None);
}

#[test]
fn test_execute_today_カテゴリ別の予定時間集計を表示する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = new_test_task_handle("投資タスク").unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Investment));
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    let task_id = task.get_id().unwrap();
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
    let task = new_test_task_handle("タスク").unwrap();
    let task_id = task.get_id().unwrap();
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

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(
        actual.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Investment)
    );
}

#[test]
fn test_execute_set_project_category_英語aliasでカテゴリを設定する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = new_test_task_handle("タスク").unwrap();
    let task_id = task.get_id().unwrap();
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

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(
        actual.get_project_category_opt().unwrap(),
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

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(
        actual.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Consumption)
    );
}

#[test]
fn test_execute_set_project_category_未分類に戻す() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = new_test_task_handle("タスク").unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Investment));
    let task_id = task.get_id().unwrap();
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

        let actual = task_repository
            .get_by_id(task_id)
            .expect("fixture repository lookup must succeed")
            .expect("fixture task must exist");
        assert_eq!(actual.get_project_category_opt().unwrap(), None);
    }
}

#[test]
fn test_execute_set_project_category_不正カテゴリでは変更しない() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = new_test_task_handle("タスク").unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Investment));
    let task_id = task.get_id().unwrap();
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

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(
        actual.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Investment)
    );
}

#[test]
fn test_execute_category_不正値はfield付き入力エラーを表示して状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("カテゴリ対象").unwrap();
    let task_id = task.get_id().unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Investment));

    let result = execute_command_for_test(task, now, Some(task_id), "類 invalid");

    assert_eq!(
        result.task.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Investment)
    );
    assert!(result.output.contains("[Error] 入力エラー: category:"));
}

#[cfg(test)]
fn execute_pack(
    stdout: &mut dyn SchronuWriter,
    task_repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) {
    let result = pack_tasks_with_end_of_day_offset_minutes(
        task_repository,
        free_time_manager,
        active_config().end_of_day_offset_minutes,
    )
    .unwrap();
    write_pack_result(stdout, &result);
}

#[cfg(test)]
fn execute(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &DateTime<Local>,
    untrimmed_line: &str,
) -> Result<(), CommandError> {
    let parsed_command = parse_command(untrimmed_line, ParseMode::NonInteractive)
        .map_err(map_command_parse_error)?;
    execute_parsed(
        stdout,
        task_repository,
        free_time_manager,
        focused_task_id_opt,
        focus_started_datetime,
        untrimmed_line,
        &parsed_command,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_parsed(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &DateTime<Local>,
    untrimmed_line: &str,
    parsed_command: &Command,
) -> Result<(), CommandError> {
    validate_non_interactive_command(parsed_command)?;
    let operation_now = task_repository.get_last_synced_time();
    validate_contextual_task_attribute_command(parsed_command, operation_now, active_config())?;
    let mut output = ErrorCapturingWriter::new(stdout);
    let mut next_id = Uuid::new_v4;
    let mut task_factory = TaskFactory::new(operation_now, &mut next_id);
    let project_or_breakdown_outcome = {
        let mut context = RuntimeProjectCommandContext {
            task_repository,
            focused_task_id_opt,
            task_factory: &mut task_factory,
        };
        match handle_project_command(parsed_command, &mut context)? {
            Some(outcome) => Some(outcome),
            None => handle_breakdown_split_command(parsed_command, &mut context)?,
        }
    };
    if let Some(outcome) = project_or_breakdown_outcome {
        apply_command_outcome(
            &mut output,
            task_repository,
            focused_task_id_opt,
            OutcomeApplicationMode::Flushed,
            outcome,
            active_config(),
        )?;
    } else if let Some(outcome) = {
        let mut context = RuntimeTaskAttributeCommandContext {
            task_repository,
            focused_task_id_opt,
            focus_started_datetime,
            config: active_config(),
        };
        handle_task_attribute_command(parsed_command, &mut context)?
    } {
        apply_command_outcome(
            &mut output,
            task_repository,
            focused_task_id_opt,
            OutcomeApplicationMode::Flushed,
            outcome,
            active_config(),
        )?;
    } else if let Some(outcome) = {
        let mut context = RuntimeDeferCommandContext {
            task_repository,
            focused_task_id_opt,
            config: active_config(),
        };
        handle_defer_command(parsed_command, &mut context)?
    } {
        apply_command_outcome(
            &mut output,
            task_repository,
            focused_task_id_opt,
            OutcomeApplicationMode::Flushed,
            outcome,
            active_config(),
        )?;
    } else if let Some(outcome) = {
        let supports_ansi_color = output.supports_ansi_color();
        let mut context = RuntimeFinishPlacementCommandContext {
            task_repository,
            free_time_manager,
            focused_task_id_opt,
            task_factory: &mut task_factory,
            focus_started_datetime: *focus_started_datetime,
            config: active_config(),
            supports_ansi_color,
        };
        handle_finish_placement_command(parsed_command, &mut context)?
    } {
        apply_command_outcome(
            &mut output,
            task_repository,
            focused_task_id_opt,
            OutcomeApplicationMode::Flushed,
            outcome,
            active_config(),
        )?;
    } else if let Some(outcome) = {
        let supports_ansi_color = output.supports_ansi_color();
        let mut context = RuntimeTaskTreeCommandContext {
            task_repository,
            free_time_manager,
            focused_task_id_opt,
            task_factory: &mut task_factory,
            config: active_config(),
            supports_ansi_color,
        };
        handle_task_tree_command(parsed_command, &mut context)?
    } {
        apply_command_outcome(
            &mut output,
            task_repository,
            focused_task_id_opt,
            OutcomeApplicationMode::Flushed,
            outcome,
            active_config(),
        )?;
    } else if let Some(outcome) = handle(parsed_command) {
        apply_command_outcome(
            &mut output,
            task_repository,
            focused_task_id_opt,
            OutcomeApplicationMode::Flushed,
            outcome,
            active_config(),
        )?;
    } else {
        execute_with_config(
            &mut output,
            task_repository,
            free_time_manager,
            focused_task_id_opt,
            focus_started_datetime,
            untrimmed_line,
            parsed_command,
            active_config(),
        )?;
    }
    match output.take_error() {
        Some(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Some(error) => Err(CommandError::Output(error)),
        None => Ok(()),
    }
}

struct RuntimeProjectCommandContext<'repository, 'factory, 'generator> {
    task_repository: &'repository mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &'repository mut Option<Uuid>,
    task_factory: &'factory mut TaskFactory<'generator>,
}

impl ProjectCommandContext for RuntimeProjectCommandContext<'_, '_, '_> {
    fn last_synced_time(&self) -> DateTime<Local> {
        self.task_repository.get_last_synced_time()
    }

    fn focused_task(&mut self) -> Result<Option<TaskHandle>, ApplicationError> {
        match self.focused_task_id_opt {
            Some(id) => self
                .task_repository
                .get_by_id(*id)
                .map_err(ApplicationError::TaskTree),
            None => Ok(None),
        }
    }

    fn create_task(&mut self, input: CreateTaskInput) -> Result<Uuid, ApplicationError> {
        create_task(self.task_repository, input, self.task_factory)
    }

    fn breakdown_task(&mut self, input: BreakdownTaskInput) -> Result<Vec<Uuid>, ApplicationError> {
        breakdown_task(self.task_repository, input, self.task_factory)
    }

    fn create_task_attr(&mut self, name: &str) -> TaskAttr {
        self.task_factory.create_task_attr(name)
    }

    fn set_estimate(&mut self, task_id: Uuid, minutes: i64) -> Result<(), ApplicationError> {
        set_estimate(self.task_repository, task_id, minutes)
    }

    fn focused_task_id(&self) -> Option<Uuid> {
        *self.focused_task_id_opt
    }

    fn set_focused_task_id(&mut self, task_id_opt: Option<Uuid>) {
        *self.focused_task_id_opt = task_id_opt;
    }
}

struct RuntimeTaskAttributeCommandContext<'a> {
    task_repository: &'a mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &'a mut Option<Uuid>,
    focus_started_datetime: &'a DateTime<Local>,
    config: &'a SchronuConfig,
}

impl RuntimeTaskAttributeCommandContext<'_> {
    fn focused_task(&self) -> Result<Option<TaskHandle>, ApplicationError> {
        match *self.focused_task_id_opt {
            Some(id) => self
                .task_repository
                .get_by_id(id)
                .map_err(ApplicationError::TaskTree),
            None => Ok(None),
        }
    }
}

impl TaskAttributeCommandContext for RuntimeTaskAttributeCommandContext<'_> {
    fn set_deadline(&mut self, value: &str) -> Result<(), ApplicationError> {
        let deadline_time = resolve_deadline_time(
            value,
            self.task_repository.get_last_synced_time(),
            self.config,
        )
        .expect("task attribute command must be contextually validated before handling");
        if let Some(task_id) = *self.focused_task_id_opt {
            set_deadline(self.task_repository, task_id, deadline_time)?;
        }
        Ok(())
    }

    fn set_estimate(&mut self, minutes: i64) -> Result<(), ApplicationError> {
        if let Some(task_id) = *self.focused_task_id_opt {
            set_estimate(self.task_repository, task_id, minutes)?;
        }
        Ok(())
    }

    fn arrange(
        &mut self,
        minutes: i64,
        includes_zero_estimate: bool,
    ) -> Result<(), ApplicationError> {
        execute_set_arrange_children_work_minutes(
            &self.focused_task()?,
            minutes,
            includes_zero_estimate,
        )
    }

    fn set_actual(&mut self, minutes: i64) -> Result<(), ApplicationError> {
        set_focused_task_actual_work_minutes(&self.focused_task()?, minutes)
    }

    fn set_priority(&mut self, priority: i64) -> Result<(), ApplicationError> {
        set_focused_task_priority(&self.focused_task()?, priority)
    }

    fn set_category(&mut self, value: &str) -> Result<(), ApplicationError> {
        let project_category = read_project_category_command_arg(value)
            .expect("task attribute command must be validated before handling");
        if let Some(task_id) = *self.focused_task_id_opt {
            set_category(self.task_repository, task_id, project_category)?;
        }
        Ok(())
    }

    fn add_work(&mut self, minutes: Option<i64>) -> Result<(), ApplicationError> {
        let additional_minutes = minutes.unwrap_or_else(|| {
            (self.task_repository.get_last_synced_time() - *self.focus_started_datetime)
                .num_minutes()
                + 1
        });
        if let Some(focused_task) = self.focused_task()? {
            let original_minutes = focused_task
                .get_actual_work_seconds()
                .map_err(ApplicationError::TaskTree)?
                / 60;
            let total_minutes = original_minutes.checked_add(additional_minutes).ok_or(
                ApplicationError::InvalidInput {
                    field: "additional_actual_work_minutes",
                    reason: "actual work minutes overflow",
                },
            )?;
            set_focused_task_actual_work_minutes(&Some(focused_task), total_minutes)?;
            *self.focused_task_id_opt = None;
        }
        Ok(())
    }
}

struct RuntimeDeferCommandContext<'a> {
    task_repository: &'a mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &'a mut Option<Uuid>,
    config: &'a SchronuConfig,
}

impl RuntimeDeferCommandContext<'_> {
    fn focused_task(&self) -> Result<Option<TaskHandle>, ApplicationError> {
        match *self.focused_task_id_opt {
            Some(id) => self
                .task_repository
                .get_by_id(id)
                .map_err(ApplicationError::TaskTree),
            None => Ok(None),
        }
    }
}

impl DeferCommandContext for RuntimeDeferCommandContext<'_> {
    fn defer(&mut self, amount: i64, unit: &str) -> Result<(), DeferCommandError> {
        execute_defer(self.task_repository, self.focused_task_id_opt, amount, unit)
            .map_err(DeferCommandError::from)
    }

    fn defer_expression(&mut self, values: &[String]) -> Result<(), DeferCommandError> {
        execute_defer_expression(self.task_repository, self.focused_task_id_opt, values)
    }

    fn defer_next_morning(&mut self) -> Result<(), DeferCommandError> {
        let now = self.task_repository.get_last_synced_time();
        let seconds = seconds_until_next_business_day_start_with_offset(now, 1)?;
        self.defer(seconds, "秒")
    }

    fn defer_next_week(&mut self) -> Result<(), DeferCommandError> {
        let now = self.task_repository.get_last_synced_time();
        let seconds = seconds_until_next_business_day_start_with_offset(now, 86400 * 6 + 1)?;
        self.defer(seconds, "秒")
    }

    fn defer_routine(&mut self) -> Result<(), ApplicationError> {
        execute_defer_routine(self.task_repository, self.focused_task_id_opt)
    }

    fn defer_five_years(&mut self) -> Result<(), DeferCommandError> {
        let now = self.task_repository.get_last_synced_time();
        let seconds =
            seconds_until_next_business_day_start_with_offset(now, 86400 * (7 * 52 * 5 - 1) + 1)?;
        self.defer(seconds, "秒")
    }

    fn defer_all_frequent_routines(&mut self) -> Result<(), ApplicationError> {
        let focused_task = self.focused_task()?;
        execute_defer_all_frequent_routines(
            self.task_repository,
            self.focused_task_id_opt,
            &focused_task,
        )
    }

    fn prepare_escape(&mut self) -> Result<bool, ApplicationError> {
        if let Some(focused_task) = self.focused_task()? {
            let estimated_work_seconds = focused_task
                .get_estimated_work_seconds()
                .map_err(ApplicationError::TaskTree)?;
            focused_task
                .set_estimated_work_seconds(estimated_work_seconds * 2)
                .map_err(ApplicationError::TaskTree)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn extrude(&mut self, step_days: Option<u16>) -> Result<(), ApplicationError> {
        let Some(step_days) = step_days else {
            return Ok(());
        };
        let focused_task = self.focused_task()?;
        let Some(task) = focused_task.as_ref() else {
            return Ok(());
        };
        let ancestors = task
            .list_all_parent_tasks_with_first_available_time()
            .map_err(ApplicationError::TaskTree)?;
        let Some((first_datetime, _)) = ancestors.first() else {
            return Ok(());
        };
        execute_extrude_with_config(
            self.focused_task_id_opt,
            &focused_task,
            first_datetime,
            step_days,
            self.config,
        )
    }

    fn clear_or_gather(
        &mut self,
        kind: CommandKind,
        values: &[String],
    ) -> Result<(), ApplicationError> {
        execute_clear_or_gather(self.task_repository, kind, values)
    }
}

struct RuntimeFinishPlacementCommandContext<'repository, 'factory, 'generator> {
    task_repository: &'repository mut dyn TaskRepositoryTrait,
    free_time_manager: &'repository mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &'repository mut Option<Uuid>,
    task_factory: &'factory mut TaskFactory<'generator>,
    focus_started_datetime: DateTime<Local>,
    config: &'repository SchronuConfig,
    supports_ansi_color: bool,
}

impl FinishPlacementCommandContext for RuntimeFinishPlacementCommandContext<'_, '_, '_> {
    fn supports_ansi_color(&self) -> bool {
        self.supports_ansi_color
    }

    fn last_synced_time(&self) -> DateTime<Local> {
        self.task_repository.get_last_synced_time()
    }

    fn focus_started_datetime(&self) -> DateTime<Local> {
        self.focus_started_datetime
    }

    fn focused_task(&self) -> Result<Option<TaskHandle>, ApplicationError> {
        match *self.focused_task_id_opt {
            Some(task_id) => self
                .task_repository
                .get_by_id(task_id)
                .map_err(ApplicationError::TaskTree),
            None => Ok(None),
        }
    }

    fn show_focused_tree(
        &mut self,
        display: &mut dyn SchronuWriter,
    ) -> Result<(), ApplicationError> {
        execute_show_tree(display, &self.focused_task()?)
    }

    fn complete_focused_task(
        &mut self,
        input: CompleteTaskInput,
    ) -> Result<Option<Uuid>, ApplicationError> {
        complete_task(self.task_repository, input, self.task_factory)
            .map(|output| output.next_focus_task_id)
    }

    fn set_focused_task_id(&mut self, task_id_opt: Option<Uuid>) {
        *self.focused_task_id_opt = task_id_opt;
    }

    fn pack(&mut self) -> Result<PackResult, ApplicationError> {
        pack_tasks_with_end_of_day_offset_minutes(
            self.task_repository,
            self.free_time_manager,
            self.config.end_of_day_offset_minutes,
        )
    }

    fn flatten(&mut self) -> Result<FlattenResult, ApplicationError> {
        flatten_tasks_with_end_of_day_offset_minutes(
            self.task_repository,
            self.free_time_manager,
            self.config.end_of_day_offset_minutes,
        )
    }
}

struct RuntimeTaskTreeCommandContext<'repository, 'factory, 'generator> {
    task_repository: &'repository mut dyn TaskRepositoryTrait,
    free_time_manager: &'repository mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &'repository mut Option<Uuid>,
    task_factory: &'factory mut TaskFactory<'generator>,
    config: &'repository SchronuConfig,
    supports_ansi_color: bool,
}

impl RuntimeTaskTreeCommandContext<'_, '_, '_> {
    fn focused_task(&self) -> Result<Option<TaskHandle>, ApplicationError> {
        match *self.focused_task_id_opt {
            Some(id) => self
                .task_repository
                .get_by_id(id)
                .map_err(ApplicationError::TaskTree),
            None => Ok(None),
        }
    }
}

impl TaskTreeCommandContext for RuntimeTaskTreeCommandContext<'_, '_, '_> {
    fn supports_ansi_color(&self) -> bool {
        self.supports_ansi_color
    }

    fn show_tree(&mut self, display: &mut dyn SchronuWriter) -> Result<(), ApplicationError> {
        execute_show_tree(display, &self.focused_task()?)
    }

    fn show_ancestor(&mut self, display: &mut dyn SchronuWriter) -> Result<(), ApplicationError> {
        execute_show_ancestor(display, &self.focused_task()?)
    }

    fn focus_root(&mut self) -> Result<(), ApplicationError> {
        if let Some(focused_task) = self.focused_task()? {
            let root_task = focused_task.root().map_err(ApplicationError::TaskTree)?;
            *self.focused_task_id_opt =
                Some(root_task.get_id().map_err(ApplicationError::TaskTree)?);
        }
        Ok(())
    }

    fn show_leaves(&mut self, display: &mut dyn SchronuWriter) -> Result<(), ApplicationError> {
        execute_show_leaf_tasks(display, self.task_repository, self.free_time_manager)
    }

    fn show_task_list(
        &mut self,
        display: &mut dyn SchronuWriter,
        pattern: Option<&str>,
        order: TaskListOrder,
        resolve_pattern: bool,
    ) -> Result<(), ApplicationError> {
        let pattern = pattern
            .map(|pattern| {
                if resolve_pattern {
                    resolve_show_all_pattern(pattern, self.task_repository.get_last_synced_time())
                } else {
                    Ok(pattern.to_string())
                }
            })
            .transpose()?;
        let order = match order {
            TaskListOrder::ScheduledStartDesc => TaskListDisplayOrder::ScheduledStartDesc,
            TaskListOrder::LowPriorityTail => TaskListDisplayOrder::LowPriorityTail,
        };
        execute_show_all_tasks_with_config(
            display,
            self.focused_task_id_opt,
            self.task_repository,
            self.free_time_manager,
            &pattern,
            order,
            self.config,
        )
    }

    fn focus(&mut self, task_id: Uuid) {
        *self.focused_task_id_opt = Some(task_id);
    }

    fn pick(&mut self, task_id: Uuid) -> Result<(), ApplicationError> {
        *self.focused_task_id_opt = Some(task_id);
        if let Some(task) = self
            .task_repository
            .get_by_id(task_id)
            .map_err(ApplicationError::TaskTree)?
        {
            task.set_orig_status(Status::Todo)
                .map_err(ApplicationError::TaskTree)?;
        }
        Ok(())
    }

    fn focus_parent(&mut self) -> Result<(), ApplicationError> {
        if let Some(focused_task) = self.focused_task()? {
            if let Some(parent_task) = focused_task.parent().map_err(ApplicationError::TaskTree)? {
                *self.focused_task_id_opt =
                    Some(parent_task.get_id().map_err(ApplicationError::TaskTree)?);
            }
        }
        Ok(())
    }

    fn focus_children(&mut self, display: &mut dyn SchronuWriter) -> Result<(), ApplicationError> {
        let focused_task_opt = self.focused_task()?;
        if let Some(focused_task) = focused_task_opt.as_ref() {
            let children = focused_task
                .get_children()
                .map_err(ApplicationError::TaskTree)?
                .into_iter()
                .filter_map(|child| match child.get_status() {
                    Ok(Status::Done) => None,
                    Ok(_) => Some(Ok(child)),
                    Err(error) => Some(Err(error)),
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(ApplicationError::TaskTree)?;
            match children.as_slice() {
                [child] => {
                    *self.focused_task_id_opt =
                        Some(child.get_id().map_err(ApplicationError::TaskTree)?);
                }
                [_, _, ..] => execute_show_tree(display, &focused_task_opt)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn focus_deepest(&mut self, display: &mut dyn SchronuWriter) -> Result<(), ApplicationError> {
        let Some(focused_task) = self.focused_task()? else {
            return Ok(());
        };
        let mut deepest_task = focused_task;
        loop {
            let children = deepest_task
                .get_children()
                .map_err(ApplicationError::TaskTree)?
                .into_iter()
                .filter_map(|child| match child.get_status() {
                    Ok(Status::Done) => None,
                    Ok(_) => Some(Ok(child)),
                    Err(error) => Some(Err(error)),
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(ApplicationError::TaskTree)?;
            let [only_child] = children.as_slice() else {
                break;
            };
            deepest_task = only_child.clone();
        }
        *self.focused_task_id_opt =
            Some(deepest_task.get_id().map_err(ApplicationError::TaskTree)?);
        if deepest_task
            .get_children()
            .map_err(ApplicationError::TaskTree)?
            .len()
            > 1
        {
            execute_show_tree(display, &Some(deepest_task))?;
        }
        Ok(())
    }

    fn next_up(
        &mut self,
        display: &mut dyn SchronuWriter,
        name: &str,
        estimated_minutes: Option<i64>,
    ) -> Result<(), ApplicationError> {
        let focused_task_opt = self.focused_task()?;
        let result = execute_next_up(
            display,
            self.focused_task_id_opt,
            &focused_task_opt,
            name,
            &estimated_minutes,
            self.task_factory,
        );
        report_application_result(display, result);
        Ok(())
    }
}

fn apply_command_outcome(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    mut application_mode: OutcomeApplicationMode<'_>,
    outcome: CommandOutcome,
    config: &SchronuConfig,
) -> Result<(), CommandError> {
    if !outcome.display.is_empty() {
        render_display_model(stdout, &outcome.display).map_err(CommandError::Output)?;
    }

    if let Some(request) = outcome.external_request {
        let focused_task_opt = match focused_task_id_opt {
            Some(task_id) => task_repository
                .get_by_id(*task_id)
                .map_err(ApplicationError::TaskTree)?,
            None => None,
        };
        match request {
            ExternalRequest::OpenFocusedLink => execute_open_link(&focused_task_opt)?,
            ExternalRequest::OpenObsidianRootSearch => {
                execute_open_obsidian_root_task_search_with_config(&focused_task_opt, config)?
            }
        }
    }

    if let Some(request) = outcome.focus_request {
        match request {
            FocusRequest::Clear => *focused_task_id_opt = None,
            request => match &mut application_mode {
                OutcomeApplicationMode::InteractiveUnflushed(focus_selection_mode) => {
                    **focus_selection_mode = focus_selection_mode_from_request(request);
                    *focused_task_id_opt = None;
                }
                OutcomeApplicationMode::Flushed => {
                    unreachable!("focus mode request must use the interactive outcome path")
                }
            },
        }
    }

    if matches!(&application_mode, OutcomeApplicationMode::Flushed)
        && outcome.kind != CommandKind::Noop
    {
        render_display_model(stdout, &DisplayModel::flush()).map_err(CommandError::Output)?;
    }
    Ok(())
}

enum OutcomeApplicationMode<'a> {
    Flushed,
    InteractiveUnflushed(&'a mut FocusSelectionMode),
}

#[test]
fn runtime外部ioとoutcome調停は共通境界に集約する() {
    let runtime_source = include_str!("runtime.rs");
    let apply_source = runtime_source
        .split_once("\nfn apply_command_outcome(")
        .expect("runtime must define the shared apply_command_outcome boundary")
        .1
        .split_once("\n#[test]\nfn runtime外部ioとoutcome調停は共通境界に集約する(")
        .expect("outcome boundary must remain bounded")
        .0;

    for required in [
        "render_display_model",
        "ExternalRequest::OpenFocusedLink",
        "execute_open_link",
        "ExternalRequest::OpenObsidianRootSearch",
        "execute_open_obsidian_root_task_search_with_config",
        "FocusRequest::Clear",
        "focus_selection_mode_from_request",
        "CommandError::Output",
    ] {
        assert!(
            apply_source.contains(required),
            "shared outcome boundary must coordinate {required}"
        );
    }

    let execute_parsed_source = runtime_source
        .split_once("fn execute_parsed(")
        .expect("non-interactive command path must exist")
        .1
        .split_once("struct RuntimeProjectCommandContext")
        .expect("non-interactive command path must remain bounded")
        .0;
    assert!(
        execute_parsed_source.contains("apply_command_outcome("),
        "non-interactive outcomes must use the shared boundary"
    );

    let interactive_source = runtime_source
        .split_once("\nfn execute_interactive_command(")
        .expect("interactive command path must exist")
        .1
        .split_once("struct InteractiveRepositoryState")
        .expect("interactive command path must remain bounded")
        .0;
    let (focus_branch, after_focus_branch) = interactive_source
        .split_once("    } else if matches!(")
        .expect("interactive focus branch must remain distinct");
    let (shortcut_branch, _) = after_focus_branch
        .split_once("    } else {")
        .expect("interactive shortcut branch must remain distinct");
    assert!(
        focus_branch.contains("apply_command_outcome("),
        "interactive focus outcomes must use the shared boundary"
    );
    assert!(
        shortcut_branch.contains("apply_command_outcome("),
        "interactive shortcut outcomes must use the shared boundary"
    );
    assert!(
        !runtime_source.contains("\nfn execute_handler_outcome("),
        "the superseded outcome coordinator must be removed"
    );

    for isolated_source in [
        include_str!("handler.rs"),
        include_str!("interactive.rs"),
        include_str!("renderer.rs"),
    ] {
        for forbidden in [
            "run_repository_transaction",
            "webbrowser::open",
            "process::Command",
        ] {
            assert!(
                !isolated_source.contains(forbidden),
                "external I/O and repository transactions must remain in runtime: {forbidden}"
            );
        }
    }
}

#[test]
fn external_open_errorはtargetとsource_reason_chainを保持する() {
    let error = external_open_error("test-target", std::io::Error::other("test-reason"));

    assert_eq!(
        error.to_string(),
        "外部起動エラー (test-target): test-reason"
    );
    assert_eq!(
        std::error::Error::source(&error).map(ToString::to_string),
        Some("test-reason".to_string())
    );
}

#[allow(clippy::too_many_arguments, unused_must_use)]
fn execute_with_config(
    stdout: &mut dyn SchronuWriter,
    _task_repository: &mut dyn TaskRepositoryTrait,
    _free_time_manager: &mut dyn FreeTimeManagerTrait,
    _focused_task_id_opt: &mut Option<Uuid>,
    _focus_started_datetime: &DateTime<Local>,
    _untrimmed_line: &str,
    parsed_command: &Command,
    _config: &SchronuConfig,
) -> Result<(), CommandError> {
    if matches!(parsed_command, Command::Noop) {
        return Ok(());
    }

    match parsed_command.kind() {
        CommandKind::Open | CommandKind::Obsidian => {
            unreachable!("migrated command must be handled before legacy dispatch")
        }
        CommandKind::Noop
        | CommandKind::FocusHighest
        | CommandKind::FocusLowest
        | CommandKind::Verify => {}
        _ => unreachable!("handler-owned command reached runtime fallback"),
    }

    render_display_model(stdout, &DisplayModel::flush()).map_err(CommandError::Output)?;
    Ok(())
}

#[cfg(test)]
fn execute_show_all_command_for_test(
    command: &str,
    now: DateTime<Local>,
    task: TaskHandle,
) -> String {
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
    task: TaskHandle,
    free_minutes: i64,
) -> String {
    execute_calendar_command_with_ansi_color_for_test(command, now, task, free_minutes, true)
}

#[cfg(test)]
fn execute_calendar_command_with_ansi_color_for_test(
    command: &str,
    now: DateTime<Local>,
    task: TaskHandle,
    free_minutes: i64,
    supports_ansi_color: bool,
) -> String {
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerWithFreeMinutes { free_minutes };
    let mut focused_task_id_opt = None;
    let mut stdout = if supports_ansi_color {
        TestWriter::new()
    } else {
        TestWriter::new_for_pipe()
    };

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
    task: TaskHandle,
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
    root: &TaskHandle,
    name: &str,
    start_time: DateTime<Local>,
    estimated_work_minutes: i64,
) -> TaskHandle {
    let child = root.create_as_last_child(new_test_task_attr(name));
    child.set_estimated_work_seconds(estimated_work_minutes * 60);
    child.set_start_time(start_time);
    child.set_pending_until(start_time);
    child.set_orig_status(Status::Pending);
    child
}

#[cfg(test)]
fn execute_flatten_command_for_test(
    command: &str,
    now: DateTime<Local>,
    task: TaskHandle,
    free_minutes_by_date: HashMap<NaiveDate, i64>,
) -> CommandTestResult {
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerByDate {
        free_minutes_by_date,
    };
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

    CommandTestResult {
        task: task_repository.task,
        focused_task_id_opt,
        output: stdout.into_string(),
    }
}

#[test]
fn test_execute_flatten_過負荷日では葉より親を先に翌日へ延期する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(30 * 60);
    root.set_start_time(now);
    root.set_pending_until(now);
    root.set_orig_status(Status::Pending);
    let child = add_scheduled_child_for_test(&root, "着手可能な葉", now, 60);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root.clone(),
        HashMap::from([(today, 60), (today + Duration::days(1), 60)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(root.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(today + Duration::days(1))
    );
    assert_eq!(
        result
            .task
            .get_by_id(child.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        now
    );
    assert!(result
        .output
        .contains(&format!("\t{}\t平テスト", root.get_id().unwrap())));
    assert!(result.output.contains("平: 1件 00:30"));
}

#[test]
fn test_execute_flatten_多階層ではrankが大きい親から延期する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(30 * 60);
    root.set_start_time(now);
    root.set_pending_until(now);
    root.set_orig_status(Status::Pending);
    let middle = add_scheduled_child_for_test(&root, "中間親", now, 30);
    add_scheduled_child_for_test(&middle, "葉", now, 60);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root.clone(),
        HashMap::from([(today, 60), (today + Duration::days(1), 120)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(root.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(today + Duration::days(1))
    );
    assert_eq!(
        result
            .task
            .get_by_id(middle.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(today + Duration::days(1))
    );
    let root_position = result.output.find("\t平テスト\n").unwrap();
    let middle_position = result.output.find("\t中間親\n").unwrap();
    assert!(root_position < middle_position);
}

#[test]
fn test_execute_flatten_親だけで解消できなければ低優先度の葉も連鎖延期する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(30 * 60);
    root.set_start_time(now);
    root.set_pending_until(now);
    root.set_orig_status(Status::Pending);
    let high = add_scheduled_child_for_test(&root, "先に予定された葉", now, 45);
    let low =
        add_scheduled_child_for_test(&root, "後に予定された葉", now + Duration::minutes(45), 45);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root.clone(),
        HashMap::from([
            (today, 60),
            (today + Duration::days(1), 30),
            (today + Duration::days(2), 90),
        ]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(root.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(today + Duration::days(2))
    );
    assert_eq!(
        result
            .task
            .get_by_id(low.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(today + Duration::days(2))
    );
    assert_eq!(
        result
            .task
            .get_by_id(high.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        now
    );
    assert_eq!(
        result
            .output
            .matches(&low.get_id().unwrap().to_string())
            .count(),
        1
    );
}

#[test]
fn test_execute_flatten_余裕日と100percentちょうどの日は変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();

    for command in ["平", "flatten", "flat"] {
        let root = new_test_task_handle("平テスト").unwrap();
        root.set_estimated_work_seconds(0);
        let target = add_scheduled_child_for_test(&root, "変更しない", now, 60);

        let result = execute_flatten_command_for_test(
            command,
            now,
            root,
            HashMap::from([(today, 60), (today + Duration::days(1), 60)]),
        );

        assert_eq!(
            result
                .task
                .get_by_id(target.get_id().unwrap())
                .unwrap()
                .get_pending_until()
                .unwrap(),
            now
        );
        assert_eq!(result.output, "[Info] 100%を超過している日はありません。\n");
    }
}

#[test]
fn test_execute_flatten_28日境界の超過を29日から34日を飛ばして35日後へ退避する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let boundary_date = today + Duration::days(28);
    let overflow_date = today + Duration::days(35);
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let boundary_start = subjective_date_start(boundary_date);
    let keeper = add_scheduled_child_for_test(&root, "境界に残す", boundary_start, 30);
    let first = add_scheduled_child_for_test(
        &root,
        "退避対象1",
        boundary_start + Duration::minutes(30),
        30,
    );
    let second = add_scheduled_child_for_test(
        &root,
        "退避対象2",
        boundary_start + Duration::minutes(60),
        30,
    );

    let result =
        execute_flatten_command_for_test("平", now, root, HashMap::from([(boundary_date, 30)]));

    assert_eq!(
        result
            .task
            .get_by_id(keeper.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(boundary_date)
    );
    assert_eq!(
        result
            .task
            .get_by_id(first.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(overflow_date)
    );
    assert_eq!(
        result
            .task
            .get_by_id(second.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(overflow_date)
    );
    assert_eq!(
        result
            .output
            .matches(&format!("平\t{}\t{}\t00:30", boundary_date, overflow_date))
            .count(),
        2
    );
    assert!(result
        .output
        .contains("[Warn] 35日後の退避先は日次容量の上限を適用していません: 2件 01:00"));
}

#[test]
fn test_execute_flatten_日容量を超えるtaskだけでは解消不能として状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let target = add_scheduled_child_for_test(&root, "大きすぎる", now, 90);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 60), (today + Duration::days(1), 60)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(target.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        now
    );
    assert!(result.output.starts_with("平: 0件 00:00 (未解消1日)\n"));
    assert!(result
        .output
        .contains("[Warn] 平\t2026-08-13\t未解消 00:30"));
    assert!(result.output.contains("1日の最大容量を超える: 1件"));
    assert!(result
        .output
        .contains(&format!("{}\t大きすぎる", target.get_id().unwrap())));
    assert!(!result.output.contains("[Stop]"));
}

#[test]
fn test_execute_flatten_未解消の超過が1分未満でも切り上げて表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let target = add_scheduled_child_for_test(&root, "1秒超過", now, 60);
    target.set_estimated_work_seconds(60 * 60 + 1);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 60), (today + Duration::days(1), 60)]),
    );

    assert!(result
        .output
        .contains("[Warn] 平\t2026-08-13\t未解消 00:01"));
}

#[test]
fn test_execute_flatten_業務日境界をまたぐtaskは延期しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let target = add_scheduled_child_for_test(&root, "境界をまたぐ", now, 25 * 60);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 60), (today + Duration::days(1), 26 * 60)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(target.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        now
    );
    assert!(result.output.contains("業務日境界をまたぐ: 1件"));
    assert!(result
        .output
        .contains(&format!("{}\t境界をまたぐ", target.get_id().unwrap())));
}

#[test]
fn test_execute_flatten_業務日境界をまたぐtaskの全作業時間を開始日の業務日に計上する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    add_scheduled_child_for_test(&root, "日境界をまたぐ", now, 25 * 60);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 24 * 60), (today + Duration::days(1), 26 * 60)]),
    );

    assert!(result.output.starts_with("平: 0件 00:00 (未解消1日)\n"));
    assert!(result
        .output
        .contains(&format!("[Warn] 平\t{}\t未解消 01:00", today)));
    assert!(result.output.contains("業務日境界をまたぐ: 1件"));
}

#[test]
fn test_execute_flatten_終了時刻が期限と等しいtaskは延期できる() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    root.set_deadline_time_opt(Some(
        subjective_date_start(today + Duration::days(1)) + Duration::minutes(30),
    ));
    let target = add_scheduled_child_for_test(&root, "期限ちょうど", now, 30);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 15), (today + Duration::days(1), 30)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(target.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(today + Duration::days(1))
    );
}

#[test]
fn test_execute_flatten_延期対象自身の期限補正で翌日06時を維持できなければ延期しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let target = add_scheduled_child_for_test(&root, "平日を表すダミータスク(8/21)", now, 30);
    target.set_deadline_time_opt(Some(
        subjective_date_start(today + Duration::days(1)) + Duration::minutes(30),
    ));

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 15), (today + Duration::days(1), 30)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(target.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        now
    );
    assert!(result
        .output
        .contains("自身の期限により翌日06:00を維持できない: 1件"));
    assert!(result.output.contains(&format!(
        "{}\t平日を表すダミータスク(8/21)",
        target.get_id().unwrap()
    )));
}

#[test]
fn test_execute_flatten_待機taskと残作業0を延期候補から除外する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let movable = add_scheduled_child_for_test(&root, "移動対象", now, 30);
    let waiting = add_scheduled_child_for_test(&root, "待機", now, 30);
    waiting.set_is_on_other_side(true);
    let zero = add_scheduled_child_for_test(&root, "残作業0", now, 0);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 30), (today + Duration::days(1), 30)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(movable.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(today + Duration::days(1))
    );
    for unchanged in [waiting.get_id().unwrap(), zero.get_id().unwrap()] {
        assert_eq!(
            result
                .task
                .get_by_id(unchanged)
                .unwrap()
                .get_pending_until()
                .unwrap(),
            now
        );
    }
}

#[test]
fn test_execute_flatten_35日後への退避で親の期限を超えるなら未解消として残す() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let boundary_date = today + Duration::days(28);
    let root = new_test_task_handle("期限のある親").unwrap();
    root.set_estimated_work_seconds(30 * 60);
    root.set_start_time(subjective_date_start(boundary_date));
    root.set_pending_until(subjective_date_start(boundary_date));
    root.set_orig_status(Status::Pending);
    root.set_deadline_time_opt(Some(subjective_date_start(today + Duration::days(35))));
    let child =
        add_scheduled_child_for_test(&root, "境界の葉", subjective_date_start(boundary_date), 60);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root.clone(),
        HashMap::from([(boundary_date, 60)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(root.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(boundary_date)
    );
    assert_eq!(
        result
            .task
            .get_by_id(child.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(boundary_date)
    );
    assert!(result.output.contains("平: 0件 00:00 (未解消1日)"));
    assert!(result
        .output
        .contains("仮延期によって関連taskの期限を超える: 1件"));
    assert!(!result.output.contains("[Stop]"));
}

#[test]
fn test_execute_flatten_延期不能日を飛ばして翌日以降の平坦化を保存する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let blocked = add_scheduled_child_for_test(&root, "今日の固定負荷", now, 90);
    let tomorrow_start = subjective_date_start(today + Duration::days(1));
    let tomorrow_first = add_scheduled_child_for_test(&root, "翌日の先行", tomorrow_start, 30);
    let tomorrow_late = add_scheduled_child_for_test(
        &root,
        "翌日の延期対象",
        tomorrow_start + Duration::minutes(30),
        30,
    );

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([
            (today, 60),
            (today + Duration::days(1), 30),
            (today + Duration::days(2), 30),
        ]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(blocked.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        now
    );
    assert_eq!(
        result
            .task
            .get_by_id(tomorrow_first.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        tomorrow_start
    );
    assert_eq!(
        result
            .task
            .get_by_id(tomorrow_late.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(today + Duration::days(2))
    );
    assert!(result.output.contains("平: 1件 00:30 (未解消1日)"));
    assert_eq!(result.output.matches("[Warn] 平\t2026-08-13").count(), 1);
}

#[test]
fn test_execute_flatten_未解消理由を固定順で表示して同じtaskを重複計上しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let waiting = add_scheduled_child_for_test(&root, "待機かつ大きすぎる", now, 90);
    waiting.set_is_on_other_side(true);
    let own_deadline = add_scheduled_child_for_test(&root, "自身に期限", now, 30);
    own_deadline.set_deadline_time_opt(Some(
        subjective_date_start(today + Duration::days(1)) + Duration::minutes(30),
    ));

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 60), (today + Duration::days(1), 60)]),
    );

    let waiting_reason = result.output.find("相手待ち: 1件").unwrap();
    let deadline_reason = result
        .output
        .find("自身の期限により翌日06:00を維持できない: 1件")
        .unwrap();
    assert!(waiting_reason < deadline_reason);
    assert_eq!(
        result
            .output
            .matches(&waiting.get_id().unwrap().to_string())
            .count(),
        1
    );
    assert!(!result.output.contains("1日の最大容量を超える:"));
}

#[test]
fn test_execute_flatten_28日目は延期可能分を35日目へ退避して固定負荷を未解消にする() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let boundary_date = today + Duration::days(28);
    let overflow_date = today + Duration::days(35);
    let boundary_start = subjective_date_start(boundary_date);
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let waiting = add_scheduled_child_for_test(&root, "境界の待機", boundary_start, 30);
    waiting.set_is_on_other_side(true);
    let movable = add_scheduled_child_for_test(
        &root,
        "35日目へ退避",
        boundary_start + Duration::minutes(30),
        30,
    );
    let deadline = add_scheduled_child_for_test(
        &root,
        "境界期限",
        boundary_start + Duration::minutes(60),
        30,
    );
    deadline.set_deadline_time_opt(Some(boundary_start + Duration::hours(18)));

    let result =
        execute_flatten_command_for_test("平", now, root, HashMap::from([(boundary_date, 30)]));

    assert_eq!(
        result
            .task
            .get_by_id(movable.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        subjective_date_start(overflow_date)
    );
    assert!(result.output.contains("平: 1件 00:30 (未解消1日)"));
    assert!(result
        .output
        .contains("[Warn] 35日後の退避先は日次容量の上限を適用していません: 1件 00:30"));
    assert!(result
        .output
        .contains(&format!("[Warn] 平\t{}\t未解消 00:30", boundary_date)));
}

#[test]
fn test_execute_flatten_各aliasで未解消日を飛ばして後続を延期する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();

    for command in ["平", "flatten", "flat"] {
        let root = new_test_task_handle("平テスト").unwrap();
        root.set_estimated_work_seconds(0);
        add_scheduled_child_for_test(&root, "固定負荷", now, 90);
        let tomorrow = subjective_date_start(today + Duration::days(1));
        add_scheduled_child_for_test(&root, "翌日の先行", tomorrow, 30);
        let movable = add_scheduled_child_for_test(
            &root,
            "翌日の延期対象",
            tomorrow + Duration::minutes(30),
            30,
        );

        let result = execute_flatten_command_for_test(
            command,
            now,
            root,
            HashMap::from([
                (today, 60),
                (today + Duration::days(1), 30),
                (today + Duration::days(2), 30),
            ]),
        );

        assert_eq!(
            result
                .task
                .get_by_id(movable.get_id().unwrap())
                .unwrap()
                .get_pending_until()
                .unwrap(),
            subjective_date_start(today + Duration::days(2))
        );
        assert!(result.output.contains("平: 1件 00:30 (未解消1日)"));
    }
}

#[test]
fn test_execute_calendar_現行出力を固定する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("暦出力固定用タスク").unwrap();
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
    let root = new_test_task_handle("暦複数日fixture").unwrap();
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
        -Duration::hours(7) - Duration::minutes(8),
        &DailyBandDurations {
            fixed_seconds: 450 * 60,
            elapsed_seconds: 0,
            repetitive_seconds: 855 * 60,
            non_repetitive_seconds: 71 * 60,
            rho_leeway_seconds: 24 * 60,
        },
        true,
    );
    let expected = format!(
        "2026-08-15(土) -07:08 +46:09 [{}{}{}{}{}]",
        "#".repeat(30),
        "=".repeat(57),
        "-".repeat(5),
        ":",
        ".".repeat(3),
    );

    assert_eq!(strip_ansi_escape_sequences(&actual), expected);
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
        Duration::hours(5) + Duration::minutes(6),
        &DailyBandDurations {
            fixed_seconds: 450 * 60,
            elapsed_seconds: 800 * 60,
            repetitive_seconds: 476 * 60,
            non_repetitive_seconds: 40 * 60,
            rho_leeway_seconds: 0,
        },
        true,
    );
    let expected = format!(
        "2026-08-11(火) +05:06 -03:04 [{}{}{}]{}",
        "#".repeat(30),
        "x".repeat(53),
        "=".repeat(13),
        ">".repeat(22),
    );

    assert_eq!(strip_ansi_escape_sequences(&actual), expected);
    assert!(actual.ends_with(&format!("\x1b[38;5;196m{}\x1b[39m", ">".repeat(22))));
}

#[test]
fn test_execute_band_日本語と英語で凡例と棒とサマリーを表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("帯出力固定用タスク").unwrap();
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
            "2026-08-11(火) -06:00 -09:00 [{}{}{}{}]\n",
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
        ),
        "#".repeat(56),
        "-".repeat(4),
        ":".repeat(24),
        ".".repeat(12),
    );

    assert_eq!(strip_ansi_escape_sequences(&japanese), expected);
    assert_eq!(strip_ansi_escape_sequences(&english), expected);
    assert!(!japanese.contains("日          "));
    assert!(!japanese.contains("帯出力固定用タスク"));
}

#[test]
fn test_execute_band_当日終了時刻と翌日締切のアラートを表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let tomorrow = now + Duration::days(1);
    let root = new_test_task_handle("帯アラートfixture").unwrap();
    root.set_estimated_work_seconds(0);
    add_scheduled_child_for_test(&root, "今日の超過", now, 11 * 60);
    add_scheduled_child_for_test(&root, "明日の予定", tomorrow, 1);
    let tomorrow_task = add_scheduled_child_for_test(&root, "明日締切", now, 11 * 60);
    tomorrow_task.set_deadline_time_opt(Some(tomorrow));

    let actual = execute_calendar_command_for_test("帯", now, root, 10 * 60);

    assert!(actual.contains(
        "[Crit] 【今日の】終了予定時刻に間に合いません。【ただちに】どれかの予定を諦めて明日以降に延期してください。"
    ), "{actual}");
    assert!(actual.contains(
        "[Warn] 【明日の】〆切に間に合いません。〆切をあさって以降にリスケする調整を【今日中に】してください。"
    ), "{actual}");
}

#[test]
fn test_execute_band_凡例と帯を7色の_ansi前景色で表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("帯色出力固定用タスク").unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);

    let actual = execute_calendar_command_for_test("帯", now, task, 10 * 60);
    let color = |value: u8, symbol: &str| format!("\x1b[38;5;{value}m{symbol}\x1b[39m");
    let expected = format!(
        concat!(
            "凡例: {} 固定  {} 経過済み  {} 繰返  {} 単発  {} 余差  {} 空き  {} 超過  (1文字=15分)\n",
            "\n",
            "2026-08-11(火) -06:00 -09:00 [{}{}{}{}]\n",
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
        ),
        color(110, "#"),
        color(244, "x"),
        color(33, "="),
        color(208, "-"),
        color(28, ":"),
        color(34, "."),
        color(196, ">"),
        color(110, &"#".repeat(56)),
        color(208, &"-".repeat(4)),
        color(28, &":".repeat(24)),
        color(34, &".".repeat(12)),
    );

    assert_eq!(actual, expected);
}

#[test]
fn test_execute_band_パイプ出力では_ansi前景色を含めない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("帯パイプ出力固定用タスク").unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);

    let actual = execute_calendar_command_with_ansi_color_for_test("帯", now, task, 10 * 60, false);

    assert!(!actual.contains("\x1b["));
    assert!(actual.contains("凡例: # 固定  x 経過済み"));
}

#[test]
fn test_execute_band_全日空き差分と繰り返し判定を帯へ反映する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root = new_test_task_handle("帯データフローfixture").unwrap();
    root.set_estimated_work_seconds(0);
    let repetitive_group = root.create_as_last_child(new_test_task_attr("繰り返しグループ"));
    repetitive_group.set_estimated_work_seconds(0);
    repetitive_group.set_repetition_interval_days_opt(Some(7));
    add_scheduled_child_for_test(&repetitive_group, "繰り返しタスク", now, 40);

    let actual = execute_band_command_with_elapsed_for_test("帯", now, root);
    let expected_row = format!(
        "2026-08-11(火) -01:45 -02:30 [{}{}{}{}{}]",
        "#".repeat(30),
        "x".repeat(53),
        "=".repeat(3),
        ":".repeat(7),
        ".".repeat(3),
    );

    assert!(
        strip_ansi_escape_sequences(&actual).contains(&expected_row),
        "{actual}"
    );
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
    let task = new_test_task_handle("TARGET_DATE_TASK").unwrap();
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
    let task = new_test_task_handle("TARGET_DATE_TASK").unwrap();
    task.set_start_time(scheduled_start);
    task.set_pending_until(scheduled_start);
    task.set_orig_status(Status::Pending);

    let abbreviated = execute_show_all_command_for_test("all 9/26", now, task.clone());
    let full = execute_show_all_command_for_test("all 2027/09/26", now, task);

    assert_eq!(abbreviated, full);
    assert!(abbreviated.contains("TARGET_DATE_TASK"));
}

// 削除できない時はNoneを返す。例えば、文字列が空の時
#[test]
fn get_byte_offset_for_deletion_noneを返す場合() {
    let line = "あ";
    let cursor_x = 0;
    let actual = get_byte_offset_for_deletion(line, cursor_x);
    let expected = None;
    assert_eq!(actual, expected);
}

#[test]
fn get_byte_offset_for_deletion_正常系() {
    let line = "あ";
    let cursor_x = 1;
    let actual = get_byte_offset_for_deletion(line, cursor_x);
    let expected = Some(0);
    assert_eq!(actual, expected);
}

fn reload_repository_for_cli(
    task_repository: &mut dyn TaskRepositoryTrait,
    now: DateTime<Local>,
) -> Result<StorageLock, CliRepositoryTransactionError> {
    let storage_lock = StorageLock::acquire_with_timeout(
        task_repository.get_project_storage_dir_name().as_ref(),
        LockMode::Cli,
        CLI_LOCK_TIMEOUT,
    )
    .map_err(CliRepositoryTransactionError::Lock)?;
    task_repository
        .reload_if_changed(now)
        .map_err(CliRepositoryTransactionError::Load)?;
    Ok(storage_lock)
}

fn run_cli_repository_transaction<T>(
    task_repository: &mut dyn TaskRepositoryTrait,
    now: DateTime<Local>,
    operation: impl FnOnce(&mut dyn TaskRepositoryTrait) -> Result<T, CommandError>,
) -> Result<T, RunError> {
    let storage_directory = task_repository.get_project_storage_dir_name().to_string();
    run_repository_transaction(
        task_repository,
        now,
        || {
            StorageLock::acquire_with_timeout(
                storage_directory.as_ref(),
                LockMode::Cli,
                CLI_LOCK_TIMEOUT,
            )
        },
        |repository| {
            let output = operation(repository)?;
            let should_save = repository
                .has_pending_changes()
                .map_err(ApplicationError::TaskTree)
                .map_err(CommandError::Application)?;
            Ok::<_, CommandError>((output, should_save))
        },
    )
    .map_err(|error| match error {
        RepositoryTransactionError::Lock(error) => {
            RunError::from(CliRepositoryTransactionError::Lock(error))
        }
        RepositoryTransactionError::Load(error) => {
            RunError::from(CliRepositoryTransactionError::Load(error))
        }
        RepositoryTransactionError::Operation(error) => RunError::from(error),
        RepositoryTransactionError::StateUncertain(error) => {
            RunError::from(CliRepositoryTransactionError::Save(error))
        }
    })
}

fn reconcile_focus_after_reload(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_selection_mode: &mut FocusSelectionMode,
) -> Result<bool, ApplicationError> {
    let should_reselect = match *focused_task_id_opt {
        Some(focused_task_id) => match task_repository
            .get_by_id(focused_task_id)
            .map_err(ApplicationError::TaskTree)?
        {
            Some(focused_task) => {
                focused_task
                    .get_status()
                    .map_err(ApplicationError::TaskTree)?
                    == Status::Done
                    && *focus_selection_mode != FocusSelectionMode::Explicit
            }
            None => true,
        },
        None => true,
    };
    if !should_reselect {
        return Ok(false);
    }

    let previous_focus = *focused_task_id_opt;
    if *focus_selection_mode == FocusSelectionMode::Explicit {
        *focus_selection_mode = FocusSelectionMode::HighestPriority;
    }
    *focused_task_id_opt = select_focus_task_id(task_repository, *focus_selection_mode)?;
    Ok(previous_focus != *focused_task_id_opt)
}

pub(super) fn application() {
    let command_opt = parse_non_interactive_command(env::args().skip(1).collect());
    let config = match load_schronu_config(env::var_os("SCHRONU_CONFIG_PATH")) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[Error] {error}");
            process::exit(1);
        }
    };
    let _ = ACTIVE_CONFIG.set(config);
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

    // controllerで実体を見るのを避けるために、1つ関数を切る
    let result = match command_opt {
        Some(command) => {
            execute_non_interactive_command(&mut task_repository, &mut free_time_manager, &command)
        }
        None => interactive_application(&mut task_repository, &mut free_time_manager),
    };
    if !report_run_result(&mut std::io::stderr(), result) {
        process::exit(1);
    }
}

fn report_run_result(stderr: &mut dyn Write, result: Result<(), RunError>) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            render_plain_display_model(stderr, &error_display_model(&error)).unwrap();
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
    execute_non_interactive_command_at(task_repository, free_time_manager, command, Local::now())
}

fn execute_non_interactive_command_at(
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    command: &str,
    operation_now: DateTime<Local>,
) -> Result<(), RunError> {
    let parsed_command = parse_command(command, ParseMode::NonInteractive)
        .map_err(map_command_parse_error)
        .map_err(RunError::Command)?;
    validate_non_interactive_command(&parsed_command).map_err(RunError::Command)?;
    if parsed_command.kind() == CommandKind::Verify {
        let _storage_lock = reload_repository_for_cli(task_repository, operation_now)?;
        println!("検証: OK");
        return Ok(());
    }
    free_time_manager.load_busy_time_slots_from_file(
        active_config()
            .busy_time_slots_yaml_path
            .to_str()
            .expect("config path was validated"),
    )?;

    let focus_started_datetime = operation_now;
    let mut stdout = stdout();
    run_cli_repository_transaction(task_repository, operation_now, |task_repository| {
        let mut focused_task_id_opt: Option<Uuid> =
            select_focus_task_id(task_repository, FocusSelectionMode::HighestPriority)?;
        execute_parsed(
            &mut stdout,
            task_repository,
            free_time_manager,
            &mut focused_task_id_opt,
            &focus_started_datetime,
            command,
            &parsed_command,
        )
    })?;
    Ok(())
}

#[test]
fn test_execute_non_interactive_command_project作成はoperation時刻を共有する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let previous_synced_time = Local.with_ymd_and_hms(2026, 8, 19, 9, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 8, 20, 14, 30, 45).unwrap();
    let mut task_repository = TestTaskRepository::new(
        new_test_task_handle("既存project").unwrap(),
        previous_synced_time,
    )
    .with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager;

    execute_non_interactive_command_at(
        &mut task_repository,
        &mut free_time_manager,
        "新 snapshot_project 30",
        operation_now,
    )
    .unwrap();

    assert_eq!(task_repository.get_last_synced_time(), operation_now);
    assert_eq!(task_repository.reload_if_changed_attempt_count.get(), 1);
    assert_eq!(task_repository.task.get_name().unwrap(), "snapshot_project");
    assert_eq!(
        task_repository.task.get_create_time().unwrap(),
        operation_now
    );
    assert_eq!(
        task_repository.task.get_start_time().unwrap(),
        operation_now
    );
}

#[test]
fn test_execute_non_interactive_command_finishはoperation時刻を共有する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let previous_synced_time = Local.with_ymd_and_hms(2026, 8, 19, 9, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 8, 20, 14, 30, 45).unwrap();
    let repetitive_parent = new_test_task_handle("反復project").unwrap();
    repetitive_parent
        .set_repetition_interval_days_opt(Some(7))
        .unwrap();
    let focused = repetitive_parent.create_as_last_child(new_test_task_attr("今回の反復task"));
    let focused_id = focused.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(repetitive_parent, previous_synced_time)
        .with_storage_directory(&storage_dir.path);
    task_repository.highest_priority_leaf_task_id_opt = Some(focused_id);
    let mut free_time_manager = TestFreeTimeManager;

    execute_non_interactive_command_at(
        &mut task_repository,
        &mut free_time_manager,
        "終",
        operation_now,
    )
    .unwrap();

    assert_eq!(task_repository.get_last_synced_time(), operation_now);
    assert_eq!(task_repository.reload_if_changed_attempt_count.get(), 1);
    let finished = task_repository
        .get_by_id(focused_id)
        .unwrap()
        .expect("完了対象のtaskはtreeに残るべきです");
    assert_eq!(finished.get_end_time_opt().unwrap(), Some(operation_now));
    let next_repetition = task_repository
        .task
        .get_children()
        .unwrap()
        .into_iter()
        .find(|task| task.get_id().unwrap() != focused_id)
        .expect("反復taskの完了時は次回taskを生成すべきです");
    assert_eq!(next_repetition.get_create_time().unwrap(), operation_now);
}

#[test]
fn test_execute_non_interactive_command_省略作業時間はoperation時刻を使う() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let previous_synced_time = Local.with_ymd_and_hms(2026, 8, 19, 9, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 8, 20, 14, 30, 45).unwrap();
    let focused = new_test_task_handle("作業対象").unwrap();
    focused.set_actual_work_seconds(2 * 60).unwrap();
    let focused_id = focused.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(focused, previous_synced_time)
        .with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager;

    execute_non_interactive_command_at(
        &mut task_repository,
        &mut free_time_manager,
        "働",
        operation_now,
    )
    .unwrap();

    assert_eq!(task_repository.get_last_synced_time(), operation_now);
    assert_eq!(task_repository.reload_if_changed_attempt_count.get(), 1);
    assert_eq!(task_repository.save_attempt_count.get(), 1);
    assert_eq!(
        task_repository
            .get_by_id(focused_id)
            .unwrap()
            .expect("作業対象のtaskはtreeに残るべきです")
            .get_actual_work_seconds()
            .unwrap(),
        3 * 60
    );
}

#[test]
fn test_execute_non_interactive_command_load失敗時はcommandを実行しない() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("変更しないtask").unwrap();
    let task_id = task.get_id().unwrap();
    let original_estimated_work_seconds = task.get_estimated_work_seconds().unwrap();
    let mut task_repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
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
            .get_estimated_work_seconds()
            .unwrap(),
        original_estimated_work_seconds
    );
}

#[test]
fn test_execute_non_interactive_command_検証はsaveとfree_time読込を行わない() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("検証対象").unwrap();
    let mut task_repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager;

    execute_non_interactive_command(&mut task_repository, &mut free_time_manager, "検証").unwrap();

    assert_eq!(task_repository.save_attempt_count.get(), 0);
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
    assert!(output.contains("project.children: must be an array or null"));
}

#[test]
fn test_execute_non_interactive_command_busy_time_slots読込失敗はstderrへ表示しrepository_transactionとcommandを実行しない(
) {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("変更しないtask").unwrap();
    let task_id = task.get_id().unwrap();
    let original_estimated_work_seconds = task.get_estimated_work_seconds().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerWithLoadError::default();
    let busy_time_slots_yaml_path = active_config().busy_time_slots_yaml_path.clone();

    let error =
        execute_non_interactive_command(&mut task_repository, &mut free_time_manager, "予 45")
            .expect_err("busy time slotsの読込失敗はRunErrorとして返るべきです");

    assert!(matches!(
        error,
        RunError::BusyTimeSlots(ref error)
            if error.to_string().contains(busy_time_slots_yaml_path.to_str().unwrap())
                && error.to_string().contains("$")
    ));
    assert_eq!(task_repository.load_attempt_count.get(), 0);
    assert_eq!(task_repository.reload_if_changed_attempt_count.get(), 0);
    assert_eq!(task_repository.save_attempt_count.get(), 0);
    assert_eq!(
        free_time_manager.loaded_path(),
        Some(busy_time_slots_yaml_path.clone())
    );
    assert_eq!(
        task_repository
            .get_by_id(task_id)
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        original_estimated_work_seconds
    );

    let mut stderr = Vec::new();
    let succeeded = report_run_result(&mut stderr, Err(error));

    assert!(!succeeded);
    let output = String::from_utf8(stderr).unwrap();
    assert!(output.contains("[Error]"));
    assert!(output.contains(busy_time_slots_yaml_path.to_str().unwrap()));
    assert!(output.contains("$"));
}

#[test]
fn test_interactive起動前のbusy_time_slots読込失敗はraw_modeなしでrun_errorとして返す() {
    let mut free_time_manager = TestFreeTimeManagerWithLoadError::default();
    let busy_time_slots_yaml_path = active_config().busy_time_slots_yaml_path.clone();

    let error = load_busy_time_slots_for_interactive_application(
        &mut free_time_manager,
        busy_time_slots_yaml_path.to_str().unwrap(),
    )
    .expect_err("対話起動前の設定読込失敗はRawModeを有効化せずRunErrorとして返すべきです");

    assert!(matches!(
        error,
        RunError::BusyTimeSlots(ref error)
            if error.path() == busy_time_slots_yaml_path.as_path()
                && error.field_path() == "$"
    ));
    assert_eq!(
        free_time_manager.loaded_path(),
        Some(busy_time_slots_yaml_path)
    );
}

#[test]
fn test_cli_repository初期load後はmcpがlockを取得できる() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path.to_str().unwrap());

    let storage_lock = reload_repository_for_cli(&mut repository, now).unwrap();
    drop(storage_lock);

    let mcp_lock = StorageLock::acquire(&storage_dir.path, LockMode::Mcp);
    assert!(mcp_lock.is_ok());
}

#[test]
fn test_cli_repository_transactionは外部更新を再読込してcommandを即時保存する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let mut cli_repository = TaskRepository::new(storage_dir.path.to_str().unwrap());
    drop(reload_repository_for_cli(&mut cli_repository, now).unwrap());

    {
        let _mcp_lock = StorageLock::acquire(&storage_dir.path, LockMode::Mcp).unwrap();
        let mut mcp_repository = TaskRepository::new(storage_dir.path.to_str().unwrap());
        mcp_repository.sync_clock(now);
        mcp_repository.load().unwrap();
        mcp_repository
            .start_new_project(new_test_task_handle("MCP更新").unwrap())
            .unwrap();
        mcp_repository.save().unwrap();
    }

    run_cli_repository_transaction(&mut cli_repository, now, |repository| {
        repository
            .start_new_project(new_test_task_handle("CLI更新").unwrap())
            .unwrap();
        Ok(())
    })
    .unwrap();

    let _mcp_lock = StorageLock::acquire(&storage_dir.path, LockMode::Mcp).unwrap();
    let mut reloaded = TaskRepository::new(storage_dir.path.to_str().unwrap());
    reloaded.sync_clock(now);
    reloaded.load().unwrap();
    let names = reloaded
        .get_all_projects()
        .iter()
        .map(|task| task.get_name().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"MCP更新".to_string()));
    assert!(names.contains(&"CLI更新".to_string()));
}

#[test]
fn test_cli_repository_transactionはread_only_operationでsaveしない() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let mut repository = TestTaskRepository::new(new_test_task_handle("cache経路").unwrap(), now)
        .with_storage_directory(&storage_dir.path);
    repository.has_pending_changes.set(false);

    run_cli_repository_transaction(&mut repository, now, |_| Ok(())).unwrap();

    assert_eq!(repository.reload_if_changed_attempt_count.get(), 1);
    assert_eq!(repository.load_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 0);
}

#[test]
fn test_cli_repository_transactionはload失敗時にcommandもsaveも実行しない() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let mut repository = TestTaskRepository::new(new_test_task_handle("変更前").unwrap(), now)
        .with_storage_directory(&storage_dir.path);
    repository.load_should_fail = true;
    let command_executed = Cell::new(false);

    let result = run_cli_repository_transaction(&mut repository, now, |_| {
        command_executed.set(true);
        Ok(())
    });

    assert!(matches!(result, Err(RunError::Repository(_))));
    assert!(!command_executed.get());
    assert_eq!(repository.save_attempt_count.get(), 0);
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_cli_repository_transactionはsave失敗をfatalなphase付きerrorにする() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("変更前").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    repository.save_failures_remaining.set(1);

    let result = run_cli_repository_transaction(&mut repository, now, |repository| {
        repository
            .get_by_id(task_id)
            .unwrap()
            .set_estimated_work_seconds(45 * 60);
        Ok(())
    });

    assert!(matches!(
        result,
        Err(RunError::CliRepositoryTransaction(
            CliRepositoryTransactionError::Save(_)
        ))
    ));
    assert_eq!(repository.save_attempt_count.get(), 1);
    assert_eq!(
        repository
            .get_by_id(task_id)
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        45 * 60
    );
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_reload後にfocus中taskがdoneなら次候補を選び直す() {
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let root = new_test_task_handle("root").unwrap();
    let done = root.create_as_last_child(new_test_task_attr("完了済みfocus"));
    done.set_orig_status(Status::Done);
    let next = root.create_as_last_child(new_test_task_attr("次候補"));
    let mut repository = TestTaskRepository::new(root, now);
    repository.highest_priority_leaf_task_id_opt = Some(next.get_id().unwrap());
    let mut focused_task_id_opt = Some(done.get_id().unwrap());
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let changed = reconcile_focus_after_reload(
        &mut repository,
        &mut focused_task_id_opt,
        &mut focus_selection_mode,
    );

    assert!(changed.unwrap());
    assert_eq!(focused_task_id_opt, Some(next.get_id().unwrap()));
}

#[test]
fn test_低優先度modeで外したfocusは低優先度候補を再選択する() {
    let now = Local.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap();
    let root = new_test_task_handle("root").unwrap();
    let high_priority_task = root.create_as_last_child(new_test_task_attr("高優先度候補"));
    let low_priority_task = root.create_as_last_child(new_test_task_attr("低優先度候補"));
    let high_priority_task_id = high_priority_task.get_id().unwrap();
    let low_priority_task_id = low_priority_task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(root, now);
    repository.highest_priority_leaf_task_id_opt = Some(high_priority_task_id);
    repository.defer_candidate_leaf_task_id_opt = Some(low_priority_task_id);
    let mut free_time_manager = TestFreeTimeManager;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(high_priority_task_id);
    let focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::LowestPriority { recent_days: 3 };

    execute_interactive_command(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        &mut focus_selection_mode,
        now,
        "外",
    )
    .unwrap();

    assert_eq!(focused_task_id_opt, Some(low_priority_task_id));
    assert_eq!(
        focus_selection_mode,
        FocusSelectionMode::LowestPriority { recent_days: 3 }
    );
    assert_eq!(repository.last_defer_candidate_recent_days_opt, Some(3));
}

#[test]
fn test_interactive_task属性更新_不正deadlineはfield付きerrorを表示して状態を維持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();
    let previous_deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
    task.set_deadline_time_opt(Some(previous_deadline));
    let mut repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    execute_interactive_command(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        &mut focus_selection_mode,
        now,
        "〆 invalid",
    )
    .unwrap();

    let actual = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(
        actual.get_deadline_time_opt().unwrap(),
        Some(previous_deadline)
    );
    assert!(stdout
        .into_string()
        .contains("[Error] 入力エラー: deadline:"));
}

#[test]
fn test_interactive_submitは製品event経路でload実行保存する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Submit { line: " 予 45 " },
    );

    assert!(matches!(
        outcome,
        InteractiveRepositoryEventOutcome::CommandExecuted(ref command, _) if command == "予 45"
    ));
    assert_eq!(repository.load_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 1);
    assert_eq!(
        repository
            .get_by_id(task_id)
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        45 * 60
    );
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_interactive_submitはoperation時刻をcommandと直後renderへ共有する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let previous_synced_time = Local.with_ymd_and_hms(2026, 8, 19, 9, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 8, 20, 14, 30, 45).unwrap();
    let existing = new_test_task_handle("既存project").unwrap();
    let existing_id = existing.get_id().unwrap();
    let mut repository = TestTaskRepository::new(existing, previous_synced_time)
        .with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(existing_id);
    let mut last_focused_task_id_opt = Some(existing_id);
    let mut focus_started_datetime = previous_synced_time;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_submit_at(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        " 新 interactive_snapshot 30 ",
        operation_now,
    );
    let render_now = match outcome {
        InteractiveRepositoryEventOutcome::CommandExecuted(command, now) => {
            assert_eq!(command, "新 interactive_snapshot 30");
            now
        }
        _ => panic!("固定時刻のSubmitはcommand実行に成功すべきです"),
    };

    assert_eq!(render_now, operation_now);
    assert_eq!(repository.get_last_synced_time(), operation_now);
    assert_eq!(repository.reload_if_changed_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 1);
    assert_eq!(repository.task.get_name().unwrap(), "interactive_snapshot");
    assert_eq!(repository.task.get_create_time().unwrap(), operation_now);
    assert_eq!(repository.task.get_start_time().unwrap(), operation_now);
    let output = strip_ansi_escape_sequences(&stdout.into_string());
    assert!(
        output.contains("2026/08/20 14:30:45.000000000> 新 interactive_snapshot 30"),
        "{output}"
    );

    let mut render_stdout = TestWriter::new();
    render_focused_task(
        &mut render_stdout,
        &repository,
        focused_task_id_opt,
        &mut last_focused_task_id_opt,
        &mut focus_started_datetime,
        render_now,
    );

    assert_eq!(last_focused_task_id_opt, focused_task_id_opt);
    assert_eq!(focus_started_datetime, operation_now);
}

#[test]
fn test_interactive_submitの見は完了済みtaskへの明示focusを更新後も保持する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let root = new_test_task_handle("root").unwrap();
    let done = root.create_as_last_child(new_test_task_attr("完了済みtask"));
    done.set_orig_status(Status::Done);
    let next = root.create_as_last_child(new_test_task_attr("次候補"));
    let done_id = done.get_id().unwrap();
    let next_id = next.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(root, now).with_storage_directory(&storage_dir.path);
    repository.highest_priority_leaf_task_id_opt = Some(next_id);
    let mut free_time_manager = TestFreeTimeManager;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(next_id);
    let mut last_focused_task_id_opt = Some(next_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;
    let command = format!("見 {done_id}");

    let submit_outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Submit { line: &command },
    );

    assert!(matches!(
        submit_outcome,
        InteractiveRepositoryEventOutcome::CommandExecuted(..)
    ));
    assert_eq!(focused_task_id_opt, Some(done_id));

    let refresh_outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Refresh,
    );

    assert!(matches!(
        refresh_outcome,
        InteractiveRepositoryEventOutcome::Continue
    ));
    assert_eq!(focused_task_id_opt, Some(done_id));
}

#[test]
fn test_interactive_submitは外部完了によるfocus切替時に開始時刻を更新する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let old_focus_started_datetime = Local.with_ymd_and_hms(2020, 8, 12, 12, 0, 0).unwrap();
    let root = new_test_task_handle("root").unwrap();
    let done = root.create_as_last_child(new_test_task_attr("外部で完了したfocus"));
    done.set_orig_status(Status::Done);
    let next = root.create_as_last_child(new_test_task_attr("次候補"));
    let done_id = done.get_id().unwrap();
    let next_id = next.get_id().unwrap();
    let mut repository = TestTaskRepository::new(root, old_focus_started_datetime)
        .with_storage_directory(&storage_dir.path);
    repository.highest_priority_leaf_task_id_opt = Some(next_id);
    let mut free_time_manager = TestFreeTimeManager;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(done_id);
    let mut last_focused_task_id_opt = Some(done_id);
    let mut focus_started_datetime = old_focus_started_datetime;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Submit { line: "" },
    );

    assert!(matches!(
        outcome,
        InteractiveRepositoryEventOutcome::CommandExecuted(..)
    ));
    assert_eq!(focused_task_id_opt, Some(next_id));
    assert!(focus_started_datetime > old_focus_started_datetime);
}

#[test]
fn test_interactive_refreshとctrl_dは外部完了によるfocus切替時に開始時刻を更新する() {
    for event in [
        InteractiveRepositoryEvent::Refresh,
        InteractiveRepositoryEvent::Exit,
    ] {
        let storage_dir = TestStorageDir::new();
        std::fs::create_dir_all(&storage_dir.path).unwrap();
        let old_focus_started_datetime = Local.with_ymd_and_hms(2020, 8, 12, 12, 0, 0).unwrap();
        let root = new_test_task_handle("root").unwrap();
        let done = root.create_as_last_child(new_test_task_attr("外部で完了したfocus"));
        done.set_orig_status(Status::Done);
        let next = root.create_as_last_child(new_test_task_attr("次候補"));
        let done_id = done.get_id().unwrap();
        let next_id = next.get_id().unwrap();
        let mut repository = TestTaskRepository::new(root, old_focus_started_datetime)
            .with_storage_directory(&storage_dir.path);
        repository.highest_priority_leaf_task_id_opt = Some(next_id);
        let mut free_time_manager = TestFreeTimeManager;
        let mut stdout = TestWriter::new();
        let mut focused_task_id_opt = Some(done_id);
        let mut last_focused_task_id_opt = Some(done_id);
        let mut focus_started_datetime = old_focus_started_datetime;
        let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

        let outcome = handle_interactive_repository_event(
            &mut stdout,
            &mut repository,
            &mut free_time_manager,
            InteractiveRepositoryState {
                focused_task_id_opt: &mut focused_task_id_opt,
                last_focused_task_id_opt: &mut last_focused_task_id_opt,
                focus_started_datetime: &mut focus_started_datetime,
                focus_selection_mode: &mut focus_selection_mode,
            },
            event,
        );

        assert!(matches!(
            outcome,
            InteractiveRepositoryEventOutcome::Continue | InteractiveRepositoryEventOutcome::Exit
        ));
        assert_eq!(focused_task_id_opt, Some(next_id));
        assert_eq!(last_focused_task_id_opt, None);
        assert!(focus_started_datetime > old_focus_started_datetime);
    }
}

#[test]
fn test_interactive_commandによるfocus切替は次のrender時刻を開始時刻にする() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let old_focus_started_datetime = Local.with_ymd_and_hms(2020, 8, 12, 12, 0, 0).unwrap();
    let first_render_datetime = Local.with_ymd_and_hms(2026, 8, 12, 13, 0, 0).unwrap();
    let second_render_datetime = Local.with_ymd_and_hms(2026, 8, 12, 14, 0, 0).unwrap();
    let task = new_test_task_handle("focus対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(task, old_focus_started_datetime)
        .with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = old_focus_started_datetime;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Submit { line: "高" },
    );
    render_focused_task(
        &mut stdout,
        &repository,
        focused_task_id_opt,
        &mut last_focused_task_id_opt,
        &mut focus_started_datetime,
        first_render_datetime,
    );
    render_focused_task(
        &mut stdout,
        &repository,
        focused_task_id_opt,
        &mut last_focused_task_id_opt,
        &mut focus_started_datetime,
        second_render_datetime,
    );

    assert!(matches!(
        outcome,
        InteractiveRepositoryEventOutcome::CommandExecuted(..)
    ));
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert_eq!(last_focused_task_id_opt, Some(task_id));
    assert_eq!(focus_started_datetime, first_render_datetime);
}

#[test]
fn test_interactive_submitはload失敗ならretryしsave失敗ならfatalにする() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();

    for (load_should_fail, save_failures, expected_fatal) in [(true, 0, false), (false, 1, true)] {
        let task = new_test_task_handle("更新対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository =
            TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
        repository.load_should_fail = load_should_fail;
        repository.save_failures_remaining.set(save_failures);
        let mut free_time_manager = TestFreeTimeManager;
        let mut stdout = TestWriter::new();
        let mut focused_task_id_opt = Some(task_id);
        let mut last_focused_task_id_opt = Some(task_id);
        let mut focus_started_datetime = now;
        let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

        let outcome = handle_interactive_repository_event(
            &mut stdout,
            &mut repository,
            &mut free_time_manager,
            InteractiveRepositoryState {
                focused_task_id_opt: &mut focused_task_id_opt,
                last_focused_task_id_opt: &mut last_focused_task_id_opt,
                focus_started_datetime: &mut focus_started_datetime,
                focus_selection_mode: &mut focus_selection_mode,
            },
            InteractiveRepositoryEvent::Submit { line: "予 45" },
        );

        assert_eq!(
            matches!(outcome, InteractiveRepositoryEventOutcome::Fatal(_)),
            expected_fatal
        );
        assert_eq!(
            matches!(outcome, InteractiveRepositoryEventOutcome::Retry(_)),
            !expected_fatal
        );
        assert_eq!(
            repository.save_attempt_count.get(),
            usize::from(!load_should_fail)
        );
    }
}

#[test]
fn test_interactive_refreshは再読込後にlockを解放する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("表示対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Refresh,
    );

    assert!(matches!(
        outcome,
        InteractiveRepositoryEventOutcome::Continue
    ));
    assert_eq!(repository.load_attempt_count.get(), 1);
    assert_eq!(repository.reload_if_changed_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 0);
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_interactive_ctrl_cは成功済みcommandを再保存せずfatal終了する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let submitted = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Submit { line: "予 45" },
    );
    let interrupted = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Interrupted,
    );

    assert!(matches!(
        submitted,
        InteractiveRepositoryEventOutcome::CommandExecuted(..)
    ));
    assert!(matches!(
        interrupted,
        InteractiveRepositoryEventOutcome::Fatal(RunError::Interrupted)
    ));
    assert_eq!(repository.save_attempt_count.get(), 1);
}

#[test]
fn test_interactive_input切断はreload後に保存してfatal終了する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("保存対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::InputDisconnected,
    );

    assert!(matches!(
        outcome,
        InteractiveRepositoryEventOutcome::Fatal(_)
    ));
    assert_eq!(repository.load_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 1);
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_interactive_ctrl_dは製品event経路でreload後に保存して終了する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("保存対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Exit,
    );

    assert!(matches!(outcome, InteractiveRepositoryEventOutcome::Exit));
    assert_eq!(repository.load_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 1);
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_interactive_input読込errorは製品event経路でreload後に保存してfatal終了する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("保存対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::InputRead(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "stdin read failure",
        )),
    );

    assert!(matches!(
        outcome,
        InteractiveRepositoryEventOutcome::Fatal(RunError::InputRead {
            input_error,
            save_error_opt: None,
        }) if input_error.kind() == std::io::ErrorKind::BrokenPipe
    ));
    assert_eq!(repository.load_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 1);
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

fn make_messages_about_focus(
    focused_task: &TaskHandle,
    focus_started_datetime: &DateTime<Local>,
    now: &DateTime<Local>,
) -> Result<[String; 2], ApplicationError> {
    let estimated_work_seconds = focused_task
        .get_estimated_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let actual_work_seconds = focused_task
        .get_actual_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let estimated_finish_datetime =
        *focus_started_datetime + Duration::seconds(estimated_work_seconds - actual_work_seconds);

    let left_duration = estimated_finish_datetime - *now;
    let for_duration = *now - *focus_started_datetime;
    let focusing_minutes = for_duration.num_minutes() + 1;
    let progress = format_focus_progress(
        estimated_work_seconds,
        actual_work_seconds,
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

    Ok([summary, progress])
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
    let task = new_test_task_handle("タスク").unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_actual_work_seconds(10 * 60);

    let actual = make_messages_about_focus(&task, &focus_started_datetime, &now).unwrap();

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
    let task = new_test_task_handle("タスク").unwrap();
    task.set_estimated_work_seconds(100 * 60);
    task.set_actual_work_seconds(39 * 60);

    let actual = make_messages_about_focus(&task, &focus_started_datetime, &now).unwrap();

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
    let task = new_test_task_handle("タスク").unwrap();
    task.set_estimated_work_seconds(100 * 60);
    task.set_actual_work_seconds(57 * 60);

    let actual = make_messages_about_focus(&task, &focus_started_datetime, &now).unwrap();

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
    let task = new_test_task_handle("タスク").unwrap();
    task.set_estimated_work_seconds(0);
    task.set_actual_work_seconds(10 * 60);

    let actual = make_messages_about_focus(&task, &focus_started_datetime, &now).unwrap();

    assert!(actual[0].ends_with("focusing for 20 minutes"));
    assert_eq!(actual[1], format!("[{}] --%", "-".repeat(100)));
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

fn handle_input_disconnected_with_reload(
    task_repository: &mut dyn TaskRepositoryTrait,
) -> RunError {
    match reload_repository_for_cli(task_repository, Local::now()) {
        Ok(_storage_lock) => handle_input_disconnected(task_repository),
        Err(repository_error) => RunError::InputDisconnectedWithRepository { repository_error },
    }
}

fn handle_input_read_error_with_reload(
    task_repository: &mut dyn TaskRepositoryTrait,
    input_error: std::io::Error,
) -> RunError {
    match reload_repository_for_cli(task_repository, Local::now()) {
        Ok(_storage_lock) => handle_input_read_error(task_repository, input_error),
        Err(repository_error) => RunError::InputReadWithRepository {
            input_error,
            repository_error,
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn try_exit_interactive(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    now: DateTime<Local>,
) -> bool {
    if !try_save_before_exit(stdout, task_repository) {
        return false;
    }

    task_repository.sync_clock(now);
    let result = execute_show_all_tasks(
        stdout,
        focused_task_id_opt,
        task_repository,
        free_time_manager,
        &Some("帯".to_string()),
        TaskListDisplayOrder::ScheduledStartDesc,
    );
    report_application_result(stdout, result);
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
    let focused_task_opt = match task_repository.get_by_id(focused_task_id) {
        Ok(task) => task,
        Err(error) => {
            report_application_result::<()>(stdout, Err(ApplicationError::TaskTree(error)));
            return;
        }
    };

    if focused_task_id_opt != *last_focused_task_id_opt {
        *focus_started_datetime = now;
        *last_focused_task_id_opt = focused_task_id_opt;
    }

    let result = execute_show_ancestor(stdout, &focused_task_opt);
    report_application_result(stdout, result);

    if let Some(focused_task) = focused_task_opt {
        let project_category_opt = match focused_task.get_project_category_opt() {
            Ok(project_category_opt) => project_category_opt,
            Err(error) => {
                report_application_result::<()>(stdout, Err(ApplicationError::TaskTree(error)));
                return;
            }
        };
        writeln_newline(stdout, &format_focused_task_header(project_category_opt)).unwrap();
        writeln_newline(stdout, &format!("{:?}", focused_task.get_attr())).unwrap();

        let messages = match make_messages_about_focus(&focused_task, focus_started_datetime, &now)
        {
            Ok(messages) => messages,
            Err(error) => {
                report_application_result::<()>(stdout, Err(error));
                return;
            }
        };
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

fn render_interactive_screen(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focus_state: FocusRenderState,
    now: DateTime<Local>,
) {
    let result = execute_show_all_tasks(
        stdout,
        focus_state.focused_task_id_opt,
        task_repository,
        free_time_manager,
        &Some("帯".to_string()),
        TaskListDisplayOrder::ScheduledStartDesc,
    );
    report_application_result(stdout, result);
    render_focused_task(
        stdout,
        task_repository,
        *focus_state.focused_task_id_opt,
        focus_state.last_focused_task_id_opt,
        focus_state.focus_started_datetime,
        now,
    );
}

#[test]
fn test_render_interactive_screen_起動時と自動更新時の既定表示は帯() {
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("対話画面表示対象").unwrap();
    let task_id = task.get_id().unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerForBand;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = None;
    let mut focus_started_datetime = now;

    render_interactive_screen(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        FocusRenderState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
        },
        now,
    );

    let output = stdout.into_string();
    let band_line = output
        .lines()
        .find(|line| line.starts_with("2026-08-12(水) "))
        .expect("起動時と自動更新時には日次帯を表示する");
    let band = band_line
        .split_once('[')
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(band, _)| band)
        .expect("日次帯は角括弧内に表示する");
    assert_eq!(
        strip_ansi_escape_sequences(band).chars().count(),
        DAILY_BAND_SEGMENTS
    );
    assert!(!output.contains("日          \t空          \t空差"));
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
            | Some('詰')
            | Some('葉')
            | Some('樹')
            | Some('清')
    ) || matches!(line.split_whitespace().next(), Some("band" | "pack"))
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
fn test_try_save_before_exit_保存成功なら終了可能にする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task_repository = TestTaskRepository::new(new_test_task_handle("保存対象").unwrap(), now);
    let mut stdout = TestWriter::new();

    let actual = try_save_before_exit(&mut stdout, &task_repository);

    assert!(actual);
    assert_eq!(stdout.into_string(), "");
}

#[test]
fn test_try_save_before_exit_保存失敗ならerrorを表示して終了を止める() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("memoryに残すtask").unwrap();
    let task_id = task.get_id().unwrap();
    let task_repository = TestTaskRepository::new(task, now);
    task_repository.save_failures_remaining.set(1);
    let mut stdout = TestWriter::new();

    let actual = try_save_before_exit(&mut stdout, &task_repository);

    assert!(!actual);
    assert_eq!(
        task_repository
            .get_by_id(task_id)
            .unwrap()
            .get_name()
            .unwrap(),
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
        let task_repository =
            TestTaskRepository::new(new_test_task_handle("保存対象").unwrap(), now);
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
        let task_repository =
            TestTaskRepository::new(new_test_task_handle("保存対象").unwrap(), now);
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
    let task = new_test_task_handle("再試行中もmemoryに残すtask").unwrap();
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    task_repository.save_failures_remaining.set(1);
    let mut free_time_manager = TestFreeTimeManager;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();
    let mut exited = false;

    for _attempt in 0..2 {
        if try_exit_interactive(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            now,
        ) {
            exited = true;
            break;
        }
    }

    assert!(exited);
    assert_eq!(task_repository.save_attempt_count.get(), 2);
    assert_eq!(
        task_repository
            .get_by_id(task_id)
            .unwrap()
            .get_name()
            .unwrap(),
        "再試行中もmemoryに残すtask"
    );
    let output = stdout.into_string();
    assert_eq!(output.matches("[Error]").count(), 1);
}

#[test]
fn test_try_exit_interactive_ctrl_d終了時は帯を表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("Ctrl-D終了表示対象").unwrap();
    let task_id = task.get_id().unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerForBand;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    let exited = try_exit_interactive(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        now,
    );

    assert!(exited);
    let output = stdout.into_string();
    assert!(strip_ansi_escape_sequences(&output).contains(
        "凡例: # 固定  x 経過済み  = 繰返  - 単発  : 余差  . 空き  > 超過  (1文字=15分)"
    ));
    assert!(!output.contains("日          \t空          \t空差"));
}

#[allow(clippy::too_many_arguments, unused_must_use)]
fn execute_interactive_command(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &DateTime<Local>,
    focus_selection_mode: &mut FocusSelectionMode,
    operation_now: DateTime<Local>,
    command: &str,
) -> Result<bool, CommandError> {
    let parsed_command =
        parse_command(command, ParseMode::Interactive).map_err(map_command_parse_error)?;
    if let Some(outcome) = handle(&parsed_command).filter(|outcome| outcome.focus_request.is_some())
    {
        apply_command_outcome(
            stdout,
            task_repository,
            focused_task_id_opt,
            OutcomeApplicationMode::InteractiveUnflushed(focus_selection_mode),
            outcome,
            active_config(),
        )?;
    } else if matches!(
        parsed_command,
        Command::Defer { .. } | Command::InteractiveShortcut(_)
    ) {
        let mut context = RuntimeDeferCommandContext {
            task_repository,
            focused_task_id_opt,
            config: active_config(),
        };
        let outcome = handle_defer_command(&parsed_command, &mut context)?
            .expect("interactive defer command must be handler-owned");
        apply_command_outcome(
            stdout,
            task_repository,
            focused_task_id_opt,
            OutcomeApplicationMode::InteractiveUnflushed(focus_selection_mode),
            outcome,
            active_config(),
        )?;
    } else {
        if let Err(error) = execute_parsed(
            stdout,
            task_repository,
            free_time_manager,
            focused_task_id_opt,
            focus_started_datetime,
            command,
            &parsed_command,
        ) {
            let _output_error = render_display_model(stdout, &error_display_model(&error))
                .map_err(CommandError::Output);
        }
        if matches!(parsed_command, Command::Focus { task_id } if *focused_task_id_opt == Some(task_id))
        {
            *focus_selection_mode = FocusSelectionMode::Explicit;
        }
    }

    task_repository.sync_clock(operation_now);
    reconcile_focus_after_reload(task_repository, focused_task_id_opt, focus_selection_mode)
        .map_err(CommandError::from)
}

struct InteractiveRepositoryState<'a> {
    focused_task_id_opt: &'a mut Option<Uuid>,
    last_focused_task_id_opt: &'a mut Option<Uuid>,
    focus_started_datetime: &'a mut DateTime<Local>,
    focus_selection_mode: &'a mut FocusSelectionMode,
}

enum InteractiveRepositoryEvent<'a> {
    Submit { line: &'a str },
    Refresh,
    Exit,
    InputDisconnected,
    InputRead(std::io::Error),
    Interrupted,
}

enum InteractiveRepositoryEventOutcome {
    Continue,
    CommandExecuted(String, DateTime<Local>),
    Retry(CliRepositoryTransactionError),
    Exit,
    Fatal(RunError),
}

fn handle_interactive_submit_at(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    mut state: InteractiveRepositoryState<'_>,
    line: &str,
    operation_now: DateTime<Local>,
) -> InteractiveRepositoryEventOutcome {
    let command = line.trim().to_string();
    let transaction_result =
        run_cli_repository_transaction(task_repository, operation_now, |task_repository| {
            reconcile_interactive_state_after_reload(task_repository, &mut state, operation_now)?;
            writeln_newline(stdout, "").unwrap();
            writeln_newline(
                stdout,
                &format!(
                    "{}{}> {}{}",
                    style::Bold,
                    operation_now.format("%Y/%m/%d %H:%M:%S.%f"),
                    command,
                    style::Reset
                ),
            )
            .unwrap();
            writeln_newline(stdout, "").unwrap();
            stdout.flush().unwrap();

            if execute_interactive_command(
                stdout,
                task_repository,
                free_time_manager,
                state.focused_task_id_opt,
                state.focus_started_datetime,
                state.focus_selection_mode,
                operation_now,
                &command,
            )? {
                *state.last_focused_task_id_opt = None;
            }
            Ok(())
        });
    match transaction_result {
        Ok(()) => InteractiveRepositoryEventOutcome::CommandExecuted(command, operation_now),
        Err(error @ RunError::CliRepositoryTransaction(CliRepositoryTransactionError::Save(_))) => {
            InteractiveRepositoryEventOutcome::Fatal(error)
        }
        Err(RunError::CliRepositoryTransaction(error)) => {
            InteractiveRepositoryEventOutcome::Retry(error)
        }
        Err(RunError::Repository(error)) => {
            InteractiveRepositoryEventOutcome::Retry(CliRepositoryTransactionError::Load(error))
        }
        Err(error) => InteractiveRepositoryEventOutcome::Fatal(error),
    }
}

fn reconcile_interactive_state_after_reload(
    task_repository: &mut dyn TaskRepositoryTrait,
    state: &mut InteractiveRepositoryState<'_>,
    now: DateTime<Local>,
) -> Result<(), ApplicationError> {
    if reconcile_focus_after_reload(
        task_repository,
        state.focused_task_id_opt,
        state.focus_selection_mode,
    )? {
        *state.last_focused_task_id_opt = None;
        *state.focus_started_datetime = now;
    }
    Ok(())
}

fn handle_interactive_repository_event(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    mut state: InteractiveRepositoryState<'_>,
    event: InteractiveRepositoryEvent<'_>,
) -> InteractiveRepositoryEventOutcome {
    match event {
        InteractiveRepositoryEvent::Submit { line } => handle_interactive_submit_at(
            stdout,
            task_repository,
            free_time_manager,
            state,
            line,
            Local::now(),
        ),
        InteractiveRepositoryEvent::Refresh => {
            let now = Local::now();
            match reload_repository_for_cli(task_repository, now) {
                Ok(storage_lock) => {
                    reconcile_interactive_state_after_reload(task_repository, &mut state, now);
                    drop(storage_lock);
                    InteractiveRepositoryEventOutcome::Continue
                }
                Err(error) => InteractiveRepositoryEventOutcome::Retry(error),
            }
        }
        InteractiveRepositoryEvent::Exit => {
            let now = Local::now();
            match reload_repository_for_cli(task_repository, now) {
                Ok(_storage_lock) => {
                    reconcile_interactive_state_after_reload(task_repository, &mut state, now);
                    if try_exit_interactive(
                        stdout,
                        task_repository,
                        free_time_manager,
                        state.focused_task_id_opt,
                        now,
                    ) {
                        InteractiveRepositoryEventOutcome::Exit
                    } else {
                        InteractiveRepositoryEventOutcome::Continue
                    }
                }
                Err(error) => InteractiveRepositoryEventOutcome::Retry(error),
            }
        }
        InteractiveRepositoryEvent::InputDisconnected => InteractiveRepositoryEventOutcome::Fatal(
            handle_input_disconnected_with_reload(task_repository),
        ),
        InteractiveRepositoryEvent::InputRead(input_error) => {
            InteractiveRepositoryEventOutcome::Fatal(handle_input_read_error_with_reload(
                task_repository,
                input_error,
            ))
        }
        InteractiveRepositoryEvent::Interrupted => {
            InteractiveRepositoryEventOutcome::Fatal(RunError::Interrupted)
        }
    }
}

fn load_busy_time_slots_for_interactive_application(
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    busy_time_slots_file_path: &str,
) -> Result<(), RunError> {
    free_time_manager.load_busy_time_slots_from_file(busy_time_slots_file_path)?;
    Ok(())
}

fn interactive_application(
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<(), RunError> {
    let now = Local::now();
    drop(reload_repository_for_cli(task_repository, now)?);
    load_busy_time_slots_for_interactive_application(
        free_time_manager,
        active_config()
            .busy_time_slots_yaml_path
            .to_str()
            .expect("config path was validated"),
    )?;

    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;
    let mut focused_task_id_opt = select_focus_task_id(task_repository, focus_selection_mode)
        .map_err(CommandError::from)
        .map_err(RunError::from)?;
    let mut last_focused_task_id_opt = None;
    let mut focus_started_datetime = now;

    interactive::run(now, |stdout, event| {
        if let interactive::DriverEvent::RenderScreen { now } = event {
            render_interactive_screen(
                stdout,
                task_repository,
                free_time_manager,
                FocusRenderState {
                    focused_task_id_opt: &mut focused_task_id_opt,
                    last_focused_task_id_opt: &mut last_focused_task_id_opt,
                    focus_started_datetime: &mut focus_started_datetime,
                },
                now,
            );
            return interactive::DriverOutcome::Continue;
        }

        let repository_event = match event {
            interactive::DriverEvent::Refresh => InteractiveRepositoryEvent::Refresh,
            interactive::DriverEvent::Submit { line } => {
                InteractiveRepositoryEvent::Submit { line }
            }
            interactive::DriverEvent::Exit => InteractiveRepositoryEvent::Exit,
            interactive::DriverEvent::Interrupted => InteractiveRepositoryEvent::Interrupted,
            interactive::DriverEvent::InputDisconnected => {
                InteractiveRepositoryEvent::InputDisconnected
            }
            interactive::DriverEvent::InputRead(error) => {
                InteractiveRepositoryEvent::InputRead(error)
            }
            interactive::DriverEvent::RenderScreen { .. } => unreachable!(),
        };
        let outcome = handle_interactive_repository_event(
            stdout,
            task_repository,
            free_time_manager,
            InteractiveRepositoryState {
                focused_task_id_opt: &mut focused_task_id_opt,
                last_focused_task_id_opt: &mut last_focused_task_id_opt,
                focus_started_datetime: &mut focus_started_datetime,
                focus_selection_mode: &mut focus_selection_mode,
            },
            repository_event,
        );

        match outcome {
            InteractiveRepositoryEventOutcome::Continue => interactive::DriverOutcome::Continue,
            InteractiveRepositoryEventOutcome::CommandExecuted(command, operation_now) => {
                if !should_suppress_leaf_tasks_after_command(&command) {
                    let result =
                        execute_show_leaf_tasks(stdout, task_repository, free_time_manager);
                    report_application_result(stdout, result);
                }
                render_focused_task(
                    stdout,
                    task_repository,
                    focused_task_id_opt,
                    &mut last_focused_task_id_opt,
                    &mut focus_started_datetime,
                    operation_now,
                );
                interactive::DriverOutcome::Submitted
            }
            InteractiveRepositoryEventOutcome::Retry(error) => {
                interactive::DriverOutcome::Retry(error)
            }
            InteractiveRepositoryEventOutcome::Exit => interactive::DriverOutcome::Exit,
            InteractiveRepositoryEventOutcome::Fatal(error) => {
                interactive::DriverOutcome::Fatal(error)
            }
        }
    })
}
