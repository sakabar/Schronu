use super::error::{WebReadCoreError, WebReadOverflowError};
use super::model::{
    AllTaskRowDto, DeadlineDisplayKind, DeferModeDto, DeferPlanDto, ScheduledTaskRowDto,
    ServerSnapshot, SessionTaskDto, TaskDisplayKind,
};
use crate::adapter::controller::deadline_display::{
    format_deadline_remaining_time, misses_deadline,
};
use crate::application::daily_capacity::{try_logical_date, try_logical_date_start};
use crate::application::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use crate::application::schedule_use_case::{
    get_schedule, scheduled_logical_dates, ScheduledTaskView,
};
use crate::application::task_use_case::{
    get_focus, plan_defer_task, ApplicationError, DeferMode, DeferTaskPlan,
};
use chrono::{DateTime, Local, NaiveDate};
use std::collections::HashMap;
use uuid::Uuid;

#[cfg(test)]
pub(in crate::adapter::controller) fn build_server_snapshot<R, F>(
    task_repository: &mut R,
    free_time_manager: &mut F,
    operation_now: DateTime<Local>,
) -> Result<ServerSnapshot, WebReadCoreError>
where
    R: TaskRepositoryTrait,
    F: FreeTimeManagerTrait,
{
    build_server_snapshot_with_offset(
        task_repository,
        free_time_manager,
        operation_now,
        crate::application::daily_capacity::END_OF_DAY_OFFSET_MINUTES,
    )
}

pub(in crate::adapter::controller) fn build_all_task_rows(
    repository: &dyn TaskRepositoryTrait,
    schedule: &[ScheduledTaskView],
    last_synced_time: DateTime<Local>,
) -> Result<Vec<AllTaskRowDto>, WebReadCoreError> {
    let logical_dates = scheduled_logical_dates(schedule).map_err(WebReadCoreError::Application)?;
    let mut task_kind_cache = HashMap::new();
    logical_dates
        .into_iter()
        .zip(schedule)
        .enumerate()
        .map(|(segment_index, (logical_date, segment))| {
            let deadline = segment.task.deadline_time;
            let deadline_label = format_deadline_remaining_time(
                deadline.as_ref(),
                segment.scheduled_end,
                last_synced_time,
            )
            .map_err(WebReadCoreError::Application)?;
            let (task_display_kind, deadline_display_kind) =
                classify_display_kinds(repository, segment, logical_date, &mut task_kind_cache)?;
            Ok(AllTaskRowDto {
                task: session_task_dto(
                    segment.task.id.hyphenated().to_string(),
                    segment.task.name.clone(),
                    segment.task.estimated_work_seconds,
                    segment.task.actual_work_seconds,
                ),
                segment_index,
                schedule_date: logical_date.format("%Y-%m-%d").to_string(),
                deadline_epoch_ms: deadline.map(|value| value.timestamp_millis()),
                deadline_label,
                misses_deadline: misses_deadline(deadline.as_ref(), segment.scheduled_end),
                task_display_kind,
                deadline_display_kind,
                is_leaf: segment.is_leaf(),
            })
        })
        .collect()
}

pub(super) fn build_server_snapshot_with_offset<R, F>(
    task_repository: &mut R,
    free_time_manager: &mut F,
    operation_now: DateTime<Local>,
    end_of_day_offset_minutes: i64,
) -> Result<ServerSnapshot, WebReadCoreError>
where
    R: TaskRepositoryTrait,
    F: FreeTimeManagerTrait,
{
    let schedule = get_schedule(task_repository).map_err(WebReadCoreError::Application)?;
    build_server_snapshot_from_schedule(
        task_repository,
        free_time_manager,
        operation_now,
        &schedule,
        end_of_day_offset_minutes,
    )
}

