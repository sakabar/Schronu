use super::daily_capacity::{
    calculate_free_time_minutes_for_subjective_date, subjective_date, subjective_date_start,
};
use super::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use super::schedule_use_case::get_schedule;
use crate::entity::task::Status;
use chrono::{DateTime, Duration, Local, NaiveDate};
use std::cmp::{min, Reverse};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const FLATTEN_TARGET_DAYS: i64 = 28;
const MINIMUM_TARGET_FREE_SECONDS: i64 = 15 * 60;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlattenedTask {
    pub task_id: Uuid,
    pub name: String,
    pub priority: i64,
    pub target_date: NaiveDate,
    pub work_seconds: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FlattenResult {
    pub flattened_tasks: Vec<FlattenedTask>,
}

#[derive(Clone)]
struct FlattenCandidate {
    task_id: Uuid,
    name: String,
    priority: i64,
    source_date: NaiveDate,
    deadline_date_opt: Option<NaiveDate>,
    rank: usize,
    work_seconds: i64,
}

pub fn flatten_tasks(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> FlattenResult {
    let today = subjective_date(repository.get_last_synced_time());
    let target_dates = (1..=FLATTEN_TARGET_DAYS)
        .map(|days| today + Duration::days(days))
        .collect::<Vec<_>>();
    let mut moved_task_ids = HashSet::new();
    let mut used_target_dates = HashSet::new();
    let mut result = FlattenResult::default();

    while let Some(flattened) = find_next_flattened_task(
        repository,
        free_time_manager,
        &target_dates,
        &moved_task_ids,
        &used_target_dates,
    ) {
        let Some(task) = repository.get_by_id(flattened.task_id) else {
            break;
        };
        task.set_pending_until(subjective_date_start(flattened.target_date));
        task.set_orig_status(Status::Pending);
        moved_task_ids.insert(flattened.task_id);
        used_target_dates.insert(flattened.target_date);
        result.flattened_tasks.push(flattened);
    }

    result
}

fn find_next_flattened_task(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    target_dates: &[NaiveDate],
    moved_task_ids: &HashSet<Uuid>,
    used_target_dates: &HashSet<NaiveDate>,
) -> Option<FlattenedTask> {
    let schedule = get_schedule(repository);
    let mut scheduled_work_seconds_by_date = HashMap::<NaiveDate, i64>::new();
    for scheduled in &schedule {
        add_scheduled_work_seconds_by_date(
            &mut scheduled_work_seconds_by_date,
            scheduled.scheduled_start,
            scheduled.scheduled_end,
        );
    }

    let mut seen_task_ids = HashSet::new();
    let mut candidates = schedule
        .into_iter()
        .filter(|scheduled| seen_task_ids.insert(scheduled.task.id))
        .filter(|scheduled| {
            !moved_task_ids.contains(&scheduled.task.id)
                && !scheduled.task.is_on_other_side
                && scheduled.total_work_seconds > 0
        })
        .map(|scheduled| FlattenCandidate {
            task_id: scheduled.task.id,
            name: scheduled.task.name,
            priority: scheduled.task.priority,
            source_date: subjective_date(scheduled.scheduled_start),
            deadline_date_opt: scheduled.task.deadline_time.map(subjective_date),
            rank: scheduled.rank,
            work_seconds: scheduled.total_work_seconds,
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| {
        Reverse((
            candidate.source_date,
            candidate.deadline_date_opt.is_none(),
            Reverse(candidate.priority),
            candidate.rank,
            candidate.task_id,
        ))
    });

    for target_date in target_dates {
        let free_time_seconds = calculate_free_time_minutes_for_subjective_date(
            target_date,
            repository.get_last_synced_time(),
            free_time_manager,
        ) * 60;
        let scheduled_work_seconds = scheduled_work_seconds_by_date
            .get(target_date)
            .copied()
            .unwrap_or(0);
        let remaining_seconds = free_time_seconds - scheduled_work_seconds;
        let minimum_remaining_seconds = if used_target_dates.contains(target_date) {
            1
        } else {
            MINIMUM_TARGET_FREE_SECONDS
        };
        if remaining_seconds < minimum_remaining_seconds {
            continue;
        }

        if let Some(candidate) = candidates.iter().find(|candidate| {
            candidate.source_date < *target_date
                && candidate.work_seconds <= remaining_seconds
                && candidate
                    .deadline_date_opt
                    .map_or(true, |deadline_date| *target_date < deadline_date)
        }) {
            return Some(FlattenedTask {
                task_id: candidate.task_id,
                name: candidate.name.clone(),
                priority: candidate.priority,
                target_date: *target_date,
                work_seconds: candidate.work_seconds,
            });
        }
    }

    None
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
