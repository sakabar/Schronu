use super::web_read::{build_auto_session_dto, build_scheduled_task_rows};
use crate::application::schedule_use_case::ScheduledTaskView;
use crate::application::task_use_case::get_task;
use crate::entity::task::TaskHandle;
use crate::test_support::TestTaskRepository;
use chrono::{Duration, Local, NaiveDate, TimeZone};
use uuid::Uuid;

#[test]
fn listは指定logical_dateだけを開始時刻のstable昇順でsegment単位に返す() {
    let date = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
    let day_start = Local.with_ymd_and_hms(2026, 9, 5, 6, 0, 0).unwrap();
    let first_id = Uuid::from_u128(1);
    let second_id = Uuid::from_u128(2);
    let first_handle = TaskHandle::with_identity("first", first_id, day_start).unwrap();
    let second_handle = TaskHandle::with_identity("second", second_id, day_start).unwrap();
    second_handle.create_as_last_child(crate::test_support::new_task_attr_at("child", day_start));
    let repository = TestTaskRepository::new(vec![first_handle, second_handle.clone()], day_start);
    let first = get_task(&repository, first_id).unwrap().unwrap();
    let second = get_task(&repository, second_id).unwrap().unwrap();
    let schedule = vec![
        ScheduledTaskView {
            task: first.clone(),
            first_available_time: day_start,
            scheduled_start: day_start + Duration::hours(3),
            scheduled_end: day_start + Duration::hours(3) + Duration::seconds(600),
            scheduled_work_seconds: 600,
            total_work_seconds: 1_200,
            rank: 0,
        },
        ScheduledTaskView {
            task: second,
            first_available_time: day_start,
            scheduled_start: day_start + Duration::hours(1),
            scheduled_end: day_start + Duration::hours(1) + Duration::seconds(900),
            scheduled_work_seconds: 900,
            total_work_seconds: 900,
            rank: 0,
        },
        ScheduledTaskView {
            task: first.clone(),
            first_available_time: day_start,
            scheduled_start: day_start + Duration::hours(1),
            scheduled_end: day_start + Duration::hours(1) + Duration::seconds(300),
            scheduled_work_seconds: 300,
            total_work_seconds: 1_200,
            rank: 0,
        },
        ScheduledTaskView {
            task: first,
            first_available_time: day_start,
            scheduled_start: day_start + Duration::days(1),
            scheduled_end: day_start + Duration::days(1) + Duration::seconds(300),
            scheduled_work_seconds: 300,
            total_work_seconds: 1_200,
            rank: 0,
        },
    ];

    let rows = build_scheduled_task_rows(&schedule, date).unwrap();

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].task.task_id, second_id.hyphenated().to_string());
    assert_eq!(rows[1].task.task_id, first_id.hyphenated().to_string());
    assert_eq!(rows[2].task.task_id, first_id.hyphenated().to_string());
    assert_eq!(
        rows[0].schedule_start_epoch_ms,
        rows[1].schedule_start_epoch_ms
    );
}

#[test]
fn listのdtoはtask値とdeadlineとleaf判定を情報を落とさず返す() {
    let date = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
    let start = Local.with_ymd_and_hms(2026, 9, 5, 8, 0, 0).unwrap();
    let task_id = Uuid::from_u128(3);
    let task_handle = TaskHandle::with_identity("DTO task", task_id, start).unwrap();
    task_handle.set_estimated_work_seconds(1_800).unwrap();
    task_handle.set_actual_work_seconds(300).unwrap();
    task_handle
        .set_deadline_time_opt(Some(start + Duration::hours(8)))
        .unwrap();
    let repository = TestTaskRepository::new(vec![task_handle], start);
    let task = get_task(&repository, task_id).unwrap().unwrap();
    let deadline = task.deadline_time.unwrap();

    let rows = build_scheduled_task_rows(
        &[ScheduledTaskView {
            task,
            first_available_time: start,
            scheduled_start: start,
            scheduled_end: start + Duration::seconds(1_200),
            scheduled_work_seconds: 1_200,
            total_work_seconds: 1_200,
            rank: 0,
        }],
        date,
    )
    .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task.task_id, task_id.hyphenated().to_string());
    assert_eq!(rows[0].task.task_name, "DTO task");
    assert_eq!(rows[0].task.estimated_work_seconds, 1_800);
    assert_eq!(rows[0].task.actual_work_seconds, 300);
    assert_eq!(rows[0].schedule_start_epoch_ms, start.timestamp_millis());
    assert_eq!(
        rows[0].schedule_end_epoch_ms,
        (start + Duration::seconds(1_200)).timestamp_millis()
    );
    assert_eq!(rows[0].deadline_epoch_ms, Some(deadline.timestamp_millis()));
    assert!(rows[0].is_leaf);

    let encoded = serde_json::to_string(&rows[0]).unwrap();
    let decoded = serde_json::from_str(&encoded).unwrap();
    assert_eq!(rows[0], decoded);
}

#[test]
fn auto_sessionは候補をdtoで返しtask_treeを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let task = TaskHandle::with_identity("auto task", Uuid::from_u128(4), now).unwrap();
    task.set_estimated_work_seconds(900).unwrap();
    task.set_actual_work_seconds(120).unwrap();
    let before = task.snapshot().unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![task.clone()], now);
    repository.set_highest_priority_leaf_task_id(Some(task_id));

    let dto = build_auto_session_dto(&mut repository).unwrap().unwrap();

    assert_eq!(dto.task_id, task_id.hyphenated().to_string());
    assert_eq!(dto.task_name, "auto task");
    assert_eq!(dto.estimated_work_seconds, 900);
    assert_eq!(dto.actual_work_seconds, 120);
    assert_eq!(task.snapshot().unwrap(), before);
    assert_eq!(repository.save_count(), 0);
}

#[test]
fn auto_sessionは候補なしを正常なnoneとして返す() {
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let mut repository = TestTaskRepository::new(vec![], now);

    assert_eq!(build_auto_session_dto(&mut repository), Ok(None));
}
