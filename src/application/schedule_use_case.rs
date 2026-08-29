use crate::application::daily_capacity::try_next_logical_date_start;
use crate::application::interface::TaskRepositoryTrait;
use crate::application::scheduling_metrics::ScheduleMetrics;
use crate::application::scheduling_policy::{
    schedule_tasks_by_priority_with_metrics, TaskScheduleCandidate,
};
use crate::application::task_use_case::ApplicationError;
use crate::application::task_view::TaskView;
use crate::entity::task::{
    extract_leaf_tasks_from_project_with_pending, TaskHandle, TaskTreeError,
};
use chrono::{DateTime, Duration, Local};
use serde::Serialize;
use std::cmp::max;
use std::collections::HashMap;
use uuid::Uuid;

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

pub(crate) struct ScheduleContext {
    candidates: Vec<TaskScheduleCandidate>,
    last_synced_time: DateTime<Local>,
}

struct TaskScheduleAttributes {
    first_available_time: DateTime<Local>,
    priority: i64,
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
                        priority: task.get_priority().map_err(ApplicationError::TaskTree)?,
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

    let mut candidates = Vec::new();
    for (id, attributes) in attributes {
        let first_available_time = attributes.first_available_time;
        // 候補の並べ替えには使わないが、従来どおりUUID順で日時を検証する。
        // これにより、複数候補が範囲外でも返すerrorが入力順へ依存しない。
        try_next_logical_date_start(first_available_time)?
            .checked_sub_signed(Duration::days(1))
            .ok_or(ApplicationError::LogicalDateOutOfRange {
                operation: "logical_date",
                datetime: first_available_time,
            })?;
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
            fixed_start: task.get_fixed_start().map_err(ApplicationError::TaskTree)?,
            fixed_start_time: task.get_start_time().map_err(ApplicationError::TaskTree)?,
            estimated_work_seconds: task
                .get_estimated_work_seconds()
                .map_err(ApplicationError::TaskTree)?,
            task,
            first_available_time: attributes.first_available_time,
            priority: attributes.priority,
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
