use super::routine_load::{build_routine_load_report, ROUTINE_LOAD_HORIZON_DAYS};
use super::schedule_use_case::{ScheduleOccurrenceKey, ScheduledTaskView};
use super::task_view::TaskView;
use crate::entity::task::{Status, TaskAttr, TaskHandle};
use crate::test_support::TestTaskRepository;
use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone};
use std::collections::BTreeMap;
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
        occurrence: ScheduleOccurrenceKey::Actual {
            task_id: task.get_id().unwrap(),
        },
        task: TaskView::try_from(task).unwrap().into(),
        first_available_time: start,
        scheduled_start: start,
        scheduled_end: start + Duration::seconds(seconds),
        scheduled_work_seconds: seconds,
        total_work_seconds: seconds,
        rank: 0,
    }
}

fn projected_segment(
    source: &TaskHandle,
    deadline: DateTime<Local>,
    start: DateTime<Local>,
    seconds: i64,
) -> ScheduledTaskView {
    ScheduledTaskView {
        occurrence: ScheduleOccurrenceKey::Projected {
            source_task_id: source.get_id().unwrap(),
            deadline,
        },
        task: TaskView::try_from(source).unwrap().into(),
        first_available_time: start,
        scheduled_start: start,
        scheduled_end: start + Duration::seconds(seconds),
        scheduled_work_seconds: seconds,
        total_work_seconds: seconds,
        rank: 0,
    }
}

#[test]
fn routine_load_estimate_difference_uses_occurrence_total_not_progress_or_segments() {
    let project = TaskHandle::with_identity("project", Uuid::from_u128(10_001), at(3)).unwrap();
    let routine = child(&project, "routine", 10_002, at(3));
    routine.set_repetition_interval_days_opt(Some(1)).unwrap();
    routine.set_estimated_work_seconds(1_800).unwrap();
    let shorter = child(&routine, "shorter occurrence", 10_003, at(3));
    shorter.set_estimated_work_seconds(900).unwrap();
    shorter.set_actual_work_seconds(600).unwrap();
    let descendant = child(&shorter, "occurrence segment", 10_004, at(3));
    descendant.set_estimated_work_seconds(120).unwrap();
    let equal = child(&routine, "equal occurrence", 10_005, at(3));
    equal.set_estimated_work_seconds(1_800).unwrap();
    let longer = child(&routine, "longer occurrence", 10_006, at(3));
    longer.set_estimated_work_seconds(2_700).unwrap();
    let repository = TestTaskRepository::new(vec![project], at(3));
    let schedule = vec![
        segment(&shorter, at(3), 60),
        segment(&shorter, at(3) + Duration::hours(1), 60),
        segment(&descendant, at(3), 120),
        segment(&descendant, at(4), 60),
        segment(&equal, at(5), 300),
        segment(&longer, at(6), 300),
        projected_segment(&routine, at(7), at(7), 1_800),
    ];
    let report = build_routine_load_report(&repository, &schedule, at(3).date_naive()).unwrap();
    assert_eq!(
        report.rows[0].estimate_difference_seconds_by_date,
        BTreeMap::from([
            (at(3).date_naive(), -780),
            (at(4).date_naive(), -780),
            (at(5).date_naive(), 0),
            (at(6).date_naive(), 900),
        ])
    );
    shorter.set_actual_work_seconds(800).unwrap();
    let updated = build_routine_load_report(&repository, &schedule, at(3).date_naive()).unwrap();
    assert_eq!(updated.rows[0], report.rows[0]);
}

