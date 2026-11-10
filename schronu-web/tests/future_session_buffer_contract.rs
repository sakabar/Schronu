use chrono::{Local, NaiveDateTime, TimeZone};
use schronu_web::client::state::{load_client_state, ClientState};
use schronu_web::client::view_projection::project_session_cards;

mod client_state_support;
use client_state_support::*;

fn local_epoch(text: &str) -> i64 {
    let time = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S").unwrap();
    Local
        .from_local_datetime(&time)
        .single()
        .unwrap()
        .timestamp_millis()
}

fn offset_minutes(epoch_ms: i64) -> i32 {
    chrono::DateTime::from_timestamp_millis(epoch_ms)
        .unwrap()
        .with_timezone(&Local)
        .offset()
        .local_minus_utc()
        / 60
}

fn planned_session(
    storage: &FakeStorage,
    now: i64,
    start: Option<i64>,
    pending: Option<i64>,
) -> ClientState {
    let mut state = load_client_state(storage, now).unwrap();
    let request_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(request_id, Ok(snapshot("2026-12-30", now)));
    let mut task_row = row(TASK_ID, 0);
    task_row.task.start_time_epoch_ms = start;
    task_row.task.pending_until_epoch_ms = pending;
    state.add_session_from_row(storage, &task_row);
    state
}

#[test]
fn future_start_or_pending_work_does_not_cancel_sleep_countdown() {
    let now = local_epoch("2026-12-30T12:00:00");
    let future = local_epoch("2027-01-02T12:00:00");
    for (start, pending, label) in [
        (Some(future), None, "2027/01/02 開始可能"),
        (None, Some(future), "2027/01/02 着手予定"),
    ] {
        let storage = FakeStorage::default();
        let mut state = planned_session(&storage, now, start, pending);
        assert_eq!(
            project_session_cards(&state, offset_minutes(state.tick_now_epoch_ms()))[0]
                .future_pending_date_label
                .as_deref(),
            Some(label)
        );
        state.tick(now + 600_000);
        assert_eq!(state.display_buffer_seconds(), Some(60 - 600));
    }
}

#[test]
fn same_day_metadata_and_legacy_or_invalid_metadata_keep_normal_credit() {
    let now = local_epoch("2026-12-30T12:00:00");
    for (start, pending) in [
        (None, None),
        (
            Some(local_epoch("2026-12-30T23:00:00")),
            Some(local_epoch("2026-12-31T05:59:59")),
        ),
        (Some(i64::MAX), Some(i64::MIN)),
    ] {
        let storage = FakeStorage::default();
        let mut state = planned_session(&storage, now, start, pending);
        assert_eq!(
            project_session_cards(&state, offset_minutes(state.tick_now_epoch_ms()))[0]
                .future_pending_date_label,
            None
        );
        state.tick(now + 600_000);
        assert_eq!(state.display_buffer_seconds(), Some(60));
    }
}

#[test]
fn six_oclock_only_credits_work_after_the_boundary_and_keeps_original_completion() {
    let now = local_epoch("2026-12-31T05:50:00");
    for (start, pending) in [
        (Some(local_epoch("2026-12-31T23:00:00")), None),
        (None, Some(local_epoch("2027-01-01T01:00:00"))),
    ] {
        let storage = FakeStorage::default();
        let mut state = planned_session(&storage, now, start, pending);
        for (elapsed, expected, attention) in [
            (599, -539, true),
            (600, -540, false),
            (840, -540, false),
            (900, -540, false),
            (960, -600, false),
        ] {
            state.tick(now + elapsed * 1_000);
            assert_eq!(
                project_session_cards(&state, offset_minutes(state.tick_now_epoch_ms()))[0]
                    .future_pending_date_label
                    .is_some(),
                attention
            );
            assert_eq!(state.display_buffer_seconds(), Some(expected));
        }
    }
}

