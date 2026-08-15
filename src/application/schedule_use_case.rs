use crate::application::interface::TaskRepositoryTrait;
use crate::application::task_use_case::ApplicationError;
use crate::application::task_view::TaskView;
use crate::entity::datetime::get_next_morning_datetime;
use crate::entity::task::{extract_leaf_tasks_from_project_with_pending, TaskHandle};
use chrono::{DateTime, Duration, Local};
use std::cmp::max;
use std::collections::HashMap;
use uuid::Uuid;

const MIN_SPLIT_SEGMENT_SECONDS: i64 = 5 * 60;

#[derive(Clone, Debug, PartialEq)]
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
    task: TaskHandle,
    first_available_time: DateTime<Local>,
    neg_priority: i64,
    rank: usize,
    deadline_time: Option<DateTime<Local>>,
    remaining_seconds: i64,
    dependency_ids: Vec<Uuid>,
    atomic: bool,
}

struct TaskScheduleAttributes {
    first_available_time: DateTime<Local>,
    neg_priority: i64,
    rank: usize,
    deadline_time: Option<DateTime<Local>>,
}

#[derive(Clone)]
struct ScheduledTask {
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
    get_schedule_with_first_available_time_overrides(repository, &HashMap::new())
}

pub(crate) fn get_schedule_with_task_first_available_time(
    repository: &dyn TaskRepositoryTrait,
    task_id: Uuid,
    first_available_time: DateTime<Local>,
) -> Result<Vec<ScheduledTaskView>, ApplicationError> {
    get_schedule_with_first_available_time_overrides(
        repository,
        &HashMap::from([(task_id, first_available_time)]),
    )
}

