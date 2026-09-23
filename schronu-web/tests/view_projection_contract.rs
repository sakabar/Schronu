use schronu_web::client::state::load_client_state;
use schronu_web::client::view_projection::{
    format_local_hh_mm, project_list_rows, project_session_cards, DeferConfirmationKind,
};
use schronu_web::{
    DeferMode, DeferPlan, RecordSessionResult, ScheduledTaskRow, SessionTask, WebSuccess,
};

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
    assert_eq!(active[0].actual_work_seconds_at_start, 300);
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
fn 終了処理中のsession表示はclick時刻で停止し失敗時に再開する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, START_EPOCH_MS).unwrap();
    state.add_session_from_row(&storage, &session_row());
    state.tick(START_EPOCH_MS + 60_000);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));

    state.tick(START_EPOCH_MS + 65_000);
    let pending = project_session_cards(&state, JST_OFFSET_MINUTES);
    assert_eq!(pending[0].remaining_seconds, 540);
    assert_eq!(pending[0].progress_percent, Some(40));

    state.apply_record_result(
        &storage,
        request_id,
        Err(schronu_web::client::state::ServerFailure::Operation(
            schronu_web::WebError {
                code: schronu_web::web_error_codes::REPOSITORY_SAVE_FAILED.to_owned(),
                message: "safe".to_owned(),
                retry_advice: schronu_web::RetryAdvice::Retry,
                current_actual_work_seconds: None,
            },
        )),
    );
    let resumed = project_session_cards(&state, JST_OFFSET_MINUTES);
    assert_eq!(resumed[0].remaining_seconds, 535);
}

#[test]
fn commit成否不明のsession表示はrepository確認までclick時刻で停止する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, START_EPOCH_MS).unwrap();
    state.add_session_from_row(&storage, &session_row());
    state.tick(START_EPOCH_MS + 60_000);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));

    state.apply_record_result(
        &storage,
        request_id,
        Err(schronu_web::client::state::ServerFailure::Transport(
            "detail".to_owned(),
        )),
    );
    state.tick(START_EPOCH_MS + 70_000);
    let uncertain = project_session_cards(&state, JST_OFFSET_MINUTES);
    assert_eq!(uncertain[0].remaining_seconds, 540);

    state.confirm_repository_checked(&storage);
    let resumed = project_session_cards(&state, JST_OFFSET_MINUTES);
    assert_eq!(resumed[0].remaining_seconds, 530);
}

#[test]
fn listはserverが生成したdeadline表示と予定超過を無変換で保持する() {
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
        deadline_label: "server deadline label".to_owned(),
        misses_deadline: true,
        is_leaf: true,
        defer_plan: DeferPlan {
            mode: DeferMode::Normal,
            requested_pending_until_epoch_ms: START_EPOCH_MS + 86_400_000,
            effective_pending_until_epoch_ms: None,
            repetition_interval_days: None,
        },
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
    assert_eq!(rows[0].deadline_label, "server deadline label");
    assert!(rows[0].misses_deadline);
}

#[test]
fn 日付別listは今日の現在時刻とtask間の1分以上の空きを表示する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, START_EPOCH_MS).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", START_EPOCH_MS)));
    let (request_id, request) = list_effect(state.request_list("2026-09-05"));

    let mut first = session_row();
    first.schedule_start_epoch_ms = START_EPOCH_MS + 90_000;
    first.schedule_end_epoch_ms = START_EPOCH_MS + 10 * 60_000;
    let mut overlapping = session_row();
    overlapping.task.task_id = OTHER_TASK_ID.to_owned();
    overlapping.schedule_start_epoch_ms = START_EPOCH_MS + 9 * 60_000;
    overlapping.schedule_end_epoch_ms = START_EPOCH_MS + 20 * 60_000;
    let mut next = session_row();
    next.task.task_id = "33333333-3333-3333-3333-333333333333".to_owned();
    next.schedule_start_epoch_ms = START_EPOCH_MS + 21 * 60_000 + 59_000;
    next.schedule_end_epoch_ms = START_EPOCH_MS + 30 * 60_000;
    state.apply_list_result(
        request_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", START_EPOCH_MS),
            data: vec![first, overlapping, next],
        }),
    );

    let rows = project_list_rows(&state, JST_OFFSET_MINUTES);
    assert_eq!(rows[0].gap_before.as_deref(), Some("1分間の空き時間"));
    assert_eq!(rows[1].gap_before, None);
    assert_eq!(rows[2].gap_before.as_deref(), Some("1分間の空き時間"));
}

