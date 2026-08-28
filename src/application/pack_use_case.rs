use super::daily_capacity::{
    calculate_daily_leeway_seconds,
    calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes,
    try_subjective_date, try_subjective_date_end, try_subjective_date_start,
    END_OF_DAY_OFFSET_MINUTES,
};
use super::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use super::schedule_use_case::{
    get_schedule_with_metrics, get_schedule_with_task_first_available_time_and_metrics,
    ScheduledTaskView,
};
use super::scheduling_metrics::PackMetrics;
use super::task_use_case::ApplicationError;
use crate::entity::task::Status;
use chrono::{DateTime, Duration, Local, NaiveDate};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const PACK_TARGET_DAYS: i64 = 7;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedTask {
    pub task_id: Uuid,
    pub name: String,
    pub priority: i64,
    pub source_date: NaiveDate,
    pub target_date: NaiveDate,
    pub work_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkippedTask {
    pub task_id: Uuid,
    pub name: String,
    pub priority: i64,
    pub required_work_seconds: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackResult {
    pub packed_tasks: Vec<PackedTask>,
    pub skipped_tasks: Vec<SkippedTask>,
}

#[derive(Clone)]
struct PackCandidate {
    task_id: Uuid,
    name: String,
    priority: i64,
    planned_start: DateTime<Local>,
    work_seconds: i64,
}

#[derive(Clone, Copy)]
struct PackTargetDay {
    date: NaiveDate,
    end: DateTime<Local>,
}

#[derive(Clone, Copy)]
struct PlacementRequest {
    task_id: Uuid,
    work_seconds: i64,
    atomic: bool,
}

pub fn pack_tasks(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<PackResult, ApplicationError> {
    pack_tasks_with_end_of_day_offset_minutes(
        repository,
        free_time_manager,
        END_OF_DAY_OFFSET_MINUTES,
    )
}

pub fn pack_tasks_with_end_of_day_offset_minutes(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    end_of_day_offset_minutes: i64,
) -> Result<PackResult, ApplicationError> {
    pack_tasks_with_end_of_day_offset_minutes_and_metrics(
        repository,
        free_time_manager,
        end_of_day_offset_minutes,
        &mut PackMetrics::default(),
    )
}

#[cfg(feature = "benchmarking")]
pub(crate) fn pack_tasks_with_metrics(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    metrics: &mut PackMetrics,
) -> Result<PackResult, ApplicationError> {
    pack_tasks_with_end_of_day_offset_minutes_and_metrics(
        repository,
        free_time_manager,
        END_OF_DAY_OFFSET_MINUTES,
        metrics,
    )
}

fn pack_tasks_with_end_of_day_offset_minutes_and_metrics(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    end_of_day_offset_minutes: i64,
    metrics: &mut PackMetrics,
) -> Result<PackResult, ApplicationError> {
    let now = repository.get_last_synced_time();
    let first_date = try_subjective_date(now)?;
    let target_dates = (0..PACK_TARGET_DAYS)
        .map(|days| {
            first_date.checked_add_signed(Duration::days(days)).ok_or(
                ApplicationError::SubjectiveDateOutOfRange {
                    operation: "pack_target_dates",
                    datetime: now,
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut candidates = collect_candidates(repository, &target_dates, metrics)?;
    metrics.record_candidate_count(candidates.len());
    candidates.sort_by_key(|candidate| {
        (
            Reverse(candidate.priority),
            candidate.planned_start,
            candidate.task_id,
        )
    });

    let mut result = PackResult::default();
    for candidate in candidates {
        let mut packed_task_opt = None;
        let current_schedule = get_schedule_with_metrics(repository, &mut metrics.schedule)?;
        let current_planned_start_opt = current_schedule
            .iter()
            .find(|scheduled| scheduled.task.id == candidate.task_id)
            .map(|scheduled| scheduled.scheduled_start);
        let Some(current_planned_start) = current_planned_start_opt else {
            continue;
        };
        let daily_leeway = calculate_daily_leeway(
            repository,
            free_time_manager,
            &current_schedule,
            &target_dates,
            end_of_day_offset_minutes,
        )?;

        for target_date in &target_dates {
            if try_subjective_date(current_planned_start)? <= *target_date
                || daily_leeway.get(target_date).copied().unwrap_or(0) < candidate.work_seconds
            {
                continue;
            }

            let Some(task) = repository
                .get_by_id(candidate.task_id)
                .map_err(ApplicationError::TaskTree)?
            else {
                continue;
            };
            let target_datetime = try_subjective_date_start(*target_date)?
                .max(task.get_start_time().map_err(ApplicationError::TaskTree)?);
            if try_subjective_date(target_datetime)? != *target_date {
                continue;
            }

            let target_day = PackTargetDay {
                date: *target_date,
                end: try_subjective_date_end(*target_date, end_of_day_offset_minutes)?,
            };
            let placement_start_opt = find_placement_start(
                repository,
                free_time_manager,
                target_datetime,
                target_day,
                PlacementRequest {
                    task_id: candidate.task_id,
                    work_seconds: candidate.work_seconds,
                    atomic: task.get_atomic().map_err(ApplicationError::TaskTree)?,
                },
                metrics,
            )?;

            if let Some(placement_start) =
                placement_start_opt.filter(|start| *start < current_planned_start)
            {
                let source_date = try_subjective_date(current_planned_start)?;
                task.set_pending_until(placement_start)
                    .map_err(ApplicationError::TaskTree)?;
                packed_task_opt = Some(PackedTask {
                    task_id: candidate.task_id,
                    name: candidate.name.clone(),
                    priority: candidate.priority,
                    source_date,
                    target_date: *target_date,
                    work_seconds: candidate.work_seconds,
                });
                break;
            }
        }

        match packed_task_opt {
            Some(packed_task) => result.packed_tasks.push(packed_task),
            None => {
                result.skipped_tasks.push(SkippedTask {
                    task_id: candidate.task_id,
                    name: candidate.name,
                    priority: candidate.priority,
                    required_work_seconds: candidate.work_seconds,
                });
            }
        }
    }

    Ok(result)
}

fn find_placement_start(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    first_available_time: DateTime<Local>,
    target_day: PackTargetDay,
    request: PlacementRequest,
    metrics: &mut PackMetrics,
) -> Result<Option<DateTime<Local>>, ApplicationError> {
    let target_end = target_day.end;
    let mut trial_time = first_available_time.max(repository.get_last_synced_time());

    while trial_time + Duration::seconds(request.work_seconds) <= target_end {
        if request.atomic {
            let Some(next_free_time) = find_next_continuous_free_time(
                free_time_manager,
                trial_time,
                target_end,
                request.work_seconds,
                metrics,
            ) else {
                return Ok(None);
            };
            trial_time = next_free_time;
        }
        metrics.record_placement_trial();
        let schedule = get_schedule_with_task_first_available_time_and_metrics(
            repository,
            request.task_id,
            trial_time,
            &mut metrics.schedule,
        )?;
        let task_segments = schedule
            .iter()
            .filter(|scheduled| scheduled.task.id == request.task_id)
            .collect::<Vec<_>>();

        if placement_fits_target_day(
            &task_segments,
            target_day,
            request.work_seconds,
            request.atomic,
            free_time_manager,
        )? {
            return Ok(task_segments
                .first()
                .map(|scheduled| scheduled.scheduled_start));
        }

        if !request.atomic {
            return Ok(None);
        }
        let next_trial_time =
            task_segments
                .first()
                .map_or(trial_time + Duration::minutes(1), |scheduled| {
                    (trial_time + Duration::minutes(1))
                        .max(scheduled.scheduled_start + Duration::minutes(1))
                });
        metrics.record_cursor_minute_advance(
            (next_trial_time - trial_time).num_minutes().max(0) as usize
        );
        trial_time = next_trial_time;
    }

    Ok(None)
}

fn find_next_continuous_free_time(
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    mut cursor: DateTime<Local>,
    target_end: DateTime<Local>,
    work_seconds: i64,
    metrics: &mut PackMetrics,
) -> Option<DateTime<Local>> {
    let required_minutes = (work_seconds + 59) / 60;
    let check_duration = Duration::minutes(required_minutes);

    while cursor + check_duration <= target_end {
        if free_time_manager.get_free_minutes(&cursor, &(cursor + check_duration))
            >= required_minutes
        {
            return Some(cursor);
        }
        cursor += Duration::minutes(1);
        metrics.record_cursor_minute_advance(1);
    }

    None
}

fn collect_candidates(
    repository: &dyn TaskRepositoryTrait,
    target_dates: &[NaiveDate],
    metrics: &mut PackMetrics,
) -> Result<Vec<PackCandidate>, ApplicationError> {
    let schedule = get_schedule_with_metrics(repository, &mut metrics.schedule)?;
    let mut seen_ids = HashSet::new();
    let mut candidates = Vec::new();
    for scheduled in schedule {
        if !seen_ids.insert(scheduled.task.id) {
            continue;
        }
        if scheduled.rank != 0
            || scheduled.task.status != Status::Pending
            || scheduled.task.is_on_other_side
            || scheduled.total_work_seconds <= 0
        {
            continue;
        }
        let scheduled_date = try_subjective_date(scheduled.scheduled_start)?;
        let task_start_date = try_subjective_date(scheduled.task.start_time)?;
        if target_dates
            .iter()
            .any(|target_date| *target_date < scheduled_date && task_start_date <= *target_date)
        {
            candidates.push(PackCandidate {
                task_id: scheduled.task.id,
                name: scheduled.task.name,
                priority: scheduled.task.priority,
                planned_start: scheduled.scheduled_start,
                work_seconds: scheduled.total_work_seconds,
            });
        }
    }
    Ok(candidates)
}

fn calculate_daily_leeway(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    schedule: &[ScheduledTaskView],
    target_dates: &[NaiveDate],
    end_of_day_offset_minutes: i64,
) -> Result<HashMap<NaiveDate, i64>, ApplicationError> {
    let mut total_work_seconds = HashMap::<NaiveDate, i64>::new();
    let mut repetitive_work_seconds = HashMap::<NaiveDate, i64>::new();

    for scheduled in schedule {
        let date = try_subjective_date(scheduled.scheduled_start)?;
        if !target_dates.contains(&date) {
            continue;
        }
        *total_work_seconds.entry(date).or_default() += scheduled.scheduled_work_seconds;
        if repository
            .get_by_id(scheduled.task.id)
            .map_err(ApplicationError::TaskTree)?
            .map(|task| {
                task.get_inherited_repetition_interval_days_opt()
                    .map(|interval| interval.is_some())
            })
            .transpose()
            .map_err(ApplicationError::TaskTree)?
            .unwrap_or(false)
        {
            *repetitive_work_seconds.entry(date).or_default() += scheduled.scheduled_work_seconds;
        }
    }

    target_dates
        .iter()
        .map(|date| {
            let free_time_minutes =
                calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
                    date,
                    repository.get_last_synced_time(),
                    free_time_manager,
                    end_of_day_offset_minutes,
                )?;
            let repetitive = repetitive_work_seconds.get(date).copied().unwrap_or(0);
            let total = total_work_seconds.get(date).copied().unwrap_or(0);
            Ok((
                *date,
                calculate_daily_leeway_seconds(free_time_minutes, repetitive, total),
            ))
        })
        .collect()
}

fn placement_fits_target_day(
    task_segments: &[&ScheduledTaskView],
    target_day: PackTargetDay,
    work_seconds: i64,
    atomic: bool,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<bool, ApplicationError> {
    let target_end = target_day.end;
    let scheduled_dates = task_segments
        .iter()
        .map(|scheduled| try_subjective_date(scheduled.scheduled_start))
        .collect::<Result<Vec<_>, _>>()?;
    let fits_in_day = !task_segments.is_empty()
        && task_segments
            .iter()
            .zip(scheduled_dates)
            .all(|(scheduled, scheduled_date)| {
                scheduled_date == target_day.date && scheduled.scheduled_end <= target_end
            })
        && task_segments
            .iter()
            .map(|scheduled| scheduled.scheduled_work_seconds)
            .sum::<i64>()
            == work_seconds;

    if !fits_in_day || !atomic || task_segments.len() != 1 {
        return Ok(fits_in_day && !atomic);
    }

    let scheduled = task_segments[0];
    let required_minutes = (work_seconds + 59) / 60;
    let free_time_check_end = scheduled.scheduled_start + Duration::minutes(required_minutes);
    Ok(
        free_time_manager.get_free_minutes(&scheduled.scheduled_start, &free_time_check_end)
            >= required_minutes,
    )
}

#[cfg(test)]
#[path = "pack_use_case_tests.rs"]
mod tests;