#[test]
fn start_label_can_expire_while_pending_still_excludes_credit() {
    let now = local_epoch("2026-12-31T05:50:00");
    let storage = FakeStorage::default();
    let mut state = planned_session(
        &storage,
        now,
        Some(local_epoch("2026-12-31T06:00:00")),
        Some(local_epoch("2027-01-01T06:00:00")),
    );
    state.tick(now + 600_000);
    assert_eq!(
        project_session_cards(&state, offset_minutes(state.tick_now_epoch_ms()))[0]
            .future_pending_date_label
            .as_deref(),
        Some("2027/01/01 着手予定")
    );
    assert_eq!(state.display_buffer_seconds(), Some(-540));
    state.tick(local_epoch("2027-01-01T06:00:00"));
    assert_eq!(
        project_session_cards(&state, offset_minutes(state.tick_now_epoch_ms()))[0]
            .future_pending_date_label,
        None
    );
    // The original completion was before eligibility; no work can be credited later.
    assert_eq!(state.display_buffer_seconds(), Some(60 - 87_000));
}

#[test]
fn reload_and_list_refresh_keep_pre_boundary_work_excluded() {
    use schronu_web::{ServerSnapshot, WebSuccess};
    let now = local_epoch("2026-12-31T05:50:00");
    let storage = FakeStorage::default();
    let mut state = planned_session(
        &storage,
        now,
        None,
        Some(local_epoch("2026-12-31T06:00:00")),
    );
    state.tick(now + 840_000);
    assert_eq!(state.display_buffer_seconds(), Some(-540));
    let (request_id, _) = list_effect(state.request_list("2026-12-31"));
    let refreshed = ServerSnapshot {
        observed_at_epoch_ms: now + 840_000,
        logical_date: "2026-12-31".to_owned(),
        buffer_seconds: 60 - 840,
    };
    state.apply_list_result(
        request_id,
        "2026-12-31",
        Ok(WebSuccess {
            snapshot: refreshed.clone(),
            data: vec![],
        }),
    );
    assert_eq!(state.display_buffer_seconds(), Some(-540));
    let mut restored = load_client_state(&storage, now + 840_000).unwrap();
    let request_id = bootstrap_effect(restored.request_bootstrap());
    restored.apply_bootstrap_result(request_id, Ok(refreshed));
    assert_eq!(restored.display_buffer_seconds(), Some(-540));
    restored.tick(now + 960_000);
    assert_eq!(restored.display_buffer_seconds(), Some(-600));
}

#[test]
fn simultaneous_sessions_only_credit_the_normal_session() {
    let now = local_epoch("2026-12-30T12:00:00");
    for both_future in [false, true] {
        let storage = FakeStorage::default();
        let future = local_epoch("2027-01-02T12:00:00");
        let mut state = planned_session(&storage, now, None, Some(future));
        let mut other = row(OTHER_TASK_ID, 0);
        other.task.start_time_epoch_ms = both_future.then_some(future);
        state.add_session_from_row(&storage, &other);
        state.tick(now + 600_000);
        assert_eq!(
            state.display_buffer_seconds(),
            Some(if both_future { -540 } else { 60 })
        );
    }
}

#[test]
fn stop_before_boundary_does_not_start_credit_when_the_label_expires() {
    let now = local_epoch("2026-12-31T05:50:00");
    let storage = FakeStorage::default();
    let mut state = planned_session(
        &storage,
        now,
        None,
        Some(local_epoch("2026-12-31T06:00:00")),
    );
    state.tick(now + 300_000);
    record_effect(state.begin_record_session(&storage, TASK_ID));
    state.tick(now + 840_000);
    assert_eq!(state.display_buffer_seconds(), Some(60 - 840));
}

