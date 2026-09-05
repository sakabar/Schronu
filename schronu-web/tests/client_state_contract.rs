use schronu_web::client::state::{load_client_state, ActiveTab, ClientEffect, ServerFailure};
use schronu_web::{web_error_codes, RecordSessionResult, RetryAdvice, SessionTask, WebSuccess};

mod client_state_support;
use client_state_support::*;

#[test]
fn 通信matrixとstorage_firstのlocal状態遷移を固定する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 1_000).unwrap();

    assert_eq!(state.active_tab(), ActiveTab::Session);
    assert_eq!(state.switch_tab(ActiveTab::List), ClientEffect::None);
    assert_eq!(state.tick(2_000), ClientEffect::None);
    assert_eq!(state.tick_now_epoch_ms(), 2_000);
    bootstrap_effect(state.request_bootstrap());
    let (_, list_request) = list_effect(state.request_list("2026-09-05"));
    assert_eq!(list_request.logical_date, "2026-09-05");
    auto_effect(state.request_auto_session());

    let task_row = row(TASK_ID, 300);
    assert_eq!(
        state.add_session_from_row(&storage, &task_row),
        ClientEffect::None
    );
    assert_eq!(state.sessions().len(), 1);
    assert_eq!(state.sessions()[0].started_at_epoch_ms, 2_000);
    assert_eq!(state.active_tab(), ActiveTab::List);
    assert_eq!(
        state.add_session_from_row(&storage, &task_row),
        ClientEffect::None
    );
    assert_eq!(state.sessions().len(), 1, "duplicate must be rejected");

    storage.fail_writes.set(true);
    assert_eq!(
        state.add_session_from_row(&storage, &row(OTHER_TASK_ID, 0)),
        ClientEffect::None
    );
    assert_eq!(
        state.sessions().len(),
        1,
        "failed write must not change memory"
    );
    assert_eq!(state.discard_session(&storage, TASK_ID), ClientEffect::None);
    assert_eq!(
        state.sessions().len(),
        1,
        "failed discard must retain memory"
    );
    storage.fail_writes.set(false);
    assert_eq!(state.discard_session(&storage, TASK_ID), ClientEffect::None);
    assert!(state.sessions().is_empty());
}

#[test]
fn snapshotとlistはlogical_date反転時にstale一覧を保持せず追加requestもしない() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 10).unwrap();
    let bootstrap_request_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_request_id, Ok(snapshot("2026-09-05", 10)));
    let (list_request_id, _) = list_effect(state.request_list("2026-09-05"));
    state.apply_list_result(
        list_request_id,
        "2026-09-05",
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 11),
            data: vec![row(TASK_ID, 0)],
        }),
    );
    assert_eq!(state.selected_logical_date(), Some("2026-09-05"));
    assert_eq!(state.scheduled_rows().len(), 1);

    let (list_request_id, _) = list_effect(state.request_list("2026-09-05"));
    let effect = state.apply_list_result(
        list_request_id,
        "2026-09-05",
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-06", 12),
            data: vec![row(OTHER_TASK_ID, 0)],
        }),
    );

    assert_eq!(effect, ClientEffect::None);
    assert_eq!(state.date_buttons()[0].logical_date, "2026-09-06");
    assert_eq!(state.selected_logical_date(), None);
    assert!(state.scheduled_rows().is_empty());
}

#[test]
fn auto_sessionはsnapshotを適用しtick開始で保存成功時だけ追加する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 5_000).unwrap();
    let selected = SessionTask {
        task_id: TASK_ID.to_owned(),
        task_name: "selected".to_owned(),
        estimated_work_seconds: 900,
        actual_work_seconds: 300,
    };

    let request_id = auto_effect(state.request_auto_session());
    state.apply_auto_session_result(
        &storage,
        request_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 1),
            data: Some(selected),
        }),
    );
    assert_eq!(state.sessions()[0].started_at_epoch_ms, 5_000);
    assert_eq!(state.sessions()[0].actual_work_seconds_at_start, 300);

    state.discard_session(&storage, TASK_ID);
    storage.fail_writes.set(true);
    let request_id = auto_effect(state.request_auto_session());
    state.apply_auto_session_result(
        &storage,
        request_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 2),
            data: Some(row(TASK_ID, 0).task),
        }),
    );
    assert!(state.sessions().is_empty());
    storage.fail_writes.set(false);
    let request_id = auto_effect(state.request_auto_session());
    state.apply_auto_session_result(
        &storage,
        request_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 3),
            data: None,
        }),
    );
    assert!(state.auto_session_empty());
}

