use super::web_service::build_all_task_rows;
use super::{AllTaskRowDto, DeadlineDisplayKind, TaskDisplayKind, WebReadError};
use crate::application::schedule_use_case::ScheduledTaskView;
use crate::application::task_use_case::get_task;
use crate::entity::task::TaskHandle;
use crate::test_support::TestTaskRepository;
use chrono::{Duration, Local, TimeZone};
use uuid::Uuid;

#[test]
fn all_task_rowはschedule順とsegment情報を保持する() {
    let start = Local.with_ymd_and_hms(2026, 9, 5, 5, 59, 0).unwrap();
    let task_id = Uuid::from_u128(41);
    let handle = TaskHandle::with_identity("multi segment", task_id, start).unwrap();
    handle.set_estimated_work_seconds(600).unwrap();
    handle
        .set_deadline_time_opt(Some(start + Duration::hours(1)))
        .unwrap();
    let repository = TestTaskRepository::new(vec![handle], start);
    let task = get_task(&repository, task_id).unwrap().unwrap();
    let schedule = vec![
        ScheduledTaskView {
            task: task.clone(),
            first_available_time: start,
            scheduled_start: start,
            scheduled_end: start + Duration::seconds(300),
            scheduled_work_seconds: 300,
            total_work_seconds: 600,
            rank: 1,
        },
        ScheduledTaskView {
            task,
            first_available_time: start,
            scheduled_start: start + Duration::minutes(2),
            scheduled_end: start + Duration::minutes(7),
            scheduled_work_seconds: 300,
            total_work_seconds: 600,
            rank: 0,
        },
    ];

    let rows = build_all_task_rows(&repository, &schedule, start).unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].segment_index, 0);
    assert_eq!(rows[1].segment_index, 1);
    assert_eq!(rows[0].task.task_id, rows[1].task.task_id);
    assert_eq!(rows[0].schedule_date, "2026-09-04");
    assert_eq!(rows[1].schedule_date, "2026-09-05");
    assert!(!rows[0].is_leaf);
    assert!(rows[1].is_leaf);
    assert_eq!(rows[0].deadline_epoch_ms, rows[1].deadline_epoch_ms);
    assert_eq!(rows[0].deadline_label, "____-00:55");
    assert!(!rows[0].misses_deadline);
    assert_eq!(rows[0].task_display_kind, TaskDisplayKind::NonRepetitive);
    assert_eq!(rows[0].deadline_display_kind, DeadlineDisplayKind::Future);
    assert_eq!(rows[1].deadline_display_kind, DeadlineDisplayKind::Today);
    assert_eq!(rows[0].task_display_kind, rows[1].task_display_kind);
    let round_trip: AllTaskRowDto =
        serde_json::from_str(&serde_json::to_string(&rows[0]).unwrap()).unwrap();
    assert_eq!(round_trip, rows[0]);
}

#[test]
fn invalid_cursorは専用errorである() {
    let error = WebReadError::InvalidCursor;
    assert_eq!(error.to_string(), "invalid all-task cursor");
}
