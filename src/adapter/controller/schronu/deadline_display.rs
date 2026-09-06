use crate::application::daily_capacity::try_logical_date;
use crate::application::task_use_case::ApplicationError;
use chrono::{DateTime, Local};

pub(super) fn format_deadline_remaining_time(
    deadline_time_opt: Option<&DateTime<Local>>,
    end_datetime: DateTime<Local>,
    last_synced_time: DateTime<Local>,
) -> Result<String, ApplicationError> {
    let Some(deadline_time) = deadline_time_opt else {
        return Ok("____/__/__".to_string());
    };
    let difference_minutes = (end_datetime - deadline_time).num_minutes().abs();
    let difference_hours = difference_minutes / 60;
    let remaining_minutes = difference_minutes % 60;

    if *deadline_time < last_synced_time {
        return Ok(format!(
            "+{:02}:{:02}ASAP",
            difference_hours, remaining_minutes
        ));
    }

    let deadline_logical_date = try_logical_date(*deadline_time)?;
    let end_logical_date = try_logical_date(end_datetime)?;
    let logical_date_difference = (deadline_logical_date - end_logical_date).num_days();

    if logical_date_difference > 0 {
        Ok(format!("_____-{:03}D", logical_date_difference))
    } else if logical_date_difference < 0 {
        Ok(format!("_____+{:03}D", logical_date_difference.abs()))
    } else if *deadline_time < end_datetime {
        Ok(format!(
            "+{:02}:{:02}____",
            difference_hours, remaining_minutes
        ))
    } else {
        Ok(format!(
            "____-{:02}:{:02}",
            difference_hours, remaining_minutes
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::format_deadline_remaining_time;
    use crate::application::task_use_case::ApplicationError;
    use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDate, TimeZone};

    #[test]
    fn 現行の締切なしと同一logical_dateとasapの表示を維持する() {
        let last_synced_time = Local.with_ymd_and_hms(2026, 9, 6, 10, 0, 0).unwrap();
        let end_datetime = Local.with_ymd_and_hms(2026, 9, 6, 18, 0, 0).unwrap();

        for (deadline_time_opt, expected) in [
            (None, "____/__/__"),
            (
                Some(Local.with_ymd_and_hms(2026, 9, 6, 9, 0, 0).unwrap()),
                "+09:00ASAP",
            ),
            (
                Some(Local.with_ymd_and_hms(2026, 9, 6, 12, 0, 0).unwrap()),
                "+06:00____",
            ),
            (
                Some(Local.with_ymd_and_hms(2026, 9, 6, 18, 0, 0).unwrap()),
                "____-00:00",
            ),
            (
                Some(Local.with_ymd_and_hms(2026, 9, 6, 20, 0, 59).unwrap()),
                "____-02:00",
            ),
        ] {
            assert_eq!(
                format_deadline_remaining_time(
                    deadline_time_opt.as_ref(),
                    end_datetime,
                    last_synced_time,
                )
                .unwrap(),
                expected
            );
        }
    }

    #[test]
    fn 締切差を06時境界のlogical_date単位で表示する() {
        let last_synced_time = Local.with_ymd_and_hms(2026, 9, 6, 10, 0, 0).unwrap();

        for (end_datetime, deadline_time, expected) in [
            (
                Local.with_ymd_and_hms(2026, 9, 6, 18, 0, 0).unwrap(),
                Local.with_ymd_and_hms(2026, 9, 7, 20, 0, 0).unwrap(),
                "_____-001D",
            ),
            (
                Local.with_ymd_and_hms(2026, 9, 6, 18, 0, 0).unwrap(),
                Local.with_ymd_and_hms(2026, 9, 8, 18, 0, 0).unwrap(),
                "_____-002D",
            ),
            (
                Local.with_ymd_and_hms(2026, 9, 7, 6, 1, 0).unwrap(),
                Local.with_ymd_and_hms(2026, 9, 7, 6, 2, 0).unwrap(),
                "____-00:01",
            ),
            (
                Local.with_ymd_and_hms(2026, 9, 7, 5, 59, 0).unwrap(),
                Local.with_ymd_and_hms(2026, 9, 7, 6, 0, 0).unwrap(),
                "_____-001D",
            ),
            (
                Local.with_ymd_and_hms(2026, 9, 7, 6, 0, 0).unwrap(),
                Local.with_ymd_and_hms(2026, 9, 7, 5, 59, 0).unwrap(),
                "_____+001D",
            ),
            (
                Local.with_ymd_and_hms(2026, 9, 7, 5, 0, 0).unwrap(),
                Local.with_ymd_and_hms(2026, 9, 7, 5, 30, 0).unwrap(),
                "____-00:30",
            ),
        ] {
            assert_eq!(
                format_deadline_remaining_time(
                    Some(&deadline_time),
                    end_datetime,
                    last_synced_time,
                )
                .unwrap(),
                expected
            );
        }

        let past_last_synced_time = Local.with_ymd_and_hms(2026, 9, 8, 10, 0, 0).unwrap();
        let overdue_deadline = Local.with_ymd_and_hms(2026, 9, 7, 9, 0, 0).unwrap();
        let overdue_end = Local.with_ymd_and_hms(2026, 9, 9, 11, 0, 0).unwrap();
        assert_eq!(
            format_deadline_remaining_time(
                Some(&overdue_deadline),
                overdue_end,
                past_last_synced_time,
            )
            .unwrap(),
            "+50:00ASAP"
        );
    }

    #[test]
    fn logical_date差が1000日以上でも日数を省略しない() {
        let last_synced_time = Local.with_ymd_and_hms(2026, 9, 6, 10, 0, 0).unwrap();
        let end_datetime = Local.with_ymd_and_hms(2026, 9, 6, 18, 0, 0).unwrap();
        let deadline_time = end_datetime + Duration::days(1000);

        assert_eq!(
            format_deadline_remaining_time(Some(&deadline_time), end_datetime, last_synced_time,)
                .unwrap(),
            "_____-1000D"
        );
    }

    #[test]
    fn logical_date計算不能は情報を保持して返す() {
        let offset = FixedOffset::east_opt(0).unwrap();
        let end_datetime = DateTime::<Local>::from_naive_utc_and_offset(
            NaiveDate::MIN.and_hms_opt(5, 0, 0).unwrap(),
            offset,
        );
        let deadline_time = DateTime::<Local>::from_naive_utc_and_offset(
            NaiveDate::MIN.and_hms_opt(6, 0, 0).unwrap(),
            offset,
        );

        assert_eq!(
            format_deadline_remaining_time(Some(&deadline_time), end_datetime, deadline_time,),
            Err(ApplicationError::LogicalDateOutOfRange {
                operation: "logical_date",
                datetime: end_datetime,
            })
        );
    }
}
