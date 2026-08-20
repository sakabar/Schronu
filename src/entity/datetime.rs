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

#[cfg(test)]
mod business_datetime_policy_contract_tests {
    use super::*;

    fn local_datetime(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap()
    }

    fn local_result_single(result: LocalResult<DateTime<Local>>) -> DateTime<Local> {
        match result {
            LocalResult::Single(datetime) => datetime,
            other => panic!("expected a single local datetime, got {other:?}"),
        }
    }

    #[test]
    fn subjective_dateは06時境界で切り替わる() {
        let policy = BusinessDateTimePolicy::new(30);

        assert_eq!(
            policy.subjective_date(local_datetime(2026, 8, 12, 5, 59)),
            chrono::NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()
        );
        assert_eq!(
            policy.subjective_date(local_datetime(2026, 8, 12, 6, 0)),
            chrono::NaiveDate::from_ymd_opt(2026, 8, 12).unwrap()
        );
        assert_eq!(
            policy.subjective_date(local_datetime(2026, 8, 12, 6, 1)),
            chrono::NaiveDate::from_ymd_opt(2026, 8, 12).unwrap()
        );
    }

    #[test]
    fn next_business_day_startは現在時刻より後の06時境界を返す() {
        let policy = BusinessDateTimePolicy::new(30);

        assert_eq!(
            local_result_single(policy.next_business_day_start(local_datetime(2026, 8, 12, 5, 59))),
            local_datetime(2026, 8, 12, 6, 0)
        );
        assert_eq!(
            local_result_single(policy.next_business_day_start(local_datetime(2026, 8, 12, 6, 0))),
            local_datetime(2026, 8, 13, 6, 0)
        );
        assert_eq!(
            local_result_single(policy.next_business_day_start(local_datetime(2026, 8, 12, 6, 1))),
            local_datetime(2026, 8, 13, 6, 0)
        );
    }

    #[test]
    fn subjective_date_startは対象日の06時を返す() {
        let policy = BusinessDateTimePolicy::new(30);
        let date = chrono::NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();

        assert_eq!(
            local_result_single(policy.subjective_date_start(date)),
            local_datetime(2026, 8, 12, 6, 0)
        );
    }

    #[test]
    fn subjective_date_endは翌日00時へ正負のoffsetを適用する() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();

        assert_eq!(
            local_result_single(BusinessDateTimePolicy::new(30).subjective_date_end(date)),
            local_datetime(2026, 8, 13, 0, 30)
        );
        assert_eq!(
            local_result_single(BusinessDateTimePolicy::new(120).subjective_date_end(date)),
            local_datetime(2026, 8, 13, 2, 0)
        );
        assert_eq!(
            local_result_single(BusinessDateTimePolicy::new(-120).subjective_date_end(date)),
            local_datetime(2026, 8, 12, 22, 0)
        );
    }

    #[test]
    fn subjective_date_endは月と年を跨ぐ() {
        let policy = BusinessDateTimePolicy::new(30);
        let date = chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();

        assert_eq!(
            local_result_single(policy.subjective_date_end(date)),
            local_datetime(2027, 1, 1, 0, 30)
        );
    }
}