#[test]
fn future_work_completion_conflict_freezes_at_click_even_across_boundary() {
    use schronu_web::client::state::ServerFailure;
    let now = local_epoch("2026-12-31T05:50:00");
    for elapsed in [300, 960] {
        let storage = FakeStorage::default();
        let mut state = planned_session(
            &storage,
            now,
            None,
            Some(local_epoch("2027-01-02T06:00:00")),
        );
        state.tick(now + elapsed * 1_000);
        let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
        state.apply_complete_result(
            &storage,
            request_id,
            Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
        );
        state.tick(now + (elapsed + 600) * 1_000);
        assert_eq!(
            state.display_buffer_seconds(),
            Some(60 - i128::from(elapsed))
        );
        complete_effect(state.confirm_completion_conflict(&storage, TASK_ID));
        state.tick(local_epoch("2027-01-02T06:00:00"));
        assert_eq!(
            state.display_buffer_seconds(),
            Some(60 - i128::from(elapsed))
        );
    }
}

#[test]
fn committed_future_session_is_excluded_even_if_local_deletion_fails() {
    use schronu_web::{RecordSessionResult, WebSuccess};
    let now = local_epoch("2026-12-30T12:00:00");
    let storage = FakeStorage::default();
    let mut state = planned_session(
        &storage,
        now,
        None,
        Some(local_epoch("2027-01-02T06:00:00")),
    );
    state.tick(now + 300_000);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    storage.fail_work_session_writes.set(true);
    state.apply_record_result(
        &storage,
        request_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-12-30", now + 300_000),
            data: RecordSessionResult {
                actual_work_seconds: 300,
            },
        }),
    );
    state.tick(now + 600_000);
    assert!(state.is_session_committed_blocked(TASK_ID));
    assert_eq!(state.display_buffer_seconds(), Some(-240));
}

#[test]
fn conflict_resume_across_six_keeps_excluded_work_after_reload() {
    use schronu_web::client::state::ServerFailure;
    let now = Local
        .with_ymd_and_hms(2026, 12, 31, 5, 50, 0)
        .unwrap()
        .timestamp_millis();
    let pending = Local
        .with_ymd_and_hms(2026, 12, 31, 6, 0, 0)
        .unwrap()
        .timestamp_millis();
    let storage = FakeStorage::default();
    let mut state = planned_session(&storage, now, None, Some(pending));
    state.tick(now + 300_000);
    let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    state.apply_complete_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
    );
    state.tick(now + 1_200_000);
    state.resume_completion_conflict(&storage, TASK_ID);
    assert_eq!(state.display_buffer_seconds(), Some(60 - 1_200));
    let mut restored = load_client_state(&storage, now + 1_200_000).unwrap();
    let request_id = bootstrap_effect(restored.request_bootstrap());
    restored.apply_bootstrap_result(request_id, Ok(snapshot("2026-12-30", now)));
    restored.tick(now + 1_500_000);
    assert_eq!(restored.display_buffer_seconds(), Some(60 - 1_200));
    restored.restart_session_without_recording(&storage, TASK_ID);
    assert_eq!(restored.display_buffer_seconds(), Some(60 - 1_500));
    restored.tick(now + 1_560_000);
    assert_eq!(restored.display_buffer_seconds(), Some(60 - 1_500));
}

#[test]
fn future_completion_conflict_freezes_fractional_click_without_second_jitter() {
    use schronu_web::client::state::ServerFailure;
    let now = local_epoch("2026-12-30T12:00:00");
    for elapsed_ms in [6_500, 960_500] {
        let storage = FakeStorage::default();
        let mut state = planned_session(
            &storage,
            now,
            None,
            Some(local_epoch("2027-01-02T06:00:00")),
        );
        state.tick(now + elapsed_ms);
        let expected = state.display_buffer_seconds();
        let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
        state.apply_complete_result(
            &storage,
            request_id,
            Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
        );
        for wait_ms in [13_500, 14_000, 23_500] {
            state.tick(now + elapsed_ms + wait_ms);
            assert_eq!(state.display_buffer_seconds(), expected);
        }
    }
}
