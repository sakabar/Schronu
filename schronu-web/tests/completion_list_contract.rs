use schronu_web::client::state::{ClientState, ServerFailure};
use schronu_web::{
    web_error_codes, RecordSessionResult, RetryAdvice, ScheduledTaskRow, WebSuccess,
};

mod client_state_support;
use client_state_support::*;

fn apply_list_rows(
    state: &mut ClientState,
    logical_date: &str,
    observed_at_epoch_ms: i64,
    rows: Vec<ScheduledTaskRow>,
) {
    if state.snapshot().is_none() {
        let request_id = bootstrap_effect(state.request_bootstrap());
        state.apply_bootstrap_result(
            request_id,
            Ok(snapshot(logical_date, observed_at_epoch_ms - 1)),
        );
    }
    let (request_id, request) = list_effect(state.request_list(logical_date));
    state.apply_list_result(
        request_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot(logical_date, observed_at_epoch_ms),
            data: rows,
        }),
    );
}

fn assert_list_effect(effect: schronu_web::client::state::ClientEffect, expected_date: &str) {
    let (_, request) = list_effect(effect);
    assert_eq!(request.logical_date, expected_date);
}

#[test]
fn 四つのsession終了操作成功後は選択中の日付を再取得する() {
    let selected_date = "2026-09-07";

    let discard_storage = FakeStorage::default();
    let mut discard_state = state_with_sessions(&discard_storage, &[TASK_ID]);
    apply_list_rows(&mut discard_state, selected_date, 1, vec![row(TASK_ID, 0)]);
    assert_list_effect(
        discard_state.discard_session(&discard_storage, TASK_ID),
        selected_date,
    );

    let record_storage = FakeStorage::default();
    let mut record_state = state_with_sessions(&record_storage, &[TASK_ID]);
    apply_list_rows(&mut record_state, selected_date, 1, vec![row(TASK_ID, 0)]);
    let (record_request_id, _) =
        record_effect(record_state.begin_record_session(&record_storage, TASK_ID));
    assert_list_effect(
        record_state.apply_record_result(
            &record_storage,
            record_request_id,
            Ok(WebSuccess {
                snapshot: snapshot("2026-09-05", 2),
                data: RecordSessionResult {
                    actual_work_seconds: 101,
                },
            }),
        ),
        selected_date,
    );

    for record_elapsed_seconds in [true, false] {
        let storage = FakeStorage::default();
        let mut state = state_with_sessions(&storage, &[TASK_ID]);
        apply_list_rows(&mut state, selected_date, 1, vec![row(TASK_ID, 0)]);
        let effect = if record_elapsed_seconds {
            state.begin_complete_session(&storage, TASK_ID)
        } else {
            state.begin_complete_session_without_recording(&storage, TASK_ID)
        };
        let (request_id, _) = complete_effect(effect);
        assert_list_effect(
            state.apply_complete_result(&storage, request_id, Ok(snapshot("2026-09-05", 2))),
            selected_date,
        );
    }
}

#[test]
fn mutation成功後は一覧を推測更新せず再取得結果で全置換する() {
    for record_elapsed_seconds in [true, false] {
        let storage = FakeStorage::default();
        let mut state = state_with_sessions(&storage, &[TASK_ID]);
        let initial_rows = vec![row(TASK_ID, 0), row(OTHER_TASK_ID, 0), row(TASK_ID, 0)];
        apply_list_rows(&mut state, "2026-09-05", 1, initial_rows.clone());

        let effect = if record_elapsed_seconds {
            state.begin_complete_session(&storage, TASK_ID)
        } else {
            state.begin_complete_session_without_recording(&storage, TASK_ID)
        };
        let (request_id, _) = complete_effect(effect);
        let (list_request_id, request) = list_effect(state.apply_complete_result(
            &storage,
            request_id,
            Ok(snapshot("2026-09-05", 2)),
        ));

        assert_eq!(state.scheduled_rows(), initial_rows);
        let server_rows = vec![row(OTHER_TASK_ID, 300)];
        state.apply_list_result(
            list_request_id,
            &request.logical_date,
            Ok(WebSuccess {
                snapshot: snapshot("2026-09-05", 3),
                data: server_rows.clone(),
            }),
        );
        assert_eq!(state.scheduled_rows(), server_rows);
    }
}

#[test]
fn 日付境界のmutation成功は新snapshotと選択日を両立して選択日を再取得する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let initial_rows = vec![row(TASK_ID, 0), row(OTHER_TASK_ID, 10)];
    apply_list_rows(&mut state, "2026-09-05", 1, initial_rows.clone());
    let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));

    let effect = state.apply_complete_result(&storage, request_id, Ok(snapshot("2026-09-06", 2)));

    assert_list_effect(effect, "2026-09-05");
    assert_eq!(state.snapshot().unwrap().logical_date, "2026-09-06");
    assert_eq!(state.date_buttons()[0].logical_date, "2026-09-06");
    assert_eq!(state.selected_logical_date(), Some("2026-09-05"));
    assert_eq!(state.scheduled_rows(), initial_rows);
}

