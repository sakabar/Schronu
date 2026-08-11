use crate::application::interface::TaskRepositoryTrait;
use crate::entity::datetime::get_next_morning_datetime;
use crate::entity::task::{ProjectCategory, RepetitionAnchor, Status, Task, TaskAttr};
use chrono::{DateTime, Datelike, Duration, Local, Timelike};
use std::cmp::{max, Ordering};
use std::error::Error;
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq)]
pub struct TaskView {
    pub id: Uuid,
    pub root_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub child_ids: Vec<Uuid>,
    pub name: String,
    pub status: Status,
    pub original_status: Status,
    pub is_on_other_side: bool,
    pub atomic: bool,
    pub pending_until: DateTime<Local>,
    pub priority: i64,
    pub create_time: DateTime<Local>,
    pub start_time: DateTime<Local>,
    pub end_time: Option<DateTime<Local>>,
    pub deadline_time: Option<DateTime<Local>>,
    pub estimated_work_seconds: i64,
    pub actual_work_seconds: i64,
    pub repetition_interval_days: Option<i64>,
    pub repetition_anchor: RepetitionAnchor,
    pub days_in_advance: i64,
    pub project_category: Option<ProjectCategory>,
}

impl From<&Task> for TaskView {
    fn from(task: &Task) -> Self {
        Self {
            id: task.get_id(),
            root_id: task.root().get_id(),
            parent_id: task.parent().map(|parent| parent.get_id()),
            child_ids: task.get_children().iter().map(Task::get_id).collect(),
            name: task.get_name(),
            status: task.get_status(),
            original_status: task.get_orig_status(),
            is_on_other_side: task.get_is_on_other_side(),
            atomic: task.get_atomic(),
            pending_until: task.get_pending_until(),
            priority: task.get_priority(),
            create_time: task.get_create_time(),
            start_time: task.get_start_time(),
            end_time: task.get_end_time_opt(),
            deadline_time: task.get_deadline_time_opt(),
            estimated_work_seconds: task.get_estimated_work_seconds(),
            actual_work_seconds: task.get_actual_work_seconds(),
            repetition_interval_days: task.get_repetition_interval_days_opt(),
            repetition_anchor: task.get_repetition_anchor(),
            days_in_advance: task.get_days_in_advance(),
            project_category: task.get_project_category_opt(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ApplicationError {
    TaskNotFound(Uuid),
    InvalidInput {
        field: &'static str,
        reason: &'static str,
    },
    HasUndoneChildren(Uuid),
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

pub fn get_focus(repository: &mut dyn TaskRepositoryTrait) -> Option<TaskView> {
    repository
        .get_highest_priority_leaf_task_id()
        .and_then(|task_id| get_task(repository, task_id))
}

pub fn get_task(repository: &dyn TaskRepositoryTrait, task_id: Uuid) -> Option<TaskView> {
    repository.get_by_id(task_id).as_ref().map(TaskView::from)
}

pub fn create_task(
    repository: &mut dyn TaskRepositoryTrait,
    input: CreateTaskInput,
) -> Result<Uuid, ApplicationError> {
    if input.name.is_empty() {
        return Err(ApplicationError::InvalidInput {
            field: "name",
            reason: "must not be empty",
        });
    }

    let root_task = Task::new(&input.name);
    root_task.set_priority(5);

    if let Some(pending_until) = input.pending_until {
        root_task.set_pending_until(pending_until);
        root_task.set_orig_status(Status::Pending);
    }

    if let Some(estimated_work_minutes) = input.estimated_work_minutes {
        root_task.set_estimated_work_seconds(estimated_work_minutes * 60);
    }

    let task_id = root_task.get_id();
    repository.start_new_project(root_task);
    Ok(task_id)
}

pub fn breakdown_task(
    repository: &dyn TaskRepositoryTrait,
    input: BreakdownTaskInput,
) -> Result<Vec<Uuid>, ApplicationError> {
    if input.names.is_empty() {
        return Err(ApplicationError::InvalidInput {
            field: "names",
            reason: "must not be empty",
        });
    }
    if input.names.iter().any(|name| name.parse::<i64>().is_ok()) {
        return Err(ApplicationError::InvalidInput {
            field: "names",
            reason: "must not contain an integer-only name",
        });
    }

    let parent_task = find_task(repository, input.parent_id)?;
    let mut child_ids = Vec::with_capacity(input.names.len());

    for name in input.names {
        let mut child_attr = TaskAttr::new(&name);
        if let Some(pending_until) = input.pending_until {
            child_attr.set_orig_status(Status::Pending);
            child_attr.set_pending_until(pending_until);
        }

        let child_task = parent_task.create_as_last_child(child_attr);
        if let Some(deadline_time) = parent_task.get_deadline_time_opt() {
            child_task.set_deadline_time_opt(Some(deadline_time));
        }
        child_ids.push(child_task.get_id());
    }

    Ok(child_ids)
}

pub fn defer_task(
    repository: &dyn TaskRepositoryTrait,
    task_id: Uuid,
    pending_until: DateTime<Local>,
) -> Result<(), ApplicationError> {
    let task = find_task(repository, task_id)?;
    task.set_pending_until(pending_until);
    task.set_orig_status(Status::Pending);
    Ok(())
}

pub fn complete_task(
    repository: &dyn TaskRepositoryTrait,
    input: CompleteTaskInput,
) -> Result<CompleteTaskOutput, ApplicationError> {
    let task = find_task(repository, input.task_id)?;
    if task.has_undone_children() {
        return Err(ApplicationError::HasUndoneChildren(input.task_id));
    }

    task.set_actual_work_seconds(
        task.get_actual_work_seconds() + input.additional_actual_work_seconds,
    );
    task.set_orig_status(Status::Done);
    task.set_end_time_opt(Some(input.finished_at));

    let next_repetition_task_id = create_next_repetition_task(&task, input.finished_at);
    let next_focus_task_id = if task.all_sibling_tasks_are_all_done() {
        task.parent().map(|parent| parent.get_id())
    } else {
        None
    };

    Ok(CompleteTaskOutput {
        next_focus_task_id,
        next_repetition_task_id,
    })
}

pub fn set_estimate(
    repository: &dyn TaskRepositoryTrait,
    task_id: Uuid,
    estimated_work_minutes: i64,
) -> Result<(), ApplicationError> {
    let task = find_task(repository, task_id)?;
    task.set_estimated_work_seconds(estimated_work_minutes * 60);
    Ok(())
}

pub fn set_deadline(
    repository: &dyn TaskRepositoryTrait,
    task_id: Uuid,
    deadline_time: Option<DateTime<Local>>,
) -> Result<(), ApplicationError> {
    let task = find_task(repository, task_id)?;
    match deadline_time {
        Some(deadline_time) => task.set_deadline_time_opt(Some(deadline_time)),
        None => task.unset_deadline_time_opt(),
    }
    Ok(())
}

pub fn set_category(
    repository: &dyn TaskRepositoryTrait,
    task_id: Uuid,
    project_category: Option<ProjectCategory>,
) -> Result<(), ApplicationError> {
    let task = find_task(repository, task_id)?;
    task.set_project_category_opt(project_category);
    Ok(())
}

fn find_task(
    repository: &dyn TaskRepositoryTrait,
    task_id: Uuid,
) -> Result<Task, ApplicationError> {
    repository
        .get_by_id(task_id)
        .ok_or(ApplicationError::TaskNotFound(task_id))
}

fn create_next_repetition_task(task: &Task, finished_at: DateTime<Local>) -> Option<Uuid> {
    let parent_task = task.parent()?;
    let repetition_interval_days = parent_task.get_repetition_interval_days_opt()?;

    adjust_repetition_estimate(&parent_task, task);
    let new_task_attr =
        build_next_repetition_task_attr(task, &parent_task, repetition_interval_days, finished_at);
    Some(parent_task.create_as_last_child(new_task_attr).get_id())
}

fn adjust_repetition_estimate(parent_task: &Task, task: &Task) {
    if task.get_actual_work_seconds() <= 0 {
        return;
    }

    let original_estimated_seconds = parent_task.get_estimated_work_seconds();
    let difference = task.get_actual_work_seconds() - original_estimated_seconds;
    let new_estimated_work_seconds = match difference.cmp(&0) {
        Ordering::Greater => original_estimated_seconds + difference * 3 / 4,
        Ordering::Less => max(60, original_estimated_seconds + difference / 4),
        Ordering::Equal => original_estimated_seconds,
    };
    parent_task.set_estimated_work_seconds(new_estimated_work_seconds);
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
    task: &Task,
    parent_task: &Task,
    repetition_interval_days: i64,
    finished_at: DateTime<Local>,
) -> TaskAttr {
    let occurrence_anchor = match parent_task.get_repetition_anchor() {
        RepetitionAnchor::Deadline => task.get_deadline_time_opt().unwrap_or(finished_at),
        RepetitionAnchor::Completion => finished_at,
    };
    let next_occurrence_day =
        get_next_morning_datetime(occurrence_anchor) + Duration::days(repetition_interval_days - 1);
    let new_start_time = apply_time_template(next_occurrence_day, parent_task.get_start_time());
    let new_deadline_time = match parent_task.get_deadline_time_opt() {
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
        parent_task.get_name(),
        new_start_time.month(),
        new_start_time.day()
    ));
    new_task_attr
        .set_start_time(new_start_time - Duration::days(parent_task.get_days_in_advance()));
    new_task_attr.set_deadline_time_opt(Some(new_deadline_time));
    new_task_attr.set_estimated_work_seconds(parent_task.get_estimated_work_seconds());
    new_task_attr.set_atomic(parent_task.get_atomic());
    new_task_attr
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::cell::Cell;

    struct TestTaskRepository {
        projects: Vec<Task>,
        now: DateTime<Local>,
        highest_priority_leaf_task_id: Option<Uuid>,
        save_count: Cell<usize>,
    }

    impl TestTaskRepository {
        fn new(projects: Vec<Task>, now: DateTime<Local>) -> Self {
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

        fn get_all_projects(&self) -> Vec<&Task> {
            self.projects.iter().collect()
        }

        fn load(&mut self) {}

        fn save(&self) {
            self.save_count.set(self.save_count.get() + 1);
        }

        fn sync_clock(&mut self, now: DateTime<Local>) {
            self.now = now;
        }

        fn get_last_synced_time(&self) -> DateTime<Local> {
            self.now
        }

        fn get_highest_priority_project(&mut self) -> Option<&Task> {
            self.projects.first()
        }

        fn get_highest_priority_leaf_task_id(&mut self) -> Option<Uuid> {
            self.highest_priority_leaf_task_id
        }

        fn get_defer_candidate_leaf_task_id(&mut self, _recent_days: i64) -> Option<Uuid> {
            None
        }

        fn get_by_id(&self, id: Uuid) -> Option<Task> {
            self.projects.iter().find_map(|task| task.get_by_id(id))
        }

        fn start_new_project(&mut self, root_task: Task) {
            self.projects.push(root_task);
        }
    }

    fn fixed_now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap()
    }

    #[test]
    fn get_task_親子関係を含むviewを返す() {
        let root = Task::new("親");
        root.set_priority(5);
        root.set_project_category_opt(Some(ProjectCategory::Investment));
        let child = root.create_as_last_child(TaskAttr::new("子"));
        let repository = TestTaskRepository::new(vec![root.clone()], fixed_now());

        let actual = get_task(&repository, child.get_id()).unwrap();

        assert_eq!(actual.id, child.get_id());
        assert_eq!(actual.root_id, root.get_id());
        assert_eq!(actual.parent_id, Some(root.get_id()));
        assert!(actual.child_ids.is_empty());
        assert_eq!(actual.name, "子");
        assert_eq!(actual.priority, 5);
        assert_eq!(actual.project_category, Some(ProjectCategory::Investment));
    }

    #[test]
    fn get_task_未知uuidはnoneを返す() {
        let repository = TestTaskRepository::new(vec![], fixed_now());
        assert_eq!(get_task(&repository, Uuid::new_v4()), None);
    }

    #[test]
    fn get_focus_最高優先度leafのviewを返す() {
        let root = Task::new("親");
        let child = root.create_as_last_child(TaskAttr::new("子"));
        let mut repository = TestTaskRepository::new(vec![root], fixed_now());
        repository.highest_priority_leaf_task_id = Some(child.get_id());

        assert_eq!(get_focus(&mut repository).unwrap().id, child.get_id());
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

        let task = repository.get_by_id(task_id).unwrap();
        assert_eq!(task.get_name(), "新規");
        assert_eq!(task.get_priority(), 5);
        assert_eq!(task.get_estimated_work_seconds(), 30 * 60);
        assert_eq!(task.get_orig_status(), Status::Pending);
        assert_eq!(task.get_pending_until(), pending_until);
        assert_eq!(repository.save_count.get(), 0);
    }

    #[test]
    fn breakdown_task_入力順と締切を維持する() {
        let parent = Task::new("親");
        let deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
        parent.set_deadline_time_opt(Some(deadline));
        let repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let child_ids = breakdown_task(
            &repository,
            BreakdownTaskInput {
                parent_id: parent.get_id(),
                names: vec!["一".to_string(), "二".to_string()],
                pending_until: None,
            },
        )
        .unwrap();

        assert_eq!(
            parent
                .get_children()
                .iter()
                .map(Task::get_name)
                .collect::<Vec<_>>(),
            vec!["一", "二"]
        );
        assert_eq!(child_ids.len(), 2);
        assert!(parent
            .get_children()
            .iter()
            .all(|child| child.get_deadline_time_opt() == Some(deadline)));
    }

    #[test]
    fn breakdown_task_数値名を含む場合は変更しない() {
        let parent = Task::new("親");
        let repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let actual = breakdown_task(
            &repository,
            BreakdownTaskInput {
                parent_id: parent.get_id(),
                names: vec!["子".to_string(), "10".to_string()],
                pending_until: None,
            },
        );

        assert!(matches!(actual, Err(ApplicationError::InvalidInput { .. })));
        assert!(parent.get_children().is_empty());
    }

    #[test]
    fn defer_task_絶対時刻までpendingにする() {
        let task = Task::new("延期");
        let task_id = task.get_id();
        let repository = TestTaskRepository::new(vec![task], fixed_now());
        let pending_until = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 1).unwrap();

        defer_task(&repository, task_id, pending_until).unwrap();

        let task = repository.get_by_id(task_id).unwrap();
        assert_eq!(task.get_orig_status(), Status::Pending);
        assert_eq!(task.get_pending_until(), pending_until);
    }

    #[test]
    fn complete_task_未完了の子があれば変更しない() {
        let task = Task::new("親");
        task.create_as_last_child(TaskAttr::new("未完了"));
        let task_id = task.get_id();
        let repository = TestTaskRepository::new(vec![task], fixed_now());

        let actual = complete_task(
            &repository,
            CompleteTaskInput {
                task_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 120,
            },
        );

        assert_eq!(actual, Err(ApplicationError::HasUndoneChildren(task_id)));
        let task = repository.get_by_id(task_id).unwrap();
        assert_eq!(task.get_status(), Status::Todo);
        assert_eq!(task.get_actual_work_seconds(), 0);
    }

    #[test]
    fn complete_task_実績を加算して完了する() {
        let task = Task::new("完了");
        task.set_actual_work_seconds(60);
        let task_id = task.get_id();
        let repository = TestTaskRepository::new(vec![task], fixed_now());

        let output = complete_task(
            &repository,
            CompleteTaskInput {
                task_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 120,
            },
        )
        .unwrap();

        let task = repository.get_by_id(task_id).unwrap();
        assert_eq!(task.get_status(), Status::Done);
        assert_eq!(task.get_end_time_opt(), Some(fixed_now()));
        assert_eq!(task.get_actual_work_seconds(), 180);
        assert_eq!(output.next_focus_task_id, None);
        assert_eq!(output.next_repetition_task_id, None);
    }

    #[test]
    fn complete_task_繰り返しtaskを生成して見積もりを補正する() {
        let parent = Task::new("ルーチン");
        parent.set_repetition_interval_days_opt(Some(7));
        parent.set_estimated_work_seconds(600);
        let child = parent.create_as_last_child(TaskAttr::new("今回"));
        child.set_actual_work_seconds(1000);
        let child_id = child.get_id();
        let repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

        let output = complete_task(
            &repository,
            CompleteTaskInput {
                task_id: child_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 0,
            },
        )
        .unwrap();

        assert_eq!(parent.get_estimated_work_seconds(), 900);
        assert!(output.next_repetition_task_id.is_some());
        assert_eq!(parent.get_children().len(), 2);
    }

    #[test]
    fn update_use_cases_見積もり締切カテゴリを設定して解除する() {
        let task = Task::new("更新");
        let task_id = task.get_id();
        let repository = TestTaskRepository::new(vec![task], fixed_now());
        let deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();

        set_estimate(&repository, task_id, 45).unwrap();
        set_deadline(&repository, task_id, Some(deadline)).unwrap();
        set_category(&repository, task_id, Some(ProjectCategory::Recovery)).unwrap();

        let task = repository.get_by_id(task_id).unwrap();
        assert_eq!(task.get_estimated_work_seconds(), 45 * 60);
        assert_eq!(task.get_deadline_time_opt(), Some(deadline));
        assert_eq!(
            task.get_project_category_opt(),
            Some(ProjectCategory::Recovery)
        );

        set_deadline(&repository, task_id, None).unwrap();
        set_category(&repository, task_id, None).unwrap();
        assert_eq!(task.get_deadline_time_opt(), None);
        assert_eq!(task.get_project_category_opt(), None);
    }

    #[test]
    fn update_use_cases_未知uuidはtask_not_foundを返す() {
        let repository = TestTaskRepository::new(vec![], fixed_now());
        let task_id = Uuid::new_v4();

        assert_eq!(
            set_estimate(&repository, task_id, 10),
            Err(ApplicationError::TaskNotFound(task_id))
        );
    }
}
