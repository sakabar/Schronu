use super::error::{WebReadCoreError, WebReadOverflowError};
use super::model::{
    AllTaskRowDto, BandDayDto, BandDurationsDto, CompletedTaskRowDto, DeadlineDisplayKind,
    DeferModeDto, DeferPlanDto, RoutineLoadReportDto, RoutineLoadRowDto, ScheduleOccurrenceDto,
    ScheduledTaskDto, ScheduledTaskRowDto, ServerSnapshot, SessionTaskDto, TaskDisplayKind,
};
use crate::adapter::controller::deadline_display::{
    classify_deadline_display, format_deadline_remaining_time, misses_deadline,
    DeadlineDisplayStatus,
};
use crate::application::completed_task_report::CompletedTaskReportRow;
use crate::application::daily_capacity::{
    calculate_daily_band_durations, calculate_daily_rho_diff_hours,
    calculate_free_time_minutes_for_logical_date_with_end_of_day_offset_minutes,
    calculate_full_day_free_time_minutes_for_logical_date_with_end_of_day_offset_minutes,
    try_logical_date, DailyLoadAccumulator, DailyLoadDayInput,
};
use crate::application::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use crate::application::routine_load::build_routine_load_report;
use crate::application::schedule_use_case::{
    format_scheduled_task_display_name, get_schedule, scheduled_logical_dates, ScheduledTaskView,
};
use crate::application::task_use_case::{
    get_focus, plan_defer_task, ApplicationError, DeferMode, DeferTaskPlan,
};
use chrono::{DateTime, Days, Local, NaiveDate};
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

pub(in crate::adapter::controller) fn build_routine_load_report_dto(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    schedule: &[ScheduledTaskView],
    operation_now: DateTime<Local>,
    end_of_day_offset_minutes: i64,
) -> Result<RoutineLoadReportDto, WebReadCoreError> {
    let start_date = try_logical_date(operation_now).map_err(WebReadCoreError::Application)?;
    let report = build_routine_load_report(repository, schedule, start_date)
        .map_err(WebReadCoreError::Application)?;
    let full_day_available_seconds_by_date = (0..report.horizon_day_count)
        .map(|offset| {
            let date = report.start_date.checked_add_days(Days::new(offset)).ok_or(
                WebReadCoreError::Application(ApplicationError::InvalidInput {
                    field: "start_date",
                    reason: "routine load horizon is out of range",
                }),
            )?;
            let minutes =
                calculate_full_day_free_time_minutes_for_logical_date_with_end_of_day_offset_minutes(
                    &date,
                    free_time_manager,
                    end_of_day_offset_minutes,
                )
                .map_err(WebReadCoreError::Application)?;
            let seconds = minutes.checked_mul(60).ok_or_else(|| {
                WebReadOverflowError::new("routine_load_available_seconds", minutes, 60)
            })?;
            Ok((date, seconds))
        })
        .collect::<Result<BTreeMap<_, _>, WebReadCoreError>>()?;
    Ok(RoutineLoadReportDto {
        start_date: report.start_date,
        end_date: report.end_date,
        horizon_day_count: report.horizon_day_count,
        full_day_available_seconds_by_date,
        rows: report
            .rows
            .into_iter()
            .map(|row| RoutineLoadRowDto {
                project_task_id: row.project_task_id.hyphenated().to_string(),
                project_name: row.project_name,
                routine_task_id: row.routine_task_id.hyphenated().to_string(),
                routine_name: row.routine_name,
                repetition_interval_days: row.repetition_interval_days,
                total_work_seconds: row.total_work_seconds,
                occurrence_day_count: row.occurrence_day_count,
                average_work_seconds: row.average_work_seconds,
                peak_date: row.peak_date,
                peak_work_seconds: row.peak_work_seconds,
                work_seconds_by_date: row.work_seconds_by_date,
            })
            .collect(),
    })
}

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
                task: scheduled_task_dto(
                    segment
                        .actual_task_id()
                        .map(|id| id.hyphenated().to_string()),
                    segment.task.name.clone(),
                    segment.task.estimated_work_seconds,
                    segment.task.actual_work_seconds,
                ),
                display_task_name: format_scheduled_task_display_name(
                    &segment.task.name,
                    segment.scheduled_work_seconds,
                    segment.total_work_seconds,
                ),
                occurrence: occurrence_dto(segment),
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

