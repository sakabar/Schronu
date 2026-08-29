use crate::application::daily_capacity::try_next_logical_date_start;
use crate::application::interface::TaskRepositoryTrait;
use crate::application::scheduling_metrics::ScheduleMetrics;
use crate::application::task_use_case::ApplicationError;
use crate::application::task_view::TaskView;
use crate::entity::task::{
    extract_leaf_tasks_from_project_with_pending, TaskHandle, TaskTreeError,
};
use chrono::{DateTime, Duration, Local};
use serde::Serialize;
use std::cmp::{max, Reverse};
use std::collections::{BinaryHeap, HashMap};
use uuid::Uuid;

const MIN_SPLIT_SEGMENT_SECONDS: i64 = 5 * 60;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScheduledTaskView {
    pub task: TaskView,
    pub first_available_time: DateTime<Local>,
    pub scheduled_start: DateTime<Local>,
    pub scheduled_end: DateTime<Local>,
    pub scheduled_work_seconds: i64,
    pub total_work_seconds: i64,
    pub rank: usize,
}

#[derive(Clone)]
struct TaskScheduleCandidate {
    id: Uuid,
    task: TaskHandle,
    first_available_time: DateTime<Local>,
    neg_priority: i64,
    rank: usize,
    deadline_time: Option<DateTime<Local>>,
    remaining_seconds: i64,
    dependency_ids: Vec<Uuid>,
    atomic: bool,
}

pub(crate) struct ScheduleContext {
    candidates: Vec<TaskScheduleCandidate>,
    last_synced_time: DateTime<Local>,
}

struct TaskScheduleAttributes {
    first_available_time: DateTime<Local>,
    neg_priority: i64,
    rank: usize,
    deadline_time: Option<DateTime<Local>>,
}

#[derive(Clone)]
struct ScheduledTask {
    id: Uuid,
    task: TaskHandle,
    first_available_time: DateTime<Local>,
    scheduled_start: DateTime<Local>,
    scheduled_end: DateTime<Local>,
    scheduled_work_seconds: i64,
    total_work_seconds: i64,
    neg_priority: i64,
    rank: usize,
    deadline_time: Option<DateTime<Local>>,
}

pub fn get_schedule(
    repository: &dyn TaskRepositoryTrait,
) -> Result<Vec<ScheduledTaskView>, ApplicationError> {
    get_schedule_with_first_available_time_overrides_and_metrics(
        repository,
        &HashMap::new(),
        &mut ScheduleMetrics::default(),
    )
}

pub(crate) fn get_schedule_with_metrics(
    repository: &dyn TaskRepositoryTrait,
    metrics: &mut ScheduleMetrics,
) -> Result<Vec<ScheduledTaskView>, ApplicationError> {
    get_schedule_with_first_available_time_overrides_and_metrics(
        repository,
        &HashMap::new(),
        metrics,
    )
}

pub(crate) fn get_schedule_with_task_first_available_time_and_metrics(
    repository: &dyn TaskRepositoryTrait,
    task_id: Uuid,
    first_available_time: DateTime<Local>,
    metrics: &mut ScheduleMetrics,
) -> Result<Vec<ScheduledTaskView>, ApplicationError> {
    get_schedule_with_first_available_time_overrides_and_metrics(
        repository,
        &HashMap::from([(task_id, first_available_time)]),
        metrics,
    )
}

pub(crate) fn get_schedule_with_first_available_time_overrides_and_metrics(
    repository: &dyn TaskRepositoryTrait,
    first_available_time_overrides: &HashMap<Uuid, DateTime<Local>>,
    metrics: &mut ScheduleMetrics,
) -> Result<Vec<ScheduledTaskView>, ApplicationError> {
    let context = build_schedule_context_with_metrics(repository, metrics)?;
    get_schedule_from_context_with_overrides_and_metrics(
        &context,
        first_available_time_overrides,
        metrics,
    )
}

pub(crate) fn build_schedule_context_with_metrics(
    repository: &dyn TaskRepositoryTrait,
    metrics: &mut ScheduleMetrics,
) -> Result<ScheduleContext, ApplicationError> {
    for project_root in repository.get_all_projects() {
        project_root
            .snapshot()
            .map_err(ApplicationError::TaskTree)?;
    }
    Ok(ScheduleContext {
        candidates: build_schedule_candidates(repository, metrics)?,
        last_synced_time: repository.get_last_synced_time(),
    })
}

