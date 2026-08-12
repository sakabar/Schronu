use super::interface::FreeTimeManagerTrait;
use crate::entity::datetime::get_next_morning_datetime;
use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Timelike};

pub const RHO_GOAL: f64 = 0.7;
pub const END_OF_DAY_DURATION: Duration = Duration::minutes(30);

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
    let local_datetime_base = subjective_date_start(*date);
    let eod = get_next_morning_datetime(last_synced_time)
        .with_hour(0)
        .expect("invalid hour")
        .with_minute(0)
        .expect("invalid minute")
        + END_OF_DAY_DURATION;

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
        calculate_full_day_free_time_minutes_for_subjective_date(date, free_time_manager)
    }
}

pub fn calculate_full_day_free_time_minutes_for_subjective_date(
    date: &NaiveDate,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> i64 {
    let start = subjective_date_start(*date);
    let local_tz = Local::now().timezone();
    let end = local_tz
        .from_local_datetime(&date.and_hms_opt(23, 59, 59).unwrap())
        .unwrap()
        + END_OF_DAY_DURATION;
    free_time_manager.get_free_minutes(&start, &end)
}

pub fn subjective_date(datetime: DateTime<Local>) -> NaiveDate {
    (get_next_morning_datetime(datetime) - Duration::days(1)).date_naive()
}

pub fn subjective_date_start(date: NaiveDate) -> DateTime<Local> {
    get_next_morning_datetime(
        Local::now()
            .timezone()
            .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
            .unwrap(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculate_daily_leeway_seconds_反復時間を分子と分母から除いてrho07までを返す() {
        let actual = calculate_daily_leeway_seconds(10 * 60, 2 * 3600, 6 * 3600);

        assert_eq!(actual, 96 * 60);
    }

    #[test]
    fn subjective_date_06時より前は前日として扱う() {
        let datetime = Local.with_ymd_and_hms(2026, 8, 12, 1, 0, 0).unwrap();

        assert_eq!(
            subjective_date(datetime),
            NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()
        );
    }
}
