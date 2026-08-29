use super::daily_capacity::{
    calculate_free_time_minutes_for_logical_date_with_end_of_day_offset_minutes, try_logical_date,
    try_logical_date_start, try_next_logical_date_start, END_OF_DAY_OFFSET_MINUTES,
};
use super::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use super::schedule_use_case::{
    build_schedule_context_with_metrics, get_schedule_from_context_with_overrides_and_metrics,
    ScheduledTaskView,
};
use super::scheduling_metrics::FlattenMetrics;
use super::task_use_case::ApplicationError;
use crate::entity::datetime::LogicalDateTimePolicy;
use crate::entity::task::Status;
use chrono::{DateTime, Duration, Local, NaiveDate};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const FLATTEN_TARGET_DAYS: i64 = 28;
const FLATTEN_OVERFLOW_DAY: i64 = 35;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlattenedTask {
    pub task_id: Uuid,
    pub name: String,
    pub priority: i64,
    pub source_date: NaiveDate,
    pub target_date: NaiveDate,
    pub work_seconds: i64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum UnresolvedReason {
    OnOtherSide,
    CrossesLogicalDate,
    ExceedsDailyCapacity,
    OwnDeadline,
    RelatedDeadline,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedReasonSummary {
    pub reason: UnresolvedReason,
    pub task_count: usize,
    pub representative_task_id: Option<Uuid>,
    pub representative_task_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedOverload {
    pub date: NaiveDate,
    pub excess_work_seconds: i64,
    pub reasons: Vec<UnresolvedReasonSummary>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FlattenResult {
    pub flattened_tasks: Vec<FlattenedTask>,
    pub overflowed_task_count: usize,
    pub overflowed_work_seconds: i64,
    pub had_overload: bool,
    pub unresolved_overloads: Vec<UnresolvedOverload>,
}

#[derive(Clone)]
struct FlattenCandidate {
    task_id: Uuid,
    name: String,
    priority: i64,
    deadline_time: Option<DateTime<Local>>,
    rank: usize,
    scheduled_start: DateTime<Local>,
    estimated_work_seconds: i64,
    total_work_seconds: i64,
    is_on_other_side: bool,
    all_work_is_on_overload_date: bool,
}

pub fn flatten_tasks(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<FlattenResult, ApplicationError> {
    flatten_tasks_with_end_of_day_offset_minutes(
        repository,
        free_time_manager,
        END_OF_DAY_OFFSET_MINUTES,
    )
}

pub fn flatten_tasks_with_end_of_day_offset_minutes(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    end_of_day_offset_minutes: i64,
) -> Result<FlattenResult, ApplicationError> {
    flatten_tasks_with_end_of_day_offset_minutes_and_metrics(
        repository,
        free_time_manager,
        end_of_day_offset_minutes,
        &mut FlattenMetrics::default(),
    )
}

#[cfg(feature = "benchmarking")]
pub(crate) fn flatten_tasks_with_metrics(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    metrics: &mut FlattenMetrics,
) -> Result<FlattenResult, ApplicationError> {
    flatten_tasks_with_end_of_day_offset_minutes_and_metrics(
        repository,
        free_time_manager,
        END_OF_DAY_OFFSET_MINUTES,
        metrics,
    )
}

fn flatten_tasks_with_end_of_day_offset_minutes_and_metrics(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    end_of_day_offset_minutes: i64,
    metrics: &mut FlattenMetrics,
) -> Result<FlattenResult, ApplicationError> {
    let operation_datetime = repository.get_last_synced_time();
    let today = try_logical_date(operation_datetime)?;
    let checked_target_date = |days| {
        today.checked_add_signed(Duration::days(days)).ok_or(
            ApplicationError::LogicalDateOutOfRange {
                operation: "flatten_target_dates",
                datetime: operation_datetime,
            },
        )
    };
    let boundary_date = checked_target_date(FLATTEN_TARGET_DAYS)?;
    let overflow_date = checked_target_date(FLATTEN_OVERFLOW_DAY)?;
    let dates = (0..=FLATTEN_TARGET_DAYS)
        .map(checked_target_date)
        .collect::<Result<Vec<_>, _>>()?;
    let capacities = dates
        .iter()
        .map(|date| {
            Ok((
                *date,
                calculate_free_time_minutes_for_logical_date_with_end_of_day_offset_minutes(
                    date,
                    operation_datetime,
                    free_time_manager,
                    end_of_day_offset_minutes,
                )? * 60,
            ))
        })
        .collect::<Result<HashMap<_, _>, ApplicationError>>()?;
    let maximum_daily_capacity = capacities.values().copied().max().unwrap_or(0);
    let schedule_context = build_schedule_context_with_metrics(repository, &mut metrics.schedule)?;
    let initial_schedule = get_schedule_from_context_with_overrides_and_metrics(
        &schedule_context,
        &HashMap::new(),
        &mut metrics.schedule,
    )?;
    let original_task_details = collect_original_task_details(&initial_schedule, metrics)?;
    let mut schedule = initial_schedule;
    let mut overrides = HashMap::<Uuid, DateTime<Local>>::new();
    let mut movement_order = Vec::<Uuid>::new();
    let mut movement_ids = HashSet::<Uuid>::new();
    let mut blocked_dates = HashSet::<NaiveDate>::new();
    let mut unresolved_overloads = Vec::<UnresolvedOverload>::new();
    let mut had_overload = false;

    loop {
        let usage = calculate_scheduled_work_seconds_by_date(&schedule, metrics)?;
        let overload_date_opt = dates.iter().find(|date| {
            !blocked_dates.contains(date)
                && usage.get(date).copied().unwrap_or(0)
                    > capacities.get(date).copied().unwrap_or(0)
        });
        let Some(overload_date) = overload_date_opt.copied() else {
            break;
        };
        metrics.record_overload_iteration();
        had_overload = true;

        let target_date = if overload_date == boundary_date {
            overflow_date
        } else {
            overload_date.checked_add_signed(Duration::days(1)).ok_or(
                ApplicationError::LogicalDateOutOfRange {
                    operation: "flatten_target_date",
                    datetime: operation_datetime,
                },
            )?
        };
        let mut candidates = collect_candidates(&schedule, overload_date, metrics)?;
        sort_candidates_for_deferral(&mut candidates);

        let mut accepted = None;
        let mut rejected = Vec::<(FlattenCandidate, UnresolvedReason)>::new();
        for candidate in candidates {
            metrics.record_candidate_trial();
            if let Some(reason) = candidate_precheck_reason(&candidate, maximum_daily_capacity) {
                rejected.push((candidate, reason));
                continue;
            }
            let target_datetime = try_logical_date_start(target_date)?;
            if effective_pending_until(
                target_datetime,
                candidate.deadline_time,
                candidate.estimated_work_seconds,
            ) != target_datetime
            {
                rejected.push((candidate, UnresolvedReason::OwnDeadline));
                continue;
            }
            metrics.record_override_clone(0);
            let previous_override = overrides.insert(candidate.task_id, target_datetime);
            let trial_schedule_result = get_schedule_from_context_with_overrides_and_metrics(
                &schedule_context,
                &overrides,
                &mut metrics.schedule,
            );
            match previous_override {
                Some(previous) => {
                    overrides.insert(candidate.task_id, previous);
                }
                None => {
                    overrides.remove(&candidate.task_id);
                }
            }
            let trial_schedule = trial_schedule_result?;
            if introduces_deadline_violation(&schedule, &trial_schedule, metrics) {
                rejected.push((candidate, UnresolvedReason::RelatedDeadline));
                continue;
            }
            accepted = Some((candidate, target_datetime, trial_schedule));
            break;
        }

        let Some((candidate, target_datetime, trial_schedule)) = accepted else {
            let excess_work_seconds = usage.get(&overload_date).copied().unwrap_or(0)
                - capacities.get(&overload_date).copied().unwrap_or(0);
            unresolved_overloads.push(summarize_unresolved_overload(
                overload_date,
                excess_work_seconds,
                rejected,
            ));
            blocked_dates.insert(overload_date);
            continue;
        };
        if movement_ids.insert(candidate.task_id) {
            movement_order.push(candidate.task_id);
        }
        overrides.insert(candidate.task_id, target_datetime);
        schedule = trial_schedule;
    }

    if !had_overload {
        return Ok(FlattenResult::default());
    }

    let mut flattened_tasks = Vec::new();
    for task_id in movement_order {
        let Some(target_datetime) = overrides.get(&task_id).copied() else {
            continue;
        };
        let Some((name, priority, source_date, work_seconds)) =
            original_task_details.get(&task_id).cloned()
        else {
            continue;
        };
        let target_date = try_logical_date(target_datetime)?;
        let Some(task) = repository
            .get_by_id(task_id)
            .map_err(ApplicationError::TaskTree)?
        else {
            continue;
        };
        task.set_pending_until(target_datetime)
            .map_err(ApplicationError::TaskTree)?;
        task.set_orig_status(Status::Pending)
            .map_err(ApplicationError::TaskTree)?;
        flattened_tasks.push(FlattenedTask {
            task_id,
            name,
            priority,
            source_date,
            target_date,
            work_seconds,
        });
    }

    let overflowed_tasks = flattened_tasks
        .iter()
        .filter(|flattened| flattened.target_date == overflow_date)
        .collect::<Vec<_>>();
    Ok(FlattenResult {
        overflowed_task_count: overflowed_tasks.len(),
        overflowed_work_seconds: overflowed_tasks
            .iter()
            .map(|flattened| flattened.work_seconds)
            .sum(),
        flattened_tasks,
        had_overload,
        unresolved_overloads,
    })
}

#[allow(clippy::type_complexity)]
fn collect_original_task_details(
    schedule: &[ScheduledTaskView],
    metrics: &mut FlattenMetrics,
) -> Result<HashMap<Uuid, (String, i64, NaiveDate, i64)>, ApplicationError> {
    let mut details = HashMap::new();
    for scheduled in schedule {
        metrics.record_full_schedule_scan(1);
        if let std::collections::hash_map::Entry::Vacant(entry) = details.entry(scheduled.task.id) {
            entry.insert((
                scheduled.task.name.clone(),
                scheduled.task.priority,
                try_logical_date(scheduled.scheduled_start)?,
                scheduled.total_work_seconds,
            ));
        }
    }
    Ok(details)
}

fn collect_candidates(
    schedule: &[ScheduledTaskView],
    overload_date: NaiveDate,
    metrics: &mut FlattenMetrics,
) -> Result<Vec<FlattenCandidate>, ApplicationError> {
    let mut segments_by_task = HashMap::<Uuid, Vec<&ScheduledTaskView>>::new();
    for scheduled in schedule {
        metrics.record_full_schedule_scan(1);
        segments_by_task
            .entry(scheduled.task.id)
            .or_default()
            .push(scheduled);
    }

    let mut candidates = Vec::new();
    let mut next_logical_date_start_opt = None;
    for segments in segments_by_task.into_values() {
        let Some(first) = segments.first().copied() else {
            continue;
        };
        if first.total_work_seconds <= 0 {
            continue;
        }
        let segment_dates = segments
            .iter()
            .map(|segment| try_logical_date(segment.scheduled_start))
            .collect::<Result<Vec<_>, _>>()?;
        if !segment_dates.contains(&overload_date) {
            continue;
        }
        let Some(scheduled_start) = segments.iter().map(|segment| segment.scheduled_start).min()
        else {
            continue;
        };
        let next_logical_date_start = match next_logical_date_start_opt {
            Some(datetime) => datetime,
            None => {
                let datetime = try_next_logical_date_start(try_logical_date_start(overload_date)?)?;
                next_logical_date_start_opt = Some(datetime);
                datetime
            }
        };
        let all_work_is_on_overload_date =
            segments.iter().zip(segment_dates).all(|(segment, date)| {
                date == overload_date && segment.scheduled_end <= next_logical_date_start
            });
        candidates.push(FlattenCandidate {
            task_id: first.task.id,
            name: first.task.name.clone(),
            priority: first.task.priority,
            deadline_time: first.task.deadline_time,
            rank: first.rank,
            scheduled_start,
            estimated_work_seconds: first.task.estimated_work_seconds,
            total_work_seconds: first.total_work_seconds,
            is_on_other_side: first.task.is_on_other_side,
            all_work_is_on_overload_date,
        });
    }
    Ok(candidates)
}

fn candidate_precheck_reason(
    candidate: &FlattenCandidate,
    maximum_daily_capacity: i64,
) -> Option<UnresolvedReason> {
    if candidate.is_on_other_side {
        Some(UnresolvedReason::OnOtherSide)
    } else if !candidate.all_work_is_on_overload_date {
        Some(UnresolvedReason::CrossesLogicalDate)
    } else if candidate.total_work_seconds > maximum_daily_capacity {
        Some(UnresolvedReason::ExceedsDailyCapacity)
    } else {
        None
    }
}

fn effective_pending_until(
    requested: DateTime<Local>,
    deadline_time: Option<DateTime<Local>>,
    estimated_work_seconds: i64,
) -> DateTime<Local> {
    let datetime_policy = LogicalDateTimePolicy::new(END_OF_DAY_OFFSET_MINUTES);
    deadline_time.map_or(requested, |deadline| {
        requested.min(datetime_policy.deadline_pending_limit(deadline, estimated_work_seconds))
    })
}

fn sort_candidates_for_deferral(candidates: &mut [FlattenCandidate]) {
    candidates.sort_by(|a, b| {
        b.rank
            .cmp(&a.rank)
            .then_with(|| a.deadline_time.is_some().cmp(&b.deadline_time.is_some()))
            .then_with(|| b.deadline_time.cmp(&a.deadline_time))
            .then_with(|| a.priority.cmp(&b.priority))
            .then_with(|| b.scheduled_start.cmp(&a.scheduled_start))
            .then_with(|| a.task_id.cmp(&b.task_id))
    });
}

fn introduces_deadline_violation(
    current_schedule: &[ScheduledTaskView],
    trial_schedule: &[ScheduledTaskView],
    metrics: &mut FlattenMetrics,
) -> bool {
    let current_ends = scheduled_end_by_task(current_schedule, metrics);
    let trial_ends = scheduled_end_by_task(trial_schedule, metrics);
    trial_schedule.iter().any(|scheduled| {
        metrics.record_full_schedule_scan(1);
        let Some(deadline) = scheduled.task.deadline_time else {
            return false;
        };
        let Some(trial_end) = trial_ends.get(&scheduled.task.id).copied() else {
            return false;
        };
        trial_end > deadline
            && current_ends
                .get(&scheduled.task.id)
                .is_none_or(|current_end| trial_end > *current_end)
    })
}

fn scheduled_end_by_task(
    schedule: &[ScheduledTaskView],
    metrics: &mut FlattenMetrics,
) -> HashMap<Uuid, DateTime<Local>> {
    let mut ends = HashMap::<Uuid, DateTime<Local>>::new();
    for scheduled in schedule {
        metrics.record_full_schedule_scan(1);
        ends.entry(scheduled.task.id)
            .and_modify(|end| *end = (*end).max(scheduled.scheduled_end))
            .or_insert(scheduled.scheduled_end);
    }
    ends
}

fn summarize_unresolved_overload(
    date: NaiveDate,
    excess_work_seconds: i64,
    rejected: Vec<(FlattenCandidate, UnresolvedReason)>,
) -> UnresolvedOverload {
    let mut summaries = Vec::<UnresolvedReasonSummary>::new();
    for reason in [
        UnresolvedReason::OnOtherSide,
        UnresolvedReason::CrossesLogicalDate,
        UnresolvedReason::ExceedsDailyCapacity,
        UnresolvedReason::OwnDeadline,
        UnresolvedReason::RelatedDeadline,
        UnresolvedReason::Other,
    ] {
        let matching = rejected
            .iter()
            .filter(|(_, rejected_reason)| *rejected_reason == reason)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            continue;
        }
        summaries.push(UnresolvedReasonSummary {
            reason,
            task_count: matching.len(),
            representative_task_id: matching.first().map(|(candidate, _)| candidate.task_id),
            representative_task_name: matching
                .first()
                .map(|(candidate, _)| candidate.name.clone()),
        });
    }

    if summaries.is_empty() {
        summaries.push(UnresolvedReasonSummary {
            reason: UnresolvedReason::Other,
            task_count: 1,
            representative_task_id: None,
            representative_task_name: None,
        });
    }

    UnresolvedOverload {
        date,
        excess_work_seconds,
        reasons: summaries,
    }
}

fn calculate_scheduled_work_seconds_by_date(
    schedule: &[ScheduledTaskView],
    metrics: &mut FlattenMetrics,
) -> Result<HashMap<NaiveDate, i64>, ApplicationError> {
    let mut usage = HashMap::new();
    for scheduled in schedule {
        metrics.record_full_schedule_scan(1);
        add_scheduled_work_seconds_by_date(
            &mut usage,
            scheduled.scheduled_start,
            scheduled.scheduled_end,
        )?;
    }
    Ok(usage)
}

fn add_scheduled_work_seconds_by_date(
    scheduled_work_seconds_by_date: &mut HashMap<NaiveDate, i64>,
    scheduled_start: DateTime<Local>,
    scheduled_end: DateTime<Local>,
) -> Result<(), ApplicationError> {
    let date = try_logical_date(scheduled_start)?;
    *scheduled_work_seconds_by_date.entry(date).or_default() +=
        (scheduled_end - scheduled_start).num_seconds();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{new_task_handle, TestFreeTimeManager, TestTaskRepository};
    use chrono::{FixedOffset, TimeZone};

    #[test]
    fn 日次使用量は予約区間ではなくscheduled_work_secondsを集計する() {
        let start = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
        let mut usage = HashMap::new();

        add_scheduled_work_seconds_by_date(
            &mut usage,
            start,
            start + Duration::hours(1),
            15 * 60,
        )
        .unwrap();

        assert_eq!(usage.get(&try_logical_date(start).unwrap()), Some(&(15 * 60)));
    }

    #[test]
    fn flatten_tasksはoperation時刻のlogical_date計算不能を伝搬しtaskを変更しない() {
        let local_datetime = NaiveDate::MIN.and_hms_opt(5, 59, 0).unwrap();
        let now = DateTime::<Local>::from_naive_utc_and_offset(
            local_datetime,
            FixedOffset::east_opt(0).unwrap(),
        );
        let task = new_task_handle("対象").unwrap();
        let original_revision = task.get_persistent_mutation_revision().unwrap();
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = flatten_tasks_with_end_of_day_offset_minutes(
            &repository,
            &mut free_time_manager,
            END_OF_DAY_OFFSET_MINUTES,
        );

        assert_eq!(
            actual,
            Err(ApplicationError::LogicalDateOutOfRange {
                operation: "logical_date",
                datetime: now,
            })
        );
        assert_eq!(
            task.get_persistent_mutation_revision().unwrap(),
            original_revision
        );
    }
}