pub(crate) fn get_schedule_from_context_with_overrides_and_metrics(
    context: &ScheduleContext,
    first_available_time_overrides: &HashMap<Uuid, DateTime<Local>>,
    metrics: &mut ScheduleMetrics,
) -> Result<Vec<ScheduledTaskView>, ApplicationError> {
    metrics.record_rebuild();
    let mut candidates = context.candidates.clone();
    for candidate in &mut candidates {
        if let Some(first_available_time) = first_available_time_overrides.get(&candidate.id) {
            candidate.first_available_time = max(*first_available_time, context.last_synced_time);
        }
    }
    schedule_tasks_by_priority_with_metrics(&candidates, context.last_synced_time, metrics)
        .map_err(ApplicationError::TaskTree)?
        .into_iter()
        .map(|scheduled| {
            Ok(ScheduledTaskView {
                task: TaskView::try_from(&scheduled.task).map_err(ApplicationError::TaskTree)?,
                first_available_time: scheduled.first_available_time,
                scheduled_start: scheduled.scheduled_start,
                scheduled_end: scheduled.scheduled_end,
                scheduled_work_seconds: scheduled.scheduled_work_seconds,
                total_work_seconds: scheduled.total_work_seconds,
                rank: scheduled.rank,
            })
        })
        .collect()
}

fn build_schedule_candidates(
    repository: &dyn TaskRepositoryTrait,
    metrics: &mut ScheduleMetrics,
) -> Result<Vec<TaskScheduleCandidate>, ApplicationError> {
    let last_synced_time = repository.get_last_synced_time();
    let mut task_schedule_attributes: HashMap<Uuid, TaskScheduleAttributes> = HashMap::new();
    let mut child_ids_by_parent_id: HashMap<Uuid, Vec<Uuid>> = HashMap::new();

    for project_root in repository.get_all_projects() {
        for leaf in extract_leaf_tasks_from_project_with_pending(project_root)
            .map_err(ApplicationError::TaskTree)?
        {
            let ancestors = leaf
                .list_all_parent_tasks_with_first_available_time()
                .map_err(ApplicationError::TaskTree)?;
            for pair in ancestors.windows(2) {
                let child_id = pair[0].1.get_id().map_err(ApplicationError::TaskTree)?;
                let parent_id = pair[1].1.get_id().map_err(ApplicationError::TaskTree)?;
                let child_ids = child_ids_by_parent_id.entry(parent_id).or_default();
                if !child_ids.contains(&child_id) {
                    child_ids.push(child_id);
                }
            }

            for (rank, (first_available_time, task)) in ancestors.iter().enumerate() {
                let first_available_time = max(*first_available_time, last_synced_time);
                task_schedule_attributes
                    .entry(task.get_id().map_err(ApplicationError::TaskTree)?)
                    .and_modify(|attributes| {
                        attributes.first_available_time =
                            max(attributes.first_available_time, first_available_time);
                        attributes.rank = max(attributes.rank, rank);
                    })
                    .or_insert(TaskScheduleAttributes {
                        first_available_time,
                        neg_priority: !task.get_priority().map_err(ApplicationError::TaskTree)?,
                        rank,
                        deadline_time: task
                            .get_deadline_time_opt()
                            .map_err(ApplicationError::TaskTree)?,
                    });
            }
        }
    }

    let mut attributes = task_schedule_attributes.into_iter().collect::<Vec<_>>();
    metrics.record_sort();
    attributes.sort_by_key(|(id, _)| *id);
    let mut attributes = attributes
        .into_iter()
        .map(|(id, attributes)| {
            let first_available_time = attributes.first_available_time;
            let logical_date = try_next_logical_date_start(first_available_time)?
                .checked_sub_signed(Duration::days(1))
                .map(|datetime| datetime.date_naive())
                .ok_or(ApplicationError::LogicalDateOutOfRange {
                    operation: "logical_date",
                    datetime: first_available_time,
                })?;
            let sort_key = (
                logical_date,
                attributes.deadline_time.is_none(),
                first_available_time,
                attributes.neg_priority,
                attributes.rank,
                attributes.deadline_time,
                id,
            );
            Ok((sort_key, (id, attributes)))
        })
        .collect::<Result<Vec<_>, ApplicationError>>()?;
    metrics.record_sort();
    attributes.sort_by_key(|entry| entry.0);

    let mut candidates = Vec::new();
    for (_, (id, attributes)) in attributes {
        let Some(task) = repository
            .get_by_id(id)
            .map_err(ApplicationError::TaskTree)?
        else {
            continue;
        };
        candidates.push(TaskScheduleCandidate {
            id,
            remaining_seconds: calculate_remaining_work_seconds(&task)
                .map_err(ApplicationError::TaskTree)?,
            dependency_ids: child_ids_by_parent_id.remove(&id).unwrap_or_default(),
            atomic: task.get_atomic().map_err(ApplicationError::TaskTree)?,
            task,
            first_available_time: attributes.first_available_time,
            neg_priority: attributes.neg_priority,
            rank: attributes.rank,
            deadline_time: attributes.deadline_time,
        });
    }
    metrics.record_candidates(candidates.len());
    Ok(candidates)
}

