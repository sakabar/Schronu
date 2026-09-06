use super::*;
use crate::test_support::TestTaskRepository;
use chrono::{Local, NaiveDate, TimeZone};
use std::cell::Cell;
use std::process::Command;
use uuid::Uuid;

const TIMEZONE_TEST_CHILD: &str = "SCHRONU_TIMEZONE_TEST_CHILD";

fn in_new_york_timezone(test: impl FnOnce()) {
    if std::env::var_os(TIMEZONE_TEST_CHILD).is_some() {
        assert_eq!(
            Local
                .with_ymd_and_hms(2026, 1, 15, 12, 0, 0)
                .unwrap()
                .offset()
                .local_minus_utc(),
            -5 * 60 * 60
        );
        assert_eq!(
            Local
                .with_ymd_and_hms(2026, 7, 15, 12, 0, 0)
                .unwrap()
                .offset()
                .local_minus_utc(),
            -4 * 60 * 60
        );
        test();
        return;
    }

    let current_thread = std::thread::current();
    let test_name = current_thread
        .name()
        .expect("timezone test thread must have a name");
    let output = Command::new(std::env::current_exe().expect("test executable must exist"))
        .arg(test_name)
        .arg("--exact")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env("TZ", "America/New_York")
        .env(TIMEZONE_TEST_CHILD, "1")
        .output()
        .expect("timezone test subprocess must start");

    assert!(
        output.status.success(),
        "timezone test subprocess failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn timezone_subprocess_harnessはnew_yorkのoffsetを適用する() {
    in_new_york_timezone(|| {});
}

fn next_repetition_child(
    interval_days: i64,
    days_in_advance: i64,
    anchor: DateTime<Local>,
    parent_deadline_time: Option<DateTime<Local>>,
) -> TaskHandle {
    let parent = crate::test_support::new_task_handle_at("ルーチン", anchor).unwrap();
    parent
        .set_repetition_interval_days_opt(Some(interval_days))
        .unwrap();
    parent
        .set_repetition_anchor(RepetitionAnchor::Deadline)
        .unwrap();
    parent.set_days_in_advance(days_in_advance).unwrap();
    parent
        .set_start_time(Local.with_ymd_and_hms(2026, 1, 1, 9, 30, 0).unwrap())
        .unwrap();
    parent.set_deadline_time_opt(parent_deadline_time).unwrap();

    let mut child_attr = crate::test_support::new_task_attr_at("今回", anchor);
    child_attr.set_deadline_time_opt(Some(anchor));
    let child = parent.create_as_last_child(child_attr);
    let mut repository = TestTaskRepository::new(vec![parent.clone()], anchor);
    let mut next_id = || Uuid::from_u128(0x3501);
    let mut factory = TaskFactory::new(anchor, &mut next_id);

    complete_task(
        &mut repository,
        CompleteTaskInput {
            task_id: child.get_id().unwrap(),
            finished_at: anchor,
            additional_actual_work_seconds: 0,
            expected_actual_work_seconds: None,
        },
        &mut factory,
    )
    .unwrap();

    parent
        .get_by_id(Uuid::from_u128(0x3501))
        .unwrap()
        .expect("next repetition child must exist")
}

#[test]
fn complete_taskはdst境界の1日と7日周期で壁時計時刻を維持する() {
    in_new_york_timezone(|| {
        for (anchor_date, interval_days, target_date) in [
            ((2026, 3, 7), 1, (2026, 3, 8)),
            ((2026, 3, 1), 7, (2026, 3, 8)),
            ((2026, 10, 31), 1, (2026, 11, 1)),
            ((2026, 10, 25), 7, (2026, 11, 1)),
        ] {
            for parent_has_deadline in [true, false] {
                let anchor = Local
                    .with_ymd_and_hms(anchor_date.0, anchor_date.1, anchor_date.2, 18, 0, 0)
                    .unwrap();
                let parent_deadline_time = parent_has_deadline
                    .then(|| Local.with_ymd_and_hms(2026, 1, 1, 18, 0, 0).unwrap());
                let next = next_repetition_child(
                    interval_days,
                    interval_days,
                    anchor,
                    parent_deadline_time,
                );
                let expected_start = Local
                    .with_ymd_and_hms(anchor_date.0, anchor_date.1, anchor_date.2, 9, 30, 0)
                    .unwrap();
                let expected_deadline = Local
                    .with_ymd_and_hms(
                        target_date.0,
                        target_date.1,
                        target_date.2,
                        if parent_has_deadline { 18 } else { 23 },
                        if parent_has_deadline { 0 } else { 59 },
                        if parent_has_deadline { 0 } else { 59 },
                    )
                    .unwrap();

                assert_eq!(next.get_start_time().unwrap(), expected_start);
                assert_eq!(
                    next.get_deadline_time_opt().unwrap(),
                    Some(expected_deadline)
                );
            }
        }
    });
}

fn assert_complete_task_calendar_error(
    anchor: DateTime<Local>,
    parent_start_time: DateTime<Local>,
    parent_deadline_time: DateTime<Local>,
    days_in_advance: i64,
    expected_error: ApplicationError,
) {
    let parent = crate::test_support::new_task_handle_at("ルーチン", anchor).unwrap();
    parent.set_repetition_interval_days_opt(Some(1)).unwrap();
    parent
        .set_repetition_anchor(RepetitionAnchor::Deadline)
        .unwrap();
    parent.set_days_in_advance(days_in_advance).unwrap();
    parent.set_start_time(parent_start_time).unwrap();
    parent
        .set_deadline_time_opt(Some(parent_deadline_time))
        .unwrap();
    let mut child_attr = crate::test_support::new_task_attr_at("今回", anchor);
    child_attr.set_deadline_time_opt(Some(anchor));
    let child = parent.create_as_last_child(child_attr);
    let child_snapshot = child.snapshot().unwrap();
    let parent_revision = parent.get_persistent_mutation_revision().unwrap();
    let children_before = parent.get_children().unwrap();
    let child_id = child.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], anchor);
    repository.set_highest_priority_leaf_task_id(Some(child_id));
    let save_count = repository.save_count();
    let id_call_count = Cell::new(0);
    let mut next_id = || {
        id_call_count.set(id_call_count.get() + 1);
        Uuid::from_u128(0x3502)
    };
    let mut factory = TaskFactory::new(anchor, &mut next_id);

    assert_eq!(
        complete_task(
            &mut repository,
            CompleteTaskInput {
                task_id: child_id,
                finished_at: anchor,
                additional_actual_work_seconds: 0,
                expected_actual_work_seconds: None,
            },
            &mut factory,
        ),
        Err(expected_error)
    );
    assert_eq!(child.snapshot().unwrap(), child_snapshot);
    assert_eq!(
        parent.get_persistent_mutation_revision().unwrap(),
        parent_revision
    );
    assert_eq!(parent.get_children().unwrap(), children_before);
    assert_eq!(repository.highest_priority_leaf_task_id(), Some(child_id));
    assert_eq!(repository.save_count(), save_count);
    assert_eq!(id_call_count.get(), 0);
}