#[test]
fn mutationは対象だけを直列化しerror助言とcommit後storage失敗を保持する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
    state.tick(62_999);

    let expected_request = schronu_web::RecordSessionRequest {
        task_id: TASK_ID.to_owned(),
        started_at_epoch_ms: 0,
        expected_actual_work_seconds: 100,
    };
    let (first_request_id, first_request) =
        record_effect(state.begin_record_session(&storage, TASK_ID));
    assert_eq!(first_request, expected_request.clone());
    assert_eq!(
        state.begin_complete_session(&storage, TASK_ID),
        ClientEffect::None
    );
    assert!(state.is_session_in_flight(TASK_ID));
    assert!(!state.is_session_in_flight(OTHER_TASK_ID));
    assert_eq!(state.discard_session(&storage, TASK_ID), ClientEffect::None);
    assert_eq!(state.sessions().len(), 2, "in-flight must disable discard");

    state.apply_record_result(
        &storage,
        first_request_id,
        Err(ServerFailure::Operation(web_error(
            "unknown_retry",
            RetryAdvice::Retry,
        ))),
    );
    assert!(!state.is_session_in_flight(TASK_ID));
    let (second_request_id, second_request) =
        record_effect(state.begin_record_session(&storage, TASK_ID));
    assert_eq!(second_request, expected_request);
    state.apply_record_result(
        &storage,
        second_request_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::ACTUAL_WORK_CONFLICT,
            RetryAdvice::ManualCheck,
        ))),
    );
    assert!(state.is_session_manual_check_blocked(TASK_ID));
    assert_eq!(
        state.begin_record_session(&storage, TASK_ID),
        ClientEffect::None
    );

    let (complete_request_id, _) =
        complete_effect(state.begin_complete_session(&storage, OTHER_TASK_ID));
    state.apply_complete_result(
        &storage,
        complete_request_id,
        Err(ServerFailure::Transport("network detail".to_owned())),
    );
    assert!(!state.display_error().unwrap().retryable());
    assert!(!state.is_session_manual_check_blocked(OTHER_TASK_ID));

    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
    state.tick(62_999);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    storage.fail_writes.set(true);
    state.apply_record_result(
        &storage,
        request_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 100),
            data: RecordSessionResult {
                actual_work_seconds: 162,
            },
        }),
    );
    assert_eq!(state.sessions().len(), 2);
    assert!(state.is_session_committed_blocked(TASK_ID));
    assert_eq!(state.display_actual_work_seconds(TASK_ID), Some(162));
    assert_eq!(
        state.begin_record_session(&storage, TASK_ID),
        ClientEffect::None
    );
    assert_eq!(
        state.begin_complete_session(&storage, TASK_ID),
        ClientEffect::None
    );
    assert_eq!(state.discard_session(&storage, TASK_ID), ClientEffect::None);
}

#[test]
fn mutation成功時だけsessionを消し履歴を最新100件へ制限する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    state.apply_record_result(
        &storage,
        request_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 1),
            data: RecordSessionResult {
                actual_work_seconds: 101,
            },
        }),
    );
    assert!(state.sessions().is_empty());

    let mut complete_state = state_with_sessions(&storage, &[OTHER_TASK_ID]);
    let (request_id, _) =
        complete_effect(complete_state.begin_complete_session(&storage, OTHER_TASK_ID));
    complete_state.apply_complete_result(&storage, request_id, Ok(snapshot("2026-09-05", 2)));
    assert!(complete_state.sessions().is_empty());

    for epoch in 0..101 {
        state.tick(epoch);
        let request_id = bootstrap_effect(state.request_bootstrap());
        state.apply_bootstrap_result(
            request_id,
            Err(ServerFailure::Operation(web_error(
                "failure",
                RetryAdvice::Retry,
            ))),
        );
    }
    assert_eq!(state.history().len(), 100);
    assert_eq!(state.history().front().unwrap().occurred_at_epoch_ms, 1);
    assert!(!state
        .history()
        .iter()
        .any(|entry| entry.summary.contains("network detail")));
}

#[test]
fn repository_state_uncertain後はpage全体のmutationを停止する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));

    state.apply_record_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_STATE_UNCERTAIN,
            RetryAdvice::ManualCheck,
        ))),
    );

    assert_eq!(
        state.begin_record_session(&storage, OTHER_TASK_ID),
        ClientEffect::None
    );
    assert_eq!(
        state.begin_complete_session(&storage, OTHER_TASK_ID),
        ClientEffect::None
    );
}

#[test]
fn repository確認buttonはglobal_block中かつ応答待ちなしの場合だけ有効になる() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
    assert!(!state.can_confirm_repository_checked());

    let (first_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    let (second_id, _) = record_effect(state.begin_record_session(&storage, OTHER_TASK_ID));
    state.apply_record_result(
        &storage,
        first_id,
        Err(ServerFailure::Transport("detail".to_owned())),
    );
    assert!(state.mutation_globally_blocked());
    assert!(!state.can_confirm_repository_checked());

    state.apply_record_result(
        &storage,
        second_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_SAVE_FAILED,
            RetryAdvice::Retry,
        ))),
    );
    assert!(state.can_confirm_repository_checked());
}

#[test]
fn repository_state_uncertainのblockは別keyへ保存しreload後も復元する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    state.apply_record_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_STATE_UNCERTAIN,
            RetryAdvice::ManualCheck,
        ))),
    );

    let mut restored = load_client_state(&storage, 0).unwrap();
    assert_eq!(
        restored.begin_record_session(&storage, OTHER_TASK_ID),
        ClientEffect::None
    );

    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    storage.fail_writes.set(true);
    state.apply_record_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_STATE_UNCERTAIN,
            RetryAdvice::ManualCheck,
        ))),
    );
    assert_eq!(
        state.begin_record_session(&storage, OTHER_TASK_ID),
        ClientEffect::None
    );
}

#[test]
fn 古いmutation応答は同じuuidの新しいsessionへ作用しない() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let first_request_id = match state.begin_record_session(&storage, TASK_ID) {
        ClientEffect::RecordSession { request_id, .. } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };
    state.apply_record_result(
        &storage,
        first_request_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_SAVE_FAILED,
            RetryAdvice::Retry,
        ))),
    );
    state.discard_session(&storage, TASK_ID);
    state.add_session_from_row(&storage, &row(TASK_ID, 200));
    let second_request_id = match state.begin_record_session(&storage, TASK_ID) {
        ClientEffect::RecordSession { request_id, .. } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };

    state.apply_record_result(
        &storage,
        first_request_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 10),
            data: RecordSessionResult {
                actual_work_seconds: 101,
            },
        }),
    );

    assert_eq!(state.sessions().len(), 1);
    assert_eq!(state.sessions()[0].actual_work_seconds_at_start, 200);
    assert!(state.is_session_in_flight(TASK_ID));
    assert_ne!(first_request_id, second_request_id);
}
