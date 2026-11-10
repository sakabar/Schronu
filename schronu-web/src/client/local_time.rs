use chrono::{DateTime, FixedOffset, NaiveDate, Timelike, Utc};

pub(super) fn local_datetime(
    epoch_ms: i64,
    utc_offset_minutes: i32,
) -> Option<DateTime<FixedOffset>> {
    let offset_seconds = utc_offset_minutes.checked_mul(60)?;
    let offset = FixedOffset::east_opt(offset_seconds)?;
    Some(DateTime::<Utc>::from_timestamp_millis(epoch_ms)?.with_timezone(&offset))
}

pub(super) fn logical_date_with_offset(
    epoch_ms: i64,
    offset_at: &impl Fn(i64) -> Option<i32>,
) -> Option<NaiveDate> {
    let datetime = local_datetime(epoch_ms, offset_at(epoch_ms)?)?;
    let date = datetime.date_naive();
    if datetime.hour() < 6 {
        date.pred_opt()
    } else {
        Some(date)
    }
}
