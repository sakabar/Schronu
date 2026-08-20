use super::interface::FreeTimeManagerTrait;
use super::task_use_case::{resolve_local_datetime, ApplicationError};
use crate::entity::datetime::{
    get_next_morning_datetime, BusinessDateTimePolicy, DEFAULT_END_OF_DAY_OFFSET_MINUTES,
};
use chrono::{DateTime, Duration, Local, NaiveDate, Timelike};

pub const RHO_GOAL: f64 = 0.7;
pub const END_OF_DAY_OFFSET_MINUTES: i64 = DEFAULT_END_OF_DAY_OFFSET_MINUTES;

pub fn calculate_daily_leeway_seconds(
    free_time_minutes: i64,
    repetitive_work_seconds: i64,
    total_work_seconds: i64,
) -> i64 {
    (-calculate_daily_rho_diff_hours(
        free_time_minutes,
        repetitive_work_seconds,
        total_work_seconds,
    ) * 3600.0)
        .floor()
        .max(0.0) as i64
}
pub fn calculate_daily_rho_diff_hours(
    free_time_minutes: i64,
    repetitive_work_seconds: i64,
    total_work_seconds: i64,
) -> f64 {
    let non_repetitive_free_seconds = free_time_minutes * 60 - repetitive_work_seconds;
    let non_repetitive_work_seconds = total_work_seconds - repetitive_work_seconds;

    if non_repetitive_free_seconds <= 0 {
        return 0.0;
    }

    (non_repetitive_work_seconds as f64 - non_repetitive_free_seconds as f64 * RHO_GOAL) / 3600.0
}

pub fn calculate_free_time_minutes_for_subjective_date(
    date: &NaiveDate,
    last_synced_time: DateTime<Local>,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> i64 {
    calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
        date,
        last_synced_time,
        free_time_manager,
        END_OF_DAY_OFFSET_MINUTES,
    )
}

pub fn calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
    date: &NaiveDate,
    last_synced_time: DateTime<Local>,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    end_of_day_offset_minutes: i64,
) -> i64 {
    let local_datetime_base = subjective_date_start(*date);
    let eod = get_next_morning_datetime(last_synced_time)
        .with_hour(0)
        .expect("invalid hour")
        .with_minute(0)
        .expect("invalid minute")
        + Duration::minutes(end_of_day_offset_minutes);

    if local_datetime_base < last_synced_time
        && last_synced_time < get_next_morning_datetime(local_datetime_base)
    {
        if last_synced_time.hour() < get_next_morning_datetime(last_synced_time).hour() {
            if last_synced_time < eod {
                (eod - last_synced_time).num_minutes()
            } else {
                0
            }
        } else {
            free_time_manager.get_free_minutes(&last_synced_time, &eod)
        }
    } else {
        calculate_full_day_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
            date,
            free_time_manager,
            end_of_day_offset_minutes,
        )
    }
}

pub fn calculate_full_day_free_time_minutes_for_subjective_date(
    date: &NaiveDate,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> i64 {
    calculate_full_day_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
        date,
        free_time_manager,
        END_OF_DAY_OFFSET_MINUTES,
    )
}

pub fn calculate_full_day_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
    date: &NaiveDate,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    end_of_day_offset_minutes: i64,
) -> i64 {
    let start = subjective_date_start(*date);
    let end = subjective_date_end(*date, end_of_day_offset_minutes);
    free_time_manager.get_free_minutes(&start, &end)
}

pub fn try_subjective_date(datetime: DateTime<Local>) -> Result<NaiveDate, ApplicationError> {
    BusinessDateTimePolicy::new(END_OF_DAY_OFFSET_MINUTES)
        .subjective_date(datetime)
        .ok_or(ApplicationError::SubjectiveDateOutOfRange {
            operation: "subjective_date",
            datetime,
        })
}

pub fn subjective_date(datetime: DateTime<Local>) -> NaiveDate {
    try_subjective_date(datetime).unwrap_or_else(|error| panic!("{error}"))
}

pub fn try_subjective_date_start(date: NaiveDate) -> Result<DateTime<Local>, ApplicationError> {
    let policy = BusinessDateTimePolicy::new(END_OF_DAY_OFFSET_MINUTES);
    let naive = policy
        .subjective_date_start_naive(date)
        .ok_or(ApplicationError::SubjectiveDateStartOutOfRange { date })?;
    resolve_local_datetime(naive, policy.subjective_date_start(date))
}

pub fn subjective_date_start(date: NaiveDate) -> DateTime<Local> {
    try_subjective_date_start(date).unwrap_or_else(|error| panic!("{error}"))
}

pub fn try_subjective_date_end(
    date: NaiveDate,
    end_of_day_offset_minutes: i64,
) -> Result<DateTime<Local>, ApplicationError> {
    let policy = BusinessDateTimePolicy::new(end_of_day_offset_minutes);
    let naive = policy.subjective_date_end_naive(date).ok_or(
        ApplicationError::SubjectiveDateEndOutOfRange {
            date,
            end_of_day_offset_minutes,
        },
    )?;
    resolve_local_datetime(naive, policy.subjective_date_end(date))
}

pub fn subjective_date_end(date: NaiveDate, end_of_day_offset_minutes: i64) -> DateTime<Local> {
    try_subjective_date_end(date, end_of_day_offset_minutes)
        .unwrap_or_else(|error| panic!("{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::task_use_case::{resolve_local_datetime, ApplicationError};
    use chrono::{FixedOffset, LocalResult, NaiveDateTime, TimeZone};

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
            subjective_date(datetime),
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
        let expected = subjective_date_start(date);

        assert_eq!(try_subjective_date_start(date), Ok(expected));
    }

    #[test]
    fn try_subjective_date_endは通常値をpolicyと同じ日時へ変換する() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();
        let expected = subjective_date_end(date, END_OF_DAY_OFFSET_MINUTES);

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

        assert_eq!(actual, 16 * 60);
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
}
