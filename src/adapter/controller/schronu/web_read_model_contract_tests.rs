use super::web_service::{
    build_all_task_rows, build_auto_session_dto, build_band_days, build_scheduled_task_rows,
    DeadlineDisplayKind, DeferModeDto, TaskDisplayKind,
};
use crate::application::schedule_use_case::{ScheduleOccurrenceKey, ScheduledTaskView};
use crate::application::task_use_case::get_task;
use crate::entity::task::{Status, TaskAttr, TaskHandle};
use crate::test_support::{TestFreeTimeManager, TestTaskRepository};
use chrono::{Duration, Local, NaiveDate, TimeZone};
use uuid::Uuid;

#[test]
fn projected_occurrenceはread_onlyの一覧rowを構築する() {
    let start = Local.with_ymd_and_hms(2026, 9, 5, 7, 0, 0).unwrap();
    let source_task_id = Uuid::from_u128(901);
    let task = TaskHandle::with_identity("projected", source_task_id, start).unwrap();
    let repository = TestTaskRepository::new(vec![task.clone()], start);
    let schedule = [ScheduledTaskView {
        occurrence: ScheduleOccurrenceKey::Projected {
            source_task_id,
            deadline: start + Duration::days(3),
        },
        task: get_task(&repository, source_task_id)
            .unwrap()
            .unwrap()
            .into(),
        first_available_time: start,
        scheduled_start: start,
        scheduled_end: start + Duration::minutes(10),
        scheduled_work_seconds: 600,
        total_work_seconds: 600,
        rank: 0,
    }];

    let scheduled_rows =
        build_scheduled_task_rows(&repository, &schedule, start.date_naive(), start).unwrap();
    let all_rows = build_all_task_rows(&repository, &schedule, start).unwrap();

    assert_eq!(scheduled_rows.len(), 1);
    assert_eq!(all_rows.len(), 1);
    assert!(scheduled_rows[0].task.task_id.is_none());
    assert!(scheduled_rows[0].defer_plan.is_none());
    assert_eq!(scheduled_rows[0].task.actual_work_seconds, 0);
    let expected_source_task_id = source_task_id.hyphenated().to_string();
    assert!(matches!(
        scheduled_rows[0].occurrence,
        super::web_service::ScheduleOccurrenceDto::Projected { ref source_task_id, .. }
            if source_task_id == &expected_source_task_id
    ));
    assert!(all_rows[0].task.task_id.is_none());
}

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
            occurrence: ScheduleOccurrenceKey::Actual { task_id: first_id },
            task: first.clone().into(),
            first_available_time: day_start - Duration::minutes(1),
            scheduled_start: day_start - Duration::minutes(1),
            scheduled_end: day_start,
            scheduled_work_seconds: 60,
            total_work_seconds: 1_200,
            rank: 0,
        },
        ScheduledTaskView {
            occurrence: ScheduleOccurrenceKey::Actual { task_id: first_id },
            task: first.clone().into(),
            first_available_time: day_start,
            scheduled_start: day_start + Duration::hours(3),
            scheduled_end: day_start + Duration::hours(3) + Duration::seconds(600),
            scheduled_work_seconds: 600,
            total_work_seconds: 1_200,
            rank: 0,
        },
        ScheduledTaskView {
            occurrence: ScheduleOccurrenceKey::Actual { task_id: second_id },
            task: second.into(),
            first_available_time: day_start,
            scheduled_start: day_start + Duration::hours(1),
            scheduled_end: day_start + Duration::hours(1) + Duration::seconds(900),
            scheduled_work_seconds: 900,
            total_work_seconds: 900,
            rank: 0,
        },
        ScheduledTaskView {
            occurrence: ScheduleOccurrenceKey::Actual { task_id: first_id },
            task: first.clone().into(),
            first_available_time: day_start,
            scheduled_start: day_start + Duration::hours(1),
            scheduled_end: day_start + Duration::hours(1) + Duration::seconds(300),
            scheduled_work_seconds: 300,
            total_work_seconds: 1_200,
            rank: 0,
        },
        ScheduledTaskView {
            occurrence: ScheduleOccurrenceKey::Actual { task_id: first_id },
            task: first.into(),
            first_available_time: day_start,
            scheduled_start: day_start + Duration::days(1),
            scheduled_end: day_start + Duration::days(1) + Duration::seconds(300),
            scheduled_work_seconds: 300,
            total_work_seconds: 1_200,
            rank: 0,
        },
    ];

    let rows = build_scheduled_task_rows(&repository, &schedule, date, day_start).unwrap();

    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].task.task_id,
        Some(second_id.hyphenated().to_string())
    );
    assert_eq!(
        rows[1].task.task_id,
        Some(first_id.hyphenated().to_string())
    );
    assert_eq!(
        rows[2].task.task_id,
        Some(first_id.hyphenated().to_string())
    );
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
        &repository,
        &[ScheduledTaskView {
            occurrence: ScheduleOccurrenceKey::Actual { task_id },
            task: task.into(),
            first_available_time: start,
            scheduled_start: start,
            scheduled_end: start + Duration::seconds(1_200),
            scheduled_work_seconds: 1_200,
            total_work_seconds: 1_200,
            rank: 0,
        }],
        date,
        start,
    )
    .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task.task_id, Some(task_id.hyphenated().to_string()));
    assert_eq!(rows[0].task.task_name, "DTO task");
    assert_eq!(rows[0].task.estimated_work_seconds, 1_800);
    assert_eq!(rows[0].task.actual_work_seconds, 300);
    assert_eq!(rows[0].schedule_start_epoch_ms, start.timestamp_millis());
    assert_eq!(
        rows[0].schedule_end_epoch_ms,
        (start + Duration::seconds(1_200)).timestamp_millis()
    );
    assert_eq!(rows[0].deadline_epoch_ms, Some(deadline.timestamp_millis()));
    assert_eq!(rows[0].deadline_label, "____-07:40");
    assert!(!rows[0].misses_deadline);
    assert!(rows[0].is_leaf);
    let defer_plan = rows[0].defer_plan.as_ref().unwrap();
    assert_eq!(defer_plan.mode, DeferModeDto::DeadlineLimited);
    assert_eq!(
        defer_plan.requested_pending_until_epoch_ms,
        Local
            .with_ymd_and_hms(2026, 9, 6, 6, 0, 0)
            .unwrap()
            .timestamp_millis()
    );
    assert_eq!(
        defer_plan.effective_pending_until_epoch_ms,
        Some(
            Local
                .with_ymd_and_hms(2026, 9, 5, 15, 25, 0)
                .unwrap()
                .timestamp_millis()
        )
    );

    let encoded = serde_json::to_string(&rows[0]).unwrap();
    let decoded = serde_json::from_str(&encoded).unwrap();
    assert_eq!(rows[0], decoded);
}

