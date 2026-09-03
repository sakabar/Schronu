use crate::application::daily_capacity::{try_local_date_and_time, try_next_logical_date_start};
use crate::application::interface::{ProjectRegistrationError, TaskRepositoryTrait};
use crate::application::schedule_use_case::get_schedule;
pub use crate::application::task_view::TaskView;
use crate::entity::task::{
    ProjectCategory, RepetitionAnchor, Status, TaskAttr, TaskHandle, TaskTreeError,
};
use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Timelike,
};
use std::cmp::{max, Ordering};
use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq)]
pub enum ApplicationError {
    TaskNotFound(Uuid),
    InvalidInput {
        field: &'static str,
        reason: &'static str,
    },
    HasUndoneChildren(Uuid),
    TaskTree(TaskTreeError),
    ProjectRegistration(ProjectRegistrationError),
    AmbiguousLocalDateTime {
        local_datetime: NaiveDateTime,
        earlier: DateTime<Local>,
        later: DateTime<Local>,
    },
    NonexistentLocalDateTime {
        local_datetime: NaiveDateTime,
    },
    LogicalDateOutOfRange {
        operation: &'static str,
        datetime: DateTime<Local>,
    },
    LogicalDateStartOutOfRange {
        date: NaiveDate,
    },
    LogicalDateEndOutOfRange {
        date: NaiveDate,
        end_of_day_offset_minutes: i64,
    },
    ScheduleTimeOutOfRange {
        task_id: Uuid,
        start_time: DateTime<Local>,
        work_seconds: i64,
    },
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TaskNotFound(task_id) => write!(formatter, "task not found: {task_id}"),
            Self::InvalidInput { field, reason } => {
                write!(formatter, "invalid input for {field}: {reason}")
            }
            Self::HasUndoneChildren(task_id) => {
                write!(formatter, "task has undone children: {task_id}")
            }
            Self::TaskTree(error) => write!(formatter, "task tree operation failed: {error}"),
            Self::ProjectRegistration(error) => error.fmt(formatter),
            Self::AmbiguousLocalDateTime {
                local_datetime,
                earlier,
                later,
            } => write!(
                formatter,
                "ambiguous local datetime {local_datetime}: {earlier} or {later}"
            ),
            Self::NonexistentLocalDateTime { local_datetime } => {
                write!(formatter, "nonexistent local datetime: {local_datetime}")
            }
            Self::LogicalDateOutOfRange {
                operation,
                datetime,
            } => write!(
                formatter,
                "logical date operation {operation} is outside the supported range: {datetime}"
            ),
            Self::LogicalDateStartOutOfRange { date } => write!(
                formatter,
                "logical date start is outside the supported range: date={date}"
            ),
            Self::LogicalDateEndOutOfRange {
                date,
                end_of_day_offset_minutes,
            } => write!(
                formatter,
                "logical date end is outside the supported range: date={date}, end_of_day_offset_minutes={end_of_day_offset_minutes}"
            ),
            Self::ScheduleTimeOutOfRange {
                task_id,
                start_time,
                work_seconds,
            } => write!(
                formatter,
                "schedule end is outside the supported range: task_id={task_id}, start_time={start_time}, work_seconds={work_seconds}"
            ),
        }
    }
}

impl Error for ApplicationError {}

