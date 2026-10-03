use crate::application::daily_capacity::{try_logical_date, try_next_logical_date_start};
use crate::application::interface::TaskRepositoryTrait;
use crate::application::scheduling_instrumentation::{record_schedule, ScheduleEvent};
pub use crate::application::scheduling_policy::ScheduleOccurrenceKey;
use crate::application::scheduling_policy::{
    schedule_tasks_by_priority, ProjectedTaskMetadata, SchedulingPolicyError, TaskScheduleCandidate,
};
use crate::application::task_use_case::{next_repetition_occurrence_times, ApplicationError};
use crate::application::task_view::TaskView;
use crate::entity::task::{
    extract_leaf_tasks_from_project_with_pending, ProjectCategory, RepetitionAnchor, Status,
    TaskHandle, TaskTreeError,
};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime};
use serde::Serialize;
use std::cmp::{max, min};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScheduledTaskView {
    pub occurrence: ScheduleOccurrenceKey,
    pub task: ScheduledTaskPayload,
    pub first_available_time: DateTime<Local>,
    pub scheduled_start: DateTime<Local>,
    pub scheduled_end: DateTime<Local>,
    pub scheduled_work_seconds: i64,
    pub total_work_seconds: i64,
    pub rank: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScheduledTaskPayload {
    pub name: String,
    pub status: Status,
    pub original_status: Status,
    pub is_on_other_side: bool,
    pub atomic: bool,
    pub fixed_start: bool,
    pub pending_until: Option<DateTime<Local>>,
    pub priority: i64,
    pub create_time: DateTime<Local>,
    pub start_time: DateTime<Local>,
    pub end_time: Option<DateTime<Local>>,
    pub deadline_time: Option<DateTime<Local>>,
    pub estimated_work_seconds: i64,
    pub actual_work_seconds: i64,
    pub repetition_interval_days: Option<i64>,
    pub repetition_start_time: Option<NaiveTime>,
    pub repetition_deadline_time: Option<NaiveTime>,
    pub repetition_anchor: RepetitionAnchor,
    pub days_in_advance: i64,
    pub project_category: Option<ProjectCategory>,
}

impl From<TaskView> for ScheduledTaskPayload {
    fn from(task: TaskView) -> Self {
        Self {
            name: task.name,
            status: task.status,
            original_status: task.original_status,
            is_on_other_side: task.is_on_other_side,
            atomic: task.atomic,
            fixed_start: task.fixed_start,
            pending_until: task.pending_until,
            priority: task.priority,
            create_time: task.create_time,
            start_time: task.start_time,
            end_time: task.end_time,
            deadline_time: task.deadline_time,
            estimated_work_seconds: task.estimated_work_seconds,
            actual_work_seconds: task.actual_work_seconds,
            repetition_interval_days: task.repetition_interval_days,
            repetition_start_time: task.repetition_start_time,
            repetition_deadline_time: task.repetition_deadline_time,
            repetition_anchor: task.repetition_anchor,
            days_in_advance: task.days_in_advance,
            project_category: task.project_category,
        }
    }
}

impl ScheduledTaskView {
    pub(crate) fn is_leaf(&self) -> bool {
        self.rank == 0
    }

    pub fn actual_task_id(&self) -> Option<Uuid> {
        match self.occurrence {
            ScheduleOccurrenceKey::Actual { task_id } => Some(task_id),
            ScheduleOccurrenceKey::Projected { .. } => None,
        }
    }

    pub fn source_task_id(&self) -> Uuid {
        match self.occurrence {
            ScheduleOccurrenceKey::Actual { task_id } => task_id,
            ScheduleOccurrenceKey::Projected { source_task_id, .. } => source_task_id,
        }
    }

    pub fn is_projected(&self) -> bool {
        matches!(self.occurrence, ScheduleOccurrenceKey::Projected { .. })
    }
}

pub(crate) fn scheduled_logical_dates(
    schedule: &[ScheduledTaskView],
) -> Result<Vec<NaiveDate>, ApplicationError> {
    schedule
        .iter()
        .map(|segment| try_logical_date(segment.scheduled_start))
        .collect()
}

pub(crate) fn scheduled_end_by_task(
    schedule: &[ScheduledTaskView],
) -> HashMap<Uuid, DateTime<Local>> {
    let mut ends = HashMap::<Uuid, DateTime<Local>>::new();
    for scheduled in schedule {
        let Some(task_id) = scheduled.actual_task_id() else {
            continue;
        };
        ends.entry(task_id)
            .and_modify(|end| *end = (*end).max(scheduled.scheduled_end))
            .or_insert(scheduled.scheduled_end);
    }
    ends
}

pub(crate) fn scheduled_end_by_occurrence(
    schedule: &[ScheduledTaskView],
) -> HashMap<ScheduleOccurrenceKey, DateTime<Local>> {
    let mut ends = HashMap::new();
    for scheduled in schedule {
        ends.entry(scheduled.occurrence)
            .and_modify(|end: &mut DateTime<Local>| *end = (*end).max(scheduled.scheduled_end))
            .or_insert(scheduled.scheduled_end);
    }
    ends
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
        if let ScheduleOccurrenceKey::Actual { task_id } = candidate.occurrence {
            if let Some(first_available_time) = first_available_time_overrides.get(&task_id) {
                candidate.first_available_time =
                    max(*first_available_time, context.last_synced_time);
            }
        }
    }
    schedule_tasks_by_priority(&candidates, context.last_synced_time)
        .map_err(map_scheduling_policy_error)?
        .into_iter()
        .map(|scheduled| {
            let mut task =
                TaskView::try_from(&scheduled.task).map_err(ApplicationError::TaskTree)?;
            if let Some(metadata) = scheduled.projected_metadata {
                task.repetition_interval_days = Some(metadata.repetition_interval_days);
                task.repetition_start_time = Some(metadata.repetition_start_time);
                task.repetition_deadline_time = Some(metadata.repetition_deadline_time);
                task.repetition_anchor = metadata.repetition_anchor;
                task.days_in_advance = metadata.days_in_advance;
            }
            Ok(ScheduledTaskView {
                occurrence: scheduled.occurrence,
                task: task.into(),
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
    let mut subtree_contains_repeating_task_by_id = HashMap::new();

    for project_root in repository.get_all_projects() {
        for leaf in extract_leaf_tasks_from_project_with_pending(project_root)
            .map_err(ApplicationError::TaskTree)?
        {
            let ancestors = list_ancestor_schedule_times_checked(
                &leaf,
                &mut subtree_contains_repeating_task_by_id,
            )?;
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
            occurrence: ScheduleOccurrenceKey::Actual { task_id: id },
            projected_metadata: None,
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
    append_projected_repetition_candidates(repository, &mut candidates)?;
    record_schedule(ScheduleEvent::Candidates(candidates.len()));
    Ok(candidates)
}

fn append_projected_repetition_candidates(
    repository: &dyn TaskRepositoryTrait,
    candidates: &mut Vec<TaskScheduleCandidate>,
) -> Result<(), ApplicationError> {
    let last_synced_time = repository.get_last_synced_time();
    let horizon_start = try_logical_date(last_synced_time)?;
    let horizon_end = horizon_start.checked_add_signed(Duration::days(28)).ok_or(
        ApplicationError::LogicalDateOutOfRange {
            operation: "repetition_projection_horizon",
            datetime: last_synced_time,
        },
    )?;
    for root in repository.get_all_projects() {
        append_projected_repetition_candidates_from_task(
            root,
            last_synced_time,
            horizon_start,
            horizon_end,
            candidates,
        )?;
    }
    Ok(())
}

fn append_projected_repetition_candidates_from_task(
    task: &TaskHandle,
    last_synced_time: DateTime<Local>,
    horizon_start: NaiveDate,
    horizon_end: NaiveDate,
    candidates: &mut Vec<TaskScheduleCandidate>,
) -> Result<(), ApplicationError> {
    if let Some(interval_days) = task
        .get_repetition_interval_days_opt()
        .map_err(ApplicationError::TaskTree)?
    {
        append_projected_occurrences(
            task,
            interval_days,
            last_synced_time,
            horizon_start,
            horizon_end,
            candidates,
        )?;
    }
    for child in task.get_children().map_err(ApplicationError::TaskTree)? {
        append_projected_repetition_candidates_from_task(
            &child,
            last_synced_time,
            horizon_start,
            horizon_end,
            candidates,
        )?;
    }
    Ok(())
}

fn append_projected_occurrences(
    parent: &TaskHandle,
    interval_days: i64,
    last_synced_time: DateTime<Local>,
    horizon_start: NaiveDate,
    horizon_end: NaiveDate,
    candidates: &mut Vec<TaskScheduleCandidate>,
) -> Result<(), ApplicationError> {
    if interval_days <= 0 {
        return Err(ApplicationError::InvalidInput {
            field: "repetition_interval_days",
            reason: "must be a positive integer",
        });
    }
    let mut persisted_occurrences = parent
        .get_children()
        .map_err(ApplicationError::TaskTree)?
        .into_iter()
        .filter_map(|child| {
            child
                .get_deadline_time_opt()
                .map(|deadline| deadline.map(|deadline| (deadline, child)))
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApplicationError::TaskTree)?;
    persisted_occurrences
        .sort_by_key(|(deadline, child)| (*deadline, child.get_id().unwrap_or(Uuid::nil())));
    let Some((mut anchor, _frontier)) = persisted_occurrences.pop() else {
        return Ok(());
    };

    let repetition_start_time = parent
        .get_repetition_start_time_opt()
        .map_err(ApplicationError::TaskTree)?
        .ok_or(ApplicationError::InvalidInput {
            field: "repetition_start_time",
            reason: "repeating task must have a start time template",
        })?;
    let repetition_deadline_time = parent
        .get_repetition_deadline_time_opt()
        .map_err(ApplicationError::TaskTree)?
        .ok_or(ApplicationError::InvalidInput {
            field: "repetition_deadline_time",
            reason: "repeating task must have a deadline time template",
        })?;
    let days_in_advance = parent
        .get_days_in_advance()
        .map_err(ApplicationError::TaskTree)?;
    let source_task_id = parent.get_id().map_err(ApplicationError::TaskTree)?;
    let name = parent.get_name().map_err(ApplicationError::TaskTree)?;
    let estimate = parent
        .get_estimated_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let priority = parent.get_priority().map_err(ApplicationError::TaskTree)?;
    let atomic = parent.get_atomic().map_err(ApplicationError::TaskTree)?;
    let fixed_start = parent
        .get_fixed_start()
        .map_err(ApplicationError::TaskTree)?;
    let category = parent
        .get_project_category_opt()
        .map_err(ApplicationError::TaskTree)?;
    let repetition_anchor = parent
        .get_repetition_anchor()
        .map_err(ApplicationError::TaskTree)?;

    let mut occurrence = next_repetition_occurrence_times(
        anchor,
        interval_days,
        repetition_start_time,
        repetition_deadline_time,
        days_in_advance,
    )?;
    let first_occurrence_date = try_logical_date(occurrence.deadline_time)?;
    if first_occurrence_date < horizon_start {
        let projection_range_error = || ApplicationError::LogicalDateOutOfRange {
            operation: "repetition_projection_horizon",
            datetime: anchor,
        };
        let distance_days = (horizon_start - first_occurrence_date).num_days();
        let skipped_intervals = distance_days
            .checked_add(
                interval_days
                    .checked_sub(1)
                    .ok_or_else(projection_range_error)?,
            )
            .and_then(|days| days.checked_div(interval_days))
            .ok_or_else(projection_range_error)?;
        let interval_count =
            skipped_intervals
                .checked_add(1)
                .ok_or(ApplicationError::LogicalDateOutOfRange {
                    operation: "repetition_projection_horizon",
                    datetime: anchor,
                })?;
        let jump_days = interval_days.checked_mul(interval_count).ok_or(
            ApplicationError::LogicalDateOutOfRange {
                operation: "repetition_projection_horizon",
                datetime: anchor,
            },
        )?;
        occurrence = next_repetition_occurrence_times(
            anchor,
            jump_days,
            repetition_start_time,
            repetition_deadline_time,
            days_in_advance,
        )?;
    }

    loop {
        anchor = occurrence.deadline_time;
        let occurrence_date = try_logical_date(occurrence.deadline_time)?;
        if occurrence_date >= horizon_end {
            break;
        }

        let projected_internal_id = projected_internal_id(source_task_id, occurrence.deadline_time);
        let projected = TaskHandle::with_identity(
            &format!(
                "{}({}/{})",
                name,
                occurrence.deadline_time.month(),
                occurrence.deadline_time.day()
            ),
            projected_internal_id,
            last_synced_time,
        )
        .map_err(ApplicationError::TaskTree)?;
        projected
            .set_start_time(occurrence.start_time)
            .map_err(ApplicationError::TaskTree)?;
        projected
            .set_deadline_time_opt(Some(occurrence.deadline_time))
            .map_err(ApplicationError::TaskTree)?;
        projected
            .set_estimated_work_seconds(estimate)
            .map_err(ApplicationError::TaskTree)?;
        projected
            .set_priority(priority)
            .map_err(ApplicationError::TaskTree)?;
        projected
            .set_atomic(atomic)
            .map_err(ApplicationError::TaskTree)?;
        projected
            .set_fixed_start(fixed_start)
            .map_err(ApplicationError::TaskTree)?;
        projected
            .set_project_category_opt(category)
            .map_err(ApplicationError::TaskTree)?;
        projected
            .sync_clock(last_synced_time)
            .map_err(ApplicationError::TaskTree)?;
        candidates.push(TaskScheduleCandidate {
            id: projected.get_id().map_err(ApplicationError::TaskTree)?,
            occurrence: ScheduleOccurrenceKey::Projected {
                source_task_id,
                deadline: occurrence.deadline_time,
            },
            projected_metadata: Some(ProjectedTaskMetadata {
                repetition_interval_days: interval_days,
                repetition_start_time,
                repetition_deadline_time,
                repetition_anchor,
                days_in_advance,
            }),
            task: projected,
            first_available_time: max(occurrence.start_time, last_synced_time),
            priority,
            rank: 0,
            deadline_time: Some(occurrence.deadline_time),
            remaining_seconds: estimate,
            dependency_ids: Vec::new(),
            atomic,
            fixed_start,
            fixed_start_time: occurrence.start_time,
            estimated_work_seconds: estimate,
        });
        occurrence = next_repetition_occurrence_times(
            anchor,
            interval_days,
            repetition_start_time,
            repetition_deadline_time,
            days_in_advance,
        )?;
    }
    Ok(())
}

fn projected_internal_id(source_task_id: Uuid, deadline: DateTime<Local>) -> Uuid {
    let deadline_bits = deadline.timestamp() as i128 as u128;
    Uuid::from_u128(source_task_id.as_u128().rotate_left(64) ^ deadline_bits)
}

/// schedule候補専用に、leafから祖先までの着手可能時刻をchecked計算する。
///
/// entityの汎用APIはfixedという配置規則を知らず、使わないpending/dependency時刻へ
/// 見積時間を加算し得る。schedule経路ではこのhelperだけを使い、fixedは指定開始を
/// 保持しつつ、flexibleには従来のdeadline補正と祖先順序をそのまま適用する。
fn list_ancestor_schedule_times_checked(
    leaf: &TaskHandle,
    subtree_contains_repeating_task_by_id: &mut HashMap<Uuid, bool>,
) -> Result<Vec<(DateTime<Local>, TaskHandle)>, ApplicationError> {
    let mut ancestors = Vec::new();
    let mut child_finish = DateTime::<Local>::MIN_UTC.with_timezone(&Local);
    let mut task = Some(leaf.clone());
    let mut boundary_deadline = None;

    // Phase 1: 子の終了を親の開始下限にする。ただしfixedはdependencyで動かさない。
    // 部分木に繰り返しtaskを含むtaskは完了不能なので、そのtaskから上を予定しない。
    while let Some(current) = task {
        if subtree_contains_repeating_task(&current, subtree_contains_repeating_task_by_id)? {
            boundary_deadline = minimum_deadline_from(&current)?;
            break;
        }
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

    // Phase 2: 親側のdeadlineから必要開始時刻を子へ伝える。繰り返し境界より上も
    // deadlineだけは維持する。fixedは動かさず、その指定開始をdependency側の
    // 必要時刻として伝える。
    let mut parent_required_start =
        boundary_deadline.unwrap_or_else(|| DateTime::<Local>::MAX_UTC.with_timezone(&Local));
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

fn minimum_deadline_from(task: &TaskHandle) -> Result<Option<DateTime<Local>>, ApplicationError> {
    let mut deadline = None;
    let mut current = Some(task.clone());
    while let Some(task) = current {
        if let Some(current_deadline) = task
            .get_deadline_time_opt()
            .map_err(ApplicationError::TaskTree)?
        {
            deadline = Some(
                deadline
                    .map(|deadline| min(deadline, current_deadline))
                    .unwrap_or(current_deadline),
            );
        }
        current = task.parent().map_err(ApplicationError::TaskTree)?;
    }
    Ok(deadline)
}

fn subtree_contains_repeating_task(
    task: &TaskHandle,
    cache: &mut HashMap<Uuid, bool>,
) -> Result<bool, ApplicationError> {
    let id = task.get_id().map_err(ApplicationError::TaskTree)?;
    if let Some(contains_repeating_task) = cache.get(&id) {
        return Ok(*contains_repeating_task);
    }

    let mut contains_repeating_task = !task
        .is_schedulable_work()
        .map_err(ApplicationError::TaskTree)?;
    for child in task.get_children().map_err(ApplicationError::TaskTree)? {
        contains_repeating_task |= subtree_contains_repeating_task(&child, cache)?;
    }
    cache.insert(id, contains_repeating_task);
    Ok(contains_repeating_task)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::new_task_handle;
    use chrono::TimeZone;

    #[test]
    fn scheduled_end_by_taskは分割taskごとの最終終了時刻を返す() {
        let start = Local.with_ymd_and_hms(2026, 9, 20, 9, 0, 0).unwrap();
        let first_task = new_task_handle("first").unwrap();
        let second_task = new_task_handle("second").unwrap();
        let first_task_id = first_task.get_id().unwrap();
        let second_task_id = second_task.get_id().unwrap();
        let scheduled = |task: &TaskHandle, scheduled_end| ScheduledTaskView {
            occurrence: ScheduleOccurrenceKey::Actual {
                task_id: task.get_id().unwrap(),
            },
            task: TaskView::try_from(task).unwrap().into(),
            first_available_time: start,
            scheduled_start: start,
            scheduled_end,
            scheduled_work_seconds: 60,
            total_work_seconds: 120,
            rank: 0,
        };
        let schedule = vec![
            scheduled(&first_task, start + Duration::minutes(2)),
            scheduled(&second_task, start + Duration::minutes(3)),
            scheduled(&first_task, start + Duration::minutes(1)),
        ];

        let ends = scheduled_end_by_task(&schedule);

        assert_eq!(ends.len(), 2);
        assert_eq!(
            ends.get(&first_task_id),
            Some(&(start + Duration::minutes(2)))
        );
        assert_eq!(
            ends.get(&second_task_id),
            Some(&(start + Duration::minutes(3)))
        );
    }

    #[test]
    fn scheduled_end_by_occurrenceはprojectedの内部identityではなくsemantic_keyで集約する() {
        let start = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
        let source_task_id = Uuid::new_v4();
        let occurrence = ScheduleOccurrenceKey::Projected {
            source_task_id,
            deadline: start + Duration::days(3),
        };
        let first_task = crate::test_support::new_task_handle("first internal").unwrap();
        let second_task = crate::test_support::new_task_handle("second internal").unwrap();
        let segment = |task: &TaskHandle, scheduled_end| ScheduledTaskView {
            occurrence,
            task: TaskView::try_from(task).unwrap().into(),
            first_available_time: start,
            scheduled_start: start,
            scheduled_end,
            scheduled_work_seconds: 60,
            total_work_seconds: 120,
            rank: 0,
        };
        let schedule = vec![
            segment(&first_task, start + Duration::minutes(1)),
            segment(&second_task, start + Duration::minutes(2)),
        ];

        let ends = scheduled_end_by_occurrence(&schedule);

        assert_eq!(ends.len(), 1);
        assert_eq!(ends.get(&occurrence), Some(&(start + Duration::minutes(2))));
        assert!(scheduled_end_by_task(&schedule).is_empty());
    }
}
