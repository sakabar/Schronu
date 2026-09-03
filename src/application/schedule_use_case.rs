use crate::application::daily_capacity::try_next_logical_date_start;
use crate::application::interface::TaskRepositoryTrait;
use crate::application::scheduling_instrumentation::{record_schedule, ScheduleEvent};
use crate::application::scheduling_policy::{
    schedule_tasks_by_priority, SchedulingPolicyError, TaskScheduleCandidate,
};
use crate::application::task_use_case::ApplicationError;
use crate::application::task_view::TaskView;
use crate::entity::task::{
    extract_leaf_tasks_from_project_with_pending, TaskHandle, TaskTreeError,
};
use chrono::{DateTime, Duration, Local};
use serde::Serialize;
use std::cmp::{max, min};
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
    let context = build_schedule_context(repository)?;
    get_schedule_from_context_with_overrides(&context, first_available_time_overrides)
}

pub(crate) fn build_schedule_context(
    repository: &dyn TaskRepositoryTrait,
) -> Result<ScheduleContext, ApplicationError> {
    for project_root in repository.get_all_projects() {
        project_root
            .snapshot()
            .map_err(ApplicationError::TaskTree)?;
    }
    Ok(ScheduleContext {
        candidates: build_schedule_candidates(repository)?,
        last_synced_time: repository.get_last_synced_time(),
    })
}

