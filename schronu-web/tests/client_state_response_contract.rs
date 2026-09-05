use schronu_web::client::state::{
    load_client_state, ClientEffect, DisplayError, Locality, Operation, Outcome, ServerFailure,
};
use schronu_web::{web_error_codes, RecordSessionResult, RetryAdvice, WebSuccess};

mod client_state_support;
use client_state_support::*;

#[test]
fn 逆順のlist応答と古いsnapshotは最新表示を巻き戻さない() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 50)));
    let first_request_id = match state.request_list("2026-09-05") {
        ClientEffect::ListTasks { request_id, .. } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };
    let second_request_id = match state.request_list("2026-09-06") {
        ClientEffect::ListTasks { request_id, .. } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };

    state.apply_list_result(
        second_request_id,
        "2026-09-06",
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 200),
            data: vec![row(OTHER_TASK_ID, 0)],
        }),
    );
    state.apply_list_result(
        first_request_id,
        "2026-09-05",
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 100),
            data: vec![row(TASK_ID, 0)],
        }),
    );

    assert_eq!(state.snapshot().unwrap().observed_at_epoch_ms, 200);
    assert_eq!(state.snapshot().unwrap().logical_date, "2026-09-05");
    assert_eq!(state.selected_logical_date(), Some("2026-09-06"));
    assert_eq!(state.scheduled_rows()[0].task.task_id, OTHER_TASK_ID);
    assert_eq!(
        state
            .history()
            .iter()
            .filter(|entry| entry.operation == Operation::ListTasks)
            .count(),
        2,
        "stale responseも受信履歴へ残す"
    );
}

#[test]
fn 未知のoperation_errorはcodeと助言を失わず表示状態へ保持する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let request_id = bootstrap_effect(state.request_bootstrap());
    let error = web_error("future_error", RetryAdvice::ManualCheck);

    state.apply_bootstrap_result(request_id, Err(ServerFailure::Operation(error.clone())));

    assert_eq!(
        state.display_error(),
        Some(&DisplayError::Operation {
            error,
            operation: Operation::Bootstrap,
            task_id: None,
        })
    );
}

#[test]
fn manual_check_blockはsession破棄成功時だけ解消する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    state.apply_record_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::ACTUAL_WORK_CONFLICT,
            RetryAdvice::ManualCheck,
        ))),
    );

    storage.fail_writes.set(true);
    state.discard_session(&storage, TASK_ID);
    assert!(state.is_session_manual_check_blocked(TASK_ID));

    storage.fail_writes.set(false);
    state.discard_session(&storage, TASK_ID);
    state.add_session_from_row(&storage, &row(TASK_ID, 0));
    assert!(matches!(
        state.begin_record_session(&storage, TASK_ID),
        ClientEffect::RecordSession { .. }
    ));
}

#[test]
fn server_commit後のlocal削除失敗はserver成功とlocal失敗を別々に記録する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    storage.fail_writes.set(true);

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

    let entries: Vec<_> = state.history().iter().rev().take(2).collect();
    assert_eq!(entries[1].operation, Operation::RecordSession);
    assert_eq!(entries[1].locality, Locality::Server);
    assert_eq!(entries[1].outcome, Outcome::Success);
    assert_eq!(entries[0].operation, Operation::DiscardSession);
    assert_eq!(entries[0].locality, Locality::Local);
    assert_eq!(entries[0].outcome, Outcome::Failure);
}

#[test]
fn 起動時のstorage警告と書込停止をclient状態から参照できる() {
    let storage = FakeStorage::default();
    *storage.value.borrow_mut() = Some("{broken".to_owned());
    let state = load_client_state(&storage, 0).unwrap();

    assert!(!state.storage_warnings().is_empty());
    assert!(state.storage_write_blocked());
}

#[test]
fn 後続操作の成功は以前の表示errorを解消する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let request_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(
        request_id,
        Err(ServerFailure::Transport("detail".to_owned())),
    );
    assert!(state.display_error().is_some());

    let request_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(request_id, Ok(snapshot("2026-09-05", 1)));

    assert_eq!(state.display_error(), None);
}