pub(in crate::adapter::controller) fn build_band_days<R, F>(
    repository: &R,
    free_time_manager: &mut F,
    schedule: &[ScheduledTaskView],
    operation_now: DateTime<Local>,
    end_of_day_offset_minutes: i64,
) -> Result<Vec<BandDayDto>, WebReadCoreError>
where
    R: TaskRepositoryTrait,
    F: FreeTimeManagerTrait,
{
    const HORIZON_DAYS: i64 = 7;
    let today = try_logical_date(operation_now).map_err(WebReadCoreError::Application)?;
    let last_synced_time = repository.get_last_synced_time();
    let logical_dates = scheduled_logical_dates(schedule).map_err(WebReadCoreError::Application)?;
    let mut totals = HashMap::<NaiveDate, i64>::new();
    let mut repetitive = HashMap::<NaiveDate, i64>::new();
    let mut adjustable = HashMap::<NaiveDate, i64>::new();
    let mut adjustable_occurrences = HashSet::new();

    for (date, segment) in logical_dates.into_iter().zip(schedule) {
        if date < today || date >= today + chrono::Duration::days(HORIZON_DAYS) {
            continue;
        }
        *totals.entry(date).or_default() += segment.scheduled_work_seconds;
        if segment.is_projected() {
            *repetitive.entry(date).or_default() += segment.scheduled_work_seconds;
        } else if let Some(task_id) = segment.actual_task_id() {
            let task = repository
                .get_by_id(task_id)
                .map_err(|error| WebReadCoreError::Application(ApplicationError::TaskTree(error)))?
                .ok_or(WebReadCoreError::Application(
                    ApplicationError::TaskNotFound(task_id),
                ))?;
            if task
                .get_inherited_repetition_interval_days_opt()
                .map_err(|error| WebReadCoreError::Application(ApplicationError::TaskTree(error)))?
                .is_some()
            {
                *repetitive.entry(date).or_default() += segment.scheduled_work_seconds;
            }
        }

        let available_date = try_logical_date(segment.first_available_time)
            .map_err(WebReadCoreError::Application)?;
        if segment.is_leaf()
            && !segment.task.is_on_other_side
            && date > available_date
            && adjustable_occurrences.insert((date, segment.occurrence))
        {
            *adjustable.entry(date).or_default() += segment.task.estimated_work_seconds;
        }
    }

    let mut accumulator = DailyLoadAccumulator::default();
    (0..HORIZON_DAYS)
        .map(|offset| {
            let date = today + chrono::Duration::days(offset);
            let total_work_seconds = *totals.get(&date).unwrap_or(&0);
            let repetitive_work_seconds = *repetitive.get(&date).unwrap_or(&0);
            let free_time_minutes =
                calculate_free_time_minutes_for_logical_date_with_end_of_day_offset_minutes(
                    &date,
                    last_synced_time,
                    free_time_manager,
                    end_of_day_offset_minutes,
                )
                .map_err(WebReadCoreError::Application)?;
            let full_day_free_minutes =
                calculate_full_day_free_time_minutes_for_logical_date_with_end_of_day_offset_minutes(
                    &date,
                    free_time_manager,
                    end_of_day_offset_minutes,
                )
                .map_err(WebReadCoreError::Application)?;
            let diff_to_goal = calculate_daily_rho_diff_hours(
                free_time_minutes,
                repetitive_work_seconds,
                total_work_seconds,
            );
            let cumulative = if totals.contains_key(&date) {
                accumulator.advance(DailyLoadDayInput {
                    free_time_minutes,
                    total_work_seconds,
                    repetitive_work_seconds,
                    adjustable_work_seconds: *adjustable.get(&date).unwrap_or(&0),
                })
            } else {
                accumulator.current()
            };
            let durations = calculate_daily_band_durations(
                date == today,
                full_day_free_minutes,
                free_time_minutes,
                total_work_seconds,
                repetitive_work_seconds,
                diff_to_goal,
            );
            Ok(BandDayDto {
                logical_date: date,
                accumulated_rho_diff_seconds: cumulative.accumulated_rho_diff_seconds,
                accumulated_free_diff_seconds: cumulative.accumulated_free_diff_seconds,
                durations: BandDurationsDto {
                    unavailable_seconds: durations.unavailable_seconds,
                    elapsed_seconds: durations.elapsed_seconds,
                    repetitive_seconds: durations.repetitive_seconds,
                    non_repetitive_seconds: durations.non_repetitive_seconds,
                    rho_leeway_seconds: durations.rho_leeway_seconds,
                },
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
                task: scheduled_task_dto(
                    segment
                        .actual_task_id()
                        .map(|id| id.hyphenated().to_string()),
                    segment.task.name.clone(),
                    segment.task.estimated_work_seconds,
                    segment.task.actual_work_seconds,
                ),
                display_task_name: format_scheduled_task_display_name(
                    &segment.task.name,
                    segment.scheduled_work_seconds,
                    segment.total_work_seconds,
                ),
                occurrence: occurrence_dto(segment),
                schedule_start_epoch_ms: segment.scheduled_start.timestamp_millis(),
                schedule_end_epoch_ms: segment.scheduled_end.timestamp_millis(),
                deadline_epoch_ms: deadline.map(|deadline| deadline.timestamp_millis()),
                deadline_label,
                misses_deadline: misses_deadline(deadline.as_ref(), segment.scheduled_end),
                task_display_kind,
                deadline_display_kind,
                is_leaf: segment.is_leaf(),
                defer_plan: segment
                    .actual_task_id()
                    .map(|task_id| {
                        plan_defer_task(repository, task_id, logical_date)
                            .map(defer_plan_dto)
                            .map_err(WebReadCoreError::Application)
                    })
                    .transpose()?,
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
    let task_display_kind = match segment.actual_task_id() {
        Some(task_id) => classify_task_display_kind(repository, task_id, task_kind_cache)?,
        None => TaskDisplayKind::Repetitive,
    };
    Ok((
        task_display_kind,
        classify_deadline_kind(segment, logical_date)?,
    ))
}

fn classify_deadline_kind(
    segment: &ScheduledTaskView,
    logical_date: NaiveDate,
) -> Result<DeadlineDisplayKind, WebReadCoreError> {
    Ok(
        match classify_deadline_display(
            segment.task.deadline_time.as_ref(),
            segment.scheduled_end,
            logical_date,
        )
        .map_err(WebReadCoreError::Application)?
        {
            DeadlineDisplayStatus::None => DeadlineDisplayKind::None,
            DeadlineDisplayStatus::Overrun => DeadlineDisplayKind::Overrun,
            DeadlineDisplayStatus::DueWithinLogicalDate => DeadlineDisplayKind::Today,
            DeadlineDisplayStatus::Future => DeadlineDisplayKind::Future,
        },
    )
}

pub(in crate::adapter::controller) fn classify_task_display_kind(
    repository: &dyn TaskRepositoryTrait,
    task_id: Uuid,
    task_kind_cache: &mut HashMap<Uuid, TaskDisplayKind>,
) -> Result<TaskDisplayKind, WebReadCoreError> {
    let task_display_kind = if let Some(kind) = task_kind_cache.get(&task_id) {
        *kind
    } else {
        let task = repository
            .get_by_id(task_id)
            .map_err(|error| WebReadCoreError::Application(ApplicationError::TaskTree(error)))?
            .ok_or(WebReadCoreError::Application(
                ApplicationError::TaskNotFound(task_id),
            ))?;
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
        task_kind_cache.insert(task_id, kind);
        kind
    };
    Ok(task_display_kind)
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

fn scheduled_task_dto(
    task_id: Option<String>,
    task_name: String,
    estimated_work_seconds: i64,
    actual_work_seconds: i64,
) -> ScheduledTaskDto {
    ScheduledTaskDto {
        task_id,
        task_name,
        estimated_work_seconds,
        actual_work_seconds,
    }
}

fn occurrence_dto(segment: &ScheduledTaskView) -> ScheduleOccurrenceDto {
    match segment.occurrence {
        crate::application::schedule_use_case::ScheduleOccurrenceKey::Actual { task_id } => {
            ScheduleOccurrenceDto::Actual {
                task_id: task_id.hyphenated().to_string(),
            }
        }
        crate::application::schedule_use_case::ScheduleOccurrenceKey::Projected {
            source_task_id,
            deadline,
        } => ScheduleOccurrenceDto::Projected {
            occurrence_key: format!(
                "{}:{}",
                source_task_id.hyphenated(),
                deadline.timestamp_millis()
            ),
            source_task_id: source_task_id.hyphenated().to_string(),
        },
    }
}

pub(in crate::adapter::controller) fn completed_task_row_dto(
    row: CompletedTaskReportRow,
    task_display_kind: TaskDisplayKind,
) -> CompletedTaskRowDto {
    CompletedTaskRowDto {
        task_id: row.task_id.hyphenated().to_string(),
        task_name: row.task_name,
        project_name: row.project_name,
        completed_at_epoch_ms: row.completed_at.timestamp_millis(),
        actual_work_seconds: row.actual_work_seconds,
        estimated_work_seconds: row.estimated_work_seconds,
        task_display_kind,
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

#[cfg(test)]
mod completed_task_report_tests {
    use super::{completed_task_row_dto, TaskDisplayKind};
    use crate::application::completed_task_report::CompletedTaskReportRow;
    use chrono::{Local, TimeZone};
    use uuid::Uuid;

    #[test]
    fn application_rowの全fieldをdtoへ保持する() {
        let completed_at = Local
            .with_ymd_and_hms(2026, 9, 5, 8, 7, 6)
            .single()
            .unwrap();
        let row = CompletedTaskReportRow {
            task_id: Uuid::from_u128(1),
            task_name: "task".to_owned(),
            project_name: "project".to_owned(),
            completed_at,
            actual_work_seconds: 3_661,
            estimated_work_seconds: 3_600,
        };

        let dto = completed_task_row_dto(row, TaskDisplayKind::Fixed);

        assert_eq!(dto.task_id, Uuid::from_u128(1).hyphenated().to_string());
        assert_eq!(dto.task_name, "task");
        assert_eq!(dto.project_name, "project");
        assert_eq!(dto.completed_at_epoch_ms, completed_at.timestamp_millis());
        assert_eq!(dto.actual_work_seconds, 3_661);
        assert_eq!(dto.estimated_work_seconds, 3_600);
        assert_eq!(dto.task_display_kind, TaskDisplayKind::Fixed);
    }
}