pub(crate) fn get_schedule_with_first_available_time_overrides(
    repository: &dyn TaskRepositoryTrait,
    first_available_time_overrides: &HashMap<Uuid, DateTime<Local>>,
) -> Result<Vec<ScheduledTaskView>, ApplicationError> {
    for project_root in repository.get_all_projects() {
        project_root
            .try_snapshot()
            .map_err(ApplicationError::TaskTree)?;
    }

    let mut candidates = build_schedule_candidates(repository);
    for candidate in &mut candidates {
        if let Some(first_available_time) =
            first_available_time_overrides.get(&candidate.task.get_id())
        {
            candidate.first_available_time =
                max(*first_available_time, repository.get_last_synced_time());
        }
    }
    schedule_tasks_by_priority(&candidates, repository.get_last_synced_time())
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

fn build_schedule_candidates(repository: &dyn TaskRepositoryTrait) -> Vec<TaskScheduleCandidate> {
    let last_synced_time = repository.get_last_synced_time();
    let mut task_schedule_attributes: HashMap<Uuid, TaskScheduleAttributes> = HashMap::new();
    let mut child_ids_by_parent_id: HashMap<Uuid, Vec<Uuid>> = HashMap::new();

    for project_root in repository.get_all_projects() {
        for leaf in extract_leaf_tasks_from_project_with_pending(project_root) {
            let ancestors = leaf.list_all_parent_tasks_with_first_available_time();
            for pair in ancestors.windows(2) {
                let child_id = pair[0].1.get_id();
                let parent_id = pair[1].1.get_id();
                let child_ids = child_ids_by_parent_id.entry(parent_id).or_default();
                if !child_ids.contains(&child_id) {
                    child_ids.push(child_id);
                }
            }

            for (rank, (first_available_time, task)) in ancestors.iter().enumerate() {
                let first_available_time = max(*first_available_time, last_synced_time);
                task_schedule_attributes
                    .entry(task.get_id())
                    .and_modify(|attributes| {
                        attributes.first_available_time =
                            max(attributes.first_available_time, first_available_time);
                        attributes.rank = max(attributes.rank, rank);
                    })
                    .or_insert(TaskScheduleAttributes {
                        first_available_time,
                        neg_priority: !task.get_priority(),
                        rank,
                        deadline_time: task.get_deadline_time_opt(),
                    });
            }
        }
    }

    let mut attributes = task_schedule_attributes.into_iter().collect::<Vec<_>>();
    attributes.sort_by_key(|(id, attributes)| {
        (
            (get_next_morning_datetime(attributes.first_available_time) - Duration::days(1))
                .date_naive(),
            attributes.deadline_time.is_none(),
            attributes.first_available_time,
            attributes.neg_priority,
            attributes.rank,
            attributes.deadline_time,
            *id,
        )
    });

    attributes
        .into_iter()
        .filter_map(|(id, attributes)| {
            repository.get_by_id(id).map(|task| TaskScheduleCandidate {
                remaining_seconds: calculate_remaining_work_seconds(&task),
                dependency_ids: child_ids_by_parent_id.remove(&id).unwrap_or_default(),
                atomic: task.get_atomic(),
                task,
                first_available_time: attributes.first_available_time,
                neg_priority: attributes.neg_priority,
                rank: attributes.rank,
                deadline_time: attributes.deadline_time,
            })
        })
        .collect()
}

fn calculate_remaining_work_seconds(task: &TaskHandle) -> i64 {
    if task.get_estimated_work_seconds() >= task.get_actual_work_seconds() {
        task.get_estimated_work_seconds() - task.get_actual_work_seconds()
    } else {
        max(
            0,
            task.get_estimated_work_seconds() * 2 - task.get_actual_work_seconds(),
        )
    }
}

fn find_earliest_non_overlapping_start(
    first_available_time: DateTime<Local>,
    remaining_seconds: i64,
    occupied_slots: &[(DateTime<Local>, DateTime<Local>)],
) -> DateTime<Local> {
    let duration = Duration::seconds(remaining_seconds);
    let mut start = first_available_time;
    loop {
        let end = start + duration;
        let mut shifted = false;
        for (occupied_start, occupied_end) in occupied_slots {
            if start < *occupied_end && *occupied_start < end {
                start = *occupied_end;
                shifted = true;
                break;
            }
        }
        if !shifted {
            return start;
        }
    }
}

fn find_next_occupied_slot(
    start: DateTime<Local>,
    occupied_slots: &[(DateTime<Local>, DateTime<Local>)],
) -> Option<(DateTime<Local>, DateTime<Local>)> {
    occupied_slots
        .iter()
        .find(|(occupied_start, occupied_end)| start < *occupied_end && start <= *occupied_start)
        .copied()
}

fn schedule_tasks_by_priority(
    candidates: &[TaskScheduleCandidate],
    last_synced_time: DateTime<Local>,
) -> Vec<ScheduledTask> {
    let mut pending_candidates = candidates.to_vec();
    pending_candidates.sort_by(|a, b| {
        (
            a.deadline_time.is_none(),
            a.deadline_time,
            a.neg_priority,
            a.first_available_time,
            a.rank,
            a.task.get_id(),
        )
            .cmp(&(
                b.deadline_time.is_none(),
                b.deadline_time,
                b.neg_priority,
                b.first_available_time,
                b.rank,
                b.task.get_id(),
            ))
    });

    let mut occupied_slots = Vec::new();
    let mut scheduled_tasks = Vec::new();
    let mut scheduled_end_by_id = HashMap::new();

    while !pending_candidates.is_empty() {
        let index = pending_candidates
            .iter()
            .position(|candidate| {
                candidate
                    .dependency_ids
                    .iter()
                    .all(|id| scheduled_end_by_id.contains_key(id))
            })
            .unwrap_or(0);
        let candidate = pending_candidates.remove(index);
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
        );
        let mut remaining_seconds = candidate.remaining_seconds;
        let total_work_seconds = remaining_seconds;
        let mut candidate_scheduled_end = segment_start;

        if remaining_seconds == 0 {
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
            );
            let end = start + Duration::seconds(remaining_seconds);
            scheduled_tasks.push(to_scheduled_task(
                &candidate,
                start,
                end,
                remaining_seconds,
                total_work_seconds,
            ));
            occupied_slots.push((start, end));
            occupied_slots.sort();
            candidate_scheduled_end = end;
        } else {
            while remaining_seconds > 0 {
                segment_start =
                    find_earliest_non_overlapping_start(segment_start, 0, &occupied_slots);
                let uninterrupted_end = segment_start + Duration::seconds(remaining_seconds);
                let segment_end = match find_next_occupied_slot(segment_start, &occupied_slots) {
                    Some((occupied_start, _)) if occupied_start < uninterrupted_end => {
                        occupied_start
                    }
                    _ => uninterrupted_end,
                };
                let work_seconds = (segment_end - segment_start).num_seconds();
                if work_seconds <= 0 {
                    segment_start = occupied_slots
                        .iter()
                        .find(|(start, end)| segment_start >= *start && segment_start < *end)
                        .map(|(_, end)| *end)
                        .unwrap_or(segment_start + Duration::seconds(1));
                    continue;
                }
                let after_split = remaining_seconds - work_seconds;
                if segment_end < uninterrupted_end
                    && (work_seconds <= MIN_SPLIT_SEGMENT_SECONDS
                        || after_split <= MIN_SPLIT_SEGMENT_SECONDS)
                {
                    segment_start = occupied_slots
                        .iter()
                        .find(|(start, _)| *start == segment_end)
                        .map(|(_, end)| *end)
                        .unwrap_or(segment_end);
                    continue;
                }
                scheduled_tasks.push(to_scheduled_task(
                    &candidate,
                    segment_start,
                    segment_end,
                    work_seconds,
                    total_work_seconds,
                ));
                occupied_slots.push((segment_start, segment_end));
                occupied_slots.sort();
                remaining_seconds -= work_seconds;
                candidate_scheduled_end = segment_end;
                segment_start = segment_end;
            }
        }
        scheduled_end_by_id.insert(candidate.task.get_id(), candidate_scheduled_end);
    }

    scheduled_tasks.sort_by(|a, b| {
        (
            a.scheduled_start,
            a.deadline_time.is_none(),
            a.neg_priority,
            a.rank,
            a.task.get_id(),
        )
            .cmp(&(
                b.scheduled_start,
                b.deadline_time.is_none(),
                b.neg_priority,
                b.rank,
                b.task.get_id(),
            ))
    });
    scheduled_tasks
}

