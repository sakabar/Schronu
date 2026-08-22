use super::*;
use chrono::TimeZone;

fn candidate(
    name: &str,
    first_available_time: DateTime<Local>,
    neg_priority: i64,
    remaining_seconds: i64,
) -> TaskScheduleCandidate {
    let task = crate::test_support::new_task_handle(name).unwrap();
    TaskScheduleCandidate {
        id: task.get_id().unwrap(),
        task,
        first_available_time,
        neg_priority,
        rank: 0,
        deadline_time: None,
        remaining_seconds,
        dependency_ids: vec![],
        atomic: false,
    }
}

#[test]
fn schedule_tasks_by_priority_5分以下の空き時間には分割しない() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let low = candidate(
        "低優先度",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
        -88,
        20 * 60,
    );
    let low_id = low.task.get_id().unwrap();
    let high = candidate(
        "高優先度",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 5, 0).unwrap(),
        -89,
        60 * 60,
    );

    let actual = schedule_tasks_by_priority(&[low, high], now).unwrap();
    let low_segments = actual
        .iter()
        .filter(|scheduled| scheduled.task.get_id().unwrap() == low_id)
        .collect::<Vec<_>>();

    assert_eq!(low_segments.len(), 1);
    assert_eq!(
        low_segments[0].scheduled_start,
        Local.with_ymd_and_hms(2026, 5, 10, 14, 5, 0).unwrap()
    );
    assert_eq!(low_segments[0].scheduled_work_seconds, 20 * 60);
}

#[test]
fn schedule_tasks_by_priority_6分の空き時間には分割する() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let low = candidate(
        "低優先度",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
        -88,
        20 * 60,
    );
    let low_id = low.task.get_id().unwrap();
    let high = candidate(
        "高優先度",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 6, 0).unwrap(),
        -89,
        60 * 60,
    );

    let actual = schedule_tasks_by_priority(&[low, high], now).unwrap();
    let low_segments = actual
        .iter()
        .filter(|scheduled| scheduled.task.get_id().unwrap() == low_id)
        .collect::<Vec<_>>();

    assert_eq!(low_segments.len(), 2);
    assert_eq!(
        low_segments[0].scheduled_start,
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap()
    );
    assert_eq!(low_segments[0].scheduled_work_seconds, 6 * 60);
    assert_eq!(
        low_segments[1].scheduled_start,
        Local.with_ymd_and_hms(2026, 5, 10, 14, 6, 0).unwrap()
    );
    assert_eq!(low_segments[1].scheduled_work_seconds, 14 * 60);
}

#[test]
fn schedule_tasks_by_priority_後半が5分以下になる分割はしない() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let low = candidate(
        "低優先度",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
        -88,
        20 * 60,
    );
    let low_id = low.task.get_id().unwrap();
    let high = candidate(
        "高優先度",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 15, 0).unwrap(),
        -89,
        60 * 60,
    );

    let actual = schedule_tasks_by_priority(&[low, high], now).unwrap();
    let low_segments = actual
        .iter()
        .filter(|scheduled| scheduled.task.get_id().unwrap() == low_id)
        .collect::<Vec<_>>();

    assert_eq!(low_segments.len(), 1);
    assert_eq!(
        low_segments[0].scheduled_start,
        Local.with_ymd_and_hms(2026, 5, 10, 14, 15, 0).unwrap()
    );
    assert_eq!(low_segments[0].scheduled_work_seconds, 20 * 60);
}

#[test]
fn schedule_tasks_by_priority_残り5分以下のtask自体は配置する() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let blocker = candidate("blocker", now, -89, 60 * 60);
    let task = candidate(
        "5分task",
        Local.with_ymd_and_hms(2026, 5, 10, 12, 55, 0).unwrap(),
        -88,
        5 * 60,
    );
    let task_id = task.task.get_id().unwrap();

    let actual = schedule_tasks_by_priority(&[blocker, task], now).unwrap();
    let scheduled = actual
        .iter()
        .find(|scheduled| scheduled.task.get_id().unwrap() == task_id)
        .unwrap();

    assert_eq!(
        scheduled.scheduled_start,
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap()
    );
    assert_eq!(scheduled.scheduled_work_seconds, 5 * 60);
}

