use super::interface::FreeTimeManagerTrait;
use super::task_use_case::{resolve_local_datetime, ApplicationError};
use crate::entity::datetime::{BusinessDateTimePolicy, DEFAULT_END_OF_DAY_OFFSET_MINUTES};
use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone, Timelike};

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
) -> Result<i64, ApplicationError> {
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
) -> Result<i64, ApplicationError> {
    let local_datetime_base = try_subjective_date_start(*date)?;
    let current_subjective_date = try_subjective_date(last_synced_time)?;
    let eod = try_subjective_date_end(current_subjective_date, end_of_day_offset_minutes)?;
    let next_boundary_for_base = try_next_business_day_start(local_datetime_base)?;
    let next_boundary_for_last_synced_time = try_next_business_day_start(last_synced_time)?;

    if local_datetime_base < last_synced_time && last_synced_time < next_boundary_for_base {
        if last_synced_time.hour() < next_boundary_for_last_synced_time.hour() {
            if last_synced_time < eod {
                Ok((eod - last_synced_time).num_minutes())
            } else {
                Ok(0)
            }
        } else {
            Ok(free_time_manager.get_free_minutes(&last_synced_time, &eod))
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
) -> Result<i64, ApplicationError> {
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
) -> Result<i64, ApplicationError> {
    let start = try_subjective_date_start(*date)?;
    let end = try_subjective_date_end(*date, end_of_day_offset_minutes)?;
    Ok(free_time_manager.get_free_minutes(&start, &end))
}

pub fn try_subjective_date(datetime: DateTime<Local>) -> Result<NaiveDate, ApplicationError> {
    BusinessDateTimePolicy::new(END_OF_DAY_OFFSET_MINUTES)
        .subjective_date(datetime)
        .ok_or(ApplicationError::SubjectiveDateOutOfRange {
            operation: "subjective_date",
            datetime,
        })
}

pub fn try_local_date_and_time(
    date: NaiveDate,
    time: NaiveTime,
) -> Result<DateTime<Local>, ApplicationError> {
    let local_datetime = date.and_time(time);
    resolve_local_datetime(local_datetime, Local.from_local_datetime(&local_datetime))
}

pub fn try_next_business_day_start(
    datetime: DateTime<Local>,
) -> Result<DateTime<Local>, ApplicationError> {
    let policy = BusinessDateTimePolicy::new(END_OF_DAY_OFFSET_MINUTES);
    let naive = policy.next_business_day_start_naive(datetime).ok_or(
        ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime,
        },
    )?;
    resolve_local_datetime(naive, policy.next_business_day_start(datetime))
}

pub fn try_subjective_date_start(date: NaiveDate) -> Result<DateTime<Local>, ApplicationError> {
    let policy = BusinessDateTimePolicy::new(END_OF_DAY_OFFSET_MINUTES);
    let naive = policy
        .subjective_date_start_naive(date)
        .ok_or(ApplicationError::SubjectiveDateStartOutOfRange { date })?;
    resolve_local_datetime(naive, policy.subjective_date_start(date))
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

#[cfg(test)]
#[path = "daily_capacity_tests.rs"]
mod tests;
