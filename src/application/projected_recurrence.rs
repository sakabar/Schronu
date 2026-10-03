use crate::application::daily_capacity::try_logical_date;
use crate::application::interface::TaskRepositoryTrait;
use crate::application::scheduling_instrumentation::{record_schedule, ScheduleEvent};
use crate::application::scheduling_policy::{
    ProjectedTaskMetadata, ScheduleOccurrenceKey, TaskScheduleCandidate,
};
use crate::application::task_use_case::{next_repetition_occurrence_times, ApplicationError};
use crate::entity::task::TaskHandle;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate};
use std::cmp::max;
use std::collections::HashSet;
use uuid::Uuid;

struct ProjectedInternalIdAllocator {
    used: HashSet<Uuid>,
    next: Option<u128>,
}

impl ProjectedInternalIdAllocator {
    fn new(used: HashSet<Uuid>) -> Self {
        Self {
            used,
            // nil UUIDはTaskHandleのdummy rootが使うため、内部task IDにも割り当てない。
            next: Some(1),
        }
    }

    fn allocate(&mut self) -> Result<Uuid, ApplicationError> {
        loop {
            let value = self.next.ok_or(ApplicationError::InvalidInput {
                field: "projected_occurrence",
                reason: "internal schedule identity space is exhausted",
            })?;
            self.next = value.checked_add(1);
            let candidate = Uuid::from_u128(value);
            if self.used.insert(candidate) {
                return Ok(candidate);
            }
        }
    }
}

pub(super) fn append_projected_repetition_candidates(
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
    let mut roots = repository.get_all_projects();
    let mut persisted_ids = HashSet::new();
    for root in &roots {
        collect_task_ids(root, &mut persisted_ids)?;
    }
    let mut id_allocator = ProjectedInternalIdAllocator::new(persisted_ids);
    roots.sort_by_key(|root| root.get_id().unwrap_or(Uuid::nil()));
    for root in roots {
        append_projected_repetition_candidates_from_task(
            root,
            last_synced_time,
            horizon_start,
            horizon_end,
            &mut id_allocator,
            candidates,
        )?;
    }
    Ok(())
}

fn collect_task_ids(task: &TaskHandle, ids: &mut HashSet<Uuid>) -> Result<(), ApplicationError> {
    ids.insert(task.get_id().map_err(ApplicationError::TaskTree)?);
    for child in task.get_children().map_err(ApplicationError::TaskTree)? {
        collect_task_ids(&child, ids)?;
    }
    Ok(())
}

fn append_projected_repetition_candidates_from_task(
    task: &TaskHandle,
    last_synced_time: DateTime<Local>,
    horizon_start: NaiveDate,
    horizon_end: NaiveDate,
    id_allocator: &mut ProjectedInternalIdAllocator,
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
            id_allocator,
            candidates,
        )?;
    }
    for child in task.get_children().map_err(ApplicationError::TaskTree)? {
        append_projected_repetition_candidates_from_task(
            &child,
            last_synced_time,
            horizon_start,
            horizon_end,
            id_allocator,
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
    id_allocator: &mut ProjectedInternalIdAllocator,
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

    let next_occurrence = |anchor, interval_days| {
        record_schedule(ScheduleEvent::ProjectionStep);
        next_repetition_occurrence_times(
            anchor,
            interval_days,
            repetition_start_time,
            repetition_deadline_time,
            days_in_advance,
        )
    };
    let mut occurrence = next_occurrence(anchor, interval_days)?;
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
        occurrence = next_occurrence(anchor, jump_days)?;
    }

    loop {
        anchor = occurrence.deadline_time;
        let occurrence_date = try_logical_date(occurrence.deadline_time)?;
        if occurrence_date >= horizon_end {
            break;
        }

        let projected_internal_id = id_allocator.allocate()?;
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
        occurrence = next_occurrence(anchor, interval_days)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projected_internal_id_allocatorは実在idを避け異なる回へ決定的に割り当てる() {
        let reserved = HashSet::from([Uuid::from_u128(1), Uuid::from_u128(3)]);
        let allocate_three = || {
            let mut allocator = ProjectedInternalIdAllocator::new(reserved.clone());
            (0..3)
                .map(|_| allocator.allocate().unwrap())
                .collect::<Vec<_>>()
        };

        let first = allocate_three();
        let second = allocate_three();

        assert_eq!(first, second);
        assert_eq!(
            first,
            [Uuid::from_u128(2), Uuid::from_u128(4), Uuid::from_u128(5)]
        );
        assert_eq!(first.iter().copied().collect::<HashSet<_>>().len(), 3);
        assert!(first.iter().all(|id| !reserved.contains(id)));
    }
}
