use super::*;
use crate::application::task_use_case::{resolve_local_datetime, ApplicationError};
use chrono::{Duration, FixedOffset, LocalResult, NaiveDateTime, TimeZone};

struct DurationFreeTimeManager;

impl FreeTimeManagerTrait for DurationFreeTimeManager {
    fn get_free_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
        (*end - *start).num_minutes()
    }

    fn get_busy_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        0
    }

    fn register_busy_time_slot(
        &mut self,
        _start: &DateTime<Local>,
        _end: &DateTime<Local>,
    ) -> Result<(), super::super::interface::BusyTimeSlotRegistrationError> {
        Ok(())
    }

    fn load_busy_time_slots_from_file(
        &mut self,
        _busy_time_slots_file_path: &str,
    ) -> Result<(), super::super::interface::BusyTimeSlotLoadError> {
        Ok(())
    }
}

#[test]
fn calculate_daily_leeway_seconds_反復時間を分子と分母から除いてrho07までを返す() {
    let actual = calculate_daily_leeway_seconds(10 * 60, 2 * 3600, 6 * 3600);

    assert_eq!(actual, 96 * 60);
}

#[test]
fn calculate_daily_rho_diff_hours_反復時間を分子と分母から除いて計算する() {
    let actual = calculate_daily_rho_diff_hours(10 * 60, 2 * 3600, 6 * 3600);

    assert_eq!(actual, -1.6);
}

#[test]
fn subjective_date_06時より前は前日として扱う() {
    let datetime = Local.with_ymd_and_hms(2026, 8, 12, 1, 0, 0).unwrap();

    assert_eq!(
        try_subjective_date(datetime).unwrap(),
        NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()
    );
}

#[test]
fn try_subjective_dateは通常値をpolicyと同じ日付へ変換する() {
    let datetime = Local.with_ymd_and_hms(2026, 8, 12, 5, 59, 0).unwrap();
    let expected = BusinessDateTimePolicy::new(END_OF_DAY_OFFSET_MINUTES)
        .subjective_date(datetime)
        .unwrap();

    assert_eq!(try_subjective_date(datetime), Ok(expected));
}

#[test]
fn try_subjective_dateは日付減算不能の操作と入力日時を保持する() {
    let local_datetime = NaiveDate::MIN.and_hms_opt(5, 59, 0).unwrap();
    let datetime = DateTime::<Local>::from_naive_utc_and_offset(
        local_datetime,
        FixedOffset::east_opt(0).unwrap(),
    );

    assert_eq!(
        try_subjective_date(datetime),
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "subjective_date",
            datetime,
        })
    );
}

#[test]
fn try_subjective_date_startは通常値をpolicyと同じ日時へ変換する() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();
    let expected = try_subjective_date_start(date).unwrap();

    assert_eq!(try_subjective_date_start(date), Ok(expected));
}

#[test]
fn try_subjective_date_endは通常値をpolicyと同じ日時へ変換する() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();
    let expected = try_subjective_date_end(date, END_OF_DAY_OFFSET_MINUTES).unwrap();

    assert_eq!(
        try_subjective_date_end(date, END_OF_DAY_OFFSET_MINUTES),
        Ok(expected)
    );
}

#[test]
fn try_subjective_date_endは最大日の終端計算不能をdateとoffset付きで返す() {
    assert_eq!(
        try_subjective_date_end(NaiveDate::MAX, END_OF_DAY_OFFSET_MINUTES),
        Err(ApplicationError::SubjectiveDateEndOutOfRange {
            date: NaiveDate::MAX,
            end_of_day_offset_minutes: END_OF_DAY_OFFSET_MINUTES,
        })
    );
}

#[test]
fn try_subjective_date_endは極端なoffsetによる計算不能をdateとoffset付きで返す() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();

    assert_eq!(
        try_subjective_date_end(date, i64::MAX),
        Err(ApplicationError::SubjectiveDateEndOutOfRange {
            date,
            end_of_day_offset_minutes: i64::MAX,
        })
    );
}

#[test]
fn end_of_day_offset_minutesがマイナスなら22時を日次終端にする() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();
    let mut free_time_manager = DurationFreeTimeManager;

    let actual =
        calculate_full_day_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
            &date,
            &mut free_time_manager,
            -120,
        );

    assert_eq!(actual, Ok(16 * 60));
}

#[test]
fn full_day_free_timeは最大日の終端計算不能をdateとoffset付きで返す() {
    let mut free_time_manager = DurationFreeTimeManager;

    let actual =
        calculate_full_day_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
            &NaiveDate::MAX,
            &mut free_time_manager,
            END_OF_DAY_OFFSET_MINUTES,
        );

    assert_eq!(
        actual,
        Err(ApplicationError::SubjectiveDateEndOutOfRange {
            date: NaiveDate::MAX,
            end_of_day_offset_minutes: END_OF_DAY_OFFSET_MINUTES,
        })
    );
}

#[test]
fn free_timeは極端なoffsetの終端計算不能をdateとoffset付きで返す() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();
    let last_synced_time = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let mut free_time_manager = DurationFreeTimeManager;

    let actual = calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
        &date,
        last_synced_time,
        &mut free_time_manager,
        i64::MAX,
    );

    assert_eq!(
        actual,
        Err(ApplicationError::SubjectiveDateEndOutOfRange {
            date,
            end_of_day_offset_minutes: i64::MAX,
        })
    );
}

#[test]
fn local_datetime変換はsingleを採用する() {
    let naive = NaiveDate::from_ymd_opt(2026, 8, 12)
        .unwrap()
        .and_hms_opt(6, 0, 0)
        .unwrap();
    let datetime = Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap();

    assert_eq!(
        resolve_local_datetime(naive, LocalResult::Single(datetime)),
        Ok(datetime)
    );
}

#[test]
fn local_datetime変換はambiguousの日時と2候補を保持する() {
    let naive = NaiveDate::from_ymd_opt(2026, 10, 25)
        .unwrap()
        .and_hms_opt(1, 30, 0)
        .unwrap();
    let earlier = Local.with_ymd_and_hms(2026, 10, 25, 1, 30, 0).unwrap();
    let later = earlier + Duration::hours(1);

    assert_eq!(
        resolve_local_datetime(naive, LocalResult::Ambiguous(earlier, later)),
        Err(ApplicationError::AmbiguousLocalDateTime {
            local_datetime: naive,
            earlier,
            later,
        })
    );
}

#[test]
fn local_datetime変換はnoneの日時を保持する() {
    let naive: NaiveDateTime = NaiveDate::from_ymd_opt(2026, 3, 29)
        .unwrap()
        .and_hms_opt(2, 30, 0)
        .unwrap();

    assert_eq!(
        resolve_local_datetime(naive, LocalResult::None),
        Err(ApplicationError::NonexistentLocalDateTime {
            local_datetime: naive,
        })
    );
}