#[test]
fn routine_load_estimate_difference_sums_split_occurrence_including_completed_descendants() {
    let project = TaskHandle::with_identity("project", Uuid::from_u128(13_001), at(3)).unwrap();
    let routine = child(&project, "routine", 13_002, at(3));
    routine.set_repetition_interval_days_opt(Some(1)).unwrap();
    routine.set_estimated_work_seconds(10 * 3_600).unwrap();
    let occurrence = child(&routine, "occurrence", 13_003, at(3));
    occurrence.set_estimated_work_seconds(0).unwrap();
    let first = child(&occurrence, "first part", 13_004, at(3));
    first.set_estimated_work_seconds(3_600).unwrap();
    let middle = child(&occurrence, "middle part", 13_005, at(3));
    middle.set_estimated_work_seconds(8 * 3_600).unwrap();
    let branch = child(&occurrence, "branch", 13_006, at(3));
    branch.set_estimated_work_seconds(0).unwrap();
    let last = child(&branch, "last part", 13_007, at(3));
    last.set_estimated_work_seconds(3_600).unwrap();
    let repository = TestTaskRepository::new(vec![project], at(3));
    let schedule = vec![
        segment(&first, at(3), 600),
        segment(&middle, at(3), 600),
        segment(&middle, at(3) + Duration::hours(1), 600),
        segment(&last, at(4), 600),
    ];
    for (hours, difference) in [(8, 0), (9, 3_600), (7, -3_600)] {
        middle.set_estimated_work_seconds(hours * 3_600).unwrap();
        let report = build_routine_load_report(&repository, &schedule, at(3).date_naive()).unwrap();
        assert_eq!(
            report.rows[0].estimate_difference_seconds_by_date,
            BTreeMap::from([
                (at(3).date_naive(), difference),
                (at(4).date_naive(), difference)
            ])
        );
    }
    middle.set_estimated_work_seconds(8 * 3_600).unwrap();
    first.set_actual_work_seconds(3_600).unwrap();
    first.set_orig_status(Status::Done).unwrap();
    let report =
        build_routine_load_report(&repository, &schedule[1..], at(3).date_naive()).unwrap();
    assert_eq!(
        report.rows[0].estimate_difference_seconds_by_date,
        BTreeMap::from([(at(3).date_naive(), 0), (at(4).date_naive(), 0)])
    );
}

#[test]
fn routine_load_estimate_difference_excludes_nested_routine_subtree() {
    let project = TaskHandle::with_identity("project", Uuid::from_u128(14_001), at(3)).unwrap();
    let routine = child(&project, "routine", 14_002, at(3));
    routine.set_repetition_interval_days_opt(Some(1)).unwrap();
    routine.set_estimated_work_seconds(600).unwrap();
    let occurrence = child(&routine, "occurrence", 14_003, at(3));
    occurrence.set_estimated_work_seconds(600).unwrap();
    let nested = child(&occurrence, "nested routine", 14_004, at(3));
    nested.set_repetition_interval_days_opt(Some(1)).unwrap();
    nested.set_estimated_work_seconds(300).unwrap();
    let nested_occurrence = child(&nested, "nested occurrence", 14_005, at(3));
    nested_occurrence.set_estimated_work_seconds(420).unwrap();
    let repository = TestTaskRepository::new(vec![project], at(3));
    let report = build_routine_load_report(
        &repository,
        &[
            segment(&occurrence, at(3), 600),
            segment(&nested_occurrence, at(3), 420),
        ],
        at(3).date_naive(),
    )
    .unwrap();
    assert_eq!(report.rows.len(), 2);
    for (id, difference) in [
        (routine.get_id().unwrap(), 0),
        (nested.get_id().unwrap(), 120),
    ] {
        let row = report
            .rows
            .iter()
            .find(|row| row.routine_task_id == id)
            .unwrap();
        assert_eq!(
            row.estimate_difference_seconds_by_date,
            BTreeMap::from([(at(3).date_naive(), difference)])
        );
    }
}

#[test]
fn routine_load_estimate_difference_reports_addition_overflow() {
    let project = TaskHandle::with_identity("project", Uuid::from_u128(15_001), at(3)).unwrap();
    let routine = child(&project, "routine", 15_002, at(3));
    routine.set_repetition_interval_days_opt(Some(1)).unwrap();
    let occurrence = child(&routine, "occurrence", 15_003, at(3));
    occurrence.set_estimated_work_seconds(i64::MAX).unwrap();
    let part = child(&occurrence, "part", 15_004, at(3));
    part.set_estimated_work_seconds(1).unwrap();
    let repository = TestTaskRepository::new(vec![project], at(3));
    let error =
        build_routine_load_report(&repository, &[segment(&part, at(3), 1)], at(3).date_naive())
            .unwrap_err();
    assert!(matches!(error,
        super::task_use_case::ApplicationError::RoutineLoadCalculationOverflow { routine_task_id }
        if routine_task_id == routine.get_id().unwrap()));
}

