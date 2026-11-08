use chrono::DateTime;
use schronu_web::client::state::load_client_state;
use schronu_web::client::view_projection::project_session_cards;
use schronu_web::{SessionTask, WebSuccess};

mod client_state_support;
use client_state_support::*;

fn epoch(text: &str) -> i64 {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .timestamp_millis()
}

#[test]
fn pending_label_uses_logical_date_after_manual_or_auto_start_and_reload() {
    let now = epoch("2026-12-30T12:00:00+09:00");
    for automatic in [false, true] {
        for (until, expected) in [
            (None, None),
            (Some("2026-12-29T12:00:00+09:00"), None),
            (Some("2026-12-30T23:00:00+09:00"), None),
            (Some("2026-12-31T05:59:59+09:00"), None),
            (
                Some("2026-12-31T06:00:00+09:00"),
                Some("2026/12/31 着手予定"),
            ),
            (
                Some("2027-01-01T01:00:00+09:00"),
                Some("2026/12/31 着手予定"),
            ),
            (
                Some("2027-01-01T06:00:00+09:00"),
                Some("2027/01/01 着手予定"),
            ),
        ] {
            let storage = FakeStorage::default();
            let mut state = load_client_state(&storage, now).unwrap();
            let mut task_row = row(TASK_ID, 0);
            task_row.task.pending_until_epoch_ms = until.map(epoch);
            if automatic {
                let request_id = auto_effect(state.request_auto_session());
                state.apply_auto_session_result(
                    &storage,
                    request_id,
                    Ok(WebSuccess {
                        snapshot: snapshot("2026-12-30", now),
                        data: task_row.task.actionable_task(),
                    }),
                );
            } else {
                state.add_session_from_row(&storage, &task_row);
            }
            let restored = load_client_state(&storage, now).unwrap();
            for current in [&state, &restored] {
                let cards = project_session_cards(current, 9 * 60);
                assert_eq!(cards.len(), 1);
                assert_eq!(cards[0].future_pending_date_label.as_deref(), expected);
                assert_eq!(
                    current.sessions()[0].pending_until_epoch_ms,
                    until.map(epoch)
                );
            }
        }
    }
}

#[test]
fn pending_attention_expires_at_logical_day_boundary_without_a_server_request() {
    let now = epoch("2026-12-31T05:59:59+09:00");
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, now).unwrap();
    let mut task_row = row(TASK_ID, 0);
    task_row.task.pending_until_epoch_ms = Some(epoch("2027-01-01T01:00:00+09:00"));
    state.add_session_from_row(&storage, &task_row);
    assert_eq!(
        project_session_cards(&state, 9 * 60)[0]
            .future_pending_date_label
            .as_deref(),
        Some("2026/12/31 着手予定")
    );
    state.tick(now + 1_000);
    assert_eq!(
        project_session_cards(&state, 9 * 60)[0].future_pending_date_label,
        None
    );
}

#[test]
fn old_wire_payload_and_saved_session_without_pending_metadata_remain_usable() {
    let task: SessionTask = serde_json::from_value(serde_json::json!({
        "task_id": TASK_ID,
        "task_name": "legacy contract",
        "estimated_work_seconds": 60,
        "actual_work_seconds": 0
    }))
    .unwrap();
    assert_eq!(task.pending_until_epoch_ms, None);
    assert_eq!(task.start_time_epoch_ms, None);
    let storage = FakeStorage::default();
    let state = state_with_sessions(&storage, &[TASK_ID]);
    assert_eq!(state.sessions()[0].pending_until_epoch_ms, None);
    assert_eq!(state.sessions()[0].start_time_epoch_ms, None);
    assert_eq!(
        project_session_cards(&state, 9 * 60)[0].future_pending_date_label,
        None
    );
}