#[test]
fn listのdtoは予定終了が締切を過ぎる場合だけmisses_deadlineにする() {
    let date = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
    let start = Local.with_ymd_and_hms(2026, 9, 5, 8, 0, 0).unwrap();
    let scheduled_end = start + Duration::hours(1);
    let cases = [
        (None, "____/__/__", false),
        (
            Some(scheduled_end + Duration::minutes(1)),
            "____-00:01",
            false,
        ),
        (Some(scheduled_end), "____-00:00", false),
        (
            Some(scheduled_end - Duration::minutes(1)),
            "+00:01____",
            true,
        ),
    ];
    let handles = cases
        .iter()
        .enumerate()
        .map(|(index, (deadline, _, _))| {
            let task = TaskHandle::with_identity(
                "deadline case",
                Uuid::from_u128(100 + index as u128),
                start,
            )
            .unwrap();
            task.set_deadline_time_opt(*deadline).unwrap();
            task
        })
        .collect::<Vec<_>>();
    let repository = TestTaskRepository::new(handles.clone(), start);
    let schedule = handles
        .iter()
        .map(|handle| ScheduledTaskView {
            occurrence: ScheduleOccurrenceKey::Actual {
                task_id: handle.get_id().unwrap(),
            },
            task: get_task(&repository, handle.get_id().unwrap())
                .unwrap()
                .unwrap()
                .into(),
            first_available_time: start,
            scheduled_start: start,
            scheduled_end,
            scheduled_work_seconds: 3_600,
            total_work_seconds: 3_600,
            rank: 0,
        })
        .collect::<Vec<_>>();

    let rows = build_scheduled_task_rows(&repository, &schedule, date, start).unwrap();

    for (row, (_, expected_label, expected_miss)) in rows.iter().zip(cases) {
        assert_eq!(row.deadline_label, expected_label);
        assert_eq!(row.misses_deadline, expected_miss);
    }
}