pub(super) fn build_server_snapshot_from_schedule<R, F>(
    task_repository: &mut R,
    free_time_manager: &mut F,
    operation_now: DateTime<Local>,
    schedule: &[ScheduledTaskView],
    end_of_day_offset_minutes: i64,
) -> Result<ServerSnapshot, WebReadCoreError>
where
    R: TaskRepositoryTrait,
    F: FreeTimeManagerTrait,
{
    let logical_date = try_logical_date(operation_now).map_err(WebReadCoreError::Application)?;
    let observed_at = task_repository.get_last_synced_time();
    let current_logical_date =
        try_logical_date(observed_at).map_err(WebReadCoreError::Application)?;
    let remaining_capacity_seconds = if logical_date == current_logical_date {
        let end = crate::application::daily_capacity::try_logical_date_end(
            logical_date,
            end_of_day_offset_minutes,
        )
        .map_err(WebReadCoreError::Application)?;
        if observed_at < end {
            free_time_manager.get_free_seconds(&observed_at, &end)
        } else {
            end.signed_duration_since(observed_at).num_seconds()
        }
    } else {
        let start = crate::application::daily_capacity::try_logical_date_start(logical_date)
            .map_err(WebReadCoreError::Application)?;
        let end = crate::application::daily_capacity::try_logical_date_end(
            logical_date,
            end_of_day_offset_minutes,
        )
        .map_err(WebReadCoreError::Application)?;
        free_time_manager.get_free_seconds(&start, &end)
    };
    let segment_logical_dates =
        scheduled_logical_dates(schedule).map_err(WebReadCoreError::Application)?;
    let scheduled_segments = segment_logical_dates
        .into_iter()
        .zip(schedule)
        .map(|(date, segment)| (date, segment.scheduled_work_seconds))
        .collect::<Vec<_>>();
    let buffer_seconds = calculate_buffer_seconds(
        logical_date,
        remaining_capacity_seconds,
        &scheduled_segments,
    )?;

    Ok(ServerSnapshot {
        observed_at_epoch_ms: operation_now.timestamp_millis(),
        logical_date: logical_date.format("%Y-%m-%d").to_string(),
        buffer_seconds,
    })
}

pub(in crate::adapter::controller) fn build_scheduled_task_rows(
    repository: &dyn TaskRepositoryTrait,
    schedule: &[ScheduledTaskView],
    logical_date: NaiveDate,
    last_synced_time: DateTime<Local>,
) -> Result<Vec<ScheduledTaskRowDto>, WebReadCoreError> {
    let segment_logical_dates =
        scheduled_logical_dates(schedule).map_err(WebReadCoreError::Application)?;
    let mut dated_segments = segment_logical_dates
        .into_iter()
        .zip(schedule)
        .collect::<Vec<_>>();
    dated_segments.retain(|(date, _)| *date == logical_date);
    dated_segments.sort_by_key(|(_, segment)| segment.scheduled_start);

    let mut task_kind_cache = HashMap::new();
    dated_segments
        .into_iter()
        .map(|(_, segment)| {
            let deadline = segment.task.deadline_time;
            let deadline_label = format_deadline_remaining_time(
                deadline.as_ref(),
                segment.scheduled_end,
                last_synced_time,
            )
            .map_err(WebReadCoreError::Application)?;
            let (task_display_kind, deadline_display_kind) =
                classify_display_kinds(repository, segment, logical_date, &mut task_kind_cache)?;
            Ok(ScheduledTaskRowDto {
                task: session_task_dto(
                    segment.task.id.hyphenated().to_string(),
                    segment.task.name.clone(),
                    segment.task.estimated_work_seconds,
                    segment.task.actual_work_seconds,
                ),
                schedule_start_epoch_ms: segment.scheduled_start.timestamp_millis(),
                schedule_end_epoch_ms: segment.scheduled_end.timestamp_millis(),
                deadline_epoch_ms: deadline.map(|deadline| deadline.timestamp_millis()),
                deadline_label,
                misses_deadline: misses_deadline(deadline.as_ref(), segment.scheduled_end),
                task_display_kind,
                deadline_display_kind,
                is_leaf: segment.is_leaf(),
                defer_plan: defer_plan_dto(
                    plan_defer_task(repository, segment.task.id, logical_date)
                        .map_err(WebReadCoreError::Application)?,
                ),
            })
        })
        .collect()
}