pub(crate) fn resolve_local_datetime(
    local_datetime: NaiveDateTime,
    result: LocalResult<DateTime<Local>>,
) -> Result<DateTime<Local>, ApplicationError> {
    match result {
        LocalResult::Single(datetime) => Ok(datetime),
        LocalResult::Ambiguous(earlier, later) => Err(ApplicationError::AmbiguousLocalDateTime {
            local_datetime,
            earlier,
            later,
        }),
        LocalResult::None => Err(ApplicationError::NonexistentLocalDateTime { local_datetime }),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreateTaskInput {
    pub name: String,
    pub estimated_work_minutes: Option<i64>,
    pub pending_until: Option<DateTime<Local>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BreakdownTaskInput {
    pub parent_id: Uuid,
    pub names: Vec<String>,
    pub pending_until: Option<DateTime<Local>>,
}

pub struct TaskFactory<'a> {
    now: DateTime<Local>,
    next_id: &'a mut dyn FnMut() -> Uuid,
}

impl<'a> TaskFactory<'a> {
    pub fn new(now: DateTime<Local>, next_id: &'a mut dyn FnMut() -> Uuid) -> Self {
        Self { now, next_id }
    }

    pub fn create_task_attr(&mut self, name: &str) -> TaskAttr {
        TaskAttr::with_identity(name, (self.next_id)(), self.now)
    }

    pub fn create_root_task(&mut self, name: &str) -> Result<TaskHandle, TaskTreeError> {
        TaskHandle::with_identity(name, (self.next_id)(), self.now)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompleteTaskInput {
    pub task_id: Uuid,
    pub finished_at: DateTime<Local>,
    pub additional_actual_work_seconds: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompleteTaskOutput {
    pub next_focus_task_id: Option<Uuid>,
    pub next_repetition_task_id: Option<Uuid>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskPeriodField {
    ScheduledStart,
    CreatedAt,
    Deadline,
    CompletedAt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskPeriodFilter {
    pub field: TaskPeriodField,
    pub from: DateTime<Local>,
    pub until: DateTime<Local>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListTasksFilter {
    pub period: Option<TaskPeriodFilter>,
    pub statuses: Vec<Status>,
    pub categories: Vec<Option<ProjectCategory>>,
}

pub fn get_focus(
    repository: &mut dyn TaskRepositoryTrait,
) -> Result<Option<TaskView>, ApplicationError> {
    get_focus_excluding(repository, &[])
}

pub fn get_focus_excluding(
    repository: &mut dyn TaskRepositoryTrait,
    excluded_task_ids: &[Uuid],
) -> Result<Option<TaskView>, ApplicationError> {
    repository
        .get_highest_priority_leaf_task_id(excluded_task_ids)
        .map_err(ApplicationError::TaskTree)?
        .map_or(Ok(None), |task_id| get_task(repository, task_id))
}

pub fn get_task(
    repository: &dyn TaskRepositoryTrait,
    task_id: Uuid,
) -> Result<Option<TaskView>, ApplicationError> {
    repository
        .get_by_id(task_id)
        .map_err(ApplicationError::TaskTree)?
        .as_ref()
        .map(|task| TaskView::try_from(task).map_err(ApplicationError::TaskTree))
        .transpose()
}

pub fn list_tasks(
    repository: &dyn TaskRepositoryTrait,
    filter: ListTasksFilter,
) -> Result<Vec<TaskView>, ApplicationError> {
    if filter
        .period
        .as_ref()
        .is_some_and(|period| period.from >= period.until)
    {
        return Err(ApplicationError::InvalidInput {
            field: "period",
            reason: "from must be earlier than until",
        });
    }

    let scheduled_task_ids = filter
        .period
        .as_ref()
        .filter(|period| period.field == TaskPeriodField::ScheduledStart)
        .map(|period| {
            Ok(get_schedule(repository)?
                .into_iter()
                .filter(|entry| {
                    period.from <= entry.scheduled_start && entry.scheduled_start < period.until
                })
                .map(|entry| entry.task.id)
                .collect::<HashSet<_>>())
        })
        .transpose()?;

    let mut tasks = Vec::new();
    for root in repository.get_all_projects() {
        collect_tasks_pre_order(root, &mut tasks).map_err(ApplicationError::TaskTree)?;
    }

    Ok(tasks
        .into_iter()
        .map(|task| TaskView::try_from(&task).map_err(ApplicationError::TaskTree))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|task| filter.statuses.is_empty() || filter.statuses.contains(&task.status))
        .filter(|task| {
            filter.categories.is_empty() || filter.categories.contains(&task.project_category)
        })
        .filter(|task| {
            filter
                .period
                .as_ref()
                .is_none_or(|period| match period.field {
                    TaskPeriodField::ScheduledStart => scheduled_task_ids
                        .as_ref()
                        .is_some_and(|task_ids| task_ids.contains(&task.id)),
                    TaskPeriodField::CreatedAt => {
                        is_in_period(task.create_time, period.from, period.until)
                    }
                    TaskPeriodField::Deadline => task
                        .deadline_time
                        .is_some_and(|time| is_in_period(time, period.from, period.until)),
                    TaskPeriodField::CompletedAt => task
                        .end_time
                        .is_some_and(|time| is_in_period(time, period.from, period.until)),
                })
        })
        .collect::<Vec<_>>())
}

fn collect_tasks_pre_order(
    task: &TaskHandle,
    tasks: &mut Vec<TaskHandle>,
) -> Result<(), TaskTreeError> {
    tasks.push(task.clone());
    for child in task.get_children()? {
        collect_tasks_pre_order(&child, tasks)?;
    }
    Ok(())
}

fn is_in_period(time: DateTime<Local>, from: DateTime<Local>, until: DateTime<Local>) -> bool {
    from <= time && time < until
}

pub fn create_task(
    repository: &mut dyn TaskRepositoryTrait,
    input: CreateTaskInput,
    factory: &mut TaskFactory<'_>,
) -> Result<Uuid, ApplicationError> {
    validate_task_name(&input.name, "name")?;

    let root_task = factory
        .create_root_task(&input.name)
        .map_err(ApplicationError::TaskTree)?;
    root_task
        .set_priority(5)
        .map_err(ApplicationError::TaskTree)?;

    if let Some(pending_until) = input.pending_until {
        root_task
            .set_pending_until(pending_until)
            .map_err(ApplicationError::TaskTree)?;
        root_task
            .set_orig_status(Status::Pending)
            .map_err(ApplicationError::TaskTree)?;
    }

    if let Some(estimated_work_minutes) = input.estimated_work_minutes {
        root_task
            .set_estimated_work_seconds(estimated_work_seconds_from_minutes(
                estimated_work_minutes,
            )?)
            .map_err(ApplicationError::TaskTree)?;
    }

    let task_id = root_task.get_id().map_err(ApplicationError::TaskTree)?;
    repository
        .start_new_project(root_task)
        .map_err(ApplicationError::ProjectRegistration)?;
    Ok(task_id)
}

pub fn breakdown_task(
    repository: &mut dyn TaskRepositoryTrait,
    input: BreakdownTaskInput,
    factory: &mut TaskFactory<'_>,
) -> Result<Vec<Uuid>, ApplicationError> {
    if input.names.is_empty() {
        return Err(ApplicationError::InvalidInput {
            field: "names",
            reason: "must not be empty",
        });
    }
    for name in &input.names {
        validate_task_name(name, "names")?;
    }

    let parent_task = find_task(repository, input.parent_id)?;
    let mut child_ids = Vec::with_capacity(input.names.len());

    for name in input.names {
        let mut child_attr = factory.create_task_attr(&name);
        if let Some(pending_until) = input.pending_until {
            child_attr.set_orig_status(Status::Pending);
            child_attr.set_pending_until(pending_until);
        }

        let child_task = parent_task
            .create_child(child_attr)
            .map_err(ApplicationError::TaskTree)?;
        if let Some(deadline_time) = parent_task
            .get_deadline_time_opt()
            .map_err(ApplicationError::TaskTree)?
        {
            child_task
                .set_deadline_time_opt(Some(deadline_time))
                .map_err(ApplicationError::TaskTree)?;
        }
        child_ids.push(child_task.get_id().map_err(ApplicationError::TaskTree)?);
    }

    Ok(child_ids)
}

pub fn defer_task(
    repository: &mut dyn TaskRepositoryTrait,
    task_id: Uuid,
    pending_until: DateTime<Local>,
) -> Result<(), ApplicationError> {
    let task = find_task(repository, task_id)?;
    task.set_pending_until(pending_until)
        .map_err(ApplicationError::TaskTree)?;
    task.set_orig_status(Status::Pending)
        .map_err(ApplicationError::TaskTree)?;
    Ok(())
}

pub fn defer_routine_task(
    repository: &mut dyn TaskRepositoryTrait,
    task_id: Uuid,
) -> Result<(), ApplicationError> {
    let task = find_task(repository, task_id)?;
    let orig_deadline_time = task
        .get_deadline_time_opt()
        .map_err(ApplicationError::TaskTree)?
        .ok_or(ApplicationError::InvalidInput {
            field: "task_id",
            reason: "task must have a deadline",
        })?;
    let parent_task = task.parent().map_err(ApplicationError::TaskTree)?.ok_or(
        ApplicationError::InvalidInput {
            field: "task_id",
            reason: "task must have a parent",
        },
    )?;
    let repetition_interval_days = parent_task
        .get_repetition_interval_days_opt()
        .map_err(ApplicationError::TaskTree)?
        .ok_or(ApplicationError::InvalidInput {
            field: "task_id",
            reason: "parent task must have a repetition interval",
        })?;
    let parent_deadline_time_opt = parent_task
        .get_deadline_time_opt()
        .map_err(ApplicationError::TaskTree)?;
    let orig_start_time = task.get_start_time().map_err(ApplicationError::TaskTree)?;

    let deadline_out_of_range = || ApplicationError::LogicalDateOutOfRange {
        operation: "defer_routine_deadline",
        datetime: orig_deadline_time,
    };
    let new_deadline_time = if let Some(parent_deadline_time) = parent_deadline_time_opt {
        let first_logical_date_start = try_next_logical_date_start(orig_deadline_time)?;
        let additional_days = repetition_interval_days
            .checked_sub(1)
            .ok_or_else(deadline_out_of_range)?;
        let additional_duration =
            Duration::try_days(additional_days).ok_or_else(deadline_out_of_range)?;
        let target_date = first_logical_date_start
            .date_naive()
            .checked_add_signed(additional_duration)
            .ok_or_else(deadline_out_of_range)?;
        try_local_date_and_time(target_date, parent_deadline_time.time())?
    } else {
        let duration =
            Duration::try_days(repetition_interval_days).ok_or_else(deadline_out_of_range)?;
        orig_deadline_time
            .checked_add_signed(duration)
            .ok_or_else(deadline_out_of_range)?
    };
    let start_out_of_range = || ApplicationError::LogicalDateOutOfRange {
        operation: "defer_routine_start",
        datetime: orig_start_time,
    };
    let start_offset_days = (new_deadline_time - orig_deadline_time).num_days();
    let start_offset = Duration::try_days(start_offset_days).ok_or_else(start_out_of_range)?;
    let new_start_time = orig_start_time
        .checked_add_signed(start_offset)
        .ok_or_else(start_out_of_range)?;

    task.replace_deadline_time(new_deadline_time)
        .map_err(ApplicationError::TaskTree)?;
    task.set_orig_status(Status::Todo)
        .map_err(ApplicationError::TaskTree)?;
    task.set_start_time(new_start_time)
        .map_err(ApplicationError::TaskTree)?;
    Ok(())
}

pub fn complete_task(
    repository: &mut dyn TaskRepositoryTrait,
    input: CompleteTaskInput,
    factory: &mut TaskFactory<'_>,
) -> Result<CompleteTaskOutput, ApplicationError> {
    let task = find_task(repository, input.task_id)?;
    if task
        .has_undone_children()
        .map_err(ApplicationError::TaskTree)?
    {
        return Err(ApplicationError::HasUndoneChildren(input.task_id));
    }

    if input.additional_actual_work_seconds < 0 {
        return Err(ApplicationError::InvalidInput {
            field: "additional_actual_work_seconds",
            reason: "must not be negative",
        });
    }
    let actual_work_seconds = task
        .get_actual_work_seconds()
        .map_err(ApplicationError::TaskTree)?
        .checked_add(input.additional_actual_work_seconds)
        .ok_or(ApplicationError::InvalidInput {
            field: "additional_actual_work_seconds",
            reason: "actual work seconds overflow",
        })?;

    let prospective_next_focus_task_id = prospective_next_focus_task_id(&task)?;
    let next_repetition_task =
        prepare_next_repetition_task(&task, actual_work_seconds, input.finished_at, factory)?;
    let next_focus_task_id = if next_repetition_task.is_some() {
        None
    } else {
        prospective_next_focus_task_id
    };

    task.set_actual_work_seconds(actual_work_seconds)
        .map_err(ApplicationError::TaskTree)?;
    task.set_orig_status(Status::Done)
        .map_err(ApplicationError::TaskTree)?;
    task.set_end_time_opt(Some(input.finished_at))
        .map_err(ApplicationError::TaskTree)?;

    let next_repetition_task_id = create_prepared_repetition_task(next_repetition_task)?;

    Ok(CompleteTaskOutput {
        next_focus_task_id,
        next_repetition_task_id,
    })
}

fn prospective_next_focus_task_id(task: &TaskHandle) -> Result<Option<Uuid>, ApplicationError> {
    if !task
        .all_sibling_tasks_are_all_done()
        .map_err(ApplicationError::TaskTree)?
    {
        return Ok(None);
    }

    task.parent()
        .map_err(ApplicationError::TaskTree)?
        .map(|parent| parent.get_id())
        .transpose()
        .map_err(ApplicationError::TaskTree)
}

pub fn set_estimate(
    repository: &mut dyn TaskRepositoryTrait,
    task_id: Uuid,
    estimated_work_minutes: i64,
) -> Result<(), ApplicationError> {
    let estimated_work_seconds = estimated_work_seconds_from_minutes(estimated_work_minutes)?;
    let task = find_task(repository, task_id)?;
    task.set_estimated_work_seconds(estimated_work_seconds)
        .map_err(ApplicationError::TaskTree)?;
    Ok(())
}

pub fn set_deadline(
    repository: &mut dyn TaskRepositoryTrait,
    task_id: Uuid,
    deadline_time: Option<DateTime<Local>>,
) -> Result<(), ApplicationError> {
    let task = find_task(repository, task_id)?;
    match deadline_time {
        Some(deadline_time) => task
            .set_deadline_time_opt(Some(deadline_time))
            .map_err(ApplicationError::TaskTree)?,
        None => task
            .unset_deadline_time_opt()
            .map_err(ApplicationError::TaskTree)?,
    }
    Ok(())
}

pub fn set_category(
    repository: &mut dyn TaskRepositoryTrait,
    task_id: Uuid,
    project_category: Option<ProjectCategory>,
) -> Result<(), ApplicationError> {
    let task = find_task(repository, task_id)?;
    task.set_project_category_opt(project_category)
        .map_err(ApplicationError::TaskTree)?;
    Ok(())
}

fn find_task(
    repository: &dyn TaskRepositoryTrait,
    task_id: Uuid,
) -> Result<TaskHandle, ApplicationError> {
    repository
        .get_by_id(task_id)
        .map_err(ApplicationError::TaskTree)?
        .ok_or(ApplicationError::TaskNotFound(task_id))
}

pub fn validate_task_name(name: &str, field: &'static str) -> Result<(), ApplicationError> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err(ApplicationError::InvalidInput {
            field,
            reason: "must not be blank",
        });
    }
    if is_integer_only_name(trimmed_name) {
        return Err(ApplicationError::InvalidInput {
            field,
            reason: "must not be an integer-only name",
        });
    }
    Ok(())
}

fn is_integer_only_name(name: &str) -> bool {
    let digits = name
        .strip_prefix('+')
        .or_else(|| name.strip_prefix('-'))
        .unwrap_or(name);
    !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit())
}

pub fn estimated_work_seconds_from_minutes(minutes: i64) -> Result<i64, ApplicationError> {
    if minutes < 0 {
        return Err(ApplicationError::InvalidInput {
            field: "estimated_work_minutes",
            reason: "must not be negative",
        });
    }

    minutes
        .checked_mul(60)
        .ok_or(ApplicationError::InvalidInput {
            field: "estimated_work_minutes",
            reason: "seconds conversion overflow",
        })
}

#[cfg(test)]
fn create_next_repetition_task(
    task: &TaskHandle,
    finished_at: DateTime<Local>,
    factory: &mut TaskFactory<'_>,
) -> Result<Option<Uuid>, ApplicationError> {
    let actual_work_seconds = task
        .get_actual_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let prepared = prepare_next_repetition_task(task, actual_work_seconds, finished_at, factory)?;
    create_prepared_repetition_task(prepared)
}

struct PreparedRepetitionTask {
    parent_task: TaskHandle,
    task_attr: TaskAttr,
    task_id: Uuid,
    adjusted_parent_estimated_work_seconds: i64,
}

fn prepare_next_repetition_task(
    task: &TaskHandle,
    actual_work_seconds: i64,
    finished_at: DateTime<Local>,
    factory: &mut TaskFactory<'_>,
) -> Result<Option<PreparedRepetitionTask>, ApplicationError> {
    let Some(parent_task) = task.parent().map_err(ApplicationError::TaskTree)? else {
        return Ok(None);
    };
    let Some(repetition_interval_days) = parent_task
        .get_repetition_interval_days_opt()
        .map_err(ApplicationError::TaskTree)?
    else {
        return Ok(None);
    };

    let original_parent_estimated_work_seconds = parent_task
        .get_estimated_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let adjusted_parent_estimated_work_seconds =
        adjusted_repetition_estimate(original_parent_estimated_work_seconds, actual_work_seconds)?;
    let task_attr = build_next_repetition_task_attr(
        task,
        &parent_task,
        repetition_interval_days,
        finished_at,
        adjusted_parent_estimated_work_seconds,
        factory,
    )?;
    let task_id = *task_attr.get_id();
    Ok(Some(PreparedRepetitionTask {
        parent_task,
        task_attr,
        task_id,
        adjusted_parent_estimated_work_seconds,
    }))
}

fn create_prepared_repetition_task(
    prepared: Option<PreparedRepetitionTask>,
) -> Result<Option<Uuid>, ApplicationError> {
    let Some(prepared) = prepared else {
        return Ok(None);
    };
    prepared
        .parent_task
        .set_estimated_work_seconds(prepared.adjusted_parent_estimated_work_seconds)
        .map_err(ApplicationError::TaskTree)?;
    prepared
        .parent_task
        .create_child(prepared.task_attr)
        .map_err(ApplicationError::TaskTree)?;
    Ok(Some(prepared.task_id))
}

fn adjusted_repetition_estimate(
    original_estimated_seconds: i64,
    actual_work_seconds: i64,
) -> Result<i64, ApplicationError> {
    if actual_work_seconds <= 0 {
        return Ok(original_estimated_seconds);
    }

    let overflow_error = || ApplicationError::InvalidInput {
        field: "estimated_work_seconds",
        reason: "repetition estimate adjustment overflow",
    };
    let difference = actual_work_seconds
        .checked_sub(original_estimated_seconds)
        .ok_or_else(overflow_error)?;
    let adjusted_estimated_seconds = match difference.cmp(&0) {
        Ordering::Greater => difference
            .checked_mul(3)
            .map(|weighted_difference| weighted_difference / 4)
            .and_then(|adjustment| original_estimated_seconds.checked_add(adjustment))
            .ok_or_else(overflow_error)?,
        Ordering::Less => original_estimated_seconds
            .checked_add(difference / 4)
            .map(|adjusted| max(60, adjusted))
            .ok_or_else(overflow_error)?,
        Ordering::Equal => original_estimated_seconds,
    };
    Ok(adjusted_estimated_seconds)
}

fn apply_time_template(
    base_datetime: DateTime<Local>,
    time_template: DateTime<Local>,
) -> Result<DateTime<Local>, ApplicationError> {
    let time = NaiveTime::from_hms_opt(
        time_template.hour(),
        time_template.minute(),
        time_template.second(),
    )
    .ok_or(ApplicationError::LogicalDateOutOfRange {
        operation: "apply_time_template",
        datetime: time_template,
    })?;
    resolve_date_and_time(base_datetime, time)
}

fn resolve_date_and_time(
    base_datetime: DateTime<Local>,
    time: NaiveTime,
) -> Result<DateTime<Local>, ApplicationError> {
    let local_datetime = base_datetime.date_naive().and_time(time);
    resolve_local_datetime(local_datetime, local_datetime.and_local_timezone(Local))
}

fn build_next_repetition_task_attr(
    task: &TaskHandle,
    parent_task: &TaskHandle,
    repetition_interval_days: i64,
    finished_at: DateTime<Local>,
    adjusted_parent_estimated_work_seconds: i64,
    factory: &mut TaskFactory<'_>,
) -> Result<TaskAttr, ApplicationError> {
    let occurrence_anchor = match parent_task
        .get_repetition_anchor()
        .map_err(ApplicationError::TaskTree)?
    {
        RepetitionAnchor::Deadline => task
            .get_deadline_time_opt()
            .map_err(ApplicationError::TaskTree)?
            .unwrap_or(finished_at),
        RepetitionAnchor::Completion => finished_at,
    };
    let parent_start_time = parent_task
        .get_start_time()
        .map_err(ApplicationError::TaskTree)?;
    let parent_deadline_time = parent_task
        .get_deadline_time_opt()
        .map_err(ApplicationError::TaskTree)?;
    let days_in_advance = parent_task
        .get_days_in_advance()
        .map_err(ApplicationError::TaskTree)?;
    let parent_name = parent_task.get_name().map_err(ApplicationError::TaskTree)?;
    let parent_atomic = parent_task
        .get_atomic()
        .map_err(ApplicationError::TaskTree)?;
    let parent_fixed_start = parent_task
        .get_fixed_start()
        .map_err(ApplicationError::TaskTree)?;

    let next_logical_date_start = try_next_logical_date_start(occurrence_anchor)?;
    let repetition_offset_days =
        repetition_interval_days
            .checked_sub(1)
            .ok_or(ApplicationError::LogicalDateOutOfRange {
                operation: "next_logical_date_start",
                datetime: occurrence_anchor,
            })?;
    let repetition_offset = Duration::try_days(repetition_offset_days).ok_or(
        ApplicationError::LogicalDateOutOfRange {
            operation: "next_logical_date_start",
            datetime: occurrence_anchor,
        },
    )?;
    let next_occurrence_day = next_logical_date_start
        .checked_add_signed(repetition_offset)
        .ok_or(ApplicationError::LogicalDateOutOfRange {
            operation: "next_logical_date_start",
            datetime: occurrence_anchor,
        })?;
    let occurrence_start_time = apply_time_template(next_occurrence_day, parent_start_time)?;
    let days_in_advance =
        Duration::try_days(days_in_advance).ok_or(ApplicationError::LogicalDateOutOfRange {
            operation: "repetition_start_time",
            datetime: occurrence_start_time,
        })?;
    let task_start_time = occurrence_start_time
        .checked_sub_signed(days_in_advance)
        .ok_or(ApplicationError::LogicalDateOutOfRange {
            operation: "repetition_start_time",
            datetime: occurrence_start_time,
        })?;
    let new_deadline_time = match parent_deadline_time {
        Some(parent_deadline_time) => {
            apply_time_template(next_occurrence_day, parent_deadline_time)?
        }
        None => {
            let end_of_day = NaiveTime::from_hms_opt(23, 59, 59).ok_or(
                ApplicationError::LogicalDateOutOfRange {
                    operation: "repetition_deadline_time",
                    datetime: next_occurrence_day,
                },
            )?;
            resolve_date_and_time(next_occurrence_day, end_of_day)?
        }
    };

    let mut new_task_attr = factory.create_task_attr(&format!(
        "{}({}/{})",
        parent_name,
        occurrence_start_time.month(),
        occurrence_start_time.day()
    ));
    new_task_attr.set_start_time(task_start_time);
    new_task_attr.set_deadline_time_opt(Some(new_deadline_time));
    new_task_attr.set_estimated_work_seconds(adjusted_parent_estimated_work_seconds);
    new_task_attr.set_atomic(parent_atomic);
    new_task_attr.set_fixed_start(parent_fixed_start);
    Ok(new_task_attr)
}

#[cfg(test)]
#[path = "task_use_case_tests.rs"]
mod tests;