#[test]
fn listは固定・祖先から継承した繰返・単発を分類する() {
    let date = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
    let start = Local.with_ymd_and_hms(2026, 9, 5, 8, 0, 0).unwrap();
    let fixed = TaskHandle::with_identity("fixed", Uuid::from_u128(201), start).unwrap();
    fixed.set_fixed_start(true).unwrap();
    let repetitive = TaskHandle::with_identity("repetitive", Uuid::from_u128(202), start).unwrap();
    repetitive
        .set_repetition_interval_days_opt(Some(7))
        .unwrap();
    let repetitive_child =
        repetitive.create_as_last_child(crate::test_support::new_task_attr_at("child", start));
    let plain = TaskHandle::with_identity("plain", Uuid::from_u128(203), start).unwrap();
    let ids = [
        fixed.get_id().unwrap(),
        repetitive_child.get_id().unwrap(),
        plain.get_id().unwrap(),
    ];
    let repository = TestTaskRepository::new(vec![fixed, repetitive, plain], start);
    let schedule = ids
        .into_iter()
        .enumerate()
        .map(|(index, id)| ScheduledTaskView {
            occurrence: ScheduleOccurrenceKey::Actual { task_id: id },
            task: get_task(&repository, id).unwrap().unwrap().into(),
            first_available_time: start,
            scheduled_start: start + Duration::minutes(index as i64 * 10),
            scheduled_end: start + Duration::minutes(index as i64 * 10 + 5),
            scheduled_work_seconds: 300,
            total_work_seconds: 300,
            rank: 0,
        })
        .collect::<Vec<_>>();

    let rows = build_scheduled_task_rows(&repository, &schedule, date, start).unwrap();

    assert_eq!(rows[0].task_display_kind, TaskDisplayKind::Fixed);
    assert_eq!(rows[1].task_display_kind, TaskDisplayKind::Repetitive);
    assert_eq!(rows[2].task_display_kind, TaskDisplayKind::NonRepetitive);
}

#[test]
fn listは締切なし・超過・当日・将来をlogical_date境界で分類する() {
    let date = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
    let start = Local.with_ymd_and_hms(2026, 9, 5, 8, 0, 0).unwrap();
    let scheduled_end = start + Duration::hours(1);
    let next_logical_date_start = Local.with_ymd_and_hms(2026, 9, 6, 6, 0, 0).unwrap();
    let deadlines = [
        None,
        Some(scheduled_end - Duration::seconds(1)),
        Some(next_logical_date_start - Duration::seconds(1)),
        Some(next_logical_date_start),
    ];
    let handles = deadlines
        .iter()
        .enumerate()
        .map(|(index, deadline)| {
            let task = TaskHandle::with_identity(
                "deadline kind",
                Uuid::from_u128(210 + index as u128),
                start,
            )
            .unwrap();
            task.set_deadline_time_opt(*deadline).unwrap();
            task
        })
        .collect::<Vec<_>>();
    let repository = TestTaskRepository::new(handles.clone(), start);
    let schedule = handles
        .iter()
        .map(|handle| ScheduledTaskView {
            occurrence: ScheduleOccurrenceKey::Actual {
                task_id: handle.get_id().unwrap(),
            },
            task: get_task(&repository, handle.get_id().unwrap())
                .unwrap()
                .unwrap()
                .into(),
            first_available_time: start,
            scheduled_start: start,
            scheduled_end,
            scheduled_work_seconds: 3_600,
            total_work_seconds: 3_600,
            rank: 0,
        })
        .collect::<Vec<_>>();

    let rows = build_scheduled_task_rows(&repository, &schedule, date, start).unwrap();

    assert_eq!(rows[0].deadline_display_kind, DeadlineDisplayKind::None);
    assert_eq!(rows[1].deadline_display_kind, DeadlineDisplayKind::Overrun);
    assert_eq!(rows[2].deadline_display_kind, DeadlineDisplayKind::Today);
    assert_eq!(rows[3].deadline_display_kind, DeadlineDisplayKind::Future);
}