fn to_scheduled_task(
    candidate: &TaskScheduleCandidate,
    scheduled_start: DateTime<Local>,
    scheduled_end: DateTime<Local>,
    scheduled_work_seconds: i64,
    total_work_seconds: i64,
) -> ScheduledTask {
    ScheduledTask {
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
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn candidate(
        name: &str,
        first_available_time: DateTime<Local>,
        neg_priority: i64,
        remaining_seconds: i64,
    ) -> TaskScheduleCandidate {
        TaskScheduleCandidate {
            task: TaskHandle::new(name),
            first_available_time,
            neg_priority,
            rank: 0,
            deadline_time: None,
            remaining_seconds,
            dependency_ids: vec![],
            atomic: false,
        }
    }

    #[test]
    fn schedule_tasks_by_priority_5分以下の空き時間には分割しない() {
        let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let low = candidate(
            "低優先度",
            Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
            -88,
            20 * 60,
        );
        let low_id = low.task.get_id();
        let high = candidate(
            "高優先度",
            Local.with_ymd_and_hms(2026, 5, 10, 13, 5, 0).unwrap(),
            -89,
            60 * 60,
        );

        let actual = schedule_tasks_by_priority(&[low, high], now);
        let low_segments = actual
            .iter()
            .filter(|scheduled| scheduled.task.get_id() == low_id)
            .collect::<Vec<_>>();

        assert_eq!(low_segments.len(), 1);
        assert_eq!(
            low_segments[0].scheduled_start,
            Local.with_ymd_and_hms(2026, 5, 10, 14, 5, 0).unwrap()
        );
        assert_eq!(low_segments[0].scheduled_work_seconds, 20 * 60);
    }

    #[test]
    fn schedule_tasks_by_priority_6分の空き時間には分割する() {
        let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let low = candidate(
            "低優先度",
            Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
            -88,
            20 * 60,
        );
        let low_id = low.task.get_id();
        let high = candidate(
            "高優先度",
            Local.with_ymd_and_hms(2026, 5, 10, 13, 6, 0).unwrap(),
            -89,
            60 * 60,
        );

        let actual = schedule_tasks_by_priority(&[low, high], now);
        let low_segments = actual
            .iter()
            .filter(|scheduled| scheduled.task.get_id() == low_id)
            .collect::<Vec<_>>();

        assert_eq!(low_segments.len(), 2);
        assert_eq!(
            low_segments[0].scheduled_start,
            Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap()
        );
        assert_eq!(low_segments[0].scheduled_work_seconds, 6 * 60);
        assert_eq!(
            low_segments[1].scheduled_start,
            Local.with_ymd_and_hms(2026, 5, 10, 14, 6, 0).unwrap()
        );
        assert_eq!(low_segments[1].scheduled_work_seconds, 14 * 60);
    }

    #[test]
    fn schedule_tasks_by_priority_後半が5分以下になる分割はしない() {
        let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let low = candidate(
            "低優先度",
            Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
            -88,
            20 * 60,
        );
        let low_id = low.task.get_id();
        let high = candidate(
            "高優先度",
            Local.with_ymd_and_hms(2026, 5, 10, 13, 15, 0).unwrap(),
            -89,
            60 * 60,
        );

        let actual = schedule_tasks_by_priority(&[low, high], now);
        let low_segments = actual
            .iter()
            .filter(|scheduled| scheduled.task.get_id() == low_id)
            .collect::<Vec<_>>();

        assert_eq!(low_segments.len(), 1);
        assert_eq!(
            low_segments[0].scheduled_start,
            Local.with_ymd_and_hms(2026, 5, 10, 14, 15, 0).unwrap()
        );
        assert_eq!(low_segments[0].scheduled_work_seconds, 20 * 60);
    }

    #[test]
    fn schedule_tasks_by_priority_残り5分以下のtask自体は配置する() {
        let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let blocker = candidate("blocker", now, -89, 60 * 60);
        let task = candidate(
            "5分task",
            Local.with_ymd_and_hms(2026, 5, 10, 12, 55, 0).unwrap(),
            -88,
            5 * 60,
        );
        let task_id = task.task.get_id();

        let actual = schedule_tasks_by_priority(&[blocker, task], now);
        let scheduled = actual
            .iter()
            .find(|scheduled| scheduled.task.get_id() == task_id)
            .unwrap();

        assert_eq!(
            scheduled.scheduled_start,
            Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap()
        );
        assert_eq!(scheduled.scheduled_work_seconds, 5 * 60);
    }

    #[test]
    fn schedule_tasks_by_priority_atomic_taskは依存終了後の連続枠に配置する() {
        let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let child = candidate("子", now, -99, 60 * 60);
        let child_id = child.task.get_id();
        let blocker = candidate(
            "blocker",
            Local.with_ymd_and_hms(2026, 5, 10, 13, 30, 0).unwrap(),
            -98,
            60 * 60,
        );
        let mut parent = candidate("atomic親", now, -90, 2 * 60 * 60);
        parent.rank = 1;
        parent.atomic = true;
        parent.dependency_ids = vec![child_id];
        let parent_id = parent.task.get_id();

        let actual = schedule_tasks_by_priority(&[parent, blocker, child], now);
        let scheduled = actual
            .iter()
            .find(|scheduled| scheduled.task.get_id() == parent_id)
            .unwrap();

        assert_eq!(
            scheduled.scheduled_start,
            Local.with_ymd_and_hms(2026, 5, 10, 14, 30, 0).unwrap()
        );
        assert_eq!(
            scheduled.scheduled_end,
            Local.with_ymd_and_hms(2026, 5, 10, 16, 30, 0).unwrap()
        );
    }

    #[test]
    fn schedule_tasks_by_priority_高優先度task間の隙間を優先度順に埋める() {
        let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let lunch = candidate("昼食", now, -89, 60 * 60);
        let lunch_id = lunch.task.get_id();
        let priority_88 = candidate(
            "優先度88",
            Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
            -88,
            4 * 60 * 60,
        );
        let priority_88_id = priority_88.task.get_id();
        let priority_87 = candidate(
            "優先度87",
            Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
            -87,
            60 * 60,
        );
        let priority_87_id = priority_87.task.get_id();
        let dinner = candidate(
            "夕食",
            Local.with_ymd_and_hms(2026, 5, 10, 18, 0, 0).unwrap(),
            -89,
            60 * 60,
        );
        let dinner_id = dinner.task.get_id();

        let actual = schedule_tasks_by_priority(&[priority_87, dinner, priority_88, lunch], now);
        let start = |id| {
            actual
                .iter()
                .find(|scheduled| scheduled.task.get_id() == id)
                .unwrap()
                .scheduled_start
        };

        assert_eq!(start(lunch_id), now);
        assert_eq!(
            start(priority_88_id),
            Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap()
        );
        assert_eq!(
            start(priority_87_id),
            Local.with_ymd_and_hms(2026, 5, 10, 17, 0, 0).unwrap()
        );
        assert_eq!(
            start(dinner_id),
            Local.with_ymd_and_hms(2026, 5, 10, 18, 0, 0).unwrap()
        );
    }

    #[test]
    fn schedule_tasks_by_priority_親は子の実schedule終了後に配置する() {
        let now = Local.with_ymd_and_hms(2026, 5, 10, 14, 0, 0).unwrap();
        let blocker = candidate("blocker", now, -90, 60 * 60);
        let child = candidate("子", now, -1, 60);
        let child_id = child.task.get_id();
        let mut parent = candidate("親", now, -99, 0);
        parent.rank = 1;
        parent.dependency_ids = vec![child_id];
        let parent_id = parent.task.get_id();

        let actual = schedule_tasks_by_priority(&[parent, blocker, child], now);
        let start = |id| {
            actual
                .iter()
                .find(|scheduled| scheduled.task.get_id() == id)
                .unwrap()
                .scheduled_start
        };

        assert_eq!(
            start(child_id),
            Local.with_ymd_and_hms(2026, 5, 10, 15, 0, 0).unwrap()
        );
        assert_eq!(
            start(parent_id),
            Local.with_ymd_and_hms(2026, 5, 10, 15, 1, 0).unwrap()
        );
    }
}