#[test]
fn complete_taskは不存在の反復startをerrorにして変更しない() {
    in_new_york_timezone(|| {
        let local_datetime = NaiveDate::from_ymd_opt(2026, 3, 8)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_complete_task_calendar_error(
            Local.with_ymd_and_hms(2026, 3, 8, 18, 0, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 1, 1, 2, 30, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 1, 1, 18, 0, 0).unwrap(),
            1,
            ApplicationError::NonexistentLocalDateTime { local_datetime },
        );
    });
}

#[test]
fn complete_taskは曖昧な反復startをerrorにして変更しない() {
    in_new_york_timezone(|| {
        let local_datetime = NaiveDate::from_ymd_opt(2026, 11, 1)
            .unwrap()
            .and_hms_opt(1, 30, 0)
            .unwrap();
        let candidates = Local.from_local_datetime(&local_datetime);
        assert_complete_task_calendar_error(
            Local.with_ymd_and_hms(2026, 11, 1, 18, 0, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 1, 1, 1, 30, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 1, 1, 18, 0, 0).unwrap(),
            1,
            ApplicationError::AmbiguousLocalDateTime {
                local_datetime,
                earlier: candidates.earliest().unwrap(),
                later: candidates.latest().unwrap(),
            },
        );
    });
}

#[test]
fn complete_taskは不存在の反復deadlineをerrorにして変更しない() {
    in_new_york_timezone(|| {
        let local_datetime = NaiveDate::from_ymd_opt(2026, 3, 8)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_complete_task_calendar_error(
            Local.with_ymd_and_hms(2026, 3, 7, 18, 0, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 1, 1, 2, 30, 0).unwrap(),
            0,
            ApplicationError::NonexistentLocalDateTime { local_datetime },
        );
    });
}

