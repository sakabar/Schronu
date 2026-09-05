use schronu_web::client::state::load_client_state;
use schronu_web::client::view_projection::{
    format_deadline_label, format_local_hh_mm, project_list_rows, project_session_cards,
};
use schronu_web::{RecordSessionResult, ScheduledTaskRow, SessionTask, WebSuccess};

mod client_state_support;
use client_state_support::*;

const START_EPOCH_MS: i64 = 1_788_568_200_000; // 2026-09-05 00:30 UTC
const JST_OFFSET_MINUTES: i32 = 9 * 60;

#[test]
fn fixed_offsetでsession時刻と進捗を生成しcommit済みtimerは停止する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, START_EPOCH_MS).unwrap();
    state.add_session_from_row(&storage, &session_row());
    state.tick(START_EPOCH_MS + 60_000);

    let active = project_session_cards(&state, JST_OFFSET_MINUTES);
    assert_eq!(active[0].started_at_hh_mm, "09:30");
    assert_eq!(active[0].completion_hh_mm.as_deref(), Some("09:40"));
    assert_eq!(active[0].progress_percent, Some(40));
    assert_eq!(active[0].remaining_seconds, 540);
    assert!(!active[0].server_committed);

    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    storage.fail_work_session_writes.set(true);
    state.apply_record_result(
        &storage,
        request_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", START_EPOCH_MS + 60_000),
            data: RecordSessionResult {
                actual_work_seconds: 360,
            },
        }),
    );
    state.tick(START_EPOCH_MS + 10 * 60_000);

    let committed = project_session_cards(&state, JST_OFFSET_MINUTES);
    assert_eq!(committed[0].progress_percent, Some(40));
    assert_eq!(committed[0].remaining_seconds, 540);
    assert!(committed[0].server_committed);
}

#[test]
fn list時刻とdeadlineは06時境界のlogical_dateで表示形式を選ぶ() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, START_EPOCH_MS).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-04", START_EPOCH_MS)));
    let (request_id, request) = list_effect(state.request_list("2026-09-04"));
    let row = ScheduledTaskRow {
        task: session_row().task,
        schedule_start_epoch_ms: START_EPOCH_MS,
        schedule_end_epoch_ms: START_EPOCH_MS + 30 * 60_000,
        deadline_epoch_ms: Some(1_788_553_800_000), // 2026-09-05 05:30 JST
        is_leaf: true,
    };
    state.apply_list_result(
        request_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-04", START_EPOCH_MS),
            data: vec![row],
        }),
    );

    let rows = project_list_rows(&state, JST_OFFSET_MINUTES);
    assert_eq!(rows[0].schedule_label, "09:30-10:00");
    assert_eq!(rows[0].deadline_label.as_deref(), Some("05:30"));
    assert_eq!(
        format_deadline_label(1_788_553_800_000, "2026-09-05", JST_OFFSET_MINUTES).as_deref(),
        Some("09/05 05:30")
    );
}

#[test]
fn invalid_epochとoffsetはplaceholderへ安全に退避する() {
    assert_eq!(format_local_hh_mm(i64::MAX, JST_OFFSET_MINUTES), "--:--");
    assert_eq!(format_local_hh_mm(START_EPOCH_MS, i32::MAX), "--:--");
    assert_eq!(
        format_deadline_label(i64::MAX, "2026-09-05", JST_OFFSET_MINUTES).as_deref(),
        Some("--:--")
    );
}

fn session_row() -> ScheduledTaskRow {
    ScheduledTaskRow {
        task: SessionTask {
            task_id: TASK_ID.to_owned(),
            task_name: "task".to_owned(),
            estimated_work_seconds: 900,
            actual_work_seconds: 300,
        },
        schedule_start_epoch_ms: START_EPOCH_MS,
        schedule_end_epoch_ms: START_EPOCH_MS + 30 * 60_000,
        deadline_epoch_ms: None,
        is_leaf: true,
    }
}
