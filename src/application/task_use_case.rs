use crate::application::daily_capacity::try_next_business_day_start;
use crate::application::interface::TaskRepositoryTrait;
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
    AmbiguousLocalDateTime {
        local_datetime: NaiveDateTime,
        earlier: DateTime<Local>,
        later: DateTime<Local>,
    },
    NonexistentLocalDateTime {
        local_datetime: NaiveDateTime,
    },
    SubjectiveDateOutOfRange {
        operation: &'static str,
        datetime: DateTime<Local>,
    },
    SubjectiveDateStartOutOfRange {
        date: NaiveDate,
    },
    SubjectiveDateEndOutOfRange {
        date: NaiveDate,
        end_of_day_offset_minutes: i64,
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
            Self::SubjectiveDateOutOfRange {
                operation,
                datetime,
            } => write!(
                formatter,
                "subjective date operation {operation} is outside the supported range: {datetime}"
            ),
            Self::SubjectiveDateStartOutOfRange { date } => write!(
                formatter,
                "subjective date start is outside the supported range: date={date}"
            ),
            Self::SubjectiveDateEndOutOfRange {
                date,
                end_of_day_offset_minutes,
            } => write!(
                formatter,
                "subjective date end is outside the supported range: date={date}, end_of_day_offset_minutes={end_of_day_offset_minutes}"
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
    repository
        .get_highest_priority_leaf_task_id()
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
        .map_err(ApplicationError::TaskTree)?;
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

    let next_repetition_task =
        prepare_next_repetition_task(&task, actual_work_seconds, input.finished_at, factory)?;

    task.set_actual_work_seconds(actual_work_seconds)
        .map_err(ApplicationError::TaskTree)?;
    task.set_orig_status(Status::Done)
        .map_err(ApplicationError::TaskTree)?;
    task.set_end_time_opt(Some(input.finished_at))
        .map_err(ApplicationError::TaskTree)?;

    let next_repetition_task_id = create_prepared_repetition_task(next_repetition_task)?;
    let next_focus_task_id = if task
        .all_sibling_tasks_are_all_done()
        .map_err(ApplicationError::TaskTree)?
    {
        task.parent()
            .map_err(ApplicationError::TaskTree)?
            .map(|parent| parent.get_id())
            .transpose()
            .map_err(ApplicationError::TaskTree)?
    } else {
        None
    };

    Ok(CompleteTaskOutput {
        next_focus_task_id,
        next_repetition_task_id,
    })
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
        adjusted_repetition_estimate(original_parent_estimated_work_seconds, actual_work_seconds);
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

fn adjusted_repetition_estimate(original_estimated_seconds: i64, actual_work_seconds: i64) -> i64 {
    if actual_work_seconds <= 0 {
        return original_estimated_seconds;
    }

    let difference = actual_work_seconds - original_estimated_seconds;
    match difference.cmp(&0) {
        Ordering::Greater => original_estimated_seconds + difference * 3 / 4,
        Ordering::Less => max(60, original_estimated_seconds + difference / 4),
        Ordering::Equal => original_estimated_seconds,
    }
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
    .ok_or(ApplicationError::SubjectiveDateOutOfRange {
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

    let next_business_day_start = try_next_business_day_start(occurrence_anchor)?;
    let repetition_offset_days = repetition_interval_days.checked_sub(1).ok_or(
        ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime: occurrence_anchor,
        },
    )?;
    let repetition_offset = Duration::try_days(repetition_offset_days).ok_or(
        ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime: occurrence_anchor,
        },
    )?;
    let next_occurrence_day = next_business_day_start
        .checked_add_signed(repetition_offset)
        .ok_or(ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime: occurrence_anchor,
        })?;
    let occurrence_start_time = apply_time_template(next_occurrence_day, parent_start_time)?;
    let days_in_advance =
        Duration::try_days(days_in_advance).ok_or(ApplicationError::SubjectiveDateOutOfRange {
            operation: "repetition_start_time",
            datetime: occurrence_start_time,
        })?;
    let task_start_time = occurrence_start_time
        .checked_sub_signed(days_in_advance)
        .ok_or(ApplicationError::SubjectiveDateOutOfRange {
            operation: "repetition_start_time",
            datetime: occurrence_start_time,
        })?;
    let new_deadline_time = match parent_deadline_time {
        Some(parent_deadline_time) => {
            apply_time_template(next_occurrence_day, parent_deadline_time)?
        }
        None => {
            let end_of_day = NaiveTime::from_hms_opt(23, 59, 59).ok_or(
                ApplicationError::SubjectiveDateOutOfRange {
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
    Ok(new_task_attr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::cell::Cell;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    struct TestTaskRepository {
        projects: Vec<TaskHandle>,
        now: DateTime<Local>,
        highest_priority_leaf_task_id: Option<Uuid>,
        save_count: Cell<usize>,
    }

    impl TestTaskRepository {
        fn new(projects: Vec<TaskHandle>, now: DateTime<Local>) -> Self {
            Self {
                projects,
                now,
                highest_priority_leaf_task_id: None,
                save_count: Cell::new(0),
            }
        }
    }

    impl TaskRepositoryTrait for TestTaskRepository {
        fn get_project_storage_dir_name(&self) -> &str {
            "unused"
        }

        fn get_all_projects(&self) -> Vec<&TaskHandle> {
            self.projects.iter().collect()
        }

        fn load(&mut self) -> Result<(), crate::application::interface::TaskRepositoryError> {
            Ok(())
        }

        fn save(&self) -> Result<(), crate::application::interface::TaskRepositoryError> {
            self.save_count.set(self.save_count.get() + 1);
            Ok(())
        }

        fn sync_clock(
            &mut self,
            now: DateTime<Local>,
        ) -> Result<(), crate::entity::task::TaskTreeError> {
            self.now = now;
            Ok(())
        }

        fn get_last_synced_time(&self) -> DateTime<Local> {
            self.now
        }

        fn get_highest_priority_project(&mut self) -> Option<&TaskHandle> {
            self.projects.first()
        }

        fn get_highest_priority_leaf_task_id(
            &mut self,
        ) -> Result<Option<Uuid>, crate::entity::task::TaskTreeError> {
            Ok(self.highest_priority_leaf_task_id)
        }

        fn get_defer_candidate_leaf_task_id(
            &mut self,
            _recent_days: i64,
        ) -> Result<Option<Uuid>, crate::entity::task::TaskTreeError> {
            Ok(None)
        }

        fn get_by_id(
            &self,
            id: Uuid,
        ) -> Result<Option<TaskHandle>, crate::entity::task::TaskTreeError> {
            for task in &self.projects {
                if let Some(found) = task.get_by_id(id)? {
                    return Ok(Some(found));
                }
            }
            Ok(None)
        }

        fn start_new_project(
            &mut self,
            root_task: TaskHandle,
        ) -> Result<(), crate::entity::task::TaskTreeError> {
            self.projects.push(root_task);
            Ok(())
        }
    }

    fn fixed_now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap()
    }

    fn create_task_with_fresh_factory(
        repository: &mut dyn TaskRepositoryTrait,
        input: CreateTaskInput,
    ) -> Result<Uuid, ApplicationError> {
        let mut next_id = Uuid::new_v4;
        let mut factory = TaskFactory::new(fixed_now(), &mut next_id);
        create_task(repository, input, &mut factory)
    }

    fn breakdown_task_with_fresh_factory(
        repository: &mut dyn TaskRepositoryTrait,
        input: BreakdownTaskInput,
    ) -> Result<Vec<Uuid>, ApplicationError> {
        let mut next_id = Uuid::new_v4;
        let mut factory = TaskFactory::new(fixed_now(), &mut next_id);
        breakdown_task(repository, input, &mut factory)
    }

    fn complete_task_with_fresh_factory(
        repository: &mut dyn TaskRepositoryTrait,
        input: CompleteTaskInput,
    ) -> Result<CompleteTaskOutput, ApplicationError> {
        let mut next_id = Uuid::new_v4;
        let mut factory = TaskFactory::new(fixed_now(), &mut next_id);
        complete_task(repository, input, &mut factory)
    }

    fn next_child_after_finish(
        repetition_anchor: RepetitionAnchor,
        days_in_advance: i64,
        focused_start_time: DateTime<Local>,
        focused_deadline_time_opt: Option<DateTime<Local>>,
        finished_at: DateTime<Local>,
    ) -> TaskHandle {
        let parent_task = crate::test_support::new_task_handle("ルーチン").unwrap();
        parent_task
            .set_repetition_interval_days_opt(Some(7))
            .unwrap();
        parent_task
            .set_repetition_anchor(repetition_anchor)
            .unwrap();
        parent_task.set_days_in_advance(days_in_advance).unwrap();
        parent_task
            .set_start_time(Local.with_ymd_and_hms(2026, 5, 10, 9, 30, 15).unwrap())
            .unwrap();
        parent_task
            .set_deadline_time_opt(Some(
                Local.with_ymd_and_hms(2026, 5, 10, 23, 59, 59).unwrap(),
            ))
            .unwrap();

        let mut child_task_attr =
            TaskAttr::with_identity("ルーチン(5/16)", Uuid::new_v4(), finished_at);
        child_task_attr.set_start_time(focused_start_time);
        child_task_attr.set_deadline_time_opt(focused_deadline_time_opt);
        let child_task = parent_task.create_as_last_child(child_task_attr);

        let mut repository = TestTaskRepository::new(vec![parent_task.clone()], finished_at);
        complete_task_with_fresh_factory(
            &mut repository,
            CompleteTaskInput {
                task_id: child_task.get_id().unwrap(),
                finished_at,
                additional_actual_work_seconds: 0,
            },
        )
        .unwrap();

        parent_task
            .get_children()
            .unwrap()
            .into_iter()
            .find(|task| task.get_status().unwrap() != Status::Done)
            .expect("next repetition child")
    }

    #[test]
    fn get_task_親子関係を含むviewを返す() {
        let root = crate::test_support::new_task_handle("親").unwrap();
        root.set_priority(5).unwrap();
        root.set_project_category_opt(Some(ProjectCategory::Investment))
            .unwrap();
        let child = root.create_as_last_child(crate::test_support::new_task_attr("子"));
        let repository = TestTaskRepository::new(vec![root.clone()], fixed_now());

        let actual = get_task(&repository, child.get_id().unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(actual.id, child.get_id().unwrap());
        assert_eq!(actual.root_id, root.get_id().unwrap());
        assert_eq!(actual.parent_id, Some(root.get_id().unwrap()));
        assert!(actual.child_ids.is_empty());
        assert_eq!(actual.name, "子");
        assert_eq!(actual.priority, 5);
        assert_eq!(actual.project_category, Some(ProjectCategory::Investment));
    }

    #[test]
    fn get_task_task_viewの全属性を返す() {
        let now = fixed_now();
        let pending_until = Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap();
        let create_time = Local.with_ymd_and_hms(2026, 8, 1, 9, 0, 0).unwrap();
        let start_time = Local.with_ymd_and_hms(2026, 8, 10, 10, 0, 0).unwrap();
        let end_time = Local.with_ymd_and_hms(2026, 8, 11, 11, 0, 0).unwrap();
        let deadline_time = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
        let root = crate::test_support::new_task_handle("全属性").unwrap();
        root.set_orig_status(Status::Pending).unwrap();
        root.set_pending_until(pending_until).unwrap();
        root.set_priority(7).unwrap();
        root.set_create_time(create_time).unwrap();
        root.set_start_time(start_time).unwrap();
        root.set_end_time_opt(Some(end_time)).unwrap();
        root.set_deadline_time_opt(Some(deadline_time)).unwrap();
        root.set_estimated_work_seconds(1_800).unwrap();
        root.set_actual_work_seconds(900).unwrap();
        root.set_atomic(true).unwrap();
        root.set_is_on_other_side(true).unwrap();
        root.set_repetition_interval_days_opt(Some(7)).unwrap();
        root.set_repetition_anchor(RepetitionAnchor::Completion)
            .unwrap();
        root.set_days_in_advance(2).unwrap();
        root.set_project_category_opt(Some(ProjectCategory::Recovery))
            .unwrap();
        root.sync_clock(now).unwrap();
        let child = root.create_as_last_child(crate::test_support::new_task_attr("子"));
        let repository = TestTaskRepository::new(vec![root.clone()], now);

        let actual = get_task(&repository, root.get_id().unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(actual.id, root.get_id().unwrap());
        assert_eq!(actual.root_id, root.get_id().unwrap());
        assert_eq!(actual.parent_id, None);
        assert_eq!(actual.child_ids, vec![child.get_id().unwrap()]);
        assert_eq!(actual.name, "全属性");
        assert_eq!(actual.status, Status::Pending);
        assert_eq!(actual.original_status, Status::Pending);
        assert!(actual.is_on_other_side);
        assert!(actual.atomic);
        assert_eq!(actual.pending_until, Some(pending_until));
        assert_eq!(actual.priority, 7);
        assert_eq!(actual.create_time, create_time);
        assert_eq!(actual.start_time, start_time);
        assert_eq!(actual.end_time, Some(end_time));
        assert_eq!(actual.deadline_time, Some(deadline_time));
        assert_eq!(actual.estimated_work_seconds, 1_800);
        assert_eq!(actual.actual_work_seconds, 900);
        assert_eq!(actual.repetition_interval_days, Some(7));
        assert_eq!(actual.repetition_anchor, RepetitionAnchor::Completion);
        assert_eq!(actual.days_in_advance, 2);
        assert_eq!(actual.project_category, Some(ProjectCategory::Recovery));
    }

    #[test]
    fn get_task_未知uuidはnoneを返す() {
        let repository = TestTaskRepository::new(vec![], fixed_now());
        assert_eq!(get_task(&repository, Uuid::new_v4()), Ok(None));
    }

    #[test]
    fn get_task_pendingでなければpending_untilはnoneを返す() {
        let task = crate::test_support::new_task_handle("未延期").unwrap();
        let repository = TestTaskRepository::new(vec![task.clone()], fixed_now());

        let actual = get_task(&repository, task.get_id().unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(actual.original_status, Status::Todo);
        assert_eq!(actual.pending_until, None);
    }

    #[test]
    fn get_focus_最高優先度leafのviewを返す() {
        let root = crate::test_support::new_task_handle("親").unwrap();
        let child = root.create_as_last_child(crate::test_support::new_task_attr("子"));
        let mut repository = TestTaskRepository::new(vec![root], fixed_now());
        repository.highest_priority_leaf_task_id = Some(child.get_id().unwrap());

        assert_eq!(
            get_focus(&mut repository).unwrap().unwrap().id,
            child.get_id().unwrap()
        );
    }

    #[test]
    fn get_focus_候補がなければnoneを返す() {
        let mut repository = TestTaskRepository::new(vec![], fixed_now());

        assert_eq!(get_focus(&mut repository), Ok(None));
    }

    #[test]
    fn get_focus_選択されたtaskが取得できなければnoneを返す() {
        let mut repository = TestTaskRepository::new(vec![], fixed_now());
        repository.highest_priority_leaf_task_id = Some(Uuid::new_v4());

        assert_eq!(get_focus(&mut repository), Ok(None));
    }

    #[test]
    fn create_task_属性を設定してsaveしない() {
        let pending_until = Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap();
        let mut repository = TestTaskRepository::new(vec![], fixed_now());

        let task_id = create_task_with_fresh_factory(
            &mut repository,
            CreateTaskInput {
                name: "新規".to_string(),
                estimated_work_minutes: Some(30),
                pending_until: Some(pending_until),
            },
        )
        .unwrap();

        let task = repository.get_by_id(task_id).unwrap().unwrap();
        assert_eq!(task.get_name().unwrap(), "新規");
        assert_eq!(task.get_priority().unwrap(), 5);
        assert_eq!(task.get_estimated_work_seconds().unwrap(), 30 * 60);
        assert_eq!(task.get_orig_status().unwrap(), Status::Pending);
        assert_eq!(task.get_pending_until().unwrap(), pending_until);
        assert_eq!(repository.save_count.get(), 0);
    }

    #[test]
    fn create_task_operationで固定したidと時刻を使う() {
        let now = fixed_now();
        let expected_id = Uuid::parse_str("00000000-0000-0000-0000-000000000101").unwrap();
        let mut next_id = || expected_id;
        let mut factory = TaskFactory::new(now, &mut next_id);
        let mut repository = TestTaskRepository::new(vec![], now);

        let actual_id = create_task(
            &mut repository,
            CreateTaskInput {
                name: "新規".to_string(),
                estimated_work_minutes: None,
                pending_until: None,
            },
            &mut factory,
        )
        .unwrap();

        let task = repository.get_by_id(actual_id).unwrap().unwrap();
        assert_eq!(actual_id, expected_id);
        assert_eq!(task.get_create_time().unwrap(), now);
        assert_eq!(task.get_start_time().unwrap(), now);
    }

    #[test]
    fn create_task_空の名前を拒否して変更しない() {
        let mut repository = TestTaskRepository::new(vec![], fixed_now());

        let actual = create_task_with_fresh_factory(
            &mut repository,
            CreateTaskInput {
                name: String::new(),
                estimated_work_minutes: None,
                pending_until: None,
            },
        );

        assert!(matches!(
            actual,
            Err(ApplicationError::InvalidInput { field: "name", .. })
        ));
        assert!(repository.projects.is_empty());
    }

    #[test]
    fn create_task_空白名と整数名を拒否して変更しない() {
        for name in [
            "   ",
            "123",
            "  -123  ",
            "+123",
            "999999999999999999999999999999999999999999",
        ] {
            let mut repository = TestTaskRepository::new(vec![], fixed_now());

            let actual = create_task_with_fresh_factory(
                &mut repository,
                CreateTaskInput {
                    name: name.to_string(),
                    estimated_work_minutes: None,
                    pending_until: None,
                },
            );

            assert!(matches!(
                actual,
                Err(ApplicationError::InvalidInput { field: "name", .. })
            ));
            assert!(repository.projects.is_empty());
        }
    }

    #[test]
    fn create_task_負の見積もりを拒否して変更しない() {
        let mut repository = TestTaskRepository::new(vec![], fixed_now());

        let actual = create_task_with_fresh_factory(
            &mut repository,
            CreateTaskInput {
                name: "負の見積もり".to_string(),
                estimated_work_minutes: Some(-1),
                pending_until: None,
            },
        );

        assert!(matches!(
            actual,
            Err(ApplicationError::InvalidInput {
                field: "estimated_work_minutes",
                ..
            })
        ));
        assert!(repository.projects.is_empty());
    }

    #[test]
    fn create_task_秒変換がoverflowする見積もりをerrorにする() {
        let mut repository = TestTaskRepository::new(vec![], fixed_now());

        let actual = catch_unwind(AssertUnwindSafe(|| {
            create_task_with_fresh_factory(
                &mut repository,
                CreateTaskInput {
                    name: "巨大な見積もり".to_string(),
                    estimated_work_minutes: Some(i64::MAX),
                    pending_until: None,
                },
            )
        }));

        assert!(matches!(
            actual,
            Ok(Err(ApplicationError::InvalidInput {
                field: "estimated_work_minutes",
                ..
            }))
        ));
        assert!(repository.projects.is_empty());
    }

    #[test]
    fn breakdown_task_入力順と締切を維持する() {
        let parent = crate::test_support::new_task_handle("親").unwrap();
        let deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
        parent.set_deadline_time_opt(Some(deadline)).unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let child_ids = breakdown_task_with_fresh_factory(
            &mut repository,
            BreakdownTaskInput {
                parent_id: parent.get_id().unwrap(),
                names: vec!["一".to_string(), "二".to_string()],
                pending_until: None,
            },
        )
        .unwrap();

        assert_eq!(
            parent
                .get_children()
                .unwrap()
                .iter()
                .map(|task| task.get_name().unwrap())
                .collect::<Vec<_>>(),
            vec!["一", "二"]
        );
        assert_eq!(child_ids.len(), 2);
        assert!(parent
            .get_children()
            .unwrap()
            .iter()
            .all(|child| child.get_deadline_time_opt().unwrap() == Some(deadline)));
    }

    #[test]
    fn breakdown_task_operationで固定したid列と時刻を使う() {
        let now = fixed_now();
        let expected_ids = [
            Uuid::parse_str("00000000-0000-0000-0000-000000000201").unwrap(),
            Uuid::parse_str("00000000-0000-0000-0000-000000000202").unwrap(),
        ];
        let mut ids = expected_ids.into_iter();
        let mut next_id = || {
            ids.next()
                .expect("task id should be consumed once per child")
        };
        let mut factory = TaskFactory::new(now, &mut next_id);
        let parent = crate::test_support::new_task_handle("親").unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], now);

        let actual_ids = breakdown_task(
            &mut repository,
            BreakdownTaskInput {
                parent_id: parent.get_id().unwrap(),
                names: vec!["一".to_string(), "二".to_string()],
                pending_until: None,
            },
            &mut factory,
        )
        .unwrap();

        assert_eq!(actual_ids, expected_ids);
        for (child, expected_id) in parent.get_children().unwrap().into_iter().zip(expected_ids) {
            assert_eq!(child.get_id().unwrap(), expected_id);
            assert_eq!(child.get_create_time().unwrap(), now);
            assert_eq!(child.get_start_time().unwrap(), now);
        }
    }

    #[test]
    fn breakdown_task_全ての子を指定時刻までpendingにする() {
        let parent = crate::test_support::new_task_handle("親").unwrap();
        let pending_until = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let child_ids = breakdown_task_with_fresh_factory(
            &mut repository,
            BreakdownTaskInput {
                parent_id: parent.get_id().unwrap(),
                names: vec!["一".to_string(), "二".to_string()],
                pending_until: Some(pending_until),
            },
        )
        .unwrap();

        assert_eq!(
            parent
                .get_children()
                .unwrap()
                .iter()
                .map(|task| task.get_name().unwrap())
                .collect::<Vec<_>>(),
            vec!["一", "二"]
        );
        assert_eq!(child_ids.len(), 2);
        assert!(parent.get_children().unwrap().iter().all(|child| {
            child.get_orig_status().unwrap() == Status::Pending
                && child.get_pending_until().unwrap() == pending_until
        }));
    }

    #[test]
    fn breakdown_task_数値名を含む場合は変更しない() {
        let parent = crate::test_support::new_task_handle("親").unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let actual = breakdown_task_with_fresh_factory(
            &mut repository,
            BreakdownTaskInput {
                parent_id: parent.get_id().unwrap(),
                names: vec!["子".to_string(), "10".to_string()],
                pending_until: None,
            },
        );

        assert!(matches!(actual, Err(ApplicationError::InvalidInput { .. })));
        assert!(parent.get_children().unwrap().is_empty());
    }

    #[test]
    fn breakdown_task_空の名前一覧を拒否して変更しない() {
        let parent = crate::test_support::new_task_handle("親").unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let actual = breakdown_task_with_fresh_factory(
            &mut repository,
            BreakdownTaskInput {
                parent_id: parent.get_id().unwrap(),
                names: vec![],
                pending_until: None,
            },
        );

        assert!(matches!(
            actual,
            Err(ApplicationError::InvalidInput { field: "names", .. })
        ));
        assert!(parent.get_children().unwrap().is_empty());
    }

    #[test]
    fn breakdown_task_空白名を含む場合は変更しない() {
        let parent = crate::test_support::new_task_handle("親").unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let actual = breakdown_task_with_fresh_factory(
            &mut repository,
            BreakdownTaskInput {
                parent_id: parent.get_id().unwrap(),
                names: vec!["子".to_string(), "   ".to_string()],
                pending_until: None,
            },
        );

        assert!(matches!(actual, Err(ApplicationError::InvalidInput { .. })));
        assert!(parent.get_children().unwrap().is_empty());
    }

    #[test]
    fn defer_task_絶対時刻までpendingにする() {
        let task = crate::test_support::new_task_handle("延期").unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());
        let pending_until = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 1).unwrap();

        defer_task(&mut repository, task_id, pending_until).unwrap();

        let task = repository.get_by_id(task_id).unwrap().unwrap();
        assert_eq!(task.get_orig_status().unwrap(), Status::Pending);
        assert_eq!(task.get_pending_until().unwrap(), pending_until);
    }

    #[test]
    fn complete_task_未完了の子があれば変更しない() {
        let task = crate::test_support::new_task_handle("親").unwrap();
        task.create_as_last_child(crate::test_support::new_task_attr("未完了"));
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());

        let actual = complete_task_with_fresh_factory(
            &mut repository,
            CompleteTaskInput {
                task_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 120,
            },
        );

        assert_eq!(actual, Err(ApplicationError::HasUndoneChildren(task_id)));
        let task = repository.get_by_id(task_id).unwrap().unwrap();
        assert_eq!(task.get_status().unwrap(), Status::Todo);
        assert_eq!(task.get_actual_work_seconds().unwrap(), 0);
    }

    #[test]
    fn complete_task_実績を加算して完了する() {
        let task = crate::test_support::new_task_handle("完了").unwrap();
        task.set_actual_work_seconds(60).unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());

        let output = complete_task_with_fresh_factory(
            &mut repository,
            CompleteTaskInput {
                task_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 120,
            },
        )
        .unwrap();

        let task = repository.get_by_id(task_id).unwrap().unwrap();
        assert_eq!(task.get_status().unwrap(), Status::Done);
        assert_eq!(task.get_end_time_opt().unwrap(), Some(fixed_now()));
        assert_eq!(task.get_actual_work_seconds().unwrap(), 180);
        assert_eq!(output.next_focus_task_id, None);
        assert_eq!(output.next_repetition_task_id, None);
    }

    #[test]
    fn complete_task_反復なしではtask_factoryのidを消費しない() {
        let task = crate::test_support::new_task_handle("単発").unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());
        let id_call_count = Cell::new(0);
        let mut next_id = || {
            id_call_count.set(id_call_count.get() + 1);
            Uuid::from_u128(0x101)
        };
        let mut factory = TaskFactory::new(fixed_now(), &mut next_id);

        complete_task(
            &mut repository,
            CompleteTaskInput {
                task_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 0,
            },
            &mut factory,
        )
        .unwrap();

        assert_eq!(id_call_count.get(), 0);
    }

    #[test]
    fn create_next_repetition_taskは構造化errorを返せるresultを維持する() {
        let task = crate::test_support::new_task_handle("単発").unwrap();
        let mut next_id = Uuid::new_v4;
        let mut factory = TaskFactory::new(fixed_now(), &mut next_id);

        let actual: Result<Option<Uuid>, ApplicationError> =
            create_next_repetition_task(&task, fixed_now(), &mut factory);

        assert_eq!(actual, Ok(None));
    }

    #[test]
    fn complete_task_反復anchorの次業務日計算不能をerrorにして変更しない() {
        let occurrence_anchor = DateTime::<Local>::from_naive_utc_and_offset(
            NaiveDate::MAX.and_hms_opt(6, 0, 0).unwrap(),
            chrono::FixedOffset::east_opt(0).unwrap(),
        );
        let parent =
            TaskHandle::with_identity("ルーチン", Uuid::from_u128(0x201), fixed_now()).unwrap();
        parent.set_repetition_interval_days_opt(Some(7)).unwrap();
        parent.set_estimated_work_seconds(600).unwrap();
        let mut child_attr = TaskAttr::with_identity("今回", Uuid::from_u128(0x202), fixed_now());
        child_attr.set_deadline_time_opt(Some(occurrence_anchor));
        child_attr.set_actual_work_seconds(120);
        let child = parent.create_as_last_child(child_attr);
        let child_id = child.get_id().unwrap();
        let child_status_before = child.get_status().unwrap();
        let child_actual_before = child.get_actual_work_seconds().unwrap();
        let child_end_before = child.get_end_time_opt().unwrap();
        let child_revision_before = child.get_persistent_mutation_revision().unwrap();
        let parent_estimate_before = parent.get_estimated_work_seconds().unwrap();
        let parent_revision_before = parent.get_persistent_mutation_revision().unwrap();
        let child_ids_before = parent
            .get_children()
            .unwrap()
            .into_iter()
            .map(|task| task.get_id().unwrap())
            .collect::<Vec<_>>();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());
        repository.highest_priority_leaf_task_id = Some(child_id);
        let focus_before = repository.highest_priority_leaf_task_id;
        let id_call_count = Cell::new(0);
        let mut next_id = || {
            id_call_count.set(id_call_count.get() + 1);
            Uuid::from_u128(0x203)
        };
        let mut factory = TaskFactory::new(fixed_now(), &mut next_id);

        let actual = catch_unwind(AssertUnwindSafe(|| {
            complete_task(
                &mut repository,
                CompleteTaskInput {
                    task_id: child_id,
                    finished_at: fixed_now(),
                    additional_actual_work_seconds: 60,
                },
                &mut factory,
            )
        }));

        let actual = actual.expect("complete_task must return an error instead of panicking");
        assert_eq!(
            actual,
            Err(ApplicationError::SubjectiveDateOutOfRange {
                operation: "next_business_day_start",
                datetime: occurrence_anchor,
            })
        );
        assert_eq!(child.get_status().unwrap(), child_status_before);
        assert_eq!(
            child.get_actual_work_seconds().unwrap(),
            child_actual_before
        );
        assert_eq!(child.get_end_time_opt().unwrap(), child_end_before);
        assert_eq!(
            child.get_persistent_mutation_revision().unwrap(),
            child_revision_before
        );
        assert_eq!(
            parent.get_estimated_work_seconds().unwrap(),
            parent_estimate_before
        );
        assert_eq!(
            parent.get_persistent_mutation_revision().unwrap(),
            parent_revision_before
        );
        assert_eq!(
            parent
                .get_children()
                .unwrap()
                .into_iter()
                .map(|task| task.get_id().unwrap())
                .collect::<Vec<_>>(),
            child_ids_before
        );
        assert_eq!(repository.highest_priority_leaf_task_id, focus_before);
        assert_eq!(id_call_count.get(), 0);
    }

    #[test]
    fn complete_task_繰り返しtaskを生成して見積もりを補正する() {
        let parent = crate::test_support::new_task_handle("ルーチン").unwrap();
        parent.set_repetition_interval_days_opt(Some(7)).unwrap();
        parent.set_estimated_work_seconds(600).unwrap();
        let child = parent.create_as_last_child(crate::test_support::new_task_attr("今回"));
        child.set_actual_work_seconds(1000).unwrap();
        let child_id = child.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let before_completion = Local::now();
        let mut next_id = Uuid::new_v4;
        let mut factory = TaskFactory::new(Local::now(), &mut next_id);
        let output = complete_task(
            &mut repository,
            CompleteTaskInput {
                task_id: child_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 0,
            },
            &mut factory,
        )
        .unwrap();
        let after_completion = Local::now();

        assert_eq!(parent.get_estimated_work_seconds().unwrap(), 900);
        assert!(output.next_repetition_task_id.is_some());
        assert_eq!(parent.get_children().unwrap().len(), 2);
        let next_task = repository
            .get_by_id(output.next_repetition_task_id.unwrap())
            .unwrap()
            .unwrap();
        let create_time = next_task.get_create_time().unwrap();
        assert!(before_completion <= create_time);
        assert!(create_time <= after_completion);
    }

    #[test]
    fn complete_task_反復taskはoperation固定のidentityを使う() {
        let parent = crate::test_support::new_task_handle("ルーチン").unwrap();
        parent.set_repetition_interval_days_opt(Some(7)).unwrap();
        let child = parent.create_as_last_child(crate::test_support::new_task_attr("今回"));
        let child_id = child.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![parent], fixed_now());
        let operation_now = Local.with_ymd_and_hms(2026, 8, 12, 14, 15, 16).unwrap();
        let expected_id = Uuid::from_u128(0x102);
        let id_call_count = Cell::new(0);
        let mut next_id = || {
            id_call_count.set(id_call_count.get() + 1);
            expected_id
        };
        let mut factory = TaskFactory::new(operation_now, &mut next_id);

        let output = complete_task(
            &mut repository,
            CompleteTaskInput {
                task_id: child_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 0,
            },
            &mut factory,
        )
        .unwrap();

        assert_eq!(output.next_repetition_task_id, Some(expected_id));
        assert_eq!(id_call_count.get(), 1);
        let next_task = repository.get_by_id(expected_id).unwrap().unwrap();
        assert_eq!(next_task.get_id().unwrap(), expected_id);
        assert_eq!(next_task.get_create_time().unwrap(), operation_now);
    }

    #[test]
    fn complete_task_repetition_anchor_deadlineは元の期限サイクルを維持する() {
        let next_child = next_child_after_finish(
            RepetitionAnchor::Deadline,
            0,
            Local.with_ymd_and_hms(2026, 5, 16, 9, 30, 15).unwrap(),
            Some(Local.with_ymd_and_hms(2026, 5, 16, 23, 59, 59).unwrap()),
            Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap(),
        );

        assert_eq!(
            next_child.get_deadline_time_opt().unwrap(),
            Some(Local.with_ymd_and_hms(2026, 5, 23, 23, 59, 59).unwrap())
        );
    }

    #[test]
    fn complete_task_repetition_anchor_completionは完了日から次回期限を決める() {
        let next_child = next_child_after_finish(
            RepetitionAnchor::Completion,
            0,
            Local.with_ymd_and_hms(2026, 5, 16, 9, 30, 15).unwrap(),
            Some(Local.with_ymd_and_hms(2026, 5, 16, 23, 59, 59).unwrap()),
            Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap(),
        );

        assert_eq!(
            next_child.get_deadline_time_opt().unwrap(),
            Some(Local.with_ymd_and_hms(2026, 5, 24, 23, 59, 59).unwrap())
        );
    }

    #[test]
    fn complete_task_days_in_advanceはstart_timeだけ前倒しする() {
        let next_child = next_child_after_finish(
            RepetitionAnchor::Deadline,
            2,
            Local.with_ymd_and_hms(2026, 5, 16, 9, 30, 15).unwrap(),
            Some(Local.with_ymd_and_hms(2026, 5, 16, 23, 59, 59).unwrap()),
            Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap(),
        );

        assert_eq!(
            next_child.get_start_time().unwrap(),
            Local.with_ymd_and_hms(2026, 5, 21, 9, 30, 15).unwrap()
        );
        assert_eq!(
            next_child.get_deadline_time_opt().unwrap(),
            Some(Local.with_ymd_and_hms(2026, 5, 23, 23, 59, 59).unwrap())
        );
    }

    #[test]
    fn complete_task_deadlineがない場合はcompletionにfallbackする() {
        let next_child = next_child_after_finish(
            RepetitionAnchor::Deadline,
            0,
            Local.with_ymd_and_hms(2026, 5, 16, 9, 30, 15).unwrap(),
            None,
            Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap(),
        );

        assert_eq!(
            next_child.get_deadline_time_opt().unwrap(),
            Some(Local.with_ymd_and_hms(2026, 5, 24, 23, 59, 59).unwrap())
        );
    }

    #[test]
    fn complete_task_繰り返し親のatomicを次回子タスクに引き継ぐ() {
        let parent_task = crate::test_support::new_task_handle("通勤").unwrap();
        parent_task
            .set_repetition_interval_days_opt(Some(7))
            .unwrap();
        parent_task.set_atomic(true).unwrap();
        parent_task
            .set_start_time(Local.with_ymd_and_hms(2026, 5, 10, 9, 0, 0).unwrap())
            .unwrap();
        parent_task
            .set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 5, 10, 10, 0, 0).unwrap()))
            .unwrap();

        let mut child_task_attr =
            TaskAttr::with_identity("通勤(5/16)", Uuid::new_v4(), fixed_now());
        child_task_attr.set_start_time(Local.with_ymd_and_hms(2026, 5, 16, 9, 0, 0).unwrap());
        child_task_attr
            .set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 5, 16, 10, 0, 0).unwrap()));
        let child_task = parent_task.create_as_last_child(child_task_attr);

        let finished_at = Local.with_ymd_and_hms(2026, 5, 16, 10, 0, 0).unwrap();
        let mut repository = TestTaskRepository::new(vec![parent_task.clone()], finished_at);
        complete_task_with_fresh_factory(
            &mut repository,
            CompleteTaskInput {
                task_id: child_task.get_id().unwrap(),
                finished_at,
                additional_actual_work_seconds: 0,
            },
        )
        .unwrap();

        let next_child = parent_task
            .get_children()
            .unwrap()
            .into_iter()
            .find(|task| task.get_status().unwrap() != Status::Done)
            .expect("next repetition child");
        assert!(next_child.get_atomic().unwrap());
    }

    #[test]
    fn complete_task_負の追加実績を拒否して変更しない() {
        let task = crate::test_support::new_task_handle("完了対象").unwrap();
        task.set_actual_work_seconds(120).unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());

        let actual = complete_task_with_fresh_factory(
            &mut repository,
            CompleteTaskInput {
                task_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: -1,
            },
        );

        assert!(matches!(
            actual,
            Err(ApplicationError::InvalidInput {
                field: "additional_actual_work_seconds",
                ..
            })
        ));
        let task = repository.get_by_id(task_id).unwrap().unwrap();
        assert_eq!(task.get_status().unwrap(), Status::Todo);
        assert_eq!(task.get_actual_work_seconds().unwrap(), 120);
        assert_eq!(task.get_end_time_opt().unwrap(), None);
    }

    #[test]
    fn complete_task_実績加算がoverflowする場合はerrorにして変更しない() {
        let task = crate::test_support::new_task_handle("完了対象").unwrap();
        task.set_actual_work_seconds(i64::MAX).unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());

        let actual = catch_unwind(AssertUnwindSafe(|| {
            complete_task_with_fresh_factory(
                &mut repository,
                CompleteTaskInput {
                    task_id,
                    finished_at: fixed_now(),
                    additional_actual_work_seconds: 1,
                },
            )
        }));

        assert!(matches!(
            actual,
            Ok(Err(ApplicationError::InvalidInput {
                field: "additional_actual_work_seconds",
                ..
            }))
        ));
        let task = repository.get_by_id(task_id).unwrap().unwrap();
        assert_eq!(task.get_status().unwrap(), Status::Todo);
        assert_eq!(task.get_actual_work_seconds().unwrap(), i64::MAX);
        assert_eq!(task.get_end_time_opt().unwrap(), None);
    }

    #[test]
    fn update_use_cases_見積もり締切カテゴリを設定して解除する() {
        let task = crate::test_support::new_task_handle("更新").unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());
        let deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();

        set_estimate(&mut repository, task_id, 45).unwrap();
        set_deadline(&mut repository, task_id, Some(deadline)).unwrap();
        set_category(&mut repository, task_id, Some(ProjectCategory::Recovery)).unwrap();

        let task = repository.get_by_id(task_id).unwrap().unwrap();
        assert_eq!(task.get_estimated_work_seconds().unwrap(), 45 * 60);
        assert_eq!(task.get_deadline_time_opt().unwrap(), Some(deadline));
        assert_eq!(
            task.get_project_category_opt().unwrap(),
            Some(ProjectCategory::Recovery)
        );

        set_deadline(&mut repository, task_id, None).unwrap();
        set_category(&mut repository, task_id, None).unwrap();
        assert_eq!(task.get_deadline_time_opt().unwrap(), None);
        assert_eq!(task.get_project_category_opt().unwrap(), None);
    }

    #[test]
    fn update_use_cases_未知uuidはtask_not_foundを返す() {
        let mut repository = TestTaskRepository::new(vec![], fixed_now());
        let task_id = Uuid::new_v4();

        assert_eq!(
            set_estimate(&mut repository, task_id, 10),
            Err(ApplicationError::TaskNotFound(task_id))
        );
        assert_eq!(
            breakdown_task_with_fresh_factory(
                &mut repository,
                BreakdownTaskInput {
                    parent_id: task_id,
                    names: vec!["子".to_string()],
                    pending_until: None,
                }
            ),
            Err(ApplicationError::TaskNotFound(task_id))
        );
        assert_eq!(
            defer_task(&mut repository, task_id, fixed_now()),
            Err(ApplicationError::TaskNotFound(task_id))
        );
        assert_eq!(
            complete_task_with_fresh_factory(
                &mut repository,
                CompleteTaskInput {
                    task_id,
                    finished_at: fixed_now(),
                    additional_actual_work_seconds: 0,
                }
            ),
            Err(ApplicationError::TaskNotFound(task_id))
        );
        assert_eq!(
            set_deadline(&mut repository, task_id, None),
            Err(ApplicationError::TaskNotFound(task_id))
        );
        assert_eq!(
            set_category(&mut repository, task_id, None),
            Err(ApplicationError::TaskNotFound(task_id))
        );
    }

    #[test]
    fn set_estimate_負数を拒否して変更しない() {
        let task = crate::test_support::new_task_handle("更新対象").unwrap();
        task.set_estimated_work_seconds(30 * 60).unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());

        let negative = set_estimate(&mut repository, task_id, -1);
        assert!(matches!(
            negative,
            Err(ApplicationError::InvalidInput {
                field: "estimated_work_minutes",
                ..
            })
        ));
        assert_eq!(
            repository
                .get_by_id(task_id)
                .unwrap()
                .unwrap()
                .get_estimated_work_seconds()
                .unwrap(),
            30 * 60
        );
    }

    #[test]
    fn set_estimate_秒変換がoverflowする場合はerrorにして変更しない() {
        let task = crate::test_support::new_task_handle("更新対象").unwrap();
        task.set_estimated_work_seconds(30 * 60).unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());

        let overflow = catch_unwind(AssertUnwindSafe(|| {
            set_estimate(&mut repository, task_id, i64::MAX)
        }));
        assert!(matches!(
            overflow,
            Ok(Err(ApplicationError::InvalidInput {
                field: "estimated_work_minutes",
                ..
            }))
        ));
        assert_eq!(
            repository
                .get_by_id(task_id)
                .unwrap()
                .unwrap()
                .get_estimated_work_seconds()
                .unwrap(),
            30 * 60
        );
    }

    #[test]
    fn write_use_cases_repositoryをsaveしない() {
        let root = crate::test_support::new_task_handle("親").unwrap();
        let root_id = root.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![root], fixed_now());

        let created_id = create_task_with_fresh_factory(
            &mut repository,
            CreateTaskInput {
                name: "新規".to_string(),
                estimated_work_minutes: None,
                pending_until: None,
            },
        )
        .unwrap();
        let child_id = breakdown_task_with_fresh_factory(
            &mut repository,
            BreakdownTaskInput {
                parent_id: root_id,
                names: vec!["子".to_string()],
                pending_until: None,
            },
        )
        .unwrap()[0];
        defer_task(&mut repository, child_id, fixed_now()).unwrap();
        set_estimate(&mut repository, child_id, 10).unwrap();
        set_deadline(&mut repository, child_id, Some(fixed_now())).unwrap();
        set_category(&mut repository, child_id, Some(ProjectCategory::Investment)).unwrap();
        complete_task_with_fresh_factory(
            &mut repository,
            CompleteTaskInput {
                task_id: child_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 0,
            },
        )
        .unwrap();

        assert!(repository.get_by_id(created_id).unwrap().is_some());
        assert_eq!(repository.save_count.get(), 0);
    }
}
