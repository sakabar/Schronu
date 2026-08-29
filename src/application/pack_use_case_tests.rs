use super::*;
use crate::entity::task::{Status, TaskHandle};
use crate::test_support::{TestFreeTimeManager, TestTaskRepository};
use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDate, TimeZone};
use uuid::Uuid;

fn fixed_now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap()
}

fn pending_task(
    name: &str,
    now: DateTime<Local>,
    pending_until: DateTime<Local>,
    work_minutes: i64,
    priority: i64,
) -> TaskHandle {
    let task = crate::test_support::new_task_handle(name).unwrap();
    task.sync_clock(now).unwrap();
    task.set_start_time(now).unwrap();
    task.set_estimated_work_seconds(work_minutes * 60).unwrap();
    task.set_priority(priority).unwrap();
    task.set_pending_until(pending_until).unwrap();
    task.set_orig_status(Status::Pending).unwrap();
    task
}

#[test]
fn pack_tasksはoperation時刻のlogical_date計算不能を伝搬しtaskを変更しない() {
    let local_datetime = NaiveDate::MIN.and_hms_opt(5, 59, 0).unwrap();
    let now = DateTime::<Local>::from_naive_utc_and_offset(
        local_datetime,
        FixedOffset::east_opt(0).unwrap(),
    );
    let task = crate::test_support::new_task_handle("対象").unwrap();
    let original_revision = task.get_persistent_mutation_revision().unwrap();
    let repository = TestTaskRepository::new(vec![task.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(60);

    let actual = pack_tasks_with_end_of_day_offset_minutes(
        &repository,
        &mut free_time_manager,
        END_OF_DAY_OFFSET_MINUTES,
    );

    assert_eq!(
        actual,
        Err(ApplicationError::LogicalDateOutOfRange {
            operation: "logical_date",
            datetime: now,
        })
    );
    assert_eq!(
        task.get_persistent_mutation_revision().unwrap(),
        original_revision
    );
}

#[test]
fn pack_tasks_優先度が高い順に今日の余差へ前倒しする() {
    let now = fixed_now();
    let low = pending_task("低", now, now + Duration::days(10), 30, 1);
    let high = pending_task("高", now, now + Duration::days(10), 30, 9);
    let repository = TestTaskRepository::new(vec![low.clone(), high.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(120);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(
        actual
            .packed_tasks
            .iter()
            .map(|packed| packed.task_id)
            .collect::<Vec<_>>(),
        vec![high.get_id().unwrap(), low.get_id().unwrap()]
    );
    assert!(actual
        .packed_tasks
        .iter()
        .all(|packed| packed.target_date == NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()));
    assert_eq!(high.get_start_time().unwrap(), now);
    assert!(high.get_pending_until().unwrap() < now + Duration::days(10));
}

#[test]
fn pack_tasks_同じ優先度では現在の予定日時が早い順に詰める() {
    let now = fixed_now();
    let later = pending_task("後", now, now + Duration::days(11), 30, 5);
    let earlier = pending_task("先", now, now + Duration::days(10), 30, 5);
    let repository = TestTaskRepository::new(vec![later.clone(), earlier.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(120);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(
        actual
            .packed_tasks
            .iter()
            .map(|packed| packed.task_id)
            .collect::<Vec<_>>(),
        vec![earlier.get_id().unwrap(), later.get_id().unwrap()]
    );
}

#[test]
fn pack_tasks_優先度と予定日時が同じならuuid昇順に詰める() {
    let now = fixed_now();
    let mut larger_id = pending_task("後", now, now + Duration::days(10), 30, 5);
    larger_id
        .set_id(Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap())
        .unwrap();
    let mut smaller_id = pending_task("先", now, now + Duration::days(10), 30, 5);
    smaller_id
        .set_id(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap())
        .unwrap();
    let repository = TestTaskRepository::new(vec![larger_id.clone(), smaller_id.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(120);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(
        actual
            .packed_tasks
            .iter()
            .map(|packed| packed.task_id)
            .collect::<Vec<_>>(),
        vec![smaller_id.get_id().unwrap(), larger_id.get_id().unwrap()]
    );
}

#[test]
fn pack_tasks_配置ごとに余差を再計算して収まらないtaskをスキップする() {
    let now = fixed_now();
    let first = pending_task("1", now, now + Duration::days(1), 30, 3);
    let second = pending_task("2", now, now + Duration::days(1), 30, 2);
    let third = pending_task("3", now, now + Duration::days(1), 30, 1);
    let fourth = pending_task("4", now, now + Duration::days(1), 15, 0);
    let repository = TestTaskRepository::new(
        vec![first.clone(), second.clone(), third.clone(), fourth.clone()],
        now,
    );
    let mut free_time_manager = TestFreeTimeManager::new(120);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(
        actual
            .packed_tasks
            .iter()
            .map(|packed| packed.task_id)
            .collect::<Vec<_>>(),
        vec![
            first.get_id().unwrap(),
            second.get_id().unwrap(),
            fourth.get_id().unwrap()
        ]
    );
    assert_eq!(actual.skipped_tasks.len(), 1);
    assert_eq!(actual.skipped_tasks[0].task_id, third.get_id().unwrap());
    assert_eq!(third.get_pending_until().unwrap(), now + Duration::days(1));
}

#[test]
fn pack_tasksはpartly_doneとzero_workのfixed予約全体を日次容量へ計上する() {
    let now = fixed_now();

    for actual_work_seconds in [45 * 60, 60 * 60] {
        let fixed = crate::test_support::new_task_handle("fixed-reservation").unwrap();
        fixed.sync_clock(now).unwrap();
        fixed.set_start_time(now).unwrap();
        fixed.set_estimated_work_seconds(60 * 60).unwrap();
        fixed.set_actual_work_seconds(actual_work_seconds).unwrap();
        fixed.set_fixed_start(true).unwrap();
        let candidate = pending_task("candidate", now, now + Duration::days(1), 30, 1);
        let repository = TestTaskRepository::new(vec![fixed, candidate.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let result = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert!(
            result.packed_tasks.is_empty(),
            "actual={actual_work_seconds}"
        );
        assert_eq!(result.skipped_tasks[0].task_id, candidate.get_id().unwrap());
    }
}

#[test]
fn pack_tasks_先行配置後の最新予定で後続taskの前倒し可否を判定する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 6, 0, 0).unwrap();
    let first = pending_task("18時間20分", now, now + Duration::days(1), 18 * 60 + 20, 9);
    let second = pending_task("後続", now, now + Duration::days(1), 30, 8);
    let original_second_pending_until = second.get_pending_until().unwrap();
    let repository = TestTaskRepository::new(vec![first.clone(), second.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(40 * 60);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(actual.packed_tasks[0].task_id, first.get_id().unwrap());
    assert_eq!(actual.skipped_tasks.len(), 1);
    assert_eq!(actual.skipped_tasks[0].task_id, second.get_id().unwrap());
    assert_eq!(
        second.get_pending_until().unwrap(),
        original_second_pending_until
    );
}

#[test]
fn pack_tasks_優先度がi64最小値でも前倒しする() {
    let now = fixed_now();
    let task = pending_task("最小", now, now + Duration::days(10), 30, i64::MIN);
    let repository = TestTaskRepository::new(vec![task.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(120);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(actual.packed_tasks[0].task_id, task.get_id().unwrap());
}

#[test]
fn pack_tasks_pending_untilを実際の配置開始時刻へ設定する() {
    let now = fixed_now();
    let blocker = crate::test_support::new_task_handle("先行").unwrap();
    blocker.sync_clock(now).unwrap();
    blocker.set_start_time(now).unwrap();
    blocker.set_estimated_work_seconds(30 * 60).unwrap();
    blocker.set_priority(10).unwrap();
    let candidate = pending_task("対象", now, now + Duration::days(10), 30, 9);
    let repository = TestTaskRepository::new(vec![blocker, candidate.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(180);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(candidate.get_start_time().unwrap(), now);
    assert_eq!(
        candidate.get_pending_until().unwrap(),
        now + Duration::minutes(30)
    );
}

#[test]
fn pack_tasks_最優先候補が収まらなければスキップして低優先度候補を詰める() {
    let now = fixed_now();
    let low = pending_task("低", now, now + Duration::days(10), 30, 1);
    let high = pending_task("高", now, now + Duration::days(10), 60, 9);
    let original_low_pending_until = low.get_pending_until().unwrap();
    let original_high_revision = high.get_persistent_mutation_revision().unwrap();
    let repository = TestTaskRepository::new(vec![low.clone(), high.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(60);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(actual.packed_tasks[0].task_id, low.get_id().unwrap());
    assert_eq!(actual.skipped_tasks.len(), 1);
    assert_eq!(actual.skipped_tasks[0].task_id, high.get_id().unwrap());
    assert!(low.get_pending_until().unwrap() < original_low_pending_until);
    assert_eq!(
        high.get_persistent_mutation_revision().unwrap(),
        original_high_revision
    );
}

#[test]
fn pack_tasks_7日合計には収まっても単一日の余差に収まらなければスキップする() {
    let now = fixed_now();
    let task = pending_task("長い", now, now + Duration::days(10), 60, 9);
    let repository = TestTaskRepository::new(vec![task.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(60);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert!(actual.packed_tasks.is_empty());
    assert_eq!(actual.skipped_tasks.len(), 1);
    assert_eq!(actual.skipped_tasks[0].task_id, task.get_id().unwrap());
}

#[test]
fn pack_tasks_対象期間は06時区切りの今日から7日間とする() {
    let now = Local.with_ymd_and_hms(2026, 8, 12, 1, 0, 0).unwrap();
    let task = pending_task("対象", now, now + Duration::days(10), 30, 9);
    task.set_start_time(Local.with_ymd_and_hms(2026, 8, 17, 6, 0, 0).unwrap())
        .unwrap();
    let repository = TestTaskRepository::new(vec![task.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(60);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(
        actual.packed_tasks[0].target_date,
        NaiveDate::from_ymd_opt(2026, 8, 17).unwrap()
    );
}

#[test]
fn pack_tasks_8日目から着手可能なtaskは対象外にする() {
    let now = Local.with_ymd_and_hms(2026, 8, 12, 1, 0, 0).unwrap();
    let task = pending_task("対象外", now, now + Duration::days(10), 30, 9);
    task.set_start_time(Local.with_ymd_and_hms(2026, 8, 18, 6, 0, 0).unwrap())
        .unwrap();
    let repository = TestTaskRepository::new(vec![task], now);
    let mut free_time_manager = TestFreeTimeManager::new(60);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert!(actual.packed_tasks.is_empty());
    assert!(actual.skipped_tasks.is_empty());
}

#[test]
fn pack_tasks_締切と依存と反復設定を変更しない() {
    let now = fixed_now();
    let task = pending_task("対象", now, now + Duration::days(10), 30, 9);
    let deadline = now + Duration::days(20);
    task.set_deadline_time_opt(Some(deadline)).unwrap();
    task.set_repetition_interval_days_opt(Some(7)).unwrap();
    let repository = TestTaskRepository::new(vec![task.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(120);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(task.get_deadline_time_opt().unwrap(), Some(deadline));
    assert_eq!(task.get_repetition_interval_days_opt().unwrap(), Some(7));
    assert!(task.get_children().unwrap().is_empty());
}

#[test]
fn pack_tasks_atomicは初期予定枠に行動不能時間が重なれば同日後刻へ前倒しする() {
    let now = fixed_now();
    let task = pending_task("atomic", now, now + Duration::days(10), 60, 9);
    task.set_atomic(true).unwrap();
    let repository = TestTaskRepository::new(vec![task.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::with_blocked_interval(
        180,
        now + Duration::minutes(30),
        now + Duration::minutes(90),
    );

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(
        actual.packed_tasks[0].target_date,
        NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()
    );
    assert_eq!(
        task.get_pending_until().unwrap(),
        now + Duration::minutes(90)
    );
}

#[test]
fn pack_tasks_atomicは同日後刻の連続空き枠へ前倒しする() {
    let now = fixed_now();
    let task = pending_task("atomic", now, now + Duration::days(10), 60, 9);
    task.set_atomic(true).unwrap();
    let repository = TestTaskRepository::new(vec![task.clone()], now);
    let mut free_time_manager =
        TestFreeTimeManager::with_blocked_interval(180, now, now + Duration::hours(1));

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(
        actual.packed_tasks[0].target_date,
        NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()
    );
    assert_eq!(task.get_pending_until().unwrap(), now + Duration::hours(1));
}

#[test]
fn pack_tasks_atomicは初日に連続空き枠がなければ翌日へ前倒しする() {
    let now = fixed_now();
    let task = pending_task("atomic", now, now + Duration::days(10), 60, 9);
    task.set_atomic(true).unwrap();
    let repository = TestTaskRepository::new(vec![task.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::with_blocked_interval(
        180,
        now,
        Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap(),
    );

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(
        actual.packed_tasks[0].target_date,
        NaiveDate::from_ymd_opt(2026, 8, 12).unwrap()
    );
}

#[test]
fn pack_tasks_atomicは残作業に秒端数があっても空き枠へ前倒しする() {
    let now = fixed_now();
    let task = pending_task("atomic", now, now + Duration::days(10), 30, 9);
    task.set_actual_work_seconds(1).unwrap();
    task.set_atomic(true).unwrap();
    let repository = TestTaskRepository::new(vec![task.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(60);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(actual.packed_tasks[0].work_seconds, 30 * 60 - 1);
    assert_eq!(task.get_pending_until().unwrap(), now);
}

#[test]
fn pack_tasks_atomicに連続空き枠がなければスキップして次のtaskを詰める() {
    let now = fixed_now();
    let atomic = pending_task("atomic", now, now + Duration::days(10), 60, 9);
    atomic.set_atomic(true).unwrap();
    let next = pending_task("次", now, now + Duration::days(10), 30, 8);
    let repository = TestTaskRepository::new(vec![atomic.clone(), next.clone()], now);
    let mut free_time_manager =
        TestFreeTimeManager::with_blocked_interval(180, now, now + Duration::days(7));

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(actual.skipped_tasks.len(), 1);
    assert_eq!(actual.skipped_tasks[0].task_id, atomic.get_id().unwrap());
    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(actual.packed_tasks[0].task_id, next.get_id().unwrap());
}

#[test]
fn pack_tasks_複数taskをスキップして収まる次のtaskを詰める() {
    let now = fixed_now();
    let first = pending_task("1", now, now + Duration::days(10), 60, 9);
    let second = pending_task("2", now, now + Duration::days(10), 60, 8);
    let third = pending_task("3", now, now + Duration::days(10), 30, 7);
    let repository =
        TestTaskRepository::new(vec![first.clone(), second.clone(), third.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(60);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(
        actual
            .skipped_tasks
            .iter()
            .map(|skipped| skipped.task_id)
            .collect::<Vec<_>>(),
        vec![first.get_id().unwrap(), second.get_id().unwrap()]
    );
    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(actual.packed_tasks[0].task_id, third.get_id().unwrap());
}

#[test]
fn pack_tasks_相手待ちや着手可能日が期間外のtaskは候補にしない() {
    let now = fixed_now();
    let waiting = pending_task("待ち", now, now + Duration::days(10), 30, 9);
    waiting.set_is_on_other_side(true).unwrap();
    let future = pending_task("未来", now, now + Duration::days(10), 30, 8);
    future.set_start_time(now + Duration::days(8)).unwrap();
    let repository = TestTaskRepository::new(vec![waiting, future], now);
    let mut free_time_manager = TestFreeTimeManager::new(120);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert!(actual.packed_tasks.is_empty());
    assert!(actual.skipped_tasks.is_empty());
}

#[test]
fn pack_tasks_親taskと完了済みtaskと残作業0のtaskは候補にしない() {
    let now = fixed_now();
    let parent = pending_task("親", now, now + Duration::days(10), 30, 9);
    let child = parent.create_as_last_child(crate::test_support::new_task_attr("子"));
    child.sync_clock(now).unwrap();
    let done = pending_task("完了", now, now + Duration::days(10), 30, 8);
    done.set_orig_status(Status::Done).unwrap();
    let zero = pending_task("ゼロ", now, now + Duration::days(10), 0, 7);
    let repository = TestTaskRepository::new(vec![parent, done, zero], now);
    let mut free_time_manager = TestFreeTimeManager::new(120);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert!(actual.packed_tasks.is_empty());
    assert!(actual.skipped_tasks.is_empty());
}

#[test]
fn pack_tasks_候補外の親taskの範囲外start_timeは正常な葉taskの配置を妨げない() {
    let now = fixed_now();
    let parent = pending_task("親", now, now + Duration::days(10), 30, 1);
    let out_of_range_start = DateTime::<Local>::from_naive_utc_and_offset(
        NaiveDate::MIN.and_hms_opt(5, 59, 0).unwrap(),
        FixedOffset::east_opt(0).unwrap(),
    );
    parent.set_start_time(out_of_range_start).unwrap();
    let child = parent.create_as_last_child(crate::test_support::new_task_attr("子"));
    child.sync_clock(now).unwrap();
    child.set_start_time(now).unwrap();
    child.set_estimated_work_seconds(30 * 60).unwrap();

    let leaf = pending_task("正常な葉", now, now + Duration::days(10), 30, 9);
    let repository = TestTaskRepository::new(vec![parent, leaf.clone()], now);
    let mut free_time_manager = TestFreeTimeManager::new(120);

    let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

    assert_eq!(actual.packed_tasks.len(), 1);
    assert_eq!(actual.packed_tasks[0].task_id, leaf.get_id().unwrap());
    assert!(leaf.get_pending_until().unwrap() < now + Duration::days(10));
}