#[test]
fn routine_load_estimate_difference_omits_ambiguous_and_projected_dates_in_any_order() {
    let project = TaskHandle::with_identity("project", Uuid::from_u128(11_001), at(3)).unwrap();
    let routine = child(&project, "routine", 11_002, at(3));
    routine.set_repetition_interval_days_opt(Some(1)).unwrap();
    let first = child(&routine, "first occurrence", 11_003, at(3));
    let second = child(&routine, "second occurrence", 11_004, at(3));
    let repository = TestTaskRepository::new(vec![project], at(3));
    let mut schedule = vec![
        segment(&first, at(3), 60),
        segment(&second, at(3), 60),
        segment(&first, at(4), 60),
        projected_segment(&routine, at(4), at(4), 60),
        projected_segment(&routine, at(5), at(5), 60),
    ];
    for _ in 0..2 {
        let report = build_routine_load_report(&repository, &schedule, at(3).date_naive()).unwrap();
        assert!(report.rows[0]
            .estimate_difference_seconds_by_date
            .is_empty());
        schedule.reverse();
    }
}

#[test]
fn routine_load_estimate_difference_reports_subtraction_overflow() {
    let project = TaskHandle::with_identity("project", Uuid::from_u128(12_001), at(3)).unwrap();
    let routine = child(&project, "routine", 12_002, at(3));
    routine.set_repetition_interval_days_opt(Some(1)).unwrap();
    routine.set_estimated_work_seconds(i64::MIN).unwrap();
    let occurrence = child(&routine, "occurrence", 12_003, at(3));
    occurrence.set_estimated_work_seconds(i64::MAX).unwrap();
    let repository = TestTaskRepository::new(vec![project], at(3));
    let error = build_routine_load_report(
        &repository,
        &[segment(&occurrence, at(3), 60)],
        at(3).date_naive(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        super::task_use_case::ApplicationError::RoutineLoadCalculationOverflow { .. }
    ));
}

#[test]
fn routine_loadは今日から27日後までを最寄りの繰返親ごとに集計する() {
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
        segment(&cleaning, at(10), 45 * 60),
        segment(&cleaning, at(30), 60 * 60),
        segment(&cleaning, at(31), 120 * 60),
        segment(&dishes, at(3), 20 * 60),
        segment(&dishes, at(4), 20 * 60),
        segment(&one_off, at(3), 10 * 60),
    ];

    let actual = build_routine_load_report(&repository, &schedule, today).unwrap();

    assert_eq!(ROUTINE_LOAD_HORIZON_DAYS, 28);
    assert_eq!(actual.horizon_day_count, 28);
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
    assert_eq!(weekly_row.total_work_seconds, 180 * 60);
    assert_eq!(weekly_row.occurrence_day_count, 4);
    assert_eq!(weekly_row.average_work_seconds, 45 * 60);
    assert_eq!(
        weekly_row.peak_date,
        NaiveDate::from_ymd_opt(2026, 10, 30).unwrap()
    );
    assert_eq!(weekly_row.peak_work_seconds, 60 * 60);
    assert_eq!(
        weekly_row.work_seconds_by_date,
        BTreeMap::from([
            (NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(), 45 * 60),
            (NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(), 30 * 60),
            (NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(), 45 * 60),
            (NaiveDate::from_ymd_opt(2026, 10, 30).unwrap(), 60 * 60),
        ])
    );
    assert_eq!(
        weekly_row.work_seconds_by_date.values().sum::<i64>(),
        weekly_row.total_work_seconds
    );
    assert_eq!(
        weekly_row.work_seconds_by_date.len(),
        weekly_row.occurrence_day_count
    );
    assert_eq!(
        weekly_row
            .work_seconds_by_date
            .iter()
            .max_by_key(|(_, seconds)| *seconds),
        Some((&weekly_row.peak_date, &weekly_row.peak_work_seconds))
    );

    let daily_row = &actual.rows[1];
    assert_eq!(daily_row.routine_task_id, Uuid::from_u128(4));
    assert_eq!(daily_row.total_work_seconds, 40 * 60);
    assert_eq!(daily_row.occurrence_day_count, 2);
    assert_eq!(
        daily_row.work_seconds_by_date,
        BTreeMap::from([
            (NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(), 20 * 60),
            (NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(), 20 * 60),
        ])
    );
}

