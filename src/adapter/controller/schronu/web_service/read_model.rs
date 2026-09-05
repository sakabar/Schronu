use super::error::{WebReadCoreError, WebReadOverflowError};
use super::model::{ScheduledTaskRowDto, ServerSnapshot, SessionTaskDto};
use crate::application::daily_capacity::try_logical_date;
use crate::application::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use crate::application::schedule_use_case::{get_schedule, ScheduledTaskView};
use crate::application::task_use_case::{get_focus, ApplicationError};
use chrono::{DateTime, Local, NaiveDate};

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
    let current_logical_date = try_logical_date(task_repository.get_last_synced_time())
        .map_err(WebReadCoreError::Application)?;
    let free_seconds = if logical_date == current_logical_date {
        let end = crate::application::daily_capacity::try_logical_date_end(
            logical_date,
            end_of_day_offset_minutes,
        )
        .map_err(WebReadCoreError::Application)?;
        if task_repository.get_last_synced_time() < end {
            free_time_manager.get_free_seconds(&task_repository.get_last_synced_time(), &end)
        } else {
            0
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
    let scheduled_segments = schedule
        .iter()
        .map(|segment| {
            try_logical_date(segment.scheduled_start)
                .map(|date| (date, segment.scheduled_work_seconds))
                .map_err(WebReadCoreError::Application)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let buffer_seconds = calculate_buffer_seconds(logical_date, free_seconds, &scheduled_segments)?;

    Ok(ServerSnapshot {
        observed_at_epoch_ms: operation_now.timestamp_millis(),
        logical_date: logical_date.format("%Y-%m-%d").to_string(),
        buffer_seconds,
    })
}

pub(in crate::adapter::controller) fn build_scheduled_task_rows(
    schedule: &[ScheduledTaskView],
    logical_date: NaiveDate,
) -> Result<Vec<ScheduledTaskRowDto>, WebReadCoreError> {
    let mut dated_segments = schedule
        .iter()
        .map(|segment| {
            try_logical_date(segment.scheduled_start)
                .map(|date| (date, segment))
                .map_err(WebReadCoreError::Application)
        })
        .collect::<Result<Vec<_>, _>>()?;
    dated_segments.retain(|(date, _)| *date == logical_date);
    dated_segments.sort_by_key(|(_, segment)| segment.scheduled_start);

    Ok(dated_segments
        .into_iter()
        .map(|(_, segment)| ScheduledTaskRowDto {
            task: session_task_dto(
                segment.task.id.hyphenated().to_string(),
                segment.task.name.clone(),
                segment.task.estimated_work_seconds,
                segment.task.actual_work_seconds,
            ),
            schedule_start_epoch_ms: segment.scheduled_start.timestamp_millis(),
            schedule_end_epoch_ms: segment.scheduled_end.timestamp_millis(),
            deadline_epoch_ms: segment
                .task
                .deadline_time
                .map(|deadline| deadline.timestamp_millis()),
            is_leaf: segment.task.child_ids.is_empty(),
        })
        .collect())
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
    free_seconds: i64,
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
    free_seconds.checked_sub(scheduled_seconds).ok_or_else(|| {
        WebReadOverflowError::new("buffer_subtraction", free_seconds, scheduled_seconds)
    })
}