pub(crate) fn get_schedule_from_context_with_overrides(
    context: &ScheduleContext,
    first_available_time_overrides: &HashMap<Uuid, DateTime<Local>>,
) -> Result<Vec<ScheduledTaskView>, ApplicationError> {
    record_schedule(ScheduleEvent::Rebuild);
    let mut candidates = context.candidates.clone();
    for candidate in &mut candidates {
        if let Some(first_available_time) = first_available_time_overrides.get(&candidate.id) {
            candidate.first_available_time = max(*first_available_time, context.last_synced_time);
        }
    }
    schedule_tasks_by_priority(&candidates, context.last_synced_time)
        .map_err(map_scheduling_policy_error)?
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
) -> Result<Vec<TaskScheduleCandidate>, ApplicationError> {
    let last_synced_time = repository.get_last_synced_time();
    let mut task_schedule_attributes: HashMap<Uuid, TaskScheduleAttributes> = HashMap::new();
    let mut child_ids_by_parent_id: HashMap<Uuid, Vec<Uuid>> = HashMap::new();

    for project_root in repository.get_all_projects() {
        for leaf in extract_leaf_tasks_from_project_with_pending(project_root)
            .map_err(ApplicationError::TaskTree)?
        {
            let ancestors = list_ancestor_schedule_times_checked(&leaf)?;
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
    record_schedule(ScheduleEvent::Sort);
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
            remaining_seconds: calculate_remaining_work_seconds(id, &task)?,
            dependency_ids: child_ids_by_parent_id.remove(&id).unwrap_or_default(),
            atomic: task.get_atomic().map_err(ApplicationError::TaskTree)?,
            fixed_start: task
                .fixed_start_applies_to_schedule()
                .map_err(ApplicationError::TaskTree)?,
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
    record_schedule(ScheduleEvent::Candidates(candidates.len()));
    Ok(candidates)
}

/// schedule候補専用に、leafから祖先までの着手可能時刻をchecked計算する。
///
/// entityの汎用APIはfixedという配置規則を知らず、使わないpending/dependency時刻へ
/// 見積時間を加算し得る。schedule経路ではこのhelperだけを使い、fixedは指定開始を
/// 保持しつつ、flexibleには従来のdeadline補正と祖先順序をそのまま適用する。
fn list_ancestor_schedule_times_checked(
    leaf: &TaskHandle,
) -> Result<Vec<(DateTime<Local>, TaskHandle)>, ApplicationError> {
    let mut ancestors = Vec::new();
    let mut child_finish = DateTime::<Local>::MIN_UTC.with_timezone(&Local);
    let mut task = Some(leaf.clone());

    // Phase 1: 子の終了を親の開始下限にする。ただしfixedはdependencyで動かさない。
    while let Some(current) = task {
        let fixed = current
            .fixed_start_applies_to_schedule()
            .map_err(ApplicationError::TaskTree)?;
        let own_start = if fixed {
            current
                .get_start_time()
                .map_err(ApplicationError::TaskTree)?
        } else {
            current
                .first_available_time()
                .map_err(ApplicationError::TaskTree)?
        };
        let start = if fixed {
            own_start
        } else {
            max(child_finish, own_start)
        };
        child_finish = checked_candidate_end(&current, start)?;
        ancestors.push((start, current.clone()));
        task = current.parent().map_err(ApplicationError::TaskTree)?;
    }

    // Phase 2: 親側のdeadlineから必要開始時刻を子へ伝える。fixedは動かさず、
    // その指定開始をdependency側の必要時刻として伝える。
    let mut parent_required_start = DateTime::<Local>::MAX_UTC.with_timezone(&Local);
    for (rough_start, current) in ancestors.iter_mut().rev() {
        if current
            .fixed_start_applies_to_schedule()
            .map_err(ApplicationError::TaskTree)?
        {
            parent_required_start = min(parent_required_start, *rough_start);
            continue;
        }
        let mut required_start = parent_required_start;
        if let Some(deadline) = current
            .get_deadline_time_opt()
            .map_err(ApplicationError::TaskTree)?
        {
            required_start = min(required_start, deadline);
        }
        let finish = checked_candidate_end(current, *rough_start)?;
        if finish >= required_start {
            let lateness = finish.signed_duration_since(required_start);
            let adjusted_start = rough_start.checked_sub_signed(lateness);
            if let Some(adjusted_start) = adjusted_start {
                *rough_start = adjusted_start;
            } else {
                let work_seconds =
                    calculate_ancestry_work_seconds(current).map_err(ApplicationError::TaskTree)?;
                return Err(map_scheduling_policy_error(schedule_time_out_of_range(
                    current,
                    *rough_start,
                    work_seconds,
                )?));
            }
            parent_required_start = *rough_start;
        }
    }

    // Phase 3: deadline補正後もflexibleの祖先順を維持する。fixedだけは子の終了より
    // 前であっても指定開始を保持し、dependency edge自体は候補生成側へ残す。
    child_finish = DateTime::<Local>::MIN_UTC.with_timezone(&Local);
    for (start, current) in &mut ancestors {
        if current
            .fixed_start_applies_to_schedule()
            .map_err(ApplicationError::TaskTree)?
        {
            *start = current
                .get_start_time()
                .map_err(ApplicationError::TaskTree)?;
        } else {
            let own_start = current
                .first_available_time()
                .map_err(ApplicationError::TaskTree)?;
            *start = max(min(*start, own_start), child_finish);
        }
        child_finish = checked_candidate_end(current, *start)?;
    }

    Ok(ancestors)
}

fn checked_candidate_end(
    task: &TaskHandle,
    start_time: DateTime<Local>,
) -> Result<DateTime<Local>, ApplicationError> {
    let work_seconds = calculate_ancestry_work_seconds(task).map_err(ApplicationError::TaskTree)?;
    if let Some(end) = Duration::try_seconds(work_seconds)
        .and_then(|duration| start_time.checked_add_signed(duration))
    {
        Ok(end)
    } else {
        Err(map_scheduling_policy_error(schedule_time_out_of_range(
            task,
            start_time,
            work_seconds,
        )?))
    }
}

fn schedule_time_out_of_range(
    task: &TaskHandle,
    start_time: DateTime<Local>,
    work_seconds: i64,
) -> Result<SchedulingPolicyError, ApplicationError> {
    Ok(SchedulingPolicyError {
        task_id: task.get_id().map_err(ApplicationError::TaskTree)?,
        start_time,
        work_seconds,
    })
}

fn map_scheduling_policy_error(error: SchedulingPolicyError) -> ApplicationError {
    ApplicationError::ScheduleTimeOutOfRange {
        task_id: error.task_id,
        start_time: error.start_time,
        work_seconds: error.work_seconds,
    }
}

fn calculate_remaining_work_seconds(
    task_id: Uuid,
    task: &TaskHandle,
) -> Result<i64, ApplicationError> {
    let estimated_work_seconds = task
        .get_estimated_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let actual_work_seconds = task
        .get_actual_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let remaining_work_seconds = if estimated_work_seconds >= actual_work_seconds {
        estimated_work_seconds.checked_sub(actual_work_seconds)
    } else {
        estimated_work_seconds
            .checked_mul(2)
            .and_then(|doubled_estimate| doubled_estimate.checked_sub(actual_work_seconds))
    };

    remaining_work_seconds
        .map(|remaining| max(0, remaining))
        .ok_or(ApplicationError::RemainingWorkCalculationOverflow {
            task_id,
            estimated_work_seconds,
            actual_work_seconds,
        })
}

fn calculate_ancestry_work_seconds(task: &TaskHandle) -> Result<i64, TaskTreeError> {
    // 祖先時刻は置換前entity契約を維持し、見積超過済みなら追加時間を要求しない。
    // candidate自身の再見積規則とは目的が異なるため、同じ残秒helperを流用しない。
    Ok(task
        .get_estimated_work_seconds()?
        .saturating_sub(task.get_actual_work_seconds()?)
        .max(0))
}
