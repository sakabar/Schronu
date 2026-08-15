use crate::application::interface::TaskRepositoryTrait;
use crate::application::schedule_use_case::get_schedule;
pub use crate::application::task_view::TaskView;
use crate::entity::datetime::get_next_morning_datetime;
use crate::entity::task::{
    ProjectCategory, RepetitionAnchor, Status, TaskAttr, TaskHandle, TaskTreeError,
};
use chrono::{DateTime, Datelike, Duration, Local, Timelike};
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
        }
    }
}

impl Error for ApplicationError {}

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
) -> Result<Uuid, ApplicationError> {
    validate_task_name(&input.name, "name")?;

    let root_task = TaskHandle::new(&input.name).map_err(ApplicationError::TaskTree)?;
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
        let mut child_attr = TaskAttr::new(&name);
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

    task.set_actual_work_seconds(actual_work_seconds)
        .map_err(ApplicationError::TaskTree)?;
    task.set_orig_status(Status::Done)
        .map_err(ApplicationError::TaskTree)?;
    task.set_end_time_opt(Some(input.finished_at))
        .map_err(ApplicationError::TaskTree)?;

    let next_repetition_task_id = create_next_repetition_task(&task, input.finished_at)?;
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

fn create_next_repetition_task(
    task: &TaskHandle,
    finished_at: DateTime<Local>,
) -> Result<Option<Uuid>, ApplicationError> {
    let Some(parent_task) = task.parent().map_err(ApplicationError::TaskTree)? else {
        return Ok(None);
    };
    let Some(repetition_interval_days) = parent_task
        .get_repetition_interval_days_opt()
        .map_err(ApplicationError::TaskTree)?
    else {
        return Ok(None);
    };

    adjust_repetition_estimate(&parent_task, task).map_err(ApplicationError::TaskTree)?;
    let new_task_attr =
        build_next_repetition_task_attr(task, &parent_task, repetition_interval_days, finished_at)
            .map_err(ApplicationError::TaskTree)?;
    let next_task = parent_task
        .create_child(new_task_attr)
        .map_err(ApplicationError::TaskTree)?;
    Ok(Some(
        next_task.get_id().map_err(ApplicationError::TaskTree)?,
    ))
}

fn adjust_repetition_estimate(
    parent_task: &TaskHandle,
    task: &TaskHandle,
) -> Result<(), TaskTreeError> {
    if task.get_actual_work_seconds()? <= 0 {
        return Ok(());
    }

    let original_estimated_seconds = parent_task.get_estimated_work_seconds()?;
    let difference = task.get_actual_work_seconds()? - original_estimated_seconds;
    let new_estimated_work_seconds = match difference.cmp(&0) {
        Ordering::Greater => original_estimated_seconds + difference * 3 / 4,
        Ordering::Less => max(60, original_estimated_seconds + difference / 4),
        Ordering::Equal => original_estimated_seconds,
    };
    parent_task.set_estimated_work_seconds(new_estimated_work_seconds)?;
    Ok(())
}

fn apply_time_template(
    base_datetime: DateTime<Local>,
    time_template: DateTime<Local>,
) -> DateTime<Local> {
    base_datetime
        .with_hour(time_template.hour())
        .expect("invalid hour")
        .with_minute(time_template.minute())
        .expect("invalid minute")
        .with_second(time_template.second())
        .expect("invalid second")
        .with_nanosecond(0)
        .expect("invalid nanosecond")
}

fn build_next_repetition_task_attr(
    task: &TaskHandle,
    parent_task: &TaskHandle,
    repetition_interval_days: i64,
    finished_at: DateTime<Local>,
) -> Result<TaskAttr, TaskTreeError> {
    let occurrence_anchor = match parent_task.get_repetition_anchor()? {
        RepetitionAnchor::Deadline => task.get_deadline_time_opt()?.unwrap_or(finished_at),
        RepetitionAnchor::Completion => finished_at,
    };
    let next_occurrence_day =
        get_next_morning_datetime(occurrence_anchor) + Duration::days(repetition_interval_days - 1);
    let new_start_time = apply_time_template(next_occurrence_day, parent_task.get_start_time()?);
    let new_deadline_time = match parent_task.get_deadline_time_opt()? {
        Some(parent_deadline_time) => {
            apply_time_template(next_occurrence_day, parent_deadline_time)
        }
        None => new_start_time
            .with_hour(23)
            .expect("invalid hour")
            .with_minute(59)
            .expect("invalid minute")
            .with_second(59)
            .expect("invalid second")
            .with_nanosecond(0)
            .expect("invalid nanosecond"),
    };

    let mut new_task_attr = TaskAttr::new(&format!(
        "{}({}/{})",
        parent_task.get_name()?,
        new_start_time.month(),
        new_start_time.day()
    ));
    new_task_attr
        .set_start_time(new_start_time - Duration::days(parent_task.get_days_in_advance()?));
    new_task_attr.set_deadline_time_opt(Some(new_deadline_time));
    new_task_attr.set_estimated_work_seconds(parent_task.get_estimated_work_seconds()?);
    new_task_attr.set_atomic(parent_task.get_atomic()?);
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

    fn next_child_after_finish(
        repetition_anchor: RepetitionAnchor,
        days_in_advance: i64,
        focused_start_time: DateTime<Local>,
        focused_deadline_time_opt: Option<DateTime<Local>>,
        finished_at: DateTime<Local>,
    ) -> TaskHandle {
        let parent_task = TaskHandle::new("ルーチン").unwrap();
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

        let mut child_task_attr = TaskAttr::new("ルーチン(5/16)");
        child_task_attr.set_start_time(focused_start_time);
        child_task_attr.set_deadline_time_opt(focused_deadline_time_opt);
        let child_task = parent_task.create_as_last_child(child_task_attr);

        let mut repository = TestTaskRepository::new(vec![parent_task.clone()], finished_at);
        complete_task(
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
        let root = TaskHandle::new("親").unwrap();
        root.set_priority(5).unwrap();
        root.set_project_category_opt(Some(ProjectCategory::Investment))
            .unwrap();
        let child = root.create_as_last_child(TaskAttr::new("子"));
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
        let root = TaskHandle::new("全属性").unwrap();
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
        let child = root.create_as_last_child(TaskAttr::new("子"));
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
        let task = TaskHandle::new("未延期").unwrap();
        let repository = TestTaskRepository::new(vec![task.clone()], fixed_now());

        let actual = get_task(&repository, task.get_id().unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(actual.original_status, Status::Todo);
        assert_eq!(actual.pending_until, None);
    }

    #[test]
    fn get_focus_最高優先度leafのviewを返す() {
        let root = TaskHandle::new("親").unwrap();
        let child = root.create_as_last_child(TaskAttr::new("子"));
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

        let task_id = create_task(
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
    fn create_task_空の名前を拒否して変更しない() {
        let mut repository = TestTaskRepository::new(vec![], fixed_now());

        let actual = create_task(
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

            let actual = create_task(
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

        let actual = create_task(
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
            create_task(
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
        let parent = TaskHandle::new("親").unwrap();
        let deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
        parent.set_deadline_time_opt(Some(deadline)).unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let child_ids = breakdown_task(
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
    fn breakdown_task_全ての子を指定時刻までpendingにする() {
        let parent = TaskHandle::new("親").unwrap();
        let pending_until = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let child_ids = breakdown_task(
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
        let parent = TaskHandle::new("親").unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let actual = breakdown_task(
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
        let parent = TaskHandle::new("親").unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let actual = breakdown_task(
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
        let parent = TaskHandle::new("親").unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let actual = breakdown_task(
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
        let task = TaskHandle::new("延期").unwrap();
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
        let task = TaskHandle::new("親").unwrap();
        task.create_as_last_child(TaskAttr::new("未完了"));
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());

        let actual = complete_task(
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
        let task = TaskHandle::new("完了").unwrap();
        task.set_actual_work_seconds(60).unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());

        let output = complete_task(
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
    fn create_next_repetition_taskは構造化errorを返せるresultを維持する() {
        let task = TaskHandle::new("単発").unwrap();

        let actual: Result<Option<Uuid>, ApplicationError> =
            create_next_repetition_task(&task, fixed_now());

        assert_eq!(actual, Ok(None));
    }

    #[test]
    fn complete_task_繰り返しtaskを生成して見積もりを補正する() {
        let parent = TaskHandle::new("ルーチン").unwrap();
        parent.set_repetition_interval_days_opt(Some(7)).unwrap();
        parent.set_estimated_work_seconds(600).unwrap();
        let child = parent.create_as_last_child(TaskAttr::new("今回"));
        child.set_actual_work_seconds(1000).unwrap();
        let child_id = child.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let output = complete_task(
            &mut repository,
            CompleteTaskInput {
                task_id: child_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 0,
            },
        )
        .unwrap();

        assert_eq!(parent.get_estimated_work_seconds().unwrap(), 900);
        assert!(output.next_repetition_task_id.is_some());
        assert_eq!(parent.get_children().unwrap().len(), 2);
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
        let parent_task = TaskHandle::new("通勤").unwrap();
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

        let mut child_task_attr = TaskAttr::new("通勤(5/16)");
        child_task_attr.set_start_time(Local.with_ymd_and_hms(2026, 5, 16, 9, 0, 0).unwrap());
        child_task_attr
            .set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 5, 16, 10, 0, 0).unwrap()));
        let child_task = parent_task.create_as_last_child(child_task_attr);

        let finished_at = Local.with_ymd_and_hms(2026, 5, 16, 10, 0, 0).unwrap();
        let mut repository = TestTaskRepository::new(vec![parent_task.clone()], finished_at);
        complete_task(
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
        let task = TaskHandle::new("完了対象").unwrap();
        task.set_actual_work_seconds(120).unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());

        let actual = complete_task(
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
        let task = TaskHandle::new("完了対象").unwrap();
        task.set_actual_work_seconds(i64::MAX).unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![task], fixed_now());

        let actual = catch_unwind(AssertUnwindSafe(|| {
            complete_task(
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
        let task = TaskHandle::new("更新").unwrap();
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
            breakdown_task(
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
            complete_task(
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
        let task = TaskHandle::new("更新対象").unwrap();
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
        let task = TaskHandle::new("更新対象").unwrap();
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
        let root = TaskHandle::new("親").unwrap();
        let root_id = root.get_id().unwrap();
        let mut repository = TestTaskRepository::new(vec![root], fixed_now());

        let created_id = create_task(
            &mut repository,
            CreateTaskInput {
                name: "新規".to_string(),
                estimated_work_minutes: None,
                pending_until: None,
            },
        )
        .unwrap();
        let child_id = breakdown_task(
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
        complete_task(
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