#[test]
fn complete_taskは曖昧な反復deadlineをerrorにして変更しない() {
    in_new_york_timezone(|| {
        let local_datetime = NaiveDate::from_ymd_opt(2026, 11, 1)
            .unwrap()
            .and_hms_opt(1, 30, 0)
            .unwrap();
        let candidates = Local.from_local_datetime(&local_datetime);
        assert_complete_task_calendar_error(
            Local.with_ymd_and_hms(2026, 10, 31, 18, 0, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 1, 1, 1, 30, 0).unwrap(),
            0,
            ApplicationError::AmbiguousLocalDateTime {
                local_datetime,
                earlier: candidates.earliest().unwrap(),
                later: candidates.latest().unwrap(),
            },
        );
    });
}

fn deferred_routine_task(
    interval_days: i64,
    original_start: DateTime<Local>,
    original_deadline: DateTime<Local>,
    parent_deadline_time: Option<DateTime<Local>>,
) -> TaskHandle {
    let parent = crate::test_support::new_task_handle_at("ルーチン", original_deadline).unwrap();
    parent
        .set_repetition_interval_days_opt(Some(interval_days))
        .unwrap();
    parent.set_deadline_time_opt(parent_deadline_time).unwrap();
    let mut child_attr = crate::test_support::new_task_attr_at("延期対象", original_deadline);
    child_attr.set_start_time(original_start);
    child_attr.set_deadline_time_opt(Some(original_deadline));
    let child = parent.create_as_last_child(child_attr);
    let mut repository = TestTaskRepository::new(vec![parent], original_deadline);

    defer_routine_task(&mut repository, child.get_id().unwrap()).unwrap();
    child
}

#[test]
fn defer_routine_taskはdst境界の1日と7日周期で壁時計時刻を維持する() {
    in_new_york_timezone(|| {
        for (original_date, interval_days, target_date) in [
            ((2026, 3, 7), 1, (2026, 3, 8)),
            ((2026, 3, 1), 7, (2026, 3, 8)),
            ((2026, 10, 31), 1, (2026, 11, 1)),
            ((2026, 10, 25), 7, (2026, 11, 1)),
        ] {
            for parent_has_deadline in [true, false] {
                let original_start = Local
                    .with_ymd_and_hms(original_date.0, original_date.1, original_date.2, 9, 0, 0)
                    .unwrap();
                let original_deadline = Local
                    .with_ymd_and_hms(original_date.0, original_date.1, original_date.2, 10, 0, 0)
                    .unwrap();
                let parent_deadline_time = parent_has_deadline
                    .then(|| Local.with_ymd_and_hms(2026, 1, 1, 18, 0, 0).unwrap());
                let task = deferred_routine_task(
                    interval_days,
                    original_start,
                    original_deadline,
                    parent_deadline_time,
                );
                let expected_start = Local
                    .with_ymd_and_hms(target_date.0, target_date.1, target_date.2, 9, 0, 0)
                    .unwrap();
                let expected_deadline = Local
                    .with_ymd_and_hms(
                        target_date.0,
                        target_date.1,
                        target_date.2,
                        if parent_has_deadline { 18 } else { 10 },
                        0,
                        0,
                    )
                    .unwrap();

                assert_eq!(task.get_start_time().unwrap(), expected_start);
                assert_eq!(
                    task.get_deadline_time_opt().unwrap(),
                    Some(expected_deadline)
                );
            }
        }
    });
}

fn assert_defer_routine_calendar_error(
    original_start: DateTime<Local>,
    original_deadline: DateTime<Local>,
    parent_deadline_time: Option<DateTime<Local>>,
    expected_error: ApplicationError,
) {
    let parent = crate::test_support::new_task_handle_at("ルーチン", original_deadline).unwrap();
    parent.set_repetition_interval_days_opt(Some(1)).unwrap();
    parent.set_deadline_time_opt(parent_deadline_time).unwrap();
    let mut child_attr = crate::test_support::new_task_attr_at("延期対象", original_deadline);
    child_attr.set_start_time(original_start);
    child_attr.set_deadline_time_opt(Some(original_deadline));
    let child = parent.create_as_last_child(child_attr);
    let child_snapshot = child.snapshot().unwrap();
    let parent_revision = parent.get_persistent_mutation_revision().unwrap();
    let child_id = child.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], original_deadline);
    let save_count = repository.save_count();

    assert_eq!(
        defer_routine_task(&mut repository, child_id),
        Err(expected_error)
    );
    assert_eq!(child.snapshot().unwrap(), child_snapshot);
    assert_eq!(
        parent.get_persistent_mutation_revision().unwrap(),
        parent_revision
    );
    assert_eq!(repository.save_count(), save_count);
}

