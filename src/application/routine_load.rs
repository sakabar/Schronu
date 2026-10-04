//! 今日から27日後までの繰返負荷をCLIとWebへ共通提供する。

use super::daily_capacity::try_logical_date;
use super::interface::TaskRepositoryTrait;
use super::schedule_use_case::ScheduledTaskView;
use super::task_use_case::ApplicationError;
use crate::entity::task::TaskHandle;
use chrono::{Days, NaiveDate};
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
}

struct RoutineAccumulator {
    metadata: RoutineMetadata,
    work_seconds_by_date: BTreeMap<NaiveDate, i64>,
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
                nearest_routine_metadata(&task)?
            };
            metadata_by_task_id.insert(cache_key, resolved.clone());
            resolved
        };
        let Some(metadata) = metadata else {
            continue;
        };
        let routine_task_id = metadata.routine_task_id;
        let accumulator =
            accumulators
                .entry(routine_task_id)
                .or_insert_with(|| RoutineAccumulator {
                    metadata,
                    work_seconds_by_date: BTreeMap::new(),
                });
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
) -> Result<Option<RoutineMetadata>, ApplicationError> {
    let mut current = task.parent().map_err(ApplicationError::TaskTree)?;
    while let Some(parent) = current {
        if let Some(repetition_interval_days) = parent
            .get_repetition_interval_days_opt()
            .map_err(ApplicationError::TaskTree)?
        {
            let project = parent.root().map_err(ApplicationError::TaskTree)?;
            return Ok(Some(RoutineMetadata {
                project_task_id: project.get_id().map_err(ApplicationError::TaskTree)?,
                project_name: project.get_name().map_err(ApplicationError::TaskTree)?,
                routine_task_id: parent.get_id().map_err(ApplicationError::TaskTree)?,
                routine_name: parent.get_name().map_err(ApplicationError::TaskTree)?,
                repetition_interval_days,
            }));
        }
        current = parent.parent().map_err(ApplicationError::TaskTree)?;
    }
    Ok(None)
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
    })
}