#[test]
fn 未来日の一覧は今日のsnapshotと共に適用する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1)));
    let (list_id, _) = list_effect(state.request_list("2026-09-06"));

    state.apply_list_result(
        list_id,
        "2026-09-06",
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 2),
            data: vec![row(TASK_ID, 0)],
        }),
    );

    assert_eq!(state.selected_logical_date(), Some("2026-09-06"));
    assert_eq!(state.scheduled_rows()[0].task.task_id, TASK_ID);
}

#[test]
fn auto_sessionは古いsnapshotを無視してtask_payloadを適用する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 5_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 200)));
    let auto_id = auto_effect(state.request_auto_session());

    state.apply_auto_session_result(
        &storage,
        auto_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 100),
            data: Some(row(TASK_ID, 0).task),
        }),
    );

    assert_eq!(state.snapshot().unwrap().observed_at_epoch_ms, 200);
    assert_eq!(state.sessions()[0].task_id, TASK_ID);
    assert_eq!(
        state
            .history()
            .iter()
            .filter(|entry| entry.operation == Operation::AutoSession)
            .count(),
        1
    );
}

#[test]
fn 未解決の手動確認errorは無関係な成功で解消しない() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (record_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    storage.fail_writes.set(true);
    state.apply_record_result(
        &storage,
        record_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 1),
            data: RecordSessionResult {
                actual_work_seconds: 101,
            },
        }),
    );
    let committed_error = state.display_error().cloned();
    storage.fail_writes.set(false);
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 2)));
    assert_eq!(state.display_error(), committed_error.as_ref());

    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (record_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    state.apply_record_result(
        &storage,
        record_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::ACTUAL_WORK_CONFLICT,
            RetryAdvice::ManualCheck,
        ))),
    );
    let manual_error = state.display_error().cloned();
    state.add_session_from_row(&storage, &row(OTHER_TASK_ID, 0));
    assert_eq!(state.display_error(), manual_error.as_ref());
}

#[test]
fn session重複追加と不存在破棄はstorage履歴へ偽装しない() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let history_len = state.history().len();

    state.add_session_from_row(&storage, &row(TASK_ID, 0));
    state.discard_session(&storage, OTHER_TASK_ID);

    assert_eq!(state.history().len(), history_len);
    assert_eq!(state.display_error(), None);
}

#[test]
fn latest_readの古いsnapshotも受信履歴へ記録する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    let list_id = list_effect(state.request_list("2026-09-05")).0;
    state.apply_list_result(
        list_id,
        "2026-09-05",
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 200),
            data: vec![],
        }),
    );

    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 100)));

    assert_eq!(
        state
            .history()
            .iter()
            .filter(|entry| entry.operation == Operation::Bootstrap)
            .count(),
        1
    );
}

#[test]
fn latest_listは古い同一logical_dateのsnapshotだけ無視してrowsを適用する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 200)));
    let list_id = list_effect(state.request_list("2026-09-06")).0;

    state.apply_list_result(
        list_id,
        "2026-09-06",
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 100),
            data: vec![row(TASK_ID, 0)],
        }),
    );

    assert_eq!(state.snapshot().unwrap().observed_at_epoch_ms, 200);
    assert_eq!(state.selected_logical_date(), Some("2026-09-06"));
    assert_eq!(state.scheduled_rows()[0].task.task_id, TASK_ID);
}

#[test]
fn task_scoped_retry_errorは無関係なlocal成功で解消しない() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    state.apply_record_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_SAVE_FAILED,
            RetryAdvice::Retry,
        ))),
    );
    let retry_error = state.display_error().cloned();

    state.add_session_from_row(&storage, &row(OTHER_TASK_ID, 0));

    assert_eq!(state.display_error(), retry_error.as_ref());
}

#[test]
fn local_storage_errorはserver成功で解消しない() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    storage.fail_writes.set(true);
    state.add_session_from_row(&storage, &row(TASK_ID, 0));
    let storage_error = state.display_error().cloned();
    storage.fail_writes.set(false);

    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1)));

    assert_eq!(state.display_error(), storage_error.as_ref());
}

#[test]
fn mutationはsafety_marker保存後だけ送信し未応答reloadをblockする() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);

    storage.fail_writes.set(true);
    assert_eq!(
        state.begin_record_session(&storage, TASK_ID),
        ClientEffect::None
    );
    assert!(!state.is_session_in_flight(TASK_ID));

    storage.fail_writes.set(false);
    assert!(matches!(
        state.begin_record_session(&storage, TASK_ID),
        ClientEffect::RecordSession { .. }
    ));
    let mut restored = load_client_state(&storage, 0).unwrap();
    assert_eq!(
        restored.begin_record_session(&storage, OTHER_TASK_ID),
        ClientEffect::None
    );
}

