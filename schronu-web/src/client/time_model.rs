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
    let remaining_at_start_seconds = (estimated - actual).max(0);
    let worked_seconds = actual + elapsed;
    let progress_percent = (estimated > 0).then(|| worked_seconds * 100 / estimated);
    let remaining_seconds = remaining_at_start_seconds - elapsed;
    let (normal_bar_percent, overrun_bar_percent) = progress_percent.map_or((0, 0), |progress| {
        (progress.clamp(0, 100), (progress - 100).max(0))
    });

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
    let window_start = i128::from(observed_at_epoch_ms);
    let window_end = i128::from(tick_now_epoch_ms.max(observed_at_epoch_ms));
    let active_milliseconds =
        session_interval_milliseconds(observed_at_epoch_ms, tick_now_epoch_ms, sessions);
    let idle_milliseconds = window_end - window_start - active_milliseconds;
    let buffer_elapsed_seconds = i64::try_from(idle_milliseconds / 1_000)
        .expect("the difference between two i64 millisecond epochs fits in i64 seconds");
    BufferTiming {
        snapshot_elapsed_seconds,
        buffer_elapsed_seconds,
        display_buffer_seconds: i128::from(buffer_seconds) - i128::from(buffer_elapsed_seconds),
    }
}

pub(crate) fn session_interval_milliseconds(
    window_start_epoch_ms: i64,
    window_end_epoch_ms: i64,
    sessions: &[(i64, Option<i64>)],
) -> i128 {
    if window_end_epoch_ms <= window_start_epoch_ms {
        return 0;
    }
    let window_start = i128::from(window_start_epoch_ms);
    let window_end = i128::from(window_end_epoch_ms);
    let mut active_intervals: Vec<_> = sessions
        .iter()
        .filter_map(|(started_at, ended_at)| {
            let start = i128::from(*started_at).max(window_start);
            let end = i128::from(ended_at.unwrap_or(window_end_epoch_ms)).min(window_end);
            (start < end).then_some((start, end))
        })
        .collect();
    active_intervals.sort_unstable_by_key(|interval| interval.0);

    let mut active_milliseconds = 0_i128;
    let mut merged: Option<(i128, i128)> = None;
    for (start, end) in active_intervals {
        match merged {
            Some((merged_start, merged_end)) if start <= merged_end => {
                merged = Some((merged_start, merged_end.max(end)));
            }
            Some((merged_start, merged_end)) => {
                active_milliseconds += merged_end - merged_start;
                merged = Some((start, end));
            }
            None => merged = Some((start, end)),
        }
    }
    if let Some((start, end)) = merged {
        active_milliseconds += end - start;
    }
    active_milliseconds
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
