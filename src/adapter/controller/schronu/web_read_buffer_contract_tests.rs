use super::web_read::{build_server_snapshot, calculate_buffer_seconds};
use crate::application::interface::TaskRepositoryTrait;
use crate::test_support::{TestFreeTimeManager, TestTaskRepository};
use chrono::{Duration, Local, NaiveDate, TimeZone};

#[test]
fn bufferは残り空き秒から同一logical_dateの全segmentを差し引く() {
    let current = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
    let other = current.succ_opt().unwrap();

    assert_eq!(
        calculate_buffer_seconds(current, 60, &[(current, 900), (current, 900), (other, 600)]),
        Ok(1_800)
    );
    assert_eq!(
        calculate_buffer_seconds(current, 30, &[(current, 900), (current, 900)]),
        Ok(0)
    );
    assert_eq!(
        calculate_buffer_seconds(current, 15, &[(current, 900), (current, 900)]),
        Ok(-900)
    );
}

#[test]
fn bufferは同じtask由来や進行中と過去のsegmentもuuid集約せず全量を数える() {
    let current = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();

    assert_eq!(
        calculate_buffer_seconds(
            current,
            120,
            &[(current, 600), (current, 900), (current, 300)]
        ),
        Ok(5_400)
    );
}

#[test]
fn bufferは分秒変換とsegment加算と減算のoverflowを情報付きで返す() {
    let current = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();

    for (free_minutes, segments, operation) in [
        (i64::MAX, vec![], "free_minutes_to_seconds"),
        (
            0,
            vec![(current, i64::MAX), (current, 1)],
            "scheduled_seconds_sum",
        ),
        (i64::MIN / 60, vec![(current, 9)], "buffer_subtraction"),
    ] {
        let error = calculate_buffer_seconds(current, free_minutes, &segments).unwrap_err();
        assert_eq!(error.operation(), operation);
        assert!(error.to_string().contains(operation));
    }
}

#[test]
fn snapshotは06時境界と同一のobserved_atを全計算へ使う() {
    let before = Local.with_ymd_and_hms(2026, 9, 5, 5, 59, 59).unwrap();
    let after = before + Duration::seconds(1);

    for (now, expected_date) in [(before, "2026-09-04"), (after, "2026-09-05")] {
        let mut repository = TestTaskRepository::new(vec![], now);
        let mut free_time = TestFreeTimeManager::new(60);

        let snapshot = build_server_snapshot(&mut repository, &mut free_time, now).unwrap();

        assert_eq!(snapshot.observed_at_epoch_ms, now.timestamp_millis());
        assert_eq!(snapshot.logical_date, expected_date);
        assert_eq!(repository.get_last_synced_time(), now);
    }
}

#[test]
fn snapshotの残り空き時間はbusy_time_slotを反映する() {
    let now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 0).unwrap();
    let mut repository = TestTaskRepository::new(vec![], now);
    let mut free_time = TestFreeTimeManager::with_blocked_interval(
        0,
        now + Duration::hours(1),
        now + Duration::hours(2),
    );

    let snapshot = build_server_snapshot(&mut repository, &mut free_time, now).unwrap();

    assert_eq!(snapshot.buffer_seconds, 4 * 60 * 60 + 30 * 60);
}
