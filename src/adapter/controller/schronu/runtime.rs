#![allow(unused_must_use)]

use super::command::{
    parse_command, Command, CommandAction, CommandKind, CommandParseError, ParseMode,
};
use super::handler::{
    handle, handle_breakdown_split_command, handle_defer_command, handle_finish_placement_command,
    handle_project_command, handle_task_attribute_command, handle_task_tree_command,
    CommandOutcome, DeferCommandContext, DeferCommandError, ExternalRequest,
    FinishPlacementCommandContext, FocusRequest, ProjectCommandContext,
    TaskAttributeCommandContext, TaskListOrder, TaskTreeCommandContext,
};
use super::interactive;
use super::renderer::{
    format_spreadsheet_task_row, render_display_model, render_plain_display_model, writeln_newline,
    DisplayModel, ErrorCapturingWriter, SchronuWriter, SpreadsheetTaskRow,
};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};
use regex::Regex;
use schronu::adapter::gateway::free_time_manager::FreeTimeManager;
use schronu::adapter::gateway::schronu_config::{load_schronu_config, SchronuConfig};
use schronu::adapter::gateway::storage_lock::{LockMode, StorageLock, StorageLockError};
use schronu::adapter::gateway::task_repository::TaskRepository;
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
use schronu::entity::datetime::parse_local_datetime;
use schronu::entity::task::{
    extract_leaf_tasks_from_project, extract_leaf_tasks_from_project_with_pending,
    read_project_category, round_up_sec_as_minute, ProjectCategory, Status, TaskAttr, TaskHandle,
    TaskTreeError,
};
use std::cmp::{max, min};
use std::collections::{HashMap, HashSet};
use std::env;
use std::io::{stdout, Write};
use std::process;
use std::sync::OnceLock;

#[path = "../storage_directory.rs"]
mod storage_directory;
use std::time::Duration as StdDuration;
use storage_directory::resolve_project_storage_directory;
use termion::color;
use termion::style;
use unicode_width::UnicodeWidthChar;
use url::Url;
use uuid::Uuid;

const MAX_ARRANGE_ESTIMATED_WORK_MINUTES: i64 = 1439;
const FOCUS_PROGRESS_BAR_SEGMENTS: usize = 100;
const CLI_LOCK_TIMEOUT: StdDuration = StdDuration::from_secs(1);

static ACTIVE_CONFIG: OnceLock<SchronuConfig> = OnceLock::new();

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
        FocusSelectionMode::LowestPriority { recent_days } => {
            let now = task_repository.get_last_synced_time();
            let first_business_day_start = try_next_business_day_start(now)?;
            let threshold_out_of_range = || ApplicationError::SubjectiveDateOutOfRange {
                operation: "defer_candidate_threshold",
                datetime: now,
            };
            let recent_duration =
                Duration::try_days(recent_days).ok_or_else(threshold_out_of_range)?;
            let recent_threshold = first_business_day_start
                .checked_add_signed(recent_duration)
                .ok_or_else(threshold_out_of_range)?;
            task_repository
                .get_defer_candidate_leaf_task_id(recent_threshold)
                .map_err(ApplicationError::TaskTree)
        }
    }
}

#[cfg(test)]
include!("runtime_test_support.rs");

