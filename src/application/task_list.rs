use crate::application::interface::TaskRepositoryTrait;
use crate::application::schedule_use_case::get_schedule;
use crate::application::task_use_case::ApplicationError;
use crate::application::task_view::TaskView;
use crate::entity::task::{ProjectCategory, Status, TaskHandle, TaskTreeError};
use chrono::{DateTime, Local};
use std::collections::HashSet;

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

#[cfg(test)]
#[path = "list_tasks_contract_tests.rs"]
mod contract_tests;
