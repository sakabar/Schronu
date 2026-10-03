use super::routine_load::{build_routine_load_report, ROUTINE_LOAD_HORIZON_DAYS};
use super::schedule_use_case::ScheduledTaskView;
use super::task_view::TaskView;
use crate::entity::task::{TaskAttr, TaskHandle};
use crate::test_support::TestTaskRepository;
use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone};
use uuid::Uuid;

fn at(day: u32) -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 10, day, 12, 0, 0).unwrap()
}

fn child(parent: &TaskHandle, name: &str, id: u128, now: DateTime<Local>) -> TaskHandle {
    parent
        .create_child(TaskAttr::with_identity(name, Uuid::from_u128(id), now))
        .unwrap()
}

fn segment(task: &TaskHandle, start: DateTime<Local>, seconds: i64) -> ScheduledTaskView {
    ScheduledTaskView {
        task: TaskView::try_from(task).unwrap(),
        first_available_time: start,
        scheduled_start: start,
        scheduled_end: start + Duration::seconds(seconds),
        scheduled_work_seconds: seconds,
        total_work_seconds: seconds,
        rank: 0,
    }
}

#[test]
fn routine_loadは28日を最寄りの繰返親ごとに集計する() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
    let project = TaskHandle::with_identity("生活", Uuid::from_u128(1), at(3)).unwrap();
    let weekly = child(&project, "週次家事", 2, at(3));
    weekly.set_repetition_interval_days_opt(Some(7)).unwrap();
    let cleaning = child(&weekly, "掃除", 3, at(3));
    let nested = child(&weekly, "日次片付け", 4, at(3));
    nested.set_repetition_interval_days_opt(Some(1)).unwrap();
    let dishes = child(&nested, "食器", 5, at(3));
    let one_off = child(&project, "単発", 6, at(3));
    let repository = TestTaskRepository::new(vec![project], at(3));
    let schedule = vec![
        segment(&cleaning, at(3), 30 * 60),
        segment(&cleaning, at(3) + Duration::hours(1), 15 * 60),
        segment(&cleaning, at(4), 30 * 60),
        segment(&cleaning, at(31), 45 * 60),
        segment(&dishes, at(3), 20 * 60),
        segment(&dishes, at(4), 20 * 60),
        segment(&one_off, at(3), 10 * 60),
    ];

    let actual = build_routine_load_report(&repository, &schedule, today).unwrap();

    assert_eq!(ROUTINE_LOAD_HORIZON_DAYS, 28);
    assert_eq!(actual.start_date, today);
    assert_eq!(
        actual.end_date,
        NaiveDate::from_ymd_opt(2026, 10, 30).unwrap()
    );
    assert_eq!(actual.rows.len(), 2);

    let weekly_row = &actual.rows[0];
    assert_eq!(weekly_row.project_task_id, Uuid::from_u128(1));
    assert_eq!(weekly_row.project_name, "生活");
    assert_eq!(weekly_row.routine_task_id, Uuid::from_u128(2));
    assert_eq!(weekly_row.routine_name, "週次家事");
    assert_eq!(weekly_row.repetition_interval_days, 7);
    assert_eq!(weekly_row.total_work_seconds, 75 * 60);
    assert_eq!(weekly_row.weekly_average_seconds, 1_125);
    assert_eq!(weekly_row.occurrence_day_count, 2);
    assert_eq!(weekly_row.peak_date, today);
    assert_eq!(weekly_row.peak_work_seconds, 45 * 60);

    let daily_row = &actual.rows[1];
    assert_eq!(daily_row.routine_task_id, Uuid::from_u128(4));
    assert_eq!(daily_row.total_work_seconds, 40 * 60);
    assert_eq!(daily_row.occurrence_day_count, 2);
}

#[test]
fn routine_loadは同値をproject名と繰返名とuuidで決定的に並べる() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
    let project_b = TaskHandle::with_identity("B", Uuid::from_u128(10), at(3)).unwrap();
    let routine_b = child(&project_b, "同名", 12, at(3));
    routine_b.set_repetition_interval_days_opt(Some(7)).unwrap();
    let task_b = child(&routine_b, "B occurrence", 13, at(3));

    let project_a = TaskHandle::with_identity("A", Uuid::from_u128(20), at(3)).unwrap();
    let routine_z = child(&project_a, "Z", 22, at(3));
    routine_z.set_repetition_interval_days_opt(Some(7)).unwrap();
    let task_z = child(&routine_z, "Z occurrence", 23, at(3));
    let routine_a = child(&project_a, "A", 24, at(3));
    routine_a.set_repetition_interval_days_opt(Some(7)).unwrap();
    let task_a = child(&routine_a, "A occurrence", 25, at(3));

    let repository = TestTaskRepository::new(vec![project_b, project_a], at(3));
    let schedule = vec![
        segment(&task_b, at(3), 10 * 60),
        segment(&task_z, at(3), 10 * 60),
        segment(&task_a, at(3), 10 * 60),
    ];

    let names = build_routine_load_report(&repository, &schedule, today)
        .unwrap()
        .rows
        .into_iter()
        .map(|row| (row.project_name, row.routine_name))
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            ("A".to_owned(), "A".to_owned()),
            ("A".to_owned(), "Z".to_owned()),
            ("B".to_owned(), "同名".to_owned()),
        ]
    );
}
