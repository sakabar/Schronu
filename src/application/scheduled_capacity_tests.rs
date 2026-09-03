use super::*;
use crate::application::task_use_case::ApplicationError;
use chrono::{Duration, FixedOffset, NaiveDate, TimeZone};

fn datetime(day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Local> {
    Local
        .with_ymd_and_hms(2026, 8, day, hour, minute, second)
        .unwrap()
}

#[test]
fn fixedを論理日境界との交差時間へ配賦する() {
    let start = datetime(11, 5, 30, 0);

    let actual = scheduled_capacity_seconds_by_logical_date(
        true,
        start,
        start + Duration::hours(1),
        15 * 60,
    )
    .unwrap();

    assert_eq!(
        actual,
        vec![
            (start.date_naive() - Duration::days(1), 30 * 60),
            (start.date_naive(), 30 * 60),
        ]
    );
}

#[test]
fn flexibleを論理日境界との交差時間比で配賦する() {
    let start = datetime(11, 5, 30, 0);

    let actual = scheduled_capacity_seconds_by_logical_date(
        false,
        start,
        start + Duration::hours(1),
        60 * 60,
    )
    .unwrap();

    assert_eq!(
        actual,
        vec![
            (start.date_naive() - Duration::days(1), 30 * 60),
            (start.date_naive(), 30 * 60),
        ]
    );
}

#[test]
fn flexibleの丸め差を最後の論理日へ集約して総量を保存する() {
    let start = datetime(11, 5, 59, 58);
    let scheduled_work_seconds = 10;

    let actual = scheduled_capacity_seconds_by_logical_date(
        false,
        start,
        start + Duration::seconds(3),
        scheduled_work_seconds,
    )
    .unwrap();

    assert_eq!(
        actual,
        vec![
            (start.date_naive() - Duration::days(1), 6),
            (start.date_naive(), 4),
        ]
    );
    assert_eq!(
        actual.iter().map(|(_, seconds)| seconds).sum::<i64>(),
        scheduled_work_seconds
    );
}

#[test]
fn 複数日を跨ぐfixedとflexibleの総容量を保存する() {
    let start = datetime(10, 5, 30, 0);
    let end = datetime(12, 6, 30, 0);

    let fixed = scheduled_capacity_seconds_by_logical_date(true, start, end, 1).unwrap();
    let flexible = scheduled_capacity_seconds_by_logical_date(false, start, end, 97).unwrap();

    assert_eq!(fixed.len(), 4);
    assert_eq!(
        fixed.iter().map(|(_, seconds)| seconds).sum::<i64>(),
        49 * 60 * 60
    );
    assert_eq!(flexible.len(), 4);
    assert_eq!(flexible.iter().map(|(_, seconds)| seconds).sum::<i64>(), 97);
    assert!(fixed.windows(2).all(|days| days[0].0 < days[1].0));
    assert!(flexible.windows(2).all(|days| days[0].0 < days[1].0));
}

#[test]
fn 既に分割済みのsegmentも欠落重複なく集計できる() {
    let first_start = datetime(11, 5, 30, 0);
    let second_start = datetime(11, 6, 30, 0);
    let first = scheduled_capacity_seconds_by_logical_date(
        false,
        first_start,
        first_start + Duration::hours(1),
        60 * 60,
    )
    .unwrap();
    let second = scheduled_capacity_seconds_by_logical_date(
        false,
        second_start,
        second_start + Duration::minutes(45),
        45 * 60,
    )
    .unwrap();

    assert_eq!(
        first
            .iter()
            .chain(&second)
            .map(|(_, seconds)| seconds)
            .sum::<i64>(),
        105 * 60
    );
}

#[test]
fn 空windowとzero_workのflexibleは容量を持たない() {
    let start = datetime(11, 12, 0, 0);

    assert!(
        scheduled_capacity_seconds_by_logical_date(true, start, start, 60)
            .unwrap()
            .is_empty()
    );
    assert!(scheduled_capacity_seconds_by_logical_date(
        true,
        start,
        start - Duration::seconds(1),
        60,
    )
    .unwrap()
    .is_empty());
    assert!(scheduled_capacity_seconds_by_logical_date(
        false,
        start,
        start + Duration::hours(1),
        0,
    )
    .unwrap()
    .is_empty());
}

#[test]
fn 論理日を計算できない日時は既存errorを保持する() {
    let local_datetime = NaiveDate::MIN.and_hms_opt(5, 59, 0).unwrap();
    let start = DateTime::<Local>::from_naive_utc_and_offset(
        local_datetime,
        FixedOffset::east_opt(0).unwrap(),
    );

    assert_eq!(
        scheduled_capacity_seconds_by_logical_date(
            false,
            start,
            start + Duration::hours(1),
            60 * 60,
        ),
        Err(ApplicationError::LogicalDateOutOfRange {
            operation: "logical_date",
            datetime: start,
        })
    );
}
