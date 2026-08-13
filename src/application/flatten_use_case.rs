use super::daily_capacity::{
    calculate_free_time_minutes_for_subjective_date, subjective_date, subjective_date_start,
};
use super::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use super::schedule_use_case::{
    get_schedule, get_schedule_with_first_available_time_overrides, ScheduledTaskView,
};
use crate::entity::task::Status;
use chrono::{DateTime, Duration, Local, NaiveDate};
use std::cmp::min;
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlattenFailure {
    pub date: NaiveDate,
    pub task_id: Option<Uuid>,
    pub task_name: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FlattenResult {
    pub flattened_tasks: Vec<FlattenedTask>,
    pub overflowed_task_count: usize,
    pub overflowed_work_seconds: i64,
    pub had_overload: bool,
    pub failure: Option<FlattenFailure>,
}

#[derive(Clone)]
struct FlattenCandidate {
    task_id: Uuid,
    priority: i64,
    deadline_time: Option<DateTime<Local>>,
    rank: usize,
    scheduled_start: DateTime<Local>,
}

pub fn flatten_tasks(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> FlattenResult {
    let today = subjective_date(repository.get_last_synced_time());
    let boundary_date = today + Duration::days(FLATTEN_TARGET_DAYS);
    let overflow_date = today + Duration::days(FLATTEN_OVERFLOW_DAY);
    let dates = (0..=FLATTEN_TARGET_DAYS)
        .map(|days| today + Duration::days(days))
        .collect::<Vec<_>>();
    let capacities = dates
        .iter()
        .map(|date| {
            (
                *date,
                calculate_free_time_minutes_for_subjective_date(
                    date,
                    repository.get_last_synced_time(),
                    free_time_manager,
                ) * 60,
            )
        })
        .collect::<HashMap<_, _>>();
    let maximum_daily_capacity = capacities.values().copied().max().unwrap_or(0);
    let initial_schedule = get_schedule(repository);
    let original_task_details = collect_original_task_details(&initial_schedule);
    let mut schedule = initial_schedule;
    let mut overrides = HashMap::<Uuid, DateTime<Local>>::new();
    let mut movement_order = Vec::<Uuid>::new();
    let mut movement_ids = HashSet::<Uuid>::new();
    let mut had_overload = false;

    loop {
        let usage = calculate_scheduled_work_seconds_by_date(&schedule);
        let overload_date_opt = dates.iter().find(|date| {
            usage.get(date).copied().unwrap_or(0) > capacities.get(date).copied().unwrap_or(0)
        });
        let Some(overload_date) = overload_date_opt.copied() else {
            break;
        };
        had_overload = true;

        let target_date = if overload_date == boundary_date {
            overflow_date
        } else {
            overload_date + Duration::days(1)
        };
        let mut candidates = collect_candidates(&schedule, overload_date, maximum_daily_capacity);
        sort_candidates_for_deferral(&mut candidates);

        let mut accepted = None;
        for candidate in candidates {
            let mut trial_overrides = overrides.clone();
            trial_overrides.insert(candidate.task_id, subjective_date_start(target_date));
            let trial_schedule =
                get_schedule_with_first_available_time_overrides(repository, &trial_overrides);
            if introduces_deadline_violation(&schedule, &trial_schedule) {
                continue;
            }
            accepted = Some((candidate, trial_overrides, trial_schedule));
            break;
        }

        let Some((candidate, trial_overrides, trial_schedule)) = accepted else {
            return failed_result(overload_date, &schedule, had_overload);
        };
        if movement_ids.insert(candidate.task_id) {
            movement_order.push(candidate.task_id);
        }
        overrides = trial_overrides;
        schedule = trial_schedule;
    }

    if !had_overload {
        return FlattenResult::default();
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
        let Some(task) = repository.get_by_id(task_id) else {
            continue;
        };
        task.set_pending_until(target_datetime);
        task.set_orig_status(Status::Pending);
        flattened_tasks.push(FlattenedTask {
            task_id,
            name,
            priority,
            source_date,
            target_date: subjective_date(target_datetime),
            work_seconds,
        });
    }

    let overflowed_tasks = flattened_tasks
        .iter()
        .filter(|flattened| flattened.target_date == overflow_date)
        .collect::<Vec<_>>();
    FlattenResult {
        overflowed_task_count: overflowed_tasks.len(),
        overflowed_work_seconds: overflowed_tasks
            .iter()
            .map(|flattened| flattened.work_seconds)
            .sum(),
        flattened_tasks,
        had_overload,
        failure: None,
    }
}

fn collect_original_task_details(
    schedule: &[ScheduledTaskView],
) -> HashMap<Uuid, (String, i64, NaiveDate, i64)> {
    let mut details = HashMap::new();
    for scheduled in schedule {
        details.entry(scheduled.task.id).or_insert_with(|| {
            (
                scheduled.task.name.clone(),
                scheduled.task.priority,
                subjective_date(scheduled.scheduled_start),
                scheduled.total_work_seconds,
            )
        });
    }
    details
}

fn collect_candidates(
    schedule: &[ScheduledTaskView],
    overload_date: NaiveDate,
    maximum_daily_capacity: i64,
) -> Vec<FlattenCandidate> {
    let mut segments_by_task = HashMap::<Uuid, Vec<&ScheduledTaskView>>::new();
    for scheduled in schedule {
        segments_by_task
            .entry(scheduled.task.id)
            .or_default()
            .push(scheduled);
    }

    segments_by_task
        .into_values()
        .filter_map(|segments| {
            let first = segments.first().copied()?;
            let scheduled_start = segments
                .iter()
                .map(|segment| segment.scheduled_start)
                .min()?;
            let all_work_is_on_overload_date = segments.iter().all(|segment| {
                subjective_date(segment.scheduled_start) == overload_date
                    && segment.scheduled_end
                        <= subjective_date_start(overload_date + Duration::days(1))
            });
            (all_work_is_on_overload_date
                && !first.task.is_on_other_side
                && first.total_work_seconds > 0
                && first.total_work_seconds <= maximum_daily_capacity)
                .then_some(FlattenCandidate {
                    task_id: first.task.id,
                    priority: first.task.priority,
                    deadline_time: first.task.deadline_time,
                    rank: first.rank,
                    scheduled_start,
                })
        })
        .collect()
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
) -> bool {
    let current_ends = scheduled_end_by_task(current_schedule);
    let trial_ends = scheduled_end_by_task(trial_schedule);
    trial_schedule.iter().any(|scheduled| {
        let Some(deadline) = scheduled.task.deadline_time else {
            return false;
        };
        let Some(trial_end) = trial_ends.get(&scheduled.task.id).copied() else {
            return false;
        };
        trial_end > deadline
            && current_ends
                .get(&scheduled.task.id)
                .map_or(true, |current_end| trial_end > *current_end)
    })
}

fn scheduled_end_by_task(schedule: &[ScheduledTaskView]) -> HashMap<Uuid, DateTime<Local>> {
    let mut ends = HashMap::<Uuid, DateTime<Local>>::new();
    for scheduled in schedule {
        ends.entry(scheduled.task.id)
            .and_modify(|end| *end = (*end).max(scheduled.scheduled_end))
            .or_insert(scheduled.scheduled_end);
    }
    ends
}

fn failed_result(
    date: NaiveDate,
    schedule: &[ScheduledTaskView],
    had_overload: bool,
) -> FlattenResult {
    let blocking = schedule
        .iter()
        .find(|scheduled| subjective_date(scheduled.scheduled_start) == date);
    FlattenResult {
        had_overload,
        failure: Some(FlattenFailure {
            date,
            task_id: blocking.map(|scheduled| scheduled.task.id),
            task_name: blocking.map(|scheduled| scheduled.task.name.clone()),
            reason: "延期可能なタスクがありません".to_string(),
        }),
        ..FlattenResult::default()
    }
}

fn calculate_scheduled_work_seconds_by_date(
    schedule: &[ScheduledTaskView],
) -> HashMap<NaiveDate, i64> {
    let mut usage = HashMap::new();
    for scheduled in schedule {
        add_scheduled_work_seconds_by_date(
            &mut usage,
            scheduled.scheduled_start,
            scheduled.scheduled_end,
        );
    }
    usage
}

fn add_scheduled_work_seconds_by_date(
    scheduled_work_seconds_by_date: &mut HashMap<NaiveDate, i64>,
    scheduled_start: DateTime<Local>,
    scheduled_end: DateTime<Local>,
) {
    let mut cursor = scheduled_start;
    while cursor < scheduled_end {
        let date = subjective_date(cursor);
        let next_date_start = subjective_date_start(date + Duration::days(1));
        let segment_end = min(scheduled_end, next_date_start);
        *scheduled_work_seconds_by_date.entry(date).or_default() +=
            (segment_end - cursor).num_seconds();
        cursor = segment_end;
    }
}
