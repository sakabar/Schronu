use super::local_time::logical_date_with_offset;
use chrono::NaiveDate;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionTiming {
    pub elapsed_seconds: i64,
    pub remaining_at_start_seconds: i128,
    pub estimated_completion_epoch_ms: Option<i64>,
    pub worked_seconds: i128,
    pub progress_percent: Option<i128>,
    pub remaining_seconds: i128,
    pub normal_bar_percent: i128,
    pub overrun_bar_percent: i128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferTiming {
    pub snapshot_elapsed_seconds: i64,
    pub buffer_elapsed_seconds: i64,
    pub display_buffer_seconds: i128,
}

pub fn session_timing(
    started_at_epoch_ms: i64,
    estimated_work_seconds_at_start: i64,
    actual_work_seconds_at_start: i64,
    tick_now_epoch_ms: i64,
) -> SessionTiming {
    let elapsed_seconds = elapsed_seconds(started_at_epoch_ms, tick_now_epoch_ms);
    let estimated = i128::from(estimated_work_seconds_at_start);
    let actual = i128::from(actual_work_seconds_at_start);
    let elapsed = i128::from(elapsed_seconds);
    let remaining_at_start_seconds = schronu_shared::work_time::remaining_work_seconds(
        estimated_work_seconds_at_start,
        actual_work_seconds_at_start,
    );
    let worked_seconds = actual + elapsed;
    let progress_percent = (estimated > 0).then(|| worked_seconds * 100 / estimated);
    let remaining_seconds = remaining_at_start_seconds - elapsed;
    let (normal_bar_percent, overrun_bar_percent) = if remaining_at_start_seconds == 0 {
        (100, 50)
    } else {
        progress_percent.map_or((0, 0), |progress| {
            (progress.clamp(0, 100), (progress - 100).max(0))
        })
    };

    SessionTiming {
        elapsed_seconds,
        remaining_at_start_seconds,
        estimated_completion_epoch_ms: completion_epoch_ms(
            started_at_epoch_ms,
            remaining_at_start_seconds,
        ),
        worked_seconds,
        progress_percent,
        remaining_seconds,
        normal_bar_percent,
        overrun_bar_percent,
    }
}

pub fn buffer_timing(
    observed_at_epoch_ms: i64,
    buffer_seconds: i64,
    tick_now_epoch_ms: i64,
    active_session_started_at_epoch_ms: &[i64],
) -> BufferTiming {
    let sessions: Vec<_> = active_session_started_at_epoch_ms
        .iter()
        .copied()
        .map(|started_at| (started_at, None))
        .collect();
    buffer_timing_with_sessions(
        observed_at_epoch_ms,
        buffer_seconds,
        tick_now_epoch_ms,
        &sessions,
    )
}

pub fn buffer_timing_with_sessions(
    observed_at_epoch_ms: i64,
    buffer_seconds: i64,
    tick_now_epoch_ms: i64,
    sessions: &[(i64, Option<i64>)],
) -> BufferTiming {
    let snapshot_elapsed_seconds = elapsed_seconds(observed_at_epoch_ms, tick_now_epoch_ms);
    let credit_until_epoch_ms = tick_now_epoch_ms.max(observed_at_epoch_ms);
    let session_credit_seconds = sessions
        .iter()
        .map(|(started_at, ended_at)| {
            let end = ended_at
                .unwrap_or(credit_until_epoch_ms)
                .min(credit_until_epoch_ms);
            i128::from(elapsed_seconds(*started_at, end))
        })
        .sum::<i128>();
    let buffer_elapsed_seconds = snapshot_elapsed_seconds;
    BufferTiming {
        snapshot_elapsed_seconds,
        buffer_elapsed_seconds,
        display_buffer_seconds: i128::from(buffer_seconds) - i128::from(buffer_elapsed_seconds)
            + session_credit_seconds,
    }
}

pub fn format_mm_ss(seconds: i128) -> String {
    let magnitude = seconds.unsigned_abs();
    format!("{:02}:{:02}", magnitude / 60, magnitude % 60)
}

pub fn format_hh_mm_ss(seconds: i128) -> String {
    let magnitude = seconds.unsigned_abs();
    let sign = if seconds < 0 { "-" } else { "" };
    format!(
        "{sign}{:02}:{:02}:{:02}",
        magnitude / 3_600,
        magnitude % 3_600 / 60,
        magnitude % 60
    )
}

fn elapsed_seconds(start_epoch_ms: i64, now_epoch_ms: i64) -> i64 {
    let elapsed_ms = i128::from(now_epoch_ms) - i128::from(start_epoch_ms);
    if elapsed_ms <= 0 {
        return 0;
    }

    i64::try_from(elapsed_ms / 1_000)
        .expect("the difference between two i64 millisecond epochs fits in i64 seconds")
}

fn completion_epoch_ms(started_at_epoch_ms: i64, remaining_seconds: i128) -> Option<i64> {
    let remaining_ms = remaining_seconds.checked_mul(1_000)?;
    let completion = i128::from(started_at_epoch_ms).checked_add(remaining_ms)?;
    i64::try_from(completion).ok()
}

// Both the card label and buffer credit use the saved session metadata.
pub(super) fn session_plan_dates(
    start_time_epoch_ms: Option<i64>,
    pending_until_epoch_ms: Option<i64>,
    offset_at: &impl Fn(i64) -> Option<i32>,
) -> [Option<NaiveDate>; 2] {
    [start_time_epoch_ms, pending_until_epoch_ms]
        .map(|epoch| epoch.and_then(|epoch| logical_date_with_offset(epoch, offset_at)))
}

pub(super) fn session_credit_start_epoch_ms(
    started_at_epoch_ms: i64,
    start_time_epoch_ms: Option<i64>,
    pending_until_epoch_ms: Option<i64>,
    now_epoch_ms: i64,
    offset_at: &impl Fn(i64) -> Option<i32>,
) -> i64 {
    let eligible_start = || -> Option<i64> {
        logical_date_with_offset(now_epoch_ms, offset_at)?;
        let started_date = logical_date_with_offset(started_at_epoch_ms, offset_at)?;
        let dates = session_plan_dates(start_time_epoch_ms, pending_until_epoch_ms, offset_at);
        let (eligible_date, upper) = dates
            .into_iter()
            .zip([start_time_epoch_ms, pending_until_epoch_ms])
            .filter_map(|(date, epoch)| Some((date?, epoch?)))
            .max_by_key(|(date, _)| *date)?;
        if eligible_date <= started_date {
            return Some(started_at_epoch_ms);
        }
        // Find the logical day boundary using the offset at each instant, including DST.
        // The metadata instant belongs to the eligible day and bounds the search.
        let mut lower = started_at_epoch_ms;
        let mut upper = upper;
        while i128::from(upper) - i128::from(lower) > 1 {
            let middle = (i128::from(lower) + (i128::from(upper) - i128::from(lower)) / 2) as i64;
            if logical_date_with_offset(middle, offset_at)? < eligible_date {
                lower = middle;
            } else {
                upper = middle;
            }
        }
        Some(upper)
    };
    eligible_start().unwrap_or(started_at_epoch_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    fn epoch(text: &str) -> i64 {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn eligibility_boundary_uses_the_offset_at_each_instant_across_dst() {
        for (started, metadata, transition, before, after, expected) in [
            (
                "2026-03-07T12:00:00Z",
                "2026-03-08T12:00:00Z",
                "2026-03-08T07:00:00Z",
                -300,
                -240,
                "2026-03-08T10:00:00Z",
            ),
            (
                "2026-10-31T12:00:00Z",
                "2026-11-01T12:00:00Z",
                "2026-11-01T06:00:00Z",
                -240,
                -300,
                "2026-11-01T11:00:00Z",
            ),
        ] {
            let offset_at = |instant| {
                Some(if instant < epoch(transition) {
                    before
                } else {
                    after
                })
            };
            assert_eq!(
                session_credit_start_epoch_ms(
                    epoch(started),
                    Some(epoch(metadata)),
                    None,
                    epoch(metadata),
                    &offset_at,
                ),
                epoch(expected)
            );
        }
    }
}
