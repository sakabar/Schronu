use crate::application::daily_capacity::{
    try_logical_date, try_logical_date_end, try_logical_date_start, try_next_logical_date_start,
};
use crate::application::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CompletedTaskReport {
    pub rows: Vec<CompletedTaskReportRow>,
    pub total_actual_work_seconds: i64,
    pub available_seconds: i64,
    pub recorded_percentage: Option<i64>,
}

pub fn build_completed_task_report(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    logical_date: NaiveDate,
    end_of_day_offset_minutes: i64,
) -> Result<CompletedTaskReport, ApplicationError> {
    let rows = list_completed_task_report(repository, logical_date)?;
    let total_actual_work_seconds_i128 = rows
        .iter()
        .map(|row| i128::from(row.actual_work_seconds))
        .sum::<i128>();
    let total_actual_work_seconds =
        i64::try_from(total_actual_work_seconds_i128).map_err(|_| {
            ApplicationError::CompletedReportCalculationOverflow {
                operation: "total_actual_work_seconds",
                value: total_actual_work_seconds_i128,
            }
        })?;
    let start = try_logical_date_start(logical_date)?;
    let end = try_logical_date_end(logical_date, end_of_day_offset_minutes)?;
    let observed_at = repository.get_last_synced_time();
    let availability_end = if try_logical_date(observed_at)? == logical_date {
        observed_at.min(end)
    } else {
        end
    };
    let available_seconds = free_time_manager.get_free_seconds(&start, &availability_end);
    let recorded_percentage = if available_seconds == 0 {
        None
    } else {
        let numerator = total_actual_work_seconds_i128 * 100;
        let denominator = i128::from(available_seconds);
        let rounded = (numerator + denominator / 2) / denominator;
        Some(i64::try_from(rounded).map_err(|_| {
            ApplicationError::CompletedReportCalculationOverflow {
                operation: "recorded_percentage",
                value: rounded,
            }
        })?)
    };

    Ok(CompletedTaskReport {
        rows,
        total_actual_work_seconds,
        available_seconds,
        recorded_percentage,
    })
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
                actual_work_seconds: if task.actual_work_seconds == 0 {
                    task.estimated_work_seconds
                } else {
                    task.actual_work_seconds
                },
                estimated_work_seconds: task.estimated_work_seconds,
            })
        })
        .collect::<Result<Vec<_>, ApplicationError>>()?;
    rows.sort_by_key(|row| (row.completed_at, row.task_id));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::{build_completed_task_report, list_completed_task_report};
    use crate::application::interface::{
        BusyTimeSlotLoadError, BusyTimeSlotRegistrationError, FreeTimeManagerTrait,
    };
    use crate::application::task_use_case::ApplicationError;
    use crate::entity::task::{Status, TaskAttr, TaskHandle};
    use crate::test_support::{new_task_attr_at, new_task_handle_at, TestTaskRepository};
    use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone};
    use uuid::Uuid;

    struct RecordingFreeTimeManager {
        free_seconds: i64,
        requested_intervals: Vec<(DateTime<Local>, DateTime<Local>)>,
    }

    impl RecordingFreeTimeManager {
        fn new(free_seconds: i64) -> Self {
            Self {
                free_seconds,
                requested_intervals: vec![],
            }
        }
    }

    impl FreeTimeManagerTrait for RecordingFreeTimeManager {
        fn get_free_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
            unreachable!("completed report must preserve second precision")
        }

        fn get_free_seconds(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
            self.requested_intervals.push((*start, *end));
            self.free_seconds
        }

        fn get_busy_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
            0
        }

        fn register_busy_time_slot(
            &mut self,
            _start: &DateTime<Local>,
            _end: &DateTime<Local>,
        ) -> Result<(), BusyTimeSlotRegistrationError> {
            Ok(())
        }

        fn load_busy_time_slots_from_file(
            &mut self,
            _busy_time_slots_file_path: &str,
        ) -> Result<(), BusyTimeSlotLoadError> {
            Ok(())
        }
    }

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

    #[test]
    fn 実績0は見積へ置換し見積も0なら0のまま返す() {
        let logical_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let fallback = completed_task("fallback", local_time(11, 9, 0, 0), 0, 3_600);
        let zero = completed_task("zero", local_time(11, 10, 0, 0), 0, 0);
        let repository = TestTaskRepository::new(vec![fallback, zero], local_time(11, 12, 0, 0));

        let rows = list_completed_task_report(&repository, logical_date).unwrap();

        assert_eq!(rows[0].actual_work_seconds, 3_600);
        assert_eq!(rows[1].actual_work_seconds, 0);
    }

    #[test]
    fn 今日の利用可能時間はlogical_date開始から現在時刻までを要求する() {
        let logical_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let root = completed_task("root", local_time(11, 9, 0, 0), 0, 100);
        root.create_child(completed_attr("parent", local_time(11, 10, 0, 0), 200, 300))
            .unwrap();
        let repository = TestTaskRepository::new(vec![root], local_time(11, 12, 0, 0));
        let mut free_time_manager = RecordingFreeTimeManager::new(1_234);

        let report =
            build_completed_task_report(&repository, &mut free_time_manager, logical_date, 120)
                .unwrap();

        assert_eq!(report.total_actual_work_seconds, 300);
        assert_eq!(report.available_seconds, 1_234);
        assert_eq!(report.rows.len(), 2);
        assert_eq!(
            free_time_manager.requested_intervals,
            vec![(local_time(11, 6, 0, 0), local_time(11, 12, 0, 0))]
        );
    }

    #[test]
    fn 今日の予定内にある未完了taskの実績を合計と記録率へ加える() {
        let logical_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let now = local_time(11, 12, 0, 0);
        let task = new_task_handle_at("in progress", now).unwrap();
        task.set_start_time(now).unwrap();
        task.set_estimated_work_seconds(3_600).unwrap();
        task.set_actual_work_seconds(900).unwrap();
        let repository = TestTaskRepository::new(vec![task], now);
        let mut free_time_manager = RecordingFreeTimeManager::new(3_600);

        let report =
            build_completed_task_report(&repository, &mut free_time_manager, logical_date, 120)
                .unwrap();

        assert!(report.rows.is_empty());
        assert_eq!(report.total_actual_work_seconds, 900);
        assert_eq!(report.recorded_percentage, Some(25));
    }

    #[test]
    fn 過去日の利用可能時間は従来どおりlogical_date全体を要求する() {
        let logical_date = NaiveDate::from_ymd_opt(2026, 8, 10).unwrap();
        let repository = TestTaskRepository::new(vec![], local_time(11, 12, 0, 0));
        let mut free_time_manager = RecordingFreeTimeManager::new(1_234);

        let report =
            build_completed_task_report(&repository, &mut free_time_manager, logical_date, 120)
                .unwrap();

        assert_eq!(report.available_seconds, 1_234);
        assert_eq!(
            free_time_manager.requested_intervals,
            vec![(local_time(10, 6, 0, 0), local_time(11, 2, 0, 0))]
        );
    }

    #[test]
    fn 今日の日次終端後は利用可能時間を日次終端で打ち切る() {
        let logical_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let repository = TestTaskRepository::new(vec![], local_time(12, 4, 0, 0));
        let mut free_time_manager = RecordingFreeTimeManager::new(1_234);

        let report =
            build_completed_task_report(&repository, &mut free_time_manager, logical_date, -120)
                .unwrap();

        assert_eq!(report.available_seconds, 1_234);
        assert_eq!(
            free_time_manager.requested_intervals,
            vec![(local_time(11, 6, 0, 0), local_time(11, 22, 0, 0))]
        );
    }

    #[test]
    fn 記録率を四捨五入し100percent超も保持する() {
        let logical_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let repository = TestTaskRepository::new(
            vec![completed_task("task", local_time(11, 9, 0, 0), 1, 1)],
            local_time(11, 12, 0, 0),
        );
        let mut free_time_manager = RecordingFreeTimeManager::new(8);
        let rounded =
            build_completed_task_report(&repository, &mut free_time_manager, logical_date, 120)
                .unwrap();
        assert_eq!(rounded.recorded_percentage, Some(13));

        let repository = TestTaskRepository::new(
            vec![completed_task("task", local_time(11, 9, 0, 0), 5, 5)],
            local_time(11, 12, 0, 0),
        );
        let mut free_time_manager = RecordingFreeTimeManager::new(4);
        let over_one_hundred =
            build_completed_task_report(&repository, &mut free_time_manager, logical_date, 120)
                .unwrap();
        assert_eq!(over_one_hundred.recorded_percentage, Some(125));
    }

    #[test]
    fn 利用可能時間0なら記録率はnone() {
        let logical_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let repository = TestTaskRepository::new(vec![], local_time(11, 6, 0, 0));
        let mut free_time_manager = RecordingFreeTimeManager::new(0);

        let report =
            build_completed_task_report(&repository, &mut free_time_manager, logical_date, 120)
                .unwrap();

        assert_eq!(report.available_seconds, 0);
        assert_eq!(report.recorded_percentage, None);
        assert_eq!(
            free_time_manager.requested_intervals,
            vec![(local_time(11, 6, 0, 0), local_time(11, 6, 0, 0))]
        );
    }

    #[test]
    fn 実績合計のi64範囲超過を値付きerrorで返す() {
        let logical_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let repository = TestTaskRepository::new(
            vec![
                completed_task("first", local_time(11, 9, 0, 0), i64::MAX, i64::MAX),
                completed_task("second", local_time(11, 10, 0, 0), i64::MAX, i64::MAX),
            ],
            local_time(11, 12, 0, 0),
        );
        let mut free_time_manager = RecordingFreeTimeManager::new(1);

        let error =
            build_completed_task_report(&repository, &mut free_time_manager, logical_date, 120)
                .unwrap_err();

        assert_eq!(
            error,
            ApplicationError::CompletedReportCalculationOverflow {
                operation: "total_actual_work_seconds",
                value: i64::MAX as i128 * 2,
            }
        );
    }

    #[test]
    fn 記録率のi64範囲超過を値付きerrorで返す() {
        let logical_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let repository = TestTaskRepository::new(
            vec![completed_task(
                "task",
                local_time(11, 9, 0, 0),
                i64::MAX,
                i64::MAX,
            )],
            local_time(11, 12, 0, 0),
        );
        let mut free_time_manager = RecordingFreeTimeManager::new(1);

        let error =
            build_completed_task_report(&repository, &mut free_time_manager, logical_date, 120)
                .unwrap_err();

        assert_eq!(
            error,
            ApplicationError::CompletedReportCalculationOverflow {
                operation: "recorded_percentage",
                value: i64::MAX as i128 * 100,
            }
        );
    }
}