#[cfg(test)]
#[path = "runtime_unit_tests.rs"]
mod tests;

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
    let yyyymmdd_reg = Regex::new(r"^(\d{4})/(\d{2})/(\d{2})$").unwrap();
    let yyyymmdd_pattern_date = pattern_opt
        .as_ref()
        .and_then(|pattern| yyyymmdd_reg.captures(pattern))
        .map(|captures| {
            let invalid_calendar_date = || ApplicationError::InvalidInput {
                field: "pattern",
                reason: "invalid calendar date",
            };
            let year = captures[1]
                .parse::<i32>()
                .map_err(|_| invalid_calendar_date())?;
            let month = captures[2]
                .parse::<u32>()
                .map_err(|_| invalid_calendar_date())?;
            let day = captures[3]
                .parse::<u32>()
                .map_err(|_| invalid_calendar_date())?;
            NaiveDate::from_ymd_opt(year, month, day).ok_or_else(invalid_calendar_date)
        })
        .transpose()?;
    let scheduled_tasks = get_schedule(task_repository)?;
    let mut task_list_display_rows: Vec<TaskListDisplayRow> = vec![];
    let mut available_biggest_row_opt: Option<TaskListDisplayRow> = None;
    let mut available_biggest_task_estimate_work_seconds = 0;

    // ここからρ計算用
    let last_synced_time = task_repository.get_last_synced_time();
    let last_synced_subjective_date = try_subjective_date(last_synced_time)?;
    let next_business_day_start = try_next_business_day_start(last_synced_time)?;

    let eod = try_subjective_date_end(
        last_synced_subjective_date,
        config.end_of_day_offset_minutes,
    )?;
    // ここまでρ計算用

    let is_calendar_func = pattern_opt
        .as_ref()
        .is_some_and(|pattern| pattern == "暦" || pattern == "calendar" || pattern == "cal");

    let is_band_func = pattern_opt
        .as_ref()
        .is_some_and(|pattern| pattern == "帯" || pattern == "band");

    let is_today_func = pattern_opt.as_ref().is_some_and(|pattern| pattern == "今");

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
        let subjective_naive_date = try_subjective_date(*scheduled_start)?;
        let needs_scheduled_boundary = pattern_opt.as_ref().is_some_and(|pattern| {
            pattern == "今"
                || pattern == "明"
                || pattern == "近"
                || pattern == "暦"
                || pattern == "帯"
                || pattern == "週"
                || pattern == "末"
                || pattern == "翌"
                || days_of_week.contains(&pattern.as_str())
        });
        let scheduled_next_business_day_start = needs_scheduled_boundary
            .then(|| try_next_business_day_start(*scheduled_start))
            .transpose()?;

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

                if let Some(scheduled_boundary) = scheduled_next_business_day_start {
                    if scheduled_boundary - next_business_day_start > Duration::days(valid_days) {
                        break;
                    }
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
                let deadline_naive_date = try_subjective_date(*deadline_time)?;

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
                    if (pattern == "今" && *scheduled_start < next_business_day_start)
                        || (pattern == "明"
                            && *current_datetime_cursor_clone >= next_business_day_start
                            && (*scheduled_start - next_business_day_start) < Duration::days(1))
                        || (pattern == "近"
                            && (*scheduled_start - next_business_day_start) < Duration::days(1))
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
                && task_deadline_time_opt.unwrap() < next_business_day_start
                && task_deadline_time_opt.unwrap() < end_datetime
            {
                &breaking_deadline_icon
            } else if task_deadline_time_opt.is_some()
                && task_deadline_time_opt.unwrap() < next_business_day_start
            {
                &deadline_icon
            } else if rank == &0 && scheduled_start < &eod {
                &today_leaf_icon
            } else {
                // - : 特に無しだが、空白にすると列数が乱れるので目立たない記号を入れる
                "-"
            };

            let deadline_string = if let Some(deadline_time) = deadline_time_opt {
                if *deadline_time < next_business_day_start {
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
                                && task_deadline_time_opt.unwrap() < next_business_day_start
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
                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary == next_business_day_start
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "明" {
                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary - next_business_day_start == Duration::days(1)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "近" {
                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            let diff = scheduled_boundary - next_business_day_start;
                            diff == Duration::zero() || diff == Duration::days(1)
                        }) {
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
                        let now_weekday_jp = get_weekday_jp(&last_synced_subjective_date);

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

                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary - next_business_day_start == Duration::days(days)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "週" {
                        // 今日を含む直近1週間のタスクを表示する
                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary - next_business_day_start < Duration::days(7)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "末" {
                        // 週末までのタスクを表示する
                        let now_weekday_jp = get_weekday_jp(&last_synced_subjective_date);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind =
                            days_of_week.iter().position(|&x| x == "日").unwrap();

                        let days_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary - next_business_day_start
                                <= Duration::days(days_diff as i64)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "翌" {
                        // 翌週末までのタスクを表示する
                        let now_weekday_jp = get_weekday_jp(&last_synced_subjective_date);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind =
                            days_of_week.iter().position(|&x| x == "日").unwrap();

                        let days_diff =
                            ((7 + target_days_of_week_ind - now_days_of_week_ind) % 7) as i64;

                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            let diff = scheduled_boundary - next_business_day_start;
                            Duration::days(days_diff) < diff
                                && diff <= Duration::days(days_diff + 7)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if let Some(pattern_date) = yyyymmdd_pattern_date {
                        if pattern_date == subjective_naive_date {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if integer_reg.is_match(pattern) {
                        let caps = integer_reg.captures(pattern).unwrap();
                        let input_minute: i64 = caps[0].parse().unwrap();
                        let target_free_time_seconds = input_minute * 60;

                        if *scheduled_start > next_business_day_start
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

    let naive_dt_today = last_synced_subjective_date;
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

    if is_today_func || is_calendar_func || is_band_func {
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

fn make_obsidian_search_url_with_vault(query: &str, vault_name: &str) -> String {
    format!(
        "obsidian://search?vault={}&query={}",
        percent_encode(vault_name.as_bytes(), OBSIDIAN_VAULT_ASCII_SET),
        percent_encode(query.as_bytes(), MY_ASCII_SET)
    )
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

fn read_project_category_command_arg(s: &str) -> Option<Option<ProjectCategory>> {
    match s.to_lowercase().as_str() {
        "_" | "none" | "clear" => Some(None),
        _ => read_project_category(s).map(Some),
    }
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

// 削除できない時はNoneを返す。例えば、文字列が空の時
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

fn parse_non_interactive_command(args: Vec<String>) -> Option<String> {
    if args.is_empty() {
        return None;
    }

    Some(args.join(" "))
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
    } else if matches!(parsed_command, Command::TuckAway) {
        execute_defer(task_repository, focused_task_id_opt, 1, "秒")?;
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

#[cfg(test)]
include!("runtime_contract_tests.rs");
