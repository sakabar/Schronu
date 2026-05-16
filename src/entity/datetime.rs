use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDateTime, ParseError, TimeZone, Timelike,
};

pub fn parse_local_datetime(
    datetime_str: &str,
    format: &str,
) -> Result<LocalResult<DateTime<Local>>, ParseError> {
    NaiveDateTime::parse_from_str(datetime_str, format)
        .map(|datetime| datetime.and_local_timezone(Local))
}

pub fn get_next_morning_datetime(now: DateTime<Local>) -> DateTime<Local> {
    if now.hour() >= 6 {
        // 翌日の午前6時
        let dt = now + Duration::days(1);
        Local
            .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 6, 0, 0)
            .unwrap()
    } else {
        // 今日の午前6時
        Local
            .with_ymd_and_hms(now.year(), now.month(), now.day(), 6, 0, 0)
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
