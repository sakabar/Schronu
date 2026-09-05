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
    pub session_credit_seconds: i64,
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
    tracking_started_at_epoch_ms: i64,
    buffer_seconds: i64,
    tick_now_epoch_ms: i64,
    active_session_started_at_epoch_ms: &[i64],
) -> BufferTiming {
    let snapshot_elapsed_seconds = elapsed_seconds(observed_at_epoch_ms, tick_now_epoch_ms);
    let earliest_active_start = active_session_started_at_epoch_ms.iter().copied().min();
    let tracking_start = tracking_started_at_epoch_ms.min(observed_at_epoch_ms);
    let buffer_elapsed_seconds =
        earliest_active_start.map_or(snapshot_elapsed_seconds, |earliest_session_start| {
            elapsed_seconds(
                observed_at_epoch_ms,
                tick_now_epoch_ms.min(earliest_session_start),
            )
        });
    let session_credit_seconds = earliest_active_start.map_or(0, |earliest_session_start| {
        elapsed_seconds(
            earliest_session_start.max(tracking_start),
            observed_at_epoch_ms,
        )
    });
    BufferTiming {
        snapshot_elapsed_seconds,
        buffer_elapsed_seconds,
        session_credit_seconds,
        display_buffer_seconds: i128::from(buffer_seconds) + i128::from(session_credit_seconds)
            - i128::from(buffer_elapsed_seconds),
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
