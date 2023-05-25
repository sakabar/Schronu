use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Timelike};

pub fn get_next_morning_datetime(now: DateTime<Local>) -> DateTime<Local> {
    if now.hour() >= 6 {
        // 翌日の午前6時
        let dt = now + Duration::days(1);
        let datetime_str = format!("{}/{}/{} 06:00", dt.year(), dt.month(), dt.day());
        Local
            .datetime_from_str(&datetime_str, "%Y/%m/%d %H:%M")
            .unwrap()
    } else {
        // 今日の午前6時
        let datetime_str = format!("{}/{}/{} 06:00", now.year(), now.month(), now.day());
        Local
            .datetime_from_str(&datetime_str, "%Y/%m/%d %H:%M")
            .unwrap()
    }
}

#[test]
fn test_get_next_morning_datetime_6時以降の場合() {
    let dt = Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap();
    let actual = get_next_morning_datetime(dt);

    assert_eq!(actual, Local.with_ymd_and_hms(2023, 4, 2, 6, 0, 0).unwrap());
}

#[test]
fn test_get_next_morning_datetime_6時以前の場合() {
    let dt = Local.with_ymd_and_hms(2023, 4, 1, 1, 0, 0).unwrap();
    let actual = get_next_morning_datetime(dt);

    assert_eq!(actual, Local.with_ymd_and_hms(2023, 4, 1, 6, 0, 0).unwrap());
}
