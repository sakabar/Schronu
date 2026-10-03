use crate::application::daily_capacity::{try_logical_date_start, try_next_logical_date_start};
use crate::application::interface::TaskRepositoryTrait;
use crate::application::task_list::{
    list_tasks, ListTasksFilter, TaskPeriodField, TaskPeriodFilter,
};
use crate::application::task_use_case::ApplicationError;
use crate::entity::task::Status;
use chrono::{DateTime, Local, NaiveDate};
use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CompletedTaskReportRow {
    pub task_id: Uuid,
    pub task_name: String,
    pub project_name: String,
    pub completed_at: DateTime<Local>,
    pub actual_work_seconds: i64,
    pub estimated_work_seconds: i64,
}

pub fn list_completed_task_report(
    repository: &dyn TaskRepositoryTrait,
    logical_date: NaiveDate,
) -> Result<Vec<CompletedTaskReportRow>, ApplicationError> {
    let from = try_logical_date_start(logical_date)?;
    let until = try_next_logical_date_start(from)?;
    let project_names = repository
        .get_all_projects()
        .into_iter()
        .map(|project| {
            Ok((
                project.get_id().map_err(ApplicationError::TaskTree)?,
                project.get_name().map_err(ApplicationError::TaskTree)?,
            ))
        })
        .collect::<Result<HashMap<_, _>, ApplicationError>>()?;

    let tasks = list_tasks(
        repository,
        ListTasksFilter {
            period: Some(TaskPeriodFilter {
                field: TaskPeriodField::CompletedAt,
                from,
                until,
            }),
            statuses: vec![Status::Done],
            categories: vec![],
        },
    )?;

    let mut rows = tasks
        .into_iter()
        .map(|task| {
            let project_name = project_names
                .get(&task.root_id)
                .cloned()
                .ok_or(ApplicationError::TaskNotFound(task.root_id))?;
            let completed_at = task
                .end_time
                .expect("completed-at period filter only returns tasks with completion times");
            Ok(CompletedTaskReportRow {
                task_id: task.id,
                task_name: task.name,
                project_name,
                completed_at,
                actual_work_seconds: task.actual_work_seconds,
                estimated_work_seconds: task.estimated_work_seconds,
            })
        })
        .collect::<Result<Vec<_>, ApplicationError>>()?;
    rows.sort_by_key(|row| (row.completed_at, row.task_id));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::list_completed_task_report;
    use crate::entity::task::{Status, TaskAttr, TaskHandle};
    use crate::test_support::{new_task_attr_at, new_task_handle_at, TestTaskRepository};
    use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone};
    use uuid::Uuid;

    fn local_time(day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 8, day, hour, minute, second)
            .unwrap()
    }

    fn completed_task(
        name: &str,
        completed_at: DateTime<Local>,
        actual_work_seconds: i64,
        estimated_work_seconds: i64,
    ) -> TaskHandle {
        let task = new_task_handle_at(name, completed_at).unwrap();
        task.set_orig_status(Status::Done).unwrap();
        task.set_end_time_opt(Some(completed_at)).unwrap();
        task.set_actual_work_seconds(actual_work_seconds).unwrap();
        task.set_estimated_work_seconds(estimated_work_seconds)
            .unwrap();
        task
    }

    fn completed_attr(
        name: &str,
        completed_at: DateTime<Local>,
        actual_work_seconds: i64,
        estimated_work_seconds: i64,
    ) -> TaskAttr {
        let mut attr = new_task_attr_at(name, completed_at);
        attr.set_orig_status(Status::Done);
        attr.set_end_time_opt(Some(completed_at));
        attr.set_actual_work_seconds(actual_work_seconds);
        attr.set_estimated_work_seconds(estimated_work_seconds);
        attr
    }

    #[test]
    fn logical_dateの半開区間にあるdoneだけを返す() {
        let logical_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let before = completed_task("before", local_time(11, 5, 59, 59), 1, 2);
        let at_start = completed_task("at start", local_time(11, 6, 0, 0), 3, 4);
        let before_end = completed_task("before end", local_time(12, 5, 59, 59), 5, 6);
        let at_end = completed_task("at end", local_time(12, 6, 0, 0), 7, 8);
        let todo = new_task_handle_at("todo", local_time(11, 12, 0, 0)).unwrap();
        todo.set_end_time_opt(Some(local_time(11, 12, 0, 0)))
            .unwrap();
        let now = local_time(11, 12, 0, 0);
        let repository = TestTaskRepository::new(
            vec![before, at_start.clone(), before_end.clone(), at_end, todo],
            now,
        );

        let report = list_completed_task_report(&repository, logical_date).unwrap();

        assert_eq!(
            report.iter().map(|row| row.task_id).collect::<Vec<_>>(),
            vec![at_start.get_id().unwrap(), before_end.get_id().unwrap()]
        );
    }

    #[test]
    fn rootと親を含めproject名と作業時間を保持して完了時刻とuuid順に並べる() {
        let logical_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let same_time = local_time(11, 10, 0, 0);
        let root = completed_task("project root", local_time(11, 11, 0, 0), 41, 42);
        let parent = root
            .create_child(completed_attr("parent", same_time, 51, 52))
            .unwrap();
        let earlier_child = parent
            .create_child(completed_attr(
                "earlier child",
                local_time(11, 9, 0, 0),
                11,
                12,
            ))
            .unwrap();
        let mut higher_id_attr = completed_attr("higher id child", same_time, 31, 32);
        higher_id_attr.set_id(Uuid::from_u128(2));
        let higher_id_child = parent.create_child(higher_id_attr).unwrap();
        let mut lower_id_attr = completed_attr("lower id child", same_time, 21, 22);
        lower_id_attr.set_id(Uuid::from_u128(1));
        let lower_id_child = parent.create_child(lower_id_attr).unwrap();
        let repository =
            TestTaskRepository::new(vec![root.clone()], same_time + Duration::hours(2));

        let report = list_completed_task_report(&repository, logical_date).unwrap();

        assert_eq!(
            report.iter().map(|row| row.task_id).collect::<Vec<_>>(),
            vec![
                earlier_child.get_id().unwrap(),
                lower_id_child.get_id().unwrap(),
                higher_id_child.get_id().unwrap(),
                parent.get_id().unwrap(),
                root.get_id().unwrap(),
            ]
        );
        assert!(report.iter().all(|row| row.project_name == "project root"));
        assert_eq!(report[0].task_name, "earlier child");
        assert_eq!(report[0].completed_at, local_time(11, 9, 0, 0));
        assert_eq!(report[0].actual_work_seconds, 11);
        assert_eq!(report[0].estimated_work_seconds, 12);
    }
}