#[test]
fn listのleaf判定はtask_treeの子ではなくschedule_rank_0だけを採用する() {
    let date = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
    let start = Local.with_ymd_and_hms(2026, 9, 5, 8, 0, 0).unwrap();
    let rank_zero_id = Uuid::from_u128(30);
    let rank_one_id = Uuid::from_u128(31);
    let rank_zero_handle = TaskHandle::with_identity("rank zero", rank_zero_id, start).unwrap();
    let completed_child = rank_zero_handle.create_as_last_child(
        crate::test_support::new_task_attr_at("completed child", start),
    );
    completed_child.set_orig_status(Status::Done).unwrap();
    let rank_one_handle = TaskHandle::with_identity("rank one", rank_one_id, start).unwrap();
    let repository = TestTaskRepository::new(
        vec![rank_zero_handle.clone(), rank_one_handle.clone()],
        start,
    );
    let rank_zero_task = get_task(&repository, rank_zero_id).unwrap().unwrap();
    let rank_one_task = get_task(&repository, rank_one_id).unwrap().unwrap();

    assert!(!rank_zero_task.child_ids.is_empty());
    assert!(rank_one_task.child_ids.is_empty());

    let rows = build_scheduled_task_rows(
        &repository,
        &[
            ScheduledTaskView {
                occurrence: ScheduleOccurrenceKey::Actual {
                    task_id: rank_zero_id,
                },
                task: rank_zero_task.into(),
                first_available_time: start,
                scheduled_start: start,
                scheduled_end: start + Duration::minutes(10),
                scheduled_work_seconds: 600,
                total_work_seconds: 600,
                rank: 0,
            },
            ScheduledTaskView {
                occurrence: ScheduleOccurrenceKey::Actual {
                    task_id: rank_one_id,
                },
                task: rank_one_task.into(),
                first_available_time: start,
                scheduled_start: start + Duration::minutes(10),
                scheduled_end: start + Duration::minutes(20),
                scheduled_work_seconds: 600,
                total_work_seconds: 600,
                rank: 1,
            },
        ],
        date,
        start,
    )
    .unwrap();

    assert!(rows[0].is_leaf);
    assert!(!rows[1].is_leaf);
}

#[test]
fn 負荷は空日で累積を進めず前倒し可能量へtask見積値を使う() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 8, 0, 0).unwrap();
    let first_id = Uuid::from_u128(401);
    let adjustable_id = Uuid::from_u128(402);
    let first = TaskHandle::with_identity("first load", first_id, operation_now).unwrap();
    first.set_estimated_work_seconds(600).unwrap();
    let adjustable =
        TaskHandle::with_identity("adjustable load", adjustable_id, operation_now).unwrap();
    adjustable.set_estimated_work_seconds(3_600).unwrap();
    let repository = TestTaskRepository::new(vec![first, adjustable], operation_now);
    let first_view = get_task(&repository, first_id).unwrap().unwrap();
    let adjustable_view = get_task(&repository, adjustable_id).unwrap().unwrap();
    let schedule = vec![
        ScheduledTaskView {
            occurrence: ScheduleOccurrenceKey::Actual { task_id: first_id },
            task: first_view.into(),
            first_available_time: operation_now,
            scheduled_start: operation_now,
            scheduled_end: operation_now + Duration::minutes(10),
            scheduled_work_seconds: 600,
            total_work_seconds: 600,
            rank: 0,
        },
        ScheduledTaskView {
            occurrence: ScheduleOccurrenceKey::Actual {
                task_id: adjustable_id,
            },
            task: adjustable_view.into(),
            first_available_time: operation_now,
            scheduled_start: operation_now + Duration::days(2),
            scheduled_end: operation_now + Duration::days(2) + Duration::minutes(30),
            scheduled_work_seconds: 1_800,
            total_work_seconds: 3_600,
            rank: 0,
        },
    ];
    let mut free_time = TestFreeTimeManager::new(60);

    let rows = build_band_days(&repository, &mut free_time, &schedule, operation_now, 120).unwrap();

    assert_eq!(rows.len(), 7);
    assert_eq!(rows[0].accumulated_free_diff_seconds, -50 * 60);
    assert_eq!(rows[1].accumulated_free_diff_seconds, -50 * 60);
    assert_eq!(rows[2].accumulated_free_diff_seconds, -80 * 60);
    assert_eq!(rows[2].durations.non_repetitive_seconds, 30 * 60);
}