fn calculate_remaining_work_seconds(task: &TaskHandle) -> Result<i64, TaskTreeError> {
    let estimated_work_seconds = task.get_estimated_work_seconds()?;
    let actual_work_seconds = task.get_actual_work_seconds()?;
    if estimated_work_seconds >= actual_work_seconds {
        Ok(estimated_work_seconds - actual_work_seconds)
    } else {
        Ok(max(0, estimated_work_seconds * 2 - actual_work_seconds))
    }
}

fn find_earliest_non_overlapping_start(
    first_available_time: DateTime<Local>,
    remaining_seconds: i64,
    occupied_slots: &[(DateTime<Local>, DateTime<Local>)],
    metrics: &mut ScheduleMetrics,
) -> DateTime<Local> {
    let duration = Duration::seconds(remaining_seconds);
    let mut start = first_available_time;
    let mut index = occupied_slots.partition_point(|(_, occupied_end)| {
        metrics.record_occupied_slot_probe();
        *occupied_end <= start
    });
    while let Some((occupied_start, occupied_end)) = occupied_slots.get(index) {
        let end = start + duration;
        metrics.record_occupied_slot_probe();
        if end <= *occupied_start {
            break;
        }
        if start < *occupied_end && *occupied_start < end {
            start = *occupied_end;
        }
        index += 1;
    }
    start
}

fn find_next_occupied_slot(
    start: DateTime<Local>,
    occupied_slots: &[(DateTime<Local>, DateTime<Local>)],
    metrics: &mut ScheduleMetrics,
) -> Option<(DateTime<Local>, DateTime<Local>)> {
    let index = occupied_slots.partition_point(|(occupied_start, _)| {
        metrics.record_occupied_slot_probe();
        *occupied_start < start
    });
    occupied_slots
        .get(index)
        .filter(|(occupied_start, occupied_end)| {
            metrics.record_occupied_slot_probe();
            start < *occupied_end && start <= *occupied_start
        })
        .copied()
}

fn find_occupied_slot_containing(
    datetime: DateTime<Local>,
    occupied_slots: &[(DateTime<Local>, DateTime<Local>)],
    metrics: &mut ScheduleMetrics,
) -> Option<(DateTime<Local>, DateTime<Local>)> {
    let index = occupied_slots.partition_point(|(_, occupied_end)| {
        metrics.record_occupied_slot_probe();
        *occupied_end <= datetime
    });
    occupied_slots
        .get(index)
        .filter(|(occupied_start, occupied_end)| {
            metrics.record_occupied_slot_probe();
            datetime >= *occupied_start && datetime < *occupied_end
        })
        .copied()
}

fn find_occupied_slot_starting_at(
    datetime: DateTime<Local>,
    occupied_slots: &[(DateTime<Local>, DateTime<Local>)],
    metrics: &mut ScheduleMetrics,
) -> Option<(DateTime<Local>, DateTime<Local>)> {
    let index = occupied_slots.partition_point(|(occupied_start, _)| {
        metrics.record_occupied_slot_probe();
        *occupied_start < datetime
    });
    occupied_slots
        .get(index)
        .filter(|(occupied_start, _)| {
            metrics.record_occupied_slot_probe();
            *occupied_start == datetime
        })
        .copied()
}

