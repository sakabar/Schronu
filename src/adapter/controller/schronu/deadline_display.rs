use crate::application::daily_capacity::{try_logical_date, try_logical_date_start};
use crate::application::task_use_case::ApplicationError;
use chrono::{DateTime, Local, NaiveDate};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DeadlineDisplayStatus {
    None,
    Overrun,
    DueWithinLogicalDate,
    Future,
}

pub(super) fn misses_deadline(
    deadline_time_opt: Option<&DateTime<Local>>,
    scheduled_end: DateTime<Local>,
) -> bool {
    deadline_time_opt.is_some_and(|deadline| *deadline < scheduled_end)
}

pub(super) fn classify_deadline_display(
    deadline_time_opt: Option<&DateTime<Local>>,
    scheduled_end: DateTime<Local>,
    scheduled_logical_date: NaiveDate,
) -> Result<DeadlineDisplayStatus, ApplicationError> {
    let Some(deadline) = deadline_time_opt else {
        return Ok(DeadlineDisplayStatus::None);
    };
    if misses_deadline(Some(deadline), scheduled_end) {
        return Ok(DeadlineDisplayStatus::Overrun);
    }

    let next_logical_date =
        scheduled_logical_date
            .succ_opt()
            .ok_or(ApplicationError::LogicalDateStartOutOfRange {
                date: scheduled_logical_date,
            })?;
    let next_logical_date_start = try_logical_date_start(next_logical_date)?;
    if *deadline < next_logical_date_start {
        Ok(DeadlineDisplayStatus::DueWithinLogicalDate)
    } else {
        Ok(DeadlineDisplayStatus::Future)
    }
}

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
    use super::{
        classify_deadline_display, format_deadline_remaining_time, misses_deadline,
        DeadlineDisplayStatus,
    };
    use crate::application::task_use_case::ApplicationError;
    use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDate, TimeZone};

    #[test]
    fn 予定終了が締切を過ぎる場合だけ締切超過とする() {
        let scheduled_end = Local.with_ymd_and_hms(2026, 9, 6, 18, 0, 0).unwrap();

        assert!(!misses_deadline(None, scheduled_end));
        assert!(!misses_deadline(
            Some(&(scheduled_end + Duration::minutes(1))),
            scheduled_end,
        ));
        assert!(!misses_deadline(Some(&scheduled_end), scheduled_end));
        assert!(misses_deadline(
            Some(&(scheduled_end - Duration::minutes(1))),
            scheduled_end,
        ));
    }

    #[test]
    fn 締切表示は予定logical_dateの06時境界で分類する() {
        let logical_date = NaiveDate::from_ymd_opt(2026, 9, 6).unwrap();
        let scheduled_end = Local.with_ymd_and_hms(2026, 9, 6, 18, 0, 0).unwrap();
        let next_logical_date_start = Local.with_ymd_and_hms(2026, 9, 7, 6, 0, 0).unwrap();

        for (deadline, expected) in [
            (None, DeadlineDisplayStatus::None),
            (
                Some(scheduled_end - Duration::seconds(1)),
                DeadlineDisplayStatus::Overrun,
            ),
            (
                Some(scheduled_end),
                DeadlineDisplayStatus::DueWithinLogicalDate,
            ),
            (
                Some(next_logical_date_start - Duration::seconds(1)),
                DeadlineDisplayStatus::DueWithinLogicalDate,
            ),
            (Some(next_logical_date_start), DeadlineDisplayStatus::Future),
        ] {
            assert_eq!(
                classify_deadline_display(deadline.as_ref(), scheduled_end, logical_date).unwrap(),
                expected
            );
        }
    }

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