#[test]
fn routine_loadの1日平均は発生日数で割り秒未満を切り捨てる() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
    let project = TaskHandle::with_identity("生活", Uuid::from_u128(301), at(3)).unwrap();
    let routine = child(&project, "3日分", 302, at(3));
    routine.set_repetition_interval_days_opt(Some(1)).unwrap();
    let task = child(&routine, "実施", 303, at(3));
    let repository = TestTaskRepository::new(vec![project], at(3));
    let schedule = vec![
        segment(&task, at(3), 3 * 60 + 40),
        segment(&task, at(4), 3 * 60 + 40),
        segment(&task, at(5), 3 * 60 + 40),
    ];

    let report = build_routine_load_report(&repository, &schedule, today).unwrap();

    assert_eq!(report.rows[0].total_work_seconds, 11 * 60);
    assert_eq!(report.rows[0].occurrence_day_count, 3);
    assert_eq!(report.rows[0].average_work_seconds, 3 * 60 + 40);
}

#[test]
fn routine_loadの最大日は同じ負荷なら早いlogical_dateを保持する() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
    let project = TaskHandle::with_identity("生活", Uuid::from_u128(401), at(3)).unwrap();
    let routine = child(&project, "週次", 402, at(3));
    routine.set_repetition_interval_days_opt(Some(7)).unwrap();
    let task = child(&routine, "実施", 403, at(3));
    let repository = TestTaskRepository::new(vec![project], at(3));
    let schedule = vec![
        segment(&task, at(3), 30 * 60),
        segment(&task, at(10), 30 * 60),
    ];

    let report = build_routine_load_report(&repository, &schedule, today).unwrap();

    assert_eq!(report.rows[0].peak_date, today);
    assert_eq!(report.rows[0].peak_work_seconds, 30 * 60);
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

#[test]
fn routine_loadはactualとprojectedを同じ繰返元へ集計する() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
    let project = TaskHandle::with_identity("生活", Uuid::from_u128(101), at(3)).unwrap();
    let routine = child(&project, "3日に1回筋トレする", 102, at(3));
    routine.set_repetition_interval_days_opt(Some(3)).unwrap();
    let actual = child(&routine, "筋トレ", 103, at(3));
    let repository = TestTaskRepository::new(vec![project], at(3));
    let schedule = vec![
        segment(&actual, at(3), 30 * 60),
        projected_segment(&routine, at(6), at(6), 30 * 60),
        projected_segment(&routine, at(9), at(9), 30 * 60),
    ];

    let report = build_routine_load_report(&repository, &schedule, today).unwrap();

    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].routine_task_id, Uuid::from_u128(102));
    assert_eq!(report.rows[0].total_work_seconds, 90 * 60);
    assert_eq!(report.rows[0].occurrence_day_count, 3);
    assert_eq!(
        report.rows[0].work_seconds_by_date,
        BTreeMap::from([
            (NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(), 30 * 60),
            (NaiveDate::from_ymd_opt(2026, 10, 6).unwrap(), 30 * 60),
            (NaiveDate::from_ymd_opt(2026, 10, 9).unwrap(), 30 * 60),
        ])
    );
}

#[test]
fn routine_loadは同じuuidのactualとprojectedを別の規則で解決する() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
    let outer_routine = TaskHandle::with_identity("週次", Uuid::from_u128(201), at(3)).unwrap();
    outer_routine
        .set_repetition_interval_days_opt(Some(7))
        .unwrap();
    let source = child(&outer_routine, "3日に1回筋トレする", 202, at(3));
    source.set_repetition_interval_days_opt(Some(3)).unwrap();
    let repository = TestTaskRepository::new(vec![outer_routine], at(3));
    let schedule = vec![
        segment(&source, at(3), 10 * 60),
        projected_segment(&source, at(6), at(6), 20 * 60),
    ];

    let report = build_routine_load_report(&repository, &schedule, today).unwrap();

    assert_eq!(report.rows.len(), 2);
    let actual_row = report
        .rows
        .iter()
        .find(|row| row.routine_task_id == Uuid::from_u128(201))
        .unwrap();
    assert_eq!(actual_row.repetition_interval_days, 7);
    assert_eq!(actual_row.total_work_seconds, 10 * 60);
    let projected_row = report
        .rows
        .iter()
        .find(|row| row.routine_task_id == Uuid::from_u128(202))
        .unwrap();
    assert_eq!(projected_row.repetition_interval_days, 3);
    assert_eq!(projected_row.total_work_seconds, 20 * 60);
}