#[test]
fn schedule_tasks_by_priority_atomic_taskは依存終了後の連続枠に配置する() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let child = candidate("子", now, -99, 60 * 60);
    let child_id = child.task.get_id().unwrap();
    let blocker = candidate(
        "blocker",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 30, 0).unwrap(),
        -98,
        60 * 60,
    );
    let mut parent = candidate("atomic親", now, -90, 2 * 60 * 60);
    parent.rank = 1;
    parent.atomic = true;
    parent.dependency_ids = vec![child_id];
    let parent_id = parent.task.get_id().unwrap();

    let actual = schedule_tasks_by_priority(&[parent, blocker, child], now).unwrap();
    let scheduled = actual
        .iter()
        .find(|scheduled| scheduled.task.get_id().unwrap() == parent_id)
        .unwrap();

    assert_eq!(
        scheduled.scheduled_start,
        Local.with_ymd_and_hms(2026, 5, 10, 14, 30, 0).unwrap()
    );
    assert_eq!(
        scheduled.scheduled_end,
        Local.with_ymd_and_hms(2026, 5, 10, 16, 30, 0).unwrap()
    );
}

#[test]
fn schedule_tasks_by_priority_高優先度task間の隙間を優先度順に埋める() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let lunch = candidate("昼食", now, -89, 60 * 60);
    let lunch_id = lunch.task.get_id().unwrap();
    let priority_88 = candidate(
        "優先度88",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
        -88,
        4 * 60 * 60,
    );
    let priority_88_id = priority_88.task.get_id().unwrap();
    let priority_87 = candidate(
        "優先度87",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
        -87,
        60 * 60,
    );
    let priority_87_id = priority_87.task.get_id().unwrap();
    let dinner = candidate(
        "夕食",
        Local.with_ymd_and_hms(2026, 5, 10, 18, 0, 0).unwrap(),
        -89,
        60 * 60,
    );
    let dinner_id = dinner.task.get_id().unwrap();

    let actual =
        schedule_tasks_by_priority(&[priority_87, dinner, priority_88, lunch], now).unwrap();
    let start = |id| {
        actual
            .iter()
            .find(|scheduled| scheduled.task.get_id().unwrap() == id)
            .unwrap()
            .scheduled_start
    };

    assert_eq!(start(lunch_id), now);
    assert_eq!(
        start(priority_88_id),
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap()
    );
    assert_eq!(
        start(priority_87_id),
        Local.with_ymd_and_hms(2026, 5, 10, 17, 0, 0).unwrap()
    );
    assert_eq!(
        start(dinner_id),
        Local.with_ymd_and_hms(2026, 5, 10, 18, 0, 0).unwrap()
    );
}

#[test]
fn schedule_tasks_by_priority_親は子の実schedule終了後に配置する() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 14, 0, 0).unwrap();
    let blocker = candidate("blocker", now, -90, 60 * 60);
    let child = candidate("子", now, -1, 60);
    let child_id = child.task.get_id().unwrap();
    let mut parent = candidate("親", now, -99, 0);
    parent.rank = 1;
    parent.dependency_ids = vec![child_id];
    let parent_id = parent.task.get_id().unwrap();

    let actual = schedule_tasks_by_priority(&[parent, blocker, child], now).unwrap();
    let start = |id| {
        actual
            .iter()
            .find(|scheduled| scheduled.task.get_id().unwrap() == id)
            .unwrap()
            .scheduled_start
    };

    assert_eq!(
        start(child_id),
        Local.with_ymd_and_hms(2026, 5, 10, 15, 0, 0).unwrap()
    );
    assert_eq!(
        start(parent_id),
        Local.with_ymd_and_hms(2026, 5, 10, 15, 1, 0).unwrap()
    );
}