#[test]
fn attention_prioritizes_future_start_day_after_manual_auto_start_and_reload() {
    let now = epoch("2026-12-30T12:00:00+09:00");
    for automatic in [false, true] {
        for (start, pending, expected) in [
            (
                "2027-01-02T06:00:00+09:00",
                None,
                Some("2027/01/02 開始予定"),
            ),
            (
                "2027-01-02T06:00:00+09:00",
                Some("2026-12-31T06:00:00+09:00"),
                Some("2027/01/02 開始予定"),
            ),
            (
                "2026-12-31T06:00:00+09:00",
                Some("2027-01-02T06:00:00+09:00"),
                Some("2026/12/31 開始予定"),
            ),
            (
                "2026-12-31T06:00:00+09:00",
                Some("2026-12-31T06:00:00+09:00"),
                Some("2026/12/31 開始予定"),
            ),
            (
                "2026-12-30T06:00:00+09:00",
                Some("2027-01-02T06:00:00+09:00"),
                Some("2027/01/02 着手予定"),
            ),
            (
                "2026-12-30T23:00:00+09:00",
                Some("2026-12-31T05:59:59+09:00"),
                None,
            ),
            ("2026-12-31T05:59:59+09:00", None, None),
            (
                "2026-12-30T06:00:00+09:00",
                Some("2026-12-29T06:00:00+09:00"),
                None,
            ),
        ] {
            let storage = FakeStorage::default();
            let mut state = load_client_state(&storage, now).unwrap();
            let mut value = serde_json::to_value(row(TASK_ID, 0)).unwrap();
            value["task"]["start_time_epoch_ms"] = epoch(start).into();
            value["task"]["pending_until_epoch_ms"] = pending.map(epoch).into();
            let task_row: schronu_web::ScheduledTaskRow = serde_json::from_value(value).unwrap();
            if automatic {
                let request_id = auto_effect(state.request_auto_session());
                state.apply_auto_session_result(
                    &storage,
                    request_id,
                    Ok(WebSuccess {
                        snapshot: snapshot("2026-12-30", now),
                        data: task_row.task.actionable_task(),
                    }),
                );
            } else {
                state.add_session_from_row(&storage, &task_row);
            }
            let restored = load_client_state(&storage, now).unwrap();
            for current in [&state, &restored] {
                assert_eq!(
                    project_session_cards(current, 9 * 60)[0]
                        .future_pending_date_label
                        .as_deref(),
                    expected
                );
            }
            state.tick(epoch("2027-01-02T06:00:00+09:00"));
            assert_eq!(
                project_session_cards(&state, 9 * 60)[0].future_pending_date_label,
                None
            );
        }
    }
}

#[test]
fn start_attention_transitions_to_pending_then_expires_at_six_without_mutation() {
    for pending in [None, Some("2027-01-01T06:00:00+09:00")] {
        let storage = FakeStorage::default();
        let now = epoch("2026-12-31T05:59:59+09:00");
        let mut state = load_client_state(&storage, now).unwrap();
        let mut task_row = row(TASK_ID, 0);
        task_row.task.start_time_epoch_ms = Some(epoch("2026-12-31T06:00:00+09:00"));
        task_row.task.pending_until_epoch_ms = pending.map(epoch);
        state.add_session_from_row(&storage, &task_row);
        let before = state.sessions().to_vec();
        let stored = storage.value.borrow().clone();
        assert_eq!(
            project_session_cards(&state, 9 * 60)[0]
                .future_pending_date_label
                .as_deref(),
            Some("2026/12/31 開始予定")
        );
        state.tick(now + 1_000);
        assert_eq!(
            project_session_cards(&state, 9 * 60)[0]
                .future_pending_date_label
                .as_deref(),
            pending.map(|_| "2027/01/01 着手予定")
        );
        state.tick(epoch("2027-01-01T05:59:59+09:00"));
        assert_eq!(
            project_session_cards(&state, 9 * 60)[0]
                .future_pending_date_label
                .as_deref(),
            pending.map(|_| "2027/01/01 着手予定")
        );
        state.tick(epoch("2027-01-01T06:00:00+09:00"));
        assert_eq!(
            project_session_cards(&state, 9 * 60)[0].future_pending_date_label,
            None
        );
        assert_eq!(state.sessions(), before);
        assert_eq!(*storage.value.borrow(), stored);
    }
}