#[test]
fn 負荷は06時境界とend_of_day_offsetで当日経過を計算する() {
    let before_boundary = Local.with_ymd_and_hms(2026, 9, 5, 5, 59, 0).unwrap();
    let at_boundary = Local.with_ymd_and_hms(2026, 9, 5, 6, 0, 0).unwrap();
    let repository = TestTaskRepository::new(vec![], before_boundary);
    let mut free_time = TestFreeTimeManager::new(600);

    let before_rows =
        build_band_days(&repository, &mut free_time, &[], before_boundary, 120).unwrap();
    let at_rows = build_band_days(
        &TestTaskRepository::new(vec![], at_boundary),
        &mut free_time,
        &[],
        at_boundary,
        120,
    )
    .unwrap();

    assert_eq!(
        before_rows[0].logical_date,
        NaiveDate::from_ymd_opt(2026, 9, 4).unwrap()
    );
    assert_eq!(
        at_rows[0].logical_date,
        NaiveDate::from_ymd_opt(2026, 9, 5).unwrap()
    );

    let evening = Local.with_ymd_and_hms(2026, 9, 5, 20, 0, 0).unwrap();
    let evening_rows = build_band_days(
        &TestTaskRepository::new(vec![], evening),
        &mut TestFreeTimeManager::new(600),
        &[],
        evening,
        120,
    )
    .unwrap();
    assert_eq!(evening_rows[0].durations.unavailable_seconds, 840 * 60);
    assert_eq!(evening_rows[0].durations.elapsed_seconds, 240 * 60);
}

#[test]
fn 負荷は祖先から継承した繰返を単発から分離する() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 8, 0, 0).unwrap();
    let parent = TaskHandle::with_identity("routine", Uuid::from_u128(410), operation_now).unwrap();
    parent.set_repetition_interval_days_opt(Some(7)).unwrap();
    let child = parent
        .create_child(TaskAttr::with_identity(
            "occurrence",
            Uuid::from_u128(411),
            operation_now,
        ))
        .unwrap();
    child.set_estimated_work_seconds(600).unwrap();
    let child_id = child.get_id().unwrap();
    let repository = TestTaskRepository::new(vec![parent], operation_now);
    let child_view = get_task(&repository, child_id).unwrap().unwrap();
    let schedule = [ScheduledTaskView {
        occurrence: ScheduleOccurrenceKey::Actual { task_id: child_id },
        task: child_view.into(),
        first_available_time: operation_now,
        scheduled_start: operation_now,
        scheduled_end: operation_now + Duration::minutes(10),
        scheduled_work_seconds: 600,
        total_work_seconds: 600,
        rank: 0,
    }];

    let rows = build_band_days(
        &repository,
        &mut TestFreeTimeManager::new(600),
        &schedule,
        operation_now,
        120,
    )
    .unwrap();

    assert_eq!(rows[0].durations.repetitive_seconds, 600);
    assert_eq!(rows[0].durations.non_repetitive_seconds, 0);
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
