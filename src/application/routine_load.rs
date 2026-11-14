//! 今日から27日後までの繰返負荷をCLIとWebへ共通提供する。

use super::daily_capacity::{try_logical_date, try_next_logical_date_start};
use super::interface::TaskRepositoryTrait;
use super::schedule_use_case::{ScheduleOccurrenceKey, ScheduledTaskView};
use super::task_use_case::ApplicationError;
use crate::entity::task::{Status, TaskHandle};
use chrono::{Days, NaiveDate, TimeDelta};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

pub const ROUTINE_LOAD_HORIZON_DAYS: u64 = 28;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutineLoadReport {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub horizon_day_count: u64,
    pub rows: Vec<RoutineLoadRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutineLoadRow {
    pub project_task_id: Uuid,
    pub project_name: String,
    pub routine_task_id: Uuid,
    pub routine_name: String,
    pub repetition_interval_days: i64,
    pub total_work_seconds: i64,
    pub occurrence_day_count: usize,
    pub average_work_seconds: i64,
    pub peak_date: NaiveDate,
    pub peak_work_seconds: i64,
    pub work_seconds_by_date: BTreeMap<NaiveDate, i64>,
    pub estimate_difference_seconds_by_date: BTreeMap<NaiveDate, i64>,
}

impl RoutineLoadRow {
    pub fn display_average_work_seconds(&self) -> Option<i64> {
        (self.occurrence_day_count > 0).then_some(self.average_work_seconds)
    }
}

#[derive(Clone)]
struct RoutineMetadata {
    project_task_id: Uuid,
    project_name: String,
    routine_task_id: Uuid,
    routine_name: String,
    repetition_interval_days: i64,
    occurrence_estimate_difference: Option<(Uuid, i64)>,
}

struct RoutineAccumulator {
    metadata: RoutineMetadata,
    work_seconds_by_date: BTreeMap<NaiveDate, i64>,
    occurrence_estimate_difference_by_date: BTreeMap<NaiveDate, Option<(Uuid, i64)>>,
}

pub fn build_routine_load_report(
    repository: &dyn TaskRepositoryTrait,
    schedule: &[ScheduledTaskView],
    start_date: NaiveDate,
) -> Result<RoutineLoadReport, ApplicationError> {
    let end_date = start_date
        .checked_add_days(Days::new(ROUTINE_LOAD_HORIZON_DAYS - 1))
        .ok_or(ApplicationError::InvalidInput {
            field: "start_date",
            reason: "routine load horizon is out of range",
        })?;
    let mut metadata_by_task_id = HashMap::<(Uuid, bool), Option<RoutineMetadata>>::new();
    let mut accumulators = HashMap::<Uuid, RoutineAccumulator>::new();
    let mut comparison_work_by_occurrence_id = HashMap::<Uuid, i64>::new();

    for segment in schedule {
        let logical_date = try_logical_date(segment.scheduled_start)?;
        if logical_date < start_date || logical_date > end_date {
            continue;
        }
        let task_id = segment.source_task_id();
        let cache_key = (task_id, segment.is_projected());
        let metadata = if let Some(cached) = metadata_by_task_id.get(&cache_key) {
            cached.clone()
        } else {
            let task = repository
                .get_by_id(task_id)
                .map_err(ApplicationError::TaskTree)?
                .ok_or(ApplicationError::TaskNotFound(task_id))?;
            let resolved = if segment.is_projected() {
                projected_routine_metadata(&task)?
            } else {
                nearest_routine_metadata(&task, &mut comparison_work_by_occurrence_id)?
            };
            metadata_by_task_id.insert(cache_key, resolved.clone());
            resolved
        };
        let Some(metadata) = metadata else {
            continue;
        };
        let routine_task_id = metadata.routine_task_id;
        let occurrence_estimate_difference = metadata.occurrence_estimate_difference;
        let accumulator =
            accumulators
                .entry(routine_task_id)
                .or_insert_with(|| RoutineAccumulator {
                    metadata,
                    work_seconds_by_date: BTreeMap::new(),
                    occurrence_estimate_difference_by_date: BTreeMap::new(),
                });
        accumulator
            .occurrence_estimate_difference_by_date
            .entry(logical_date)
            .and_modify(|comparison| {
                if *comparison != occurrence_estimate_difference {
                    *comparison = None;
                }
            })
            .or_insert(occurrence_estimate_difference);
        let daily_seconds = accumulator
            .work_seconds_by_date
            .entry(logical_date)
            .or_default();
        *daily_seconds = daily_seconds
            .checked_add(segment.scheduled_work_seconds)
            .ok_or(ApplicationError::RoutineLoadCalculationOverflow { routine_task_id })?;
    }

    let mut rows = accumulators
        .into_values()
        .map(into_report_row)
        .collect::<Result<Vec<_>, _>>()?;
    rows.sort_by(|left, right| {
        right
            .total_work_seconds
            .cmp(&left.total_work_seconds)
            .then_with(|| left.project_name.cmp(&right.project_name))
            .then_with(|| left.routine_name.cmp(&right.routine_name))
            .then_with(|| left.routine_task_id.cmp(&right.routine_task_id))
    });

    Ok(RoutineLoadReport {
        start_date,
        end_date,
        horizon_day_count: ROUTINE_LOAD_HORIZON_DAYS,
        rows,
    })
}

fn nearest_routine_metadata(
    task: &TaskHandle,
    comparison_work_by_occurrence_id: &mut HashMap<Uuid, i64>,
) -> Result<Option<RoutineMetadata>, ApplicationError> {
    let Some((parent, occurrence, repetition_interval_days)) = nearest_routine(task)? else {
        return Ok(None);
    };
    let project = parent.root().map_err(ApplicationError::TaskTree)?;
    let routine_task_id = parent.get_id().map_err(ApplicationError::TaskTree)?;
    let occurrence_id = occurrence.get_id().map_err(ApplicationError::TaskTree)?;
    let occurrence_work_seconds =
        if let Some(seconds) = comparison_work_by_occurrence_id.get(&occurrence_id) {
            *seconds
        } else {
            let seconds = occurrence_comparison_work_seconds(&occurrence, routine_task_id)?;
            comparison_work_by_occurrence_id.insert(occurrence_id, seconds);
            seconds
        };
    let estimate_difference = occurrence_work_seconds
        .checked_sub(
            parent
                .get_estimated_work_seconds()
                .map_err(ApplicationError::TaskTree)?,
        )
        .ok_or(ApplicationError::RoutineLoadCalculationOverflow { routine_task_id })?;
    Ok(Some(RoutineMetadata {
        project_task_id: project.get_id().map_err(ApplicationError::TaskTree)?,
        project_name: project.get_name().map_err(ApplicationError::TaskTree)?,
        routine_task_id,
        routine_name: parent.get_name().map_err(ApplicationError::TaskTree)?,
        repetition_interval_days,
        occurrence_estimate_difference: Some((occurrence_id, estimate_difference)),
    }))
}

fn nearest_routine(
    task: &TaskHandle,
) -> Result<Option<(TaskHandle, TaskHandle, i64)>, ApplicationError> {
    let mut occurrence = task.clone();
    let mut current = task.parent().map_err(ApplicationError::TaskTree)?;
    while let Some(parent) = current {
        if let Some(interval) = parent
            .get_repetition_interval_days_opt()
            .map_err(ApplicationError::TaskTree)?
        {
            return Ok(Some((parent, occurrence, interval)));
        }
        occurrence = parent.clone();
        current = parent.parent().map_err(ApplicationError::TaskTree)?;
    }
    Ok(None)
}

fn occurrence_comparison_work_seconds(
    occurrence: &TaskHandle,
    routine_task_id: Uuid,
) -> Result<i64, ApplicationError> {
    let mut total = occurrence
        .get_estimated_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let mut pending = occurrence
        .get_children()
        .map_err(ApplicationError::TaskTree)?;
    while let Some(task) = pending.pop() {
        if task
            .is_repeating_task()
            .map_err(ApplicationError::TaskTree)?
        {
            continue;
        }
        let seconds = if task.get_status().map_err(ApplicationError::TaskTree)? == Status::Done {
            task.get_actual_work_seconds()
                .map_err(ApplicationError::TaskTree)?
        } else {
            task.get_estimated_work_seconds()
                .map_err(ApplicationError::TaskTree)?
        };
        total = total
            .checked_add(seconds)
            .ok_or(ApplicationError::RoutineLoadCalculationOverflow { routine_task_id })?;
        pending.extend(task.get_children().map_err(ApplicationError::TaskTree)?);
    }
    Ok(total)
}

fn projected_routine_metadata(
    task: &TaskHandle,
) -> Result<Option<RoutineMetadata>, ApplicationError> {
    let Some(repetition_interval_days) = task
        .get_repetition_interval_days_opt()
        .map_err(ApplicationError::TaskTree)?
    else {
        return Ok(None);
    };
    let project = task.root().map_err(ApplicationError::TaskTree)?;
    Ok(Some(RoutineMetadata {
        project_task_id: project.get_id().map_err(ApplicationError::TaskTree)?,
        project_name: project.get_name().map_err(ApplicationError::TaskTree)?,
        routine_task_id: task.get_id().map_err(ApplicationError::TaskTree)?,
        routine_name: task.get_name().map_err(ApplicationError::TaskTree)?,
        repetition_interval_days,
        occurrence_estimate_difference: None,
    }))
}

fn into_report_row(accumulator: RoutineAccumulator) -> Result<RoutineLoadRow, ApplicationError> {
    let routine_task_id = accumulator.metadata.routine_task_id;
    let total_work_seconds =
        accumulator
            .work_seconds_by_date
            .values()
            .try_fold(0_i64, |sum, seconds| {
                sum.checked_add(*seconds)
                    .ok_or(ApplicationError::RoutineLoadCalculationOverflow { routine_task_id })
            })?;
    let occurrence_day_count = accumulator.work_seconds_by_date.len();
    let occurrence_day_count_i64 = i64::try_from(occurrence_day_count)
        .map_err(|_| ApplicationError::RoutineLoadCalculationOverflow { routine_task_id })?;
    let average_work_seconds = total_work_seconds
        .checked_div(occurrence_day_count_i64)
        .ok_or(ApplicationError::RoutineLoadCalculationOverflow { routine_task_id })?;
    let (peak_date, peak_work_seconds) = accumulator
        .work_seconds_by_date
        .iter()
        .fold(None, |peak, (date, seconds)| match peak {
            Some((_, peak_seconds)) if peak_seconds >= *seconds => peak,
            _ => Some((*date, *seconds)),
        })
        .expect("routine accumulator always has at least one date");
    Ok(RoutineLoadRow {
        project_task_id: accumulator.metadata.project_task_id,
        project_name: accumulator.metadata.project_name,
        routine_task_id,
        routine_name: accumulator.metadata.routine_name,
        repetition_interval_days: accumulator.metadata.repetition_interval_days,
        total_work_seconds,
        occurrence_day_count,
        average_work_seconds,
        peak_date,
        peak_work_seconds,
        work_seconds_by_date: accumulator.work_seconds_by_date,
        estimate_difference_seconds_by_date: accumulator
            .occurrence_estimate_difference_by_date
            .into_iter()
            .filter_map(|(date, comparison)| comparison.map(|(_, difference)| (date, difference)))
            .collect(),
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RoutineLoadForecast {
    pub mean_remaining_seconds: f64,
    pub variance_seconds_squared: f64,
    pub estimated_occurrence_count: usize,
}

struct OccurrenceForecast {
    routine_id: Uuid,
    distribution: Option<(f64, f64)>,
    seconds_by_date: BTreeMap<NaiveDate, i64>,
}

pub(crate) fn build_routine_load_forecasts(
    repository: &dyn TaskRepositoryTrait,
    schedule: &[ScheduledTaskView],
    report: &RoutineLoadReport,
) -> Result<BTreeMap<NaiveDate, RoutineLoadForecast>, ApplicationError> {
    let mut occurrences = BTreeMap::new();
    let mut occurrence_keys = HashMap::new();
    for segment in schedule {
        if segment.scheduled_work_seconds <= 0
            || (!segment.is_projected() && segment.task.status == Status::Done)
        {
            continue;
        }
        let key = if let Some(&key) = occurrence_keys.get(&segment.occurrence) {
            key
        } else {
            let key = resolve_forecast_occurrence(repository, segment, &mut occurrences)?;
            occurrence_keys.insert(segment.occurrence, key);
            key
        };
        let Some(key) = key else { continue };
        let entry = occurrences
            .get_mut(&key)
            .expect("forecast occurrence was resolved");
        let overflow = || ApplicationError::RoutineLoadCalculationOverflow {
            routine_task_id: entry.routine_id,
        };
        let mut start = segment.scheduled_start;
        let end = start
            .checked_add_signed(
                TimeDelta::try_seconds(segment.scheduled_work_seconds).ok_or_else(overflow)?,
            )
            .ok_or_else(overflow)?;
        while start < end {
            let date = try_logical_date(start)?;
            let boundary = try_next_logical_date_start(start)?.min(end);
            // Preserve the segment's total whole seconds even if it starts between seconds.
            let seconds = (boundary - segment.scheduled_start).num_seconds()
                - (start - segment.scheduled_start).num_seconds();
            add_forecast_work(&mut entry.seconds_by_date, date, seconds, entry.routine_id)?;
            start = boundary;
        }
    }

    let mut by_date: BTreeMap<_, RoutineLoadForecast> = (0..report.horizon_day_count)
        .filter_map(|offset| report.start_date.checked_add_days(Days::new(offset)))
        .map(|date| (date, RoutineLoadForecast::default()))
        .collect();
    for occurrence in occurrences.into_values() {
        let total: f64 = occurrence
            .seconds_by_date
            .values()
            .map(|&seconds| seconds as f64)
            .sum();
        for (date, seconds) in occurrence.seconds_by_date {
            let Some(day) = by_date.get_mut(&date) else {
                continue;
            };
            match occurrence.distribution {
                Some((mean, variance)) => {
                    let fraction = seconds as f64 / total;
                    day.mean_remaining_seconds += mean * fraction;
                    day.variance_seconds_squared += variance * fraction.powi(2);
                }
                None => {
                    day.mean_remaining_seconds += seconds as f64;
                    day.estimated_occurrence_count += 1;
                }
            }
            if !day.mean_remaining_seconds.is_finite() || !day.variance_seconds_squared.is_finite()
            {
                return Err(ApplicationError::RoutineLoadCalculationOverflow {
                    routine_task_id: occurrence.routine_id,
                });
            }
        }
    }
    Ok(by_date)
}

fn resolve_forecast_occurrence(
    repository: &dyn TaskRepositoryTrait,
    segment: &ScheduledTaskView,
    occurrences: &mut BTreeMap<ScheduleOccurrenceKey, OccurrenceForecast>,
) -> Result<Option<ScheduleOccurrenceKey>, ApplicationError> {
    let source_id = segment.source_task_id();
    let key = segment.occurrence;
    let task = repository
        .get_by_id(source_id)
        .map_err(ApplicationError::TaskTree)?
        .ok_or(ApplicationError::TaskNotFound(source_id))?;
    let (routine, actual, key) = match key {
        ScheduleOccurrenceKey::Actual { .. } => {
            let Some((routine, occurrence, _)) = nearest_routine(&task)? else {
                return Ok(None);
            };
            if occurrence
                .get_status()
                .map_err(ApplicationError::TaskTree)?
                == Status::Done
            {
                return Ok(None);
            }
            let task_id = occurrence.get_id().map_err(ApplicationError::TaskTree)?;
            (
                routine,
                Some(occurrence),
                ScheduleOccurrenceKey::Actual { task_id },
            )
        }
        ScheduleOccurrenceKey::Projected { .. } => (task, None, key),
    };
    let routine_id = routine.get_id().map_err(ApplicationError::TaskTree)?;
    if let std::collections::btree_map::Entry::Vacant(entry) = occurrences.entry(key) {
        let (count, mean, m2) = routine
            .duration_statistics()
            .map_err(ApplicationError::TaskTree)?
            .values();
        let distribution = if count >= 2 {
            let recorded = actual
                .as_ref()
                .map(recorded_occurrence_seconds)
                .transpose()?
                .unwrap_or_default();
            Some(((mean - recorded).max(0.0), m2 / (count - 1) as f64))
        } else {
            None
        };
        entry.insert(OccurrenceForecast {
            routine_id,
            distribution,
            seconds_by_date: BTreeMap::new(),
        });
    }
    Ok(Some(key))
}

fn add_forecast_work(
    dates: &mut BTreeMap<NaiveDate, i64>,
    date: NaiveDate,
    seconds: i64,
    routine_task_id: Uuid,
) -> Result<(), ApplicationError> {
    if seconds > 0 {
        let total = dates.entry(date).or_default();
        *total = total
            .checked_add(seconds)
            .ok_or(ApplicationError::RoutineLoadCalculationOverflow { routine_task_id })?;
    }
    Ok(())
}

fn recorded_occurrence_seconds(occurrence: &TaskHandle) -> Result<f64, ApplicationError> {
    let mut total = 0.0;
    let mut pending = vec![occurrence.clone()];
    while let Some(task) = pending.pop() {
        if task
            .is_repeating_task()
            .map_err(ApplicationError::TaskTree)?
        {
            continue;
        }
        let actual = task
            .get_actual_work_seconds()
            .map_err(ApplicationError::TaskTree)?;
        let seconds = if actual == 0
            && task.get_status().map_err(ApplicationError::TaskTree)? == Status::Done
        {
            task.get_estimated_work_seconds()
                .map_err(ApplicationError::TaskTree)?
        } else {
            actual
        };
        total += seconds as f64;
        pending.extend(task.get_children().map_err(ApplicationError::TaskTree)?);
    }
    Ok(total)
}