#[test]
fn defer_routine_taskは不存在のdeadlineをerrorにして変更しない() {
    in_new_york_timezone(|| {
        let local_datetime = NaiveDate::from_ymd_opt(2026, 3, 8)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_defer_routine_calendar_error(
            Local.with_ymd_and_hms(2026, 3, 7, 1, 0, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 3, 7, 2, 30, 0).unwrap(),
            None,
            ApplicationError::NonexistentLocalDateTime { local_datetime },
        );
    });
}

#[test]
fn defer_routine_taskは曖昧なdeadlineをerrorにして変更しない() {
    in_new_york_timezone(|| {
        let local_datetime = NaiveDate::from_ymd_opt(2026, 11, 1)
            .unwrap()
            .and_hms_opt(1, 30, 0)
            .unwrap();
        let candidates = Local.from_local_datetime(&local_datetime);
        assert_defer_routine_calendar_error(
            Local.with_ymd_and_hms(2026, 10, 31, 0, 30, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 10, 31, 1, 30, 0).unwrap(),
            None,
            ApplicationError::AmbiguousLocalDateTime {
                local_datetime,
                earlier: candidates.earliest().unwrap(),
                later: candidates.latest().unwrap(),
            },
        );
    });
}

#[test]
fn defer_routine_taskは親の不存在deadline時刻をerrorにして変更しない() {
    in_new_york_timezone(|| {
        let local_datetime = NaiveDate::from_ymd_opt(2026, 3, 8)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_defer_routine_calendar_error(
            Local.with_ymd_and_hms(2026, 3, 7, 9, 0, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 3, 7, 10, 0, 0).unwrap(),
            Some(Local.with_ymd_and_hms(2026, 1, 1, 2, 30, 0).unwrap()),
            ApplicationError::NonexistentLocalDateTime { local_datetime },
        );
    });
}

#[test]
fn defer_routine_taskは親の曖昧deadline時刻をerrorにして変更しない() {
    in_new_york_timezone(|| {
        let local_datetime = NaiveDate::from_ymd_opt(2026, 11, 1)
            .unwrap()
            .and_hms_opt(1, 30, 0)
            .unwrap();
        let candidates = Local.from_local_datetime(&local_datetime);
        assert_defer_routine_calendar_error(
            Local.with_ymd_and_hms(2026, 10, 31, 9, 0, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 10, 31, 10, 0, 0).unwrap(),
            Some(Local.with_ymd_and_hms(2026, 1, 1, 1, 30, 0).unwrap()),
            ApplicationError::AmbiguousLocalDateTime {
                local_datetime,
                earlier: candidates.earliest().unwrap(),
                later: candidates.latest().unwrap(),
            },
        );
    });
}

#[test]
fn defer_routine_taskは不存在のstartをerrorにして変更しない() {
    in_new_york_timezone(|| {
        let local_datetime = NaiveDate::from_ymd_opt(2026, 3, 8)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_defer_routine_calendar_error(
            Local.with_ymd_and_hms(2026, 3, 7, 2, 30, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 3, 7, 18, 0, 0).unwrap(),
            Some(Local.with_ymd_and_hms(2026, 1, 1, 18, 0, 0).unwrap()),
            ApplicationError::NonexistentLocalDateTime { local_datetime },
        );
    });
}

#[test]
fn defer_routine_taskは曖昧なstartをerrorにして変更しない() {
    in_new_york_timezone(|| {
        let local_datetime = NaiveDate::from_ymd_opt(2026, 11, 1)
            .unwrap()
            .and_hms_opt(1, 30, 0)
            .unwrap();
        let candidates = Local.from_local_datetime(&local_datetime);
        assert_defer_routine_calendar_error(
            Local.with_ymd_and_hms(2026, 10, 31, 1, 30, 0).unwrap(),
            Local.with_ymd_and_hms(2026, 10, 31, 18, 0, 0).unwrap(),
            Some(Local.with_ymd_and_hms(2026, 1, 1, 18, 0, 0).unwrap()),
            ApplicationError::AmbiguousLocalDateTime {
                local_datetime,
                earlier: candidates.earliest().unwrap(),
                later: candidates.latest().unwrap(),
            },
        );
    });
}