#[test]
fn 日付別listは今日以外の先頭と1分未満の空きを表示しない() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, START_EPOCH_MS).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", START_EPOCH_MS)));
    let (request_id, request) = list_effect(state.request_list("2026-09-06"));

    let mut first = session_row();
    first.schedule_start_epoch_ms = START_EPOCH_MS + 24 * 60 * 60_000;
    first.schedule_end_epoch_ms = first.schedule_start_epoch_ms + 10 * 60_000;
    let mut next = session_row();
    next.task.task_id = OTHER_TASK_ID.to_owned();
    next.schedule_start_epoch_ms = first.schedule_end_epoch_ms + 59_999;
    next.schedule_end_epoch_ms = next.schedule_start_epoch_ms + 10 * 60_000;
    state.apply_list_result(
        request_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", START_EPOCH_MS),
            data: vec![first, next],
        }),
    );

    let rows = project_list_rows(&state, JST_OFFSET_MINUTES);
    assert_eq!(rows[0].gap_before, None);
    assert_eq!(rows[1].gap_before, None);
}

#[test]
fn invalid_epochとoffsetはplaceholderへ安全に退避する() {
    assert_eq!(format_local_hh_mm(i64::MAX, JST_OFFSET_MINUTES), "--:--");
    assert_eq!(format_local_hh_mm(START_EPOCH_MS, i32::MAX), "--:--");
}

#[test]
fn listの先送り確認はserverのplanだけから生成する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, START_EPOCH_MS).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", START_EPOCH_MS - 1)));
    let (request_id, request) = list_effect(state.request_list("2026-09-05"));
    let mut limited = session_row();
    limited.defer_plan = DeferPlan {
        mode: DeferMode::DeadlineLimited,
        requested_pending_until_epoch_ms: START_EPOCH_MS + 86_400_000,
        effective_pending_until_epoch_ms: Some(START_EPOCH_MS + 3_600_000),
        repetition_interval_days: None,
    };
    let mut routine = session_row();
    routine.task.task_id = OTHER_TASK_ID.to_owned();
    routine.defer_plan = DeferPlan {
        mode: DeferMode::RoutinePeriod,
        requested_pending_until_epoch_ms: START_EPOCH_MS + 86_400_000,
        effective_pending_until_epoch_ms: None,
        repetition_interval_days: Some(7),
    };
    state.apply_list_result(
        request_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", START_EPOCH_MS),
            data: vec![limited, routine],
        }),
    );

    let rows = project_list_rows(&state, JST_OFFSET_MINUTES);
    assert_eq!(
        rows[0].defer_plan.as_ref().unwrap().mode,
        DeferMode::DeadlineLimited
    );
    assert_eq!(
        rows[0].defer_confirmation.as_ref().unwrap().kind,
        DeferConfirmationKind::DeadlineLimited
    );
    assert_eq!(
        rows[1].defer_plan.as_ref().unwrap().mode,
        DeferMode::RoutinePeriod
    );
    assert_eq!(
        rows[1].defer_confirmation.as_ref().unwrap().detail_label,
        "7日"
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
        deadline_label: "____/__/__".to_owned(),
        misses_deadline: false,
        is_leaf: true,
        defer_plan: DeferPlan {
            mode: DeferMode::Normal,
            requested_pending_until_epoch_ms: START_EPOCH_MS + 86_400_000,
            effective_pending_until_epoch_ms: None,
            repetition_interval_days: None,
        },
    }
}