#[test]
fn 一覧未選択なら最新snapshotのlogical_dateを再取得する() {
    let discard_storage = FakeStorage::default();
    let mut discard_state = state_with_sessions(&discard_storage, &[TASK_ID]);
    let bootstrap_id = bootstrap_effect(discard_state.request_bootstrap());
    discard_state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1)));
    assert_list_effect(
        discard_state.discard_session(&discard_storage, TASK_ID),
        "2026-09-05",
    );

    let record_storage = FakeStorage::default();
    let mut record_state = state_with_sessions(&record_storage, &[TASK_ID]);
    let (request_id, _) =
        record_effect(record_state.begin_record_session(&record_storage, TASK_ID));
    assert_list_effect(
        record_state.apply_record_result(
            &record_storage,
            request_id,
            Ok(WebSuccess {
                snapshot: snapshot("2026-09-06", 2),
                data: RecordSessionResult {
                    actual_work_seconds: 101,
                },
            }),
        ),
        "2026-09-06",
    );
}

#[test]
fn session終了失敗時は再取得せず一覧とsessionを保持する() {
    for record_elapsed_seconds in [true, false] {
        let storage = FakeStorage::default();
        let mut state = state_with_sessions(&storage, &[TASK_ID]);
        apply_list_rows(&mut state, "2026-09-05", 1, vec![row(TASK_ID, 0)]);
        let expected_rows = state.scheduled_rows().to_vec();
        let effect = if record_elapsed_seconds {
            state.begin_complete_session(&storage, TASK_ID)
        } else {
            state.begin_complete_session_without_recording(&storage, TASK_ID)
        };
        let (request_id, _) = complete_effect(effect);

        let follow_up = state.apply_complete_result(
            &storage,
            request_id,
            Err(ServerFailure::Operation(web_error(
                web_error_codes::REPOSITORY_SAVE_FAILED,
                RetryAdvice::Retry,
            ))),
        );

        assert_eq!(follow_up, schronu_web::client::state::ClientEffect::None);
        assert_eq!(state.scheduled_rows(), expected_rows);
        assert_eq!(state.sessions().len(), 1);
    }

    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1)));
    storage.fail_work_session_writes.set(true);
    assert_eq!(
        state.discard_session(&storage, TASK_ID),
        schronu_web::client::state::ClientEffect::None
    );
    assert_eq!(state.sessions().len(), 1);
}

#[test]
fn server完了後のlocal_session削除失敗でも安全状態を維持して再取得する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    apply_list_rows(
        &mut state,
        "2026-09-05",
        1,
        vec![row(TASK_ID, 0), row(OTHER_TASK_ID, 0)],
    );
    let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));

    storage.fail_work_session_writes.set(true);
    let effect = state.apply_complete_result(&storage, request_id, Ok(snapshot("2026-09-05", 2)));

    assert_list_effect(effect, "2026-09-05");
    assert!(state.is_session_committed_blocked(TASK_ID));
    assert_eq!(state.scheduled_rows().len(), 2);
}

#[test]
fn mutation前のlist応答は無視して後続再取得応答だけを適用する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let initial_rows = vec![row(TASK_ID, 0), row(OTHER_TASK_ID, 10)];
    apply_list_rows(&mut state, "2026-09-05", 1, initial_rows.clone());
    let (old_request_id, old_request) = list_effect(state.request_list("2026-09-05"));
    let (complete_request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    let (refresh_request_id, refresh_request) = list_effect(state.apply_complete_result(
        &storage,
        complete_request_id,
        Ok(snapshot("2026-09-05", 3)),
    ));

    state.apply_list_result(
        old_request_id,
        &old_request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 2),
            data: vec![row(TASK_ID, 100)],
        }),
    );
    assert_eq!(state.scheduled_rows(), initial_rows);

    let refreshed_rows = vec![row(OTHER_TASK_ID, 200)];
    state.apply_list_result(
        refresh_request_id,
        &refresh_request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 4),
            data: refreshed_rows.clone(),
        }),
    );
    assert_eq!(state.scheduled_rows(), refreshed_rows);
}

#[test]
fn 再取得snapshot後も残存sessionに応じてbufferを計算する() {
    let storage = FakeStorage::default();
    let mut state = schronu_web::client::state::load_client_state(&storage, 1_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1_000)));
    state.add_session_from_row(&storage, &row(TASK_ID, 0));
    state.add_session_from_row(&storage, &row(OTHER_TASK_ID, 0));

    let (first_refresh_id, first_request) = list_effect(state.discard_session(&storage, TASK_ID));
    state.apply_list_result(
        first_refresh_id,
        &first_request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 2_000),
            data: vec![],
        }),
    );
    state.tick(12_000);
    assert_eq!(state.display_buffer_seconds(), Some(60));

    let (second_refresh_id, second_request) =
        list_effect(state.discard_session(&storage, OTHER_TASK_ID));
    state.apply_list_result(
        second_refresh_id,
        &second_request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 13_000),
            data: vec![],
        }),
    );
    state.tick(23_000);
    assert_eq!(state.display_buffer_seconds(), Some(50));
}