fn insert_occupied_slot(
    occupied_slots: &mut Vec<(DateTime<Local>, DateTime<Local>)>,
    mut slot: (DateTime<Local>, DateTime<Local>),
    metrics: &mut ScheduleMetrics,
) {
    let first_merged = occupied_slots.partition_point(|(_, existing_end)| {
        metrics.record_occupied_slot_probe();
        *existing_end < slot.0
    });
    let mut past_merged = first_merged;
    while let Some((existing_start, existing_end)) = occupied_slots.get(past_merged) {
        metrics.record_occupied_slot_probe();
        if *existing_start > slot.1 {
            break;
        }
        slot.0 = slot.0.min(*existing_start);
        slot.1 = slot.1.max(*existing_end);
        past_merged += 1;
    }
    occupied_slots.splice(first_merged..past_merged, [slot]);
}

#[cfg(test)]
fn schedule_tasks_by_priority(
    candidates: &[TaskScheduleCandidate],
    last_synced_time: DateTime<Local>,
) -> Result<Vec<ScheduledTask>, TaskTreeError> {
    schedule_tasks_by_priority_with_metrics(
        candidates,
        last_synced_time,
        &mut ScheduleMetrics::default(),
    )
}

fn schedule_tasks_by_priority_with_metrics(
    candidates: &[TaskScheduleCandidate],
    last_synced_time: DateTime<Local>,
    metrics: &mut ScheduleMetrics,
) -> Result<Vec<ScheduledTask>, TaskTreeError> {
    let mut pending_candidates = candidates.to_vec();
    metrics.record_sort();
    pending_candidates.sort_by(|a, b| {
        (
            a.deadline_time.is_none(),
            a.deadline_time,
            a.neg_priority,
            a.first_available_time,
            a.rank,
            a.id,
        )
            .cmp(&(
                b.deadline_time.is_none(),
                b.deadline_time,
                b.neg_priority,
                b.first_available_time,
                b.rank,
                b.id,
            ))
    });

    let candidate_index_by_id = pending_candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| (candidate.id, index))
        .collect::<HashMap<_, _>>();
    let mut dependent_indices_by_id = HashMap::<Uuid, Vec<usize>>::new();
    let mut unresolved_dependency_counts = Vec::with_capacity(pending_candidates.len());
    let mut ready_indices = BinaryHeap::<Reverse<usize>>::new();
    for (index, candidate) in pending_candidates.iter().enumerate() {
        metrics.record_dependency_candidate_probe();
        let unresolved_count = candidate.dependency_ids.len();
        unresolved_dependency_counts.push(unresolved_count);
        if unresolved_count == 0 {
            ready_indices.push(Reverse(index));
        }
        for dependency_id in &candidate.dependency_ids {
            if candidate_index_by_id.contains_key(dependency_id) {
                dependent_indices_by_id
                    .entry(*dependency_id)
                    .or_default()
                    .push(index);
            }
        }
    }
    let mut pending_candidates = pending_candidates.into_iter().map(Some).collect::<Vec<_>>();
    let mut remaining_candidate_count = pending_candidates.len();
    let mut occupied_slots = Vec::new();
    let mut scheduled_tasks = Vec::new();
    let mut scheduled_end_by_id = HashMap::new();

    while remaining_candidate_count > 0 {
        let index = ready_indices
            .pop()
            .map(|Reverse(index)| index)
            .filter(|index| pending_candidates[*index].is_some())
            .or_else(|| pending_candidates.iter().position(Option::is_some))
            .expect("remaining candidate count guarantees a pending candidate");
        let candidate = pending_candidates[index]
            .take()
            .expect("ready candidate is pending");
        remaining_candidate_count -= 1;
        let dependency_end = candidate
            .dependency_ids
            .iter()
            .filter_map(|id| scheduled_end_by_id.get(id))
            .max()
            .copied()
            .unwrap_or(last_synced_time);
        let mut segment_start = find_earliest_non_overlapping_start(
            max(
                max(candidate.first_available_time, last_synced_time),
                dependency_end,
            ),
            0,
            &occupied_slots,
            metrics,
        );
        let mut remaining_seconds = candidate.remaining_seconds;
        let total_work_seconds = remaining_seconds;
        let mut candidate_scheduled_end = segment_start;

        if remaining_seconds == 0 {
            metrics.record_segment();
            scheduled_tasks.push(to_scheduled_task(
                &candidate,
                segment_start,
                segment_start,
                0,
                total_work_seconds,
            ));
        } else if candidate.atomic {
            let start = find_earliest_non_overlapping_start(
                segment_start,
                remaining_seconds,
                &occupied_slots,
                metrics,
            );
            let end = start + Duration::seconds(remaining_seconds);
            metrics.record_segment();
            scheduled_tasks.push(to_scheduled_task(
                &candidate,
                start,
                end,
                remaining_seconds,
                total_work_seconds,
            ));
            insert_occupied_slot(&mut occupied_slots, (start, end), metrics);
            candidate_scheduled_end = end;
        } else {
            while remaining_seconds > 0 {
                segment_start =
                    find_earliest_non_overlapping_start(segment_start, 0, &occupied_slots, metrics);
                let uninterrupted_end = segment_start + Duration::seconds(remaining_seconds);
                let segment_end =
                    match find_next_occupied_slot(segment_start, &occupied_slots, metrics) {
                        Some((occupied_start, _)) if occupied_start < uninterrupted_end => {
                            occupied_start
                        }
                        _ => uninterrupted_end,
                    };
                let work_seconds = (segment_end - segment_start).num_seconds();
                if work_seconds <= 0 {
                    segment_start =
                        find_occupied_slot_containing(segment_start, &occupied_slots, metrics)
                            .map(|(_, end)| end)
                            .unwrap_or(segment_start + Duration::seconds(1));
                    continue;
                }
                let after_split = remaining_seconds - work_seconds;
                if segment_end < uninterrupted_end
                    && (work_seconds <= MIN_SPLIT_SEGMENT_SECONDS
                        || after_split <= MIN_SPLIT_SEGMENT_SECONDS)
                {
                    segment_start =
                        find_occupied_slot_starting_at(segment_end, &occupied_slots, metrics)
                            .map(|(_, end)| end)
                            .unwrap_or(segment_end);
                    continue;
                }
                metrics.record_segment();
                scheduled_tasks.push(to_scheduled_task(
                    &candidate,
                    segment_start,
                    segment_end,
                    work_seconds,
                    total_work_seconds,
                ));
                insert_occupied_slot(&mut occupied_slots, (segment_start, segment_end), metrics);
                remaining_seconds -= work_seconds;
                candidate_scheduled_end = segment_end;
                segment_start = segment_end;
            }
        }
        scheduled_end_by_id.insert(candidate.id, candidate_scheduled_end);
        if let Some(dependent_indices) = dependent_indices_by_id.get(&candidate.id) {
            for dependent_index in dependent_indices {
                let unresolved_count = &mut unresolved_dependency_counts[*dependent_index];
                *unresolved_count = unresolved_count.saturating_sub(1);
                if *unresolved_count == 0 && pending_candidates[*dependent_index].is_some() {
                    ready_indices.push(Reverse(*dependent_index));
                }
            }
        }
    }

    metrics.record_sort();
    scheduled_tasks.sort_by(|a, b| {
        (
            a.scheduled_start,
            a.deadline_time.is_none(),
            a.neg_priority,
            a.rank,
            a.id,
        )
            .cmp(&(
                b.scheduled_start,
                b.deadline_time.is_none(),
                b.neg_priority,
                b.rank,
                b.id,
            ))
    });
    Ok(scheduled_tasks)
}

fn to_scheduled_task(
    candidate: &TaskScheduleCandidate,
    scheduled_start: DateTime<Local>,
    scheduled_end: DateTime<Local>,
    scheduled_work_seconds: i64,
    total_work_seconds: i64,
) -> ScheduledTask {
    ScheduledTask {
        id: candidate.id,
        task: candidate.task.clone(),
        first_available_time: candidate.first_available_time,
        scheduled_start,
        scheduled_end,
        scheduled_work_seconds,
        total_work_seconds,
        neg_priority: candidate.neg_priority,
        rank: candidate.rank,
        deadline_time: candidate.deadline_time,
    }
}

#[cfg(test)]
#[path = "schedule_use_case_tests.rs"]
mod tests;