#[test]
fn 復元したsafety_blockは診断でき手動確認成功時だけ解除する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    record_effect(state.begin_record_session(&storage, TASK_ID));
    let mut restored = load_client_state(&storage, 0).unwrap();
    assert!(restored.mutation_globally_blocked());
    assert!(restored.mutation_safety_warning().is_some());

    storage.fail_writes.set(true);
    assert_eq!(
        restored.confirm_repository_checked(&storage),
        ClientEffect::None
    );
    assert!(restored.mutation_globally_blocked());

    storage.fail_writes.set(false);
    restored.confirm_repository_checked(&storage);
    assert!(!restored.mutation_globally_blocked());
    let mut reloaded = load_client_state(&storage, 0).unwrap();
    assert!(matches!(
        reloaded.begin_record_session(&storage, TASK_ID),
        ClientEffect::RecordSession { .. }
    ));
}

#[test]
fn safety_markerは全mutationの確定応答後だけ自動解除する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
    let (first_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    let (second_id, _) = record_effect(state.begin_record_session(&storage, OTHER_TASK_ID));

    state.apply_record_result(
        &storage,
        first_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_SAVE_FAILED,
            RetryAdvice::Retry,
        ))),
    );
    assert!(load_client_state(&storage, 0)
        .unwrap()
        .mutation_globally_blocked());

    state.apply_record_result(
        &storage,
        second_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_SAVE_FAILED,
            RetryAdvice::Retry,
        ))),
    );
    assert!(!load_client_state(&storage, 0)
        .unwrap()
        .mutation_globally_blocked());
}

#[test]
fn transport不確実性は並行mutation完了後もglobal_blockを維持する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
    let (first_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    let (second_id, _) = record_effect(state.begin_record_session(&storage, OTHER_TASK_ID));

    state.apply_record_result(
        &storage,
        first_id,
        Err(ServerFailure::Transport("detail".to_owned())),
    );
    assert!(state.mutation_globally_blocked());
    state.apply_record_result(
        &storage,
        second_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_SAVE_FAILED,
            RetryAdvice::Retry,
        ))),
    );

    let mut restored = load_client_state(&storage, 0).unwrap();
    assert!(restored.mutation_globally_blocked());
    assert_eq!(
        restored.begin_record_session(&storage, TASK_ID),
        ClientEffect::None
    );
}

#[test]
fn server成功後のsession削除失敗はsafety_markerを解除しない() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    storage.fail_work_session_writes.set(true);

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

    let mut restored = load_client_state(&storage, 0).unwrap();
    assert!(restored.mutation_globally_blocked());
    assert_eq!(
        restored.begin_record_session(&storage, TASK_ID),
        ClientEffect::None
    );
}

#[test]
fn repository確認時はcommit済みsessionを永続層から除去してからmarkerを解除する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    storage.fail_work_session_writes.set(true);
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

    storage.fail_work_session_writes.set(false);
    state.confirm_repository_checked(&storage);
    let mut restored = load_client_state(&storage, 0).unwrap();

    assert!(restored.sessions().is_empty());
    assert!(!restored.mutation_globally_blocked());
    assert_eq!(
        restored.begin_record_session(&storage, TASK_ID),
        ClientEffect::None
    );
}

#[test]
fn read_manual_errorは同じoperation成功時に解消する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let first_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(
        first_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::INVALID_INPUT,
            RetryAdvice::ManualCheck,
        ))),
    );
    let second_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(second_id, Ok(snapshot("2026-09-05", 1)));

    assert_eq!(state.display_error(), None);
}

#[test]
fn repository確認後はuncertain由来のtask_blockも残さない() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    state.apply_record_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_STATE_UNCERTAIN,
            RetryAdvice::ManualCheck,
        ))),
    );
    state.confirm_repository_checked(&storage);

    assert!(matches!(
        state.begin_record_session(&storage, TASK_ID),
        ClientEffect::RecordSession { .. }
    ));
}
