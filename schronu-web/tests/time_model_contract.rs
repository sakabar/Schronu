use schronu_web::client::time_model::{
    buffer_timing, format_hh_mm_ss, format_mm_ss, session_timing,
};

#[test]
fn session_timing_uses_the_start_epoch_across_reload_and_delayed_ticks() {
    let timing = session_timing(1_000_000, 900, 300, 1_123_999);

    assert_eq!(timing.elapsed_seconds, 123);
    assert_eq!(timing.remaining_at_start_seconds, 600);
    assert_eq!(timing.estimated_completion_epoch_ms, Some(1_600_000));
    assert_eq!(timing.worked_seconds, 423);
    assert_eq!(timing.progress_percent, Some(47));
    assert_eq!(timing.remaining_seconds, 477);
    assert_eq!(timing.normal_bar_percent, 47);
    assert_eq!(timing.overrun_bar_percent, 0);
}

#[test]
fn session_timing_clamps_a_backward_clock_to_zero_elapsed() {
    let timing = session_timing(1_000_000, 900, 300, 999_000);

    assert_eq!(timing.elapsed_seconds, 0);
    assert_eq!(timing.worked_seconds, 300);
    assert_eq!(timing.remaining_seconds, 600);
}

#[test]
fn session_progress_and_bars_cover_33_100_and_133_percent() {
    let one_third = session_timing(0, 900, 300, 0);
    assert_eq!(one_third.progress_percent, Some(33));
    assert_eq!(one_third.normal_bar_percent, 33);
    assert_eq!(one_third.overrun_bar_percent, 0);

    let complete = session_timing(0, 900, 900, 0);
    assert_eq!(complete.progress_percent, Some(100));
    assert_eq!(complete.normal_bar_percent, 100);
    assert_eq!(complete.overrun_bar_percent, 0);

    let overrun = session_timing(0, 900, 1_200, 0);
    assert_eq!(overrun.progress_percent, Some(133));
    assert_eq!(overrun.normal_bar_percent, 100);
    assert_eq!(overrun.overrun_bar_percent, 33);
}

#[test]
fn session_timing_handles_zero_estimate_and_signed_remaining_time() {
    let zero_estimate = session_timing(10_000, 0, 0, 11_000);
    assert_eq!(zero_estimate.progress_percent, None);
    assert_eq!(zero_estimate.normal_bar_percent, 0);
    assert_eq!(zero_estimate.overrun_bar_percent, 0);
    assert_eq!(zero_estimate.remaining_seconds, -1);

    let overrun = session_timing(10_000, 60, 0, 71_999);
    assert_eq!(overrun.remaining_seconds, -1);
    assert_eq!(format_mm_ss(overrun.remaining_seconds), "00:01");
}

#[test]
fn session_timing_avoids_i64_overflow() {
    let extreme_elapsed = session_timing(i64::MIN, i64::MAX, i64::MAX, i64::MAX);
    assert_eq!(extreme_elapsed.elapsed_seconds, 18_446_744_073_709_551);
    assert_eq!(
        extreme_elapsed.worked_seconds,
        i128::from(i64::MAX) + 18_446_744_073_709_551_i128
    );
    assert!(extreme_elapsed.progress_percent.is_some());

    let completion_overflow = session_timing(i64::MAX - 999, 1, 0, i64::MAX - 999);
    assert_eq!(completion_overflow.estimated_completion_epoch_ms, None);
}

#[test]
fn buffer_timing_counts_down_from_the_server_observation_epoch() {
    let timing = buffer_timing(1_000_000, 60, 1_061_999, &[]);
    assert_eq!(timing.snapshot_elapsed_seconds, 61);
    assert_eq!(timing.buffer_elapsed_seconds, 61);
    assert_eq!(timing.display_buffer_seconds, -1);

    let backward_clock = buffer_timing(1_000_000, 60, 999_000, &[]);
    assert_eq!(backward_clock.snapshot_elapsed_seconds, 0);
    assert_eq!(backward_clock.buffer_elapsed_seconds, 0);
    assert_eq!(backward_clock.display_buffer_seconds, 60);

    let boundary = buffer_timing(i64::MIN, i64::MIN, i64::MAX, &[]);
    assert_eq!(boundary.snapshot_elapsed_seconds, 18_446_744_073_709_551);
    assert_eq!(boundary.buffer_elapsed_seconds, 18_446_744_073_709_551);
    assert_eq!(
        boundary.display_buffer_seconds,
        i128::from(i64::MIN) - 18_446_744_073_709_551_i128
    );
}

#[test]
fn buffer_timing_counts_only_time_without_an_active_session() {
    let active_at_snapshot = buffer_timing(1_000_000, 60, 1_061_999, &[900_000]);
    assert_eq!(active_at_snapshot.snapshot_elapsed_seconds, 61);
    assert_eq!(active_at_snapshot.buffer_elapsed_seconds, 0);
    assert_eq!(active_at_snapshot.display_buffer_seconds, 60);

    let started_after_idle = buffer_timing(1_000_000, 60, 1_061_999, &[1_010_500]);
    assert_eq!(started_after_idle.snapshot_elapsed_seconds, 61);
    assert_eq!(started_after_idle.buffer_elapsed_seconds, 10);
    assert_eq!(started_after_idle.display_buffer_seconds, 50);

    let future_start_after_clock_reversal =
        buffer_timing(1_000_000, 60, 1_005_000, &[1_010_000]);
    assert_eq!(future_start_after_clock_reversal.buffer_elapsed_seconds, 5);
    assert_eq!(future_start_after_clock_reversal.display_buffer_seconds, 55);
}

#[test]
fn buffer_timing_uses_the_union_of_remaining_sessions_after_discard() {
    let overlapping_sessions =
        buffer_timing(1_000_000, 60, 1_061_999, &[1_010_500, 1_020_500]);
    assert_eq!(overlapping_sessions.buffer_elapsed_seconds, 10);
    assert_eq!(overlapping_sessions.display_buffer_seconds, 50);

    let earliest_discarded = buffer_timing(1_000_000, 60, 1_061_999, &[1_020_500]);
    assert_eq!(earliest_discarded.buffer_elapsed_seconds, 20);
    assert_eq!(earliest_discarded.display_buffer_seconds, 40);

    let all_discarded = buffer_timing(1_000_000, 60, 1_061_999, &[]);
    assert_eq!(all_discarded.buffer_elapsed_seconds, 61);
    assert_eq!(all_discarded.display_buffer_seconds, -1);
}

#[test]
fn duration_formatters_keep_total_minutes_and_hours() {
    assert_eq!(format_mm_ss(0), "00:00");
    assert_eq!(format_mm_ss(6_001), "100:01");
    assert_eq!(format_mm_ss(-61), "01:01");
    assert_eq!(format_hh_mm_ss(90_061), "25:01:01");
    assert_eq!(format_hh_mm_ss(-90_061), "-25:01:01");
}
