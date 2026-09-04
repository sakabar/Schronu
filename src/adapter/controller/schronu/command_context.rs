use super::command::{parse_project_category_input, CommandKind, CommandParseError};
use super::handler::{
    DeferCommandContext, DeferCommandError, FinishPlacementCommandContext, HandlerError,
    NextUpResult, ProjectCommandContext, TaskAttributeCommandContext, TaskListOrder,
    TaskTreeCommandContext,
};
use super::renderer::{DisplayModel, TreeDisplay};
use super::view::{
    build_ancestor_tree_display, build_leaf_tree_display, build_show_all_tasks_display_with_config,
    build_tree_display, get_weekday_jp, TaskListDisplayOrder,
};
use crate::adapter::gateway::schronu_config::SchronuConfig;
use crate::application::daily_capacity::{
    try_local_date_and_time, try_logical_date, try_logical_date_start, try_next_logical_date_start,
};
use crate::application::flatten_use_case::{
    flatten_tasks_with_end_of_day_offset_minutes, FlattenResult,
};
use crate::application::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use crate::application::pack_use_case::{pack_tasks_with_end_of_day_offset_minutes, PackResult};
use crate::application::schedule_use_case::get_schedule;
use crate::application::task_use_case::{
    breakdown_task, complete_task, create_task, defer_routine_task, defer_task,
    estimated_work_seconds_from_minutes, set_category, set_deadline, set_estimate,
    validate_task_name, ApplicationError, BreakdownTaskInput, CompleteTaskInput, CreateTaskInput,
    TaskFactory,
};
use crate::entity::task::{
    extract_leaf_tasks_from_project, extract_leaf_tasks_from_project_with_pending, Status,
    TaskAttr, TaskHandle, TaskTreeError,
};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const MAX_ARRANGE_ESTIMATED_WORK_MINUTES: i64 = 1439;

pub(super) fn resolve_upcoming_mmdd(
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
    let out_of_range = || ApplicationError::LogicalDateOutOfRange {
        operation: "upcoming_calendar_date",
        datetime: now,
    };
    let current_year_date =
        NaiveDate::from_ymd_opt(now.year(), month, day).ok_or_else(out_of_range)?;
    let current_year_start = try_logical_date_start(current_year_date)?;
    if current_year_start >= now {
        return Ok(Some(current_year_start));
    }

    let next_year = now.year().checked_add(1).ok_or_else(out_of_range)?;
    let next_validation_year = 2000 + next_year.rem_euclid(400);
    if NaiveDate::from_ymd_opt(next_validation_year, month, day).is_none() {
        return Ok(None);
    }
    let next_year_date = NaiveDate::from_ymd_opt(next_year, month, day).ok_or_else(out_of_range)?;
    Ok(Some(try_logical_date_start(next_year_date)?))
}

pub(super) fn resolve_upcoming_clear_or_gather_day(
    date: &str,
    now: DateTime<Local>,
) -> Result<Option<DateTime<Local>>, ApplicationError> {
    if date == "明" {
        return Ok(Some(try_next_logical_date_start(now)?));
    }

    let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];
    if let Some(target_days_of_week_ind) = days_of_week.iter().position(|day| *day == date) {
        let logical_date = try_logical_date(now)?;
        let now_days_of_week_ind = days_of_week
            .iter()
            .position(|day| *day == get_weekday_jp(&logical_date))
            .expect("logical weekday must be in the Japanese weekday table");
        let days_until_target =
            (7 + target_days_of_week_ind - now_days_of_week_ind) % days_of_week.len();
        let days = if days_until_target == 0 {
            7
        } else {
            days_until_target
        };

        let target_date = logical_date
            .checked_add_signed(Duration::days(days as i64))
            .ok_or(ApplicationError::LogicalDateOutOfRange {
                operation: "weekday_date",
                datetime: now,
            })?;
        let target_datetime = try_logical_date_start(target_date)?;
        return Ok(Some(target_datetime));
    }

    resolve_upcoming_mmdd(date, now)
}

