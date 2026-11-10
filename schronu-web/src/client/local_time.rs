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

#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub(super) fn local_utc_offset_minutes(epoch_ms: i64) -> Option<i32> {
    DateTime::<Utc>::from_timestamp_millis(epoch_ms)?;
    let date = js_sys::Date::new_0();
    date.set_time(epoch_ms as f64);
    let utc_minus_local = date.get_timezone_offset();
    if !utc_minus_local.is_finite()
        || utc_minus_local.fract() != 0.0
        || utc_minus_local < f64::from(i32::MIN)
        || utc_minus_local > f64::from(i32::MAX)
    {
        return None;
    }
    (-(utc_minus_local as i64)).try_into().ok()
}

#[cfg(not(all(feature = "web", target_arch = "wasm32")))]
pub(super) fn local_utc_offset_minutes(epoch_ms: i64) -> Option<i32> {
    let utc = DateTime::<Utc>::from_timestamp_millis(epoch_ms)?;
    Some(utc.with_timezone(&chrono::Local).offset().local_minus_utc() / 60)
}