fn classify_display_kinds(
    repository: &dyn TaskRepositoryTrait,
    segment: &ScheduledTaskView,
    logical_date: NaiveDate,
    task_kind_cache: &mut HashMap<Uuid, TaskDisplayKind>,
) -> Result<(TaskDisplayKind, DeadlineDisplayKind), WebReadCoreError> {
    let task_display_kind = if let Some(kind) = task_kind_cache.get(&segment.task.id) {
        *kind
    } else {
        let task = repository
            .get_by_id(segment.task.id)
            .map_err(|error| WebReadCoreError::Application(ApplicationError::TaskTree(error)))?
            .ok_or({
                WebReadCoreError::Application(ApplicationError::TaskNotFound(segment.task.id))
            })?;
        let kind = if task
            .fixed_start_applies_to_schedule()
            .map_err(|error| WebReadCoreError::Application(ApplicationError::TaskTree(error)))?
        {
            TaskDisplayKind::Fixed
        } else if task
            .get_inherited_repetition_interval_days_opt()
            .map_err(|error| WebReadCoreError::Application(ApplicationError::TaskTree(error)))?
            .is_some()
        {
            TaskDisplayKind::Repetitive
        } else {
            TaskDisplayKind::NonRepetitive
        };
        task_kind_cache.insert(segment.task.id, kind);
        kind
    };
    let deadline_display_kind = match segment.task.deadline_time {
        None => DeadlineDisplayKind::None,
        Some(deadline) if segment.scheduled_end > deadline => DeadlineDisplayKind::Overrun,
        Some(deadline) => {
            let next_logical_date = logical_date.succ_opt().ok_or({
                WebReadCoreError::Application(ApplicationError::LogicalDateStartOutOfRange {
                    date: logical_date,
                })
            })?;
            let next_logical_date_start =
                try_logical_date_start(next_logical_date).map_err(WebReadCoreError::Application)?;
            if deadline < next_logical_date_start {
                DeadlineDisplayKind::Today
            } else {
                DeadlineDisplayKind::Future
            }
        }
    };

    Ok((task_display_kind, deadline_display_kind))
}

fn defer_plan_dto(plan: DeferTaskPlan) -> DeferPlanDto {
    DeferPlanDto {
        mode: match plan.mode {
            DeferMode::Normal => DeferModeDto::Normal,
            DeferMode::DeadlineLimited => DeferModeDto::DeadlineLimited,
            DeferMode::RoutinePeriod => DeferModeDto::RoutinePeriod,
        },
        requested_pending_until_epoch_ms: plan.requested_pending_until.timestamp_millis(),
        effective_pending_until_epoch_ms: plan
            .effective_pending_until
            .map(|datetime| datetime.timestamp_millis()),
        repetition_interval_days: plan.repetition_interval_days,
    }
}

pub(in crate::adapter::controller) fn build_auto_session_dto(
    task_repository: &mut dyn TaskRepositoryTrait,
) -> Result<Option<SessionTaskDto>, ApplicationError> {
    get_focus(task_repository).map(|task| {
        task.map(|task| {
            session_task_dto(
                task.id.hyphenated().to_string(),
                task.name,
                task.estimated_work_seconds,
                task.actual_work_seconds,
            )
        })
    })
}

fn session_task_dto(
    task_id: String,
    task_name: String,
    estimated_work_seconds: i64,
    actual_work_seconds: i64,
) -> SessionTaskDto {
    SessionTaskDto {
        task_id,
        task_name,
        estimated_work_seconds,
        actual_work_seconds,
    }
}

pub(in crate::adapter::controller) fn calculate_buffer_seconds(
    current_logical_date: NaiveDate,
    remaining_capacity_seconds: i64,
    scheduled_segments: &[(NaiveDate, i64)],
) -> Result<i64, WebReadOverflowError> {
    let scheduled_seconds = scheduled_segments
        .iter()
        .filter(|(date, _)| *date == current_logical_date)
        .try_fold(0_i64, |total, (_, seconds)| {
            total
                .checked_add(*seconds)
                .ok_or_else(|| WebReadOverflowError::new("scheduled_seconds_sum", total, *seconds))
        })?;
    remaining_capacity_seconds
        .checked_sub(scheduled_seconds)
        .ok_or_else(|| {
            WebReadOverflowError::new(
                "buffer_subtraction",
                remaining_capacity_seconds,
                scheduled_seconds,
            )
        })
}