pub(super) fn resolve_show_all_pattern(
    pattern: &str,
    now: DateTime<Local>,
) -> Result<String, ApplicationError> {
    Ok(match resolve_upcoming_mmdd(pattern, now)? {
        Some(datetime) => datetime.format("%Y/%m/%d").to_string(),
        None => pattern.to_string(),
    })
}

pub(super) fn parse_clear_or_gather_defer_to_datetime(
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
        let logical_date = try_logical_date(now)?;
        let target_date = logical_date
            .checked_add_signed(Duration::days(i64::from(hour / 24)))
            .ok_or(ApplicationError::LogicalDateOutOfRange {
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
            .ok_or(ApplicationError::LogicalDateOutOfRange {
                operation: "clear_or_gather_minutes",
                datetime: now,
            })?;
        return Ok(Some(defer_to_datetime));
    }

    Ok(None)
}

type ClearOrGatherTimeRange = (DateTime<Local>, DateTime<Local>);

pub(super) fn resolve_dated_clear_or_gather_end_naive(
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

pub(super) fn parse_dated_clear_or_gather_time_range(
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
            try_logical_date_start(try_logical_date(scheduled.scheduled_start)?)?;
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

pub(super) fn execute_next_up(
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

pub(super) fn split_amount_and_unit(input: &str) -> Vec<String> {
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

pub(super) fn execute_defer(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    amount: i64,
    unit_str: &str,
) -> Result<(), ApplicationError> {
    let now = task_repository.get_last_synced_time();
    let duration_out_of_range = || ApplicationError::LogicalDateOutOfRange {
        operation: "defer_pending_until",
        datetime: now,
    };
    let duration = match unit_str.chars().next() {
        // 日単位の延期は固定24時間ではなく、次の論理日開始を基準にする
        Some('日') | Some('d') => {
            let target = defer_logical_date_target(now, amount)?;
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

pub(super) fn defer_logical_date_target(
    now: DateTime<Local>,
    amount: i64,
) -> Result<DateTime<Local>, ApplicationError> {
    if amount <= 0 {
        return Ok(now);
    }

    let first_logical_date_start = try_next_logical_date_start(now)?;
    let out_of_range = || ApplicationError::LogicalDateOutOfRange {
        operation: "defer_logical_dates",
        datetime: now,
    };
    let additional_days = amount.checked_sub(1).ok_or_else(out_of_range)?;
    let additional_duration = Duration::try_days(additional_days).ok_or_else(out_of_range)?;
    let target_date = first_logical_date_start
        .date_naive()
        .checked_add_signed(additional_duration)
        .ok_or_else(out_of_range)?;
    try_logical_date_start(target_date)
}

fn seconds_until_next_logical_date_start_with_offset(
    now: DateTime<Local>,
    offset_seconds: i64,
) -> Result<i64, ApplicationError> {
    let next_logical_date_start = try_next_logical_date_start(now)?;
    (next_logical_date_start - now)
        .num_seconds()
        .checked_add(offset_seconds)
        .ok_or(ApplicationError::LogicalDateOutOfRange {
            operation: "next_logical_date_start",
            datetime: now,
        })
}

pub(super) fn execute_defer_expression(
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
                        let defer_dst_time = try_logical_date_start(date)?;
                        Some((defer_dst_time - now).num_seconds().checked_add(1).ok_or(
                            ApplicationError::LogicalDateOutOfRange {
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
                let out_of_range = || ApplicationError::LogicalDateOutOfRange {
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
                let today = try_logical_date(now)?;
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
                    .ok_or(ApplicationError::LogicalDateOutOfRange {
                        operation: "next_logical_date_start",
                        datetime: now,
                    })?;
                Some(seconds_until_next_logical_date_start_with_offset(
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
pub(super) fn execute_defer_routine(
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
    if focused_task
        .get_deadline_time_opt()
        .map_err(ApplicationError::TaskTree)?
        .is_none()
    {
        return Ok(());
    }
    let Some(parent_task) = focused_task.parent().map_err(ApplicationError::TaskTree)? else {
        return Ok(());
    };
    if parent_task
        .get_repetition_interval_days_opt()
        .map_err(ApplicationError::TaskTree)?
        .is_none()
    {
        return Ok(());
    }

    defer_routine_task(task_repository, focused_task_id)?;
    *focused_task_id_opt = None;
    Ok(())
}

// 長期間未起動時に蓄積した未完了taskのうち、短周期(7日以内)のroutineを自動的に先送りする
// 24時間の閾値により締切直後のroutineを対象外とし、1日以上滞留したものだけに限定する
// 長周期のroutineは年次taskなど重要な予定を含み得るため、自動的には先送りしない
pub(super) fn execute_defer_all_frequent_routines(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    _focused_task_opt: &Option<TaskHandle>,
) -> Result<(), ApplicationError> {
    const MAX_REPETITION_INTERVAL_DAYS: i64 = 7;
    const MIN_OVERDUE_HOURS: i64 = 24;
    let now = task_repository.get_last_synced_time();
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

        // 条件を満たす未完了の葉taskがなくなるまで、routineの先送りを適用する
        for task_id in candidate_task_ids.into_iter() {
            *focused_task_id_opt = Some(task_id);
            let orig_focused_task_id_opt = *focused_task_id_opt;
            execute_defer_routine(task_repository, focused_task_id_opt)?;

            // deferが成功してフォーカスが移ったら記録しておく
            if orig_focused_task_id_opt != *focused_task_id_opt {
                any_is_changed = true;
            }
        }

        if !any_is_changed {
            break;
        }
    }
    Ok(())
}

fn command_parse_error(
    command: &'static str,
    field: &'static str,
    reason: &'static str,
    usage: &'static str,
) -> HandlerError {
    HandlerError::Parse(CommandParseError::new(command, field, reason, usage))
}

pub(super) fn resolve_deadline_date(
    value: &str,
    now: DateTime<Local>,
) -> Result<String, HandlerError> {
    if value == "消" {
        return Ok(value.to_string());
    }
    if value.starts_with('今') {
        return Ok(try_logical_date(now)?.format("%Y/%m/%d").to_string());
    }
    if value.starts_with('明') {
        return Ok(try_next_logical_date_start(now)?
            .format("%Y/%m/%d")
            .to_string());
    }

    let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];
    if days_of_week.contains(&value) {
        let today = try_logical_date(now)?;
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
            ApplicationError::LogicalDateOutOfRange {
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
        let out_of_range = || ApplicationError::LogicalDateOutOfRange {
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
) -> Result<Option<DateTime<Local>>, HandlerError> {
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

pub(super) fn set_focused_task_priority(
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

pub(super) struct RuntimeTaskAttributeCommandContext<'a> {
    pub(super) task_repository: &'a mut dyn TaskRepositoryTrait,
    pub(super) focused_task_id_opt: &'a mut Option<Uuid>,
    pub(super) focus_started_datetime: &'a DateTime<Local>,
    pub(super) config: &'a SchronuConfig,
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
    fn set_deadline(&mut self, value: &str) -> Result<(), HandlerError> {
        let deadline_time = resolve_deadline_time(
            value,
            self.task_repository.get_last_synced_time(),
            self.config,
        )?;
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

    fn set_category(&mut self, value: &str) -> Result<(), HandlerError> {
        let project_category = parse_project_category_input(value).map_err(HandlerError::Parse)?;
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

pub(super) struct RuntimeDeferCommandContext<'a> {
    pub(super) task_repository: &'a mut dyn TaskRepositoryTrait,
    pub(super) focused_task_id_opt: &'a mut Option<Uuid>,
    pub(super) config: &'a SchronuConfig,
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
        let seconds = seconds_until_next_logical_date_start_with_offset(now, 1)?;
        self.defer(seconds, "秒")
    }

    fn defer_next_week(&mut self) -> Result<(), DeferCommandError> {
        let now = self.task_repository.get_last_synced_time();
        let seconds = seconds_until_next_logical_date_start_with_offset(now, 86400 * 6 + 1)?;
        self.defer(seconds, "秒")
    }

    fn defer_routine(&mut self) -> Result<(), ApplicationError> {
        execute_defer_routine(self.task_repository, self.focused_task_id_opt)
    }

    fn defer_five_years(&mut self) -> Result<(), DeferCommandError> {
        let now = self.task_repository.get_last_synced_time();
        let seconds =
            seconds_until_next_logical_date_start_with_offset(now, 86400 * (7 * 52 * 5 - 1) + 1)?;
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

pub(super) struct RuntimeTaskTreeCommandContext<'repository, 'factory, 'generator> {
    pub(super) task_repository: &'repository mut dyn TaskRepositoryTrait,
    pub(super) free_time_manager: &'repository mut dyn FreeTimeManagerTrait,
    pub(super) focused_task_id_opt: &'repository mut Option<Uuid>,
    pub(super) task_factory: &'factory mut TaskFactory<'generator>,
    pub(super) config: &'repository SchronuConfig,
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
    fn show_tree(&mut self) -> Result<TreeDisplay, ApplicationError> {
        build_tree_display(&self.focused_task()?)
    }

    fn show_ancestor(&mut self) -> Result<TreeDisplay, ApplicationError> {
        build_ancestor_tree_display(&self.focused_task()?)
    }

    fn focus_root(&mut self) -> Result<(), ApplicationError> {
        if let Some(focused_task) = self.focused_task()? {
            let root_task = focused_task.root().map_err(ApplicationError::TaskTree)?;
            *self.focused_task_id_opt =
                Some(root_task.get_id().map_err(ApplicationError::TaskTree)?);
        }
        Ok(())
    }

    fn show_leaves(&mut self) -> Result<TreeDisplay, ApplicationError> {
        build_leaf_tree_display(self.task_repository)
    }

    fn show_task_list(
        &mut self,
        pattern: Option<&str>,
        order: TaskListOrder,
        resolve_pattern: bool,
    ) -> Result<DisplayModel, ApplicationError> {
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
        build_show_all_tasks_display_with_config(
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

    fn pick(&mut self, task_id: Option<Uuid>) -> Result<(), ApplicationError> {
        let Some(task_id) = task_id.or(*self.focused_task_id_opt) else {
            return Ok(());
        };
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

    fn focus_children(&mut self) -> Result<Option<DisplayModel>, ApplicationError> {
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
                [_, _, ..] => {
                    return Ok(Some(DisplayModel::Tree(build_tree_display(
                        &focused_task_opt,
                    )?)));
                }
                _ => {}
            }
        }
        Ok(None)
    }

    fn focus_deepest(&mut self) -> Result<Option<DisplayModel>, ApplicationError> {
        let Some(focused_task) = self.focused_task()? else {
            return Ok(None);
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
            return Ok(Some(DisplayModel::Tree(build_tree_display(&Some(
                deepest_task,
            ))?)));
        }
        Ok(None)
    }

    fn next_up(
        &mut self,
        name: &str,
        estimated_minutes: Option<i64>,
    ) -> Result<NextUpResult, ApplicationError> {
        let focused_task_opt = self.focused_task()?;
        let result = execute_next_up(
            self.focused_task_id_opt,
            &focused_task_opt,
            name,
            &estimated_minutes,
            self.task_factory,
        );
        match result {
            Ok(_) => Ok(NextUpResult::NoDisplay),
            Err(error) => Ok(NextUpResult::ReportedError(error)),
        }
    }
}

pub(super) struct CliCommandContext<'repository, 'factory, 'generator> {
    pub(super) task_repository: &'repository mut dyn TaskRepositoryTrait,
    pub(super) free_time_manager: &'repository mut dyn FreeTimeManagerTrait,
    pub(super) focused_task_id_opt: &'repository mut Option<Uuid>,
    pub(super) task_factory: &'factory mut TaskFactory<'generator>,
    pub(super) focus_started_datetime: DateTime<Local>,
    pub(super) config: &'repository SchronuConfig,
}

impl ProjectCommandContext for CliCommandContext<'_, '_, '_> {
    fn last_synced_time(&self) -> DateTime<Local> {
        self.task_repository.get_last_synced_time()
    }

    fn focused_task(&mut self) -> Result<Option<TaskHandle>, ApplicationError> {
        match *self.focused_task_id_opt {
            Some(id) => self
                .task_repository
                .get_by_id(id)
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

impl TaskAttributeCommandContext for CliCommandContext<'_, '_, '_> {
    fn set_deadline(&mut self, value: &str) -> Result<(), HandlerError> {
        let mut context = RuntimeTaskAttributeCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            focus_started_datetime: &self.focus_started_datetime,
            config: self.config,
        };
        context.set_deadline(value)
    }

    fn set_estimate(&mut self, minutes: i64) -> Result<(), ApplicationError> {
        let mut context = RuntimeTaskAttributeCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            focus_started_datetime: &self.focus_started_datetime,
            config: self.config,
        };
        context.set_estimate(minutes)
    }

    fn arrange(
        &mut self,
        minutes: i64,
        includes_zero_estimate: bool,
    ) -> Result<(), ApplicationError> {
        let mut context = RuntimeTaskAttributeCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            focus_started_datetime: &self.focus_started_datetime,
            config: self.config,
        };
        context.arrange(minutes, includes_zero_estimate)
    }

    fn set_actual(&mut self, minutes: i64) -> Result<(), ApplicationError> {
        let mut context = RuntimeTaskAttributeCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            focus_started_datetime: &self.focus_started_datetime,
            config: self.config,
        };
        context.set_actual(minutes)
    }

    fn set_priority(&mut self, priority: i64) -> Result<(), ApplicationError> {
        let mut context = RuntimeTaskAttributeCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            focus_started_datetime: &self.focus_started_datetime,
            config: self.config,
        };
        context.set_priority(priority)
    }

    fn set_category(&mut self, value: &str) -> Result<(), HandlerError> {
        let mut context = RuntimeTaskAttributeCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            focus_started_datetime: &self.focus_started_datetime,
            config: self.config,
        };
        context.set_category(value)
    }

    fn add_work(&mut self, minutes: Option<i64>) -> Result<(), ApplicationError> {
        let mut context = RuntimeTaskAttributeCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            focus_started_datetime: &self.focus_started_datetime,
            config: self.config,
        };
        context.add_work(minutes)
    }
}

impl DeferCommandContext for CliCommandContext<'_, '_, '_> {
    fn defer(&mut self, amount: i64, unit: &str) -> Result<(), DeferCommandError> {
        RuntimeDeferCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            config: self.config,
        }
        .defer(amount, unit)
    }

    fn defer_expression(&mut self, values: &[String]) -> Result<(), DeferCommandError> {
        RuntimeDeferCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            config: self.config,
        }
        .defer_expression(values)
    }

    fn defer_next_morning(&mut self) -> Result<(), DeferCommandError> {
        RuntimeDeferCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            config: self.config,
        }
        .defer_next_morning()
    }

    fn defer_next_week(&mut self) -> Result<(), DeferCommandError> {
        RuntimeDeferCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            config: self.config,
        }
        .defer_next_week()
    }

    fn defer_routine(&mut self) -> Result<(), ApplicationError> {
        RuntimeDeferCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            config: self.config,
        }
        .defer_routine()
    }

    fn defer_five_years(&mut self) -> Result<(), DeferCommandError> {
        RuntimeDeferCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            config: self.config,
        }
        .defer_five_years()
    }

    fn defer_all_frequent_routines(&mut self) -> Result<(), ApplicationError> {
        RuntimeDeferCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            config: self.config,
        }
        .defer_all_frequent_routines()
    }

    fn prepare_escape(&mut self) -> Result<bool, ApplicationError> {
        RuntimeDeferCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            config: self.config,
        }
        .prepare_escape()
    }

    fn extrude(&mut self, step_days: Option<u16>) -> Result<(), ApplicationError> {
        RuntimeDeferCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            config: self.config,
        }
        .extrude(step_days)
    }

    fn clear_or_gather(
        &mut self,
        kind: CommandKind,
        values: &[String],
    ) -> Result<(), ApplicationError> {
        RuntimeDeferCommandContext {
            task_repository: self.task_repository,
            focused_task_id_opt: self.focused_task_id_opt,
            config: self.config,
        }
        .clear_or_gather(kind, values)
    }
}

impl FinishPlacementCommandContext for CliCommandContext<'_, '_, '_> {
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

    fn show_focused_tree(&mut self) -> Result<TreeDisplay, ApplicationError> {
        build_tree_display(&FinishPlacementCommandContext::focused_task(self)?)
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

impl TaskTreeCommandContext for CliCommandContext<'_, '_, '_> {
    fn show_tree(&mut self) -> Result<TreeDisplay, ApplicationError> {
        RuntimeTaskTreeCommandContext {
            task_repository: self.task_repository,
            free_time_manager: self.free_time_manager,
            focused_task_id_opt: self.focused_task_id_opt,
            task_factory: self.task_factory,
            config: self.config,
        }
        .show_tree()
    }

    fn show_ancestor(&mut self) -> Result<TreeDisplay, ApplicationError> {
        RuntimeTaskTreeCommandContext {
            task_repository: self.task_repository,
            free_time_manager: self.free_time_manager,
            focused_task_id_opt: self.focused_task_id_opt,
            task_factory: self.task_factory,
            config: self.config,
        }
        .show_ancestor()
    }

    fn focus_root(&mut self) -> Result<(), ApplicationError> {
        RuntimeTaskTreeCommandContext {
            task_repository: self.task_repository,
            free_time_manager: self.free_time_manager,
            focused_task_id_opt: self.focused_task_id_opt,
            task_factory: self.task_factory,
            config: self.config,
        }
        .focus_root()
    }

    fn show_leaves(&mut self) -> Result<TreeDisplay, ApplicationError> {
        RuntimeTaskTreeCommandContext {
            task_repository: self.task_repository,
            free_time_manager: self.free_time_manager,
            focused_task_id_opt: self.focused_task_id_opt,
            task_factory: self.task_factory,
            config: self.config,
        }
        .show_leaves()
    }

    fn show_task_list(
        &mut self,
        pattern: Option<&str>,
        order: TaskListOrder,
        resolve_pattern: bool,
    ) -> Result<DisplayModel, ApplicationError> {
        RuntimeTaskTreeCommandContext {
            task_repository: self.task_repository,
            free_time_manager: self.free_time_manager,
            focused_task_id_opt: self.focused_task_id_opt,
            task_factory: self.task_factory,
            config: self.config,
        }
        .show_task_list(pattern, order, resolve_pattern)
    }

    fn focus(&mut self, task_id: Uuid) {
        *self.focused_task_id_opt = Some(task_id);
    }

    fn pick(&mut self, task_id: Option<Uuid>) -> Result<(), ApplicationError> {
        RuntimeTaskTreeCommandContext {
            task_repository: self.task_repository,
            free_time_manager: self.free_time_manager,
            focused_task_id_opt: self.focused_task_id_opt,
            task_factory: self.task_factory,
            config: self.config,
        }
        .pick(task_id)
    }

    fn focus_parent(&mut self) -> Result<(), ApplicationError> {
        RuntimeTaskTreeCommandContext {
            task_repository: self.task_repository,
            free_time_manager: self.free_time_manager,
            focused_task_id_opt: self.focused_task_id_opt,
            task_factory: self.task_factory,
            config: self.config,
        }
        .focus_parent()
    }

    fn focus_children(&mut self) -> Result<Option<DisplayModel>, ApplicationError> {
        RuntimeTaskTreeCommandContext {
            task_repository: self.task_repository,
            free_time_manager: self.free_time_manager,
            focused_task_id_opt: self.focused_task_id_opt,
            task_factory: self.task_factory,
            config: self.config,
        }
        .focus_children()
    }

    fn focus_deepest(&mut self) -> Result<Option<DisplayModel>, ApplicationError> {
        RuntimeTaskTreeCommandContext {
            task_repository: self.task_repository,
            free_time_manager: self.free_time_manager,
            focused_task_id_opt: self.focused_task_id_opt,
            task_factory: self.task_factory,
            config: self.config,
        }
        .focus_deepest()
    }

    fn next_up(
        &mut self,
        name: &str,
        estimated_minutes: Option<i64>,
    ) -> Result<NextUpResult, ApplicationError> {
        RuntimeTaskTreeCommandContext {
            task_repository: self.task_repository,
            free_time_manager: self.free_time_manager,
            focused_task_id_opt: self.focused_task_id_opt,
            task_factory: self.task_factory,
            config: self.config,
        }
        .next_up(name, estimated_minutes)
    }
}
