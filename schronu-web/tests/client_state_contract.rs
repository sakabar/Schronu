use schronu_web::client::state::{
    ActiveTab, ClientEffect, ClientState, DisplayError, Locality, Operation, Outcome, ServerFailure,
};
use schronu_web::client::work_sessions::{
    load_work_sessions, KeyValueStorage, StorageError, WorkSession, WORK_SESSIONS_STORAGE_KEY,
};
use schronu_web::{
    web_error_codes, RecordSessionResult, RetryAdvice, ScheduledTaskRow, ServerSnapshot,
    SessionTask, WebError, WebSuccess,
};
use std::cell::{Cell, RefCell};

const TASK_ID: &str = "00000000-0000-4000-8000-000000000001";
const OTHER_TASK_ID: &str = "00000000-0000-4000-8000-000000000002";

#[test]
fn 通信matrixとstorage_firstのlocal状態遷移を固定する() {
    let storage = FakeStorage::default();
    let sessions = load_work_sessions(&storage).unwrap();
    let mut state = ClientState::new(sessions, 1_000);

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
    let mut state = ClientState::new(load_work_sessions(&storage).unwrap(), 10);
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
    let mut state = ClientState::new(load_work_sessions(&storage).unwrap(), 5_000);
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
    let (first_request_id, first_request) = record_effect(state.begin_record_session(TASK_ID));
    assert_eq!(first_request, expected_request.clone());
    assert_eq!(state.begin_complete_session(TASK_ID), ClientEffect::None);
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
    let (second_request_id, second_request) = record_effect(state.begin_record_session(TASK_ID));
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
    assert_eq!(state.begin_record_session(TASK_ID), ClientEffect::None);

    let (complete_request_id, _) = complete_effect(state.begin_complete_session(OTHER_TASK_ID));
    state.apply_complete_result(
        &storage,
        complete_request_id,
        Err(ServerFailure::Transport("network detail".to_owned())),
    );
    assert!(state.display_error().unwrap().retryable());
    assert!(!state.is_session_manual_check_blocked(OTHER_TASK_ID));

    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
    state.tick(62_999);
    let (request_id, _) = record_effect(state.begin_record_session(TASK_ID));
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
    assert_eq!(state.begin_record_session(TASK_ID), ClientEffect::None);
    assert_eq!(state.begin_complete_session(TASK_ID), ClientEffect::None);
    assert_eq!(state.discard_session(&storage, TASK_ID), ClientEffect::None);
}

#[test]
fn mutation成功時だけsessionを消し履歴を最新100件へ制限する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(TASK_ID));
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
    let (request_id, _) = complete_effect(complete_state.begin_complete_session(OTHER_TASK_ID));
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
    let (request_id, _) = record_effect(state.begin_record_session(TASK_ID));

    state.apply_record_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_STATE_UNCERTAIN,
            RetryAdvice::ManualCheck,
        ))),
    );

    assert_eq!(
        state.begin_record_session(OTHER_TASK_ID),
        ClientEffect::None
    );
    assert_eq!(
        state.begin_complete_session(OTHER_TASK_ID),
        ClientEffect::None
    );
}

#[test]
fn 古いmutation応答は同じuuidの新しいsessionへ作用しない() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let first_request_id = match state.begin_record_session(TASK_ID) {
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
    let second_request_id = match state.begin_record_session(TASK_ID) {
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

#[test]
fn 逆順のlist応答と古いsnapshotは最新表示を巻き戻さない() {
    let storage = FakeStorage::default();
    let mut state = ClientState::new(load_work_sessions(&storage).unwrap(), 0);
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
            snapshot: snapshot("2026-09-06", 200),
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
    assert_eq!(state.snapshot().unwrap().logical_date, "2026-09-06");
    assert_eq!(state.scheduled_rows()[0].task.task_id, OTHER_TASK_ID);
}

#[test]
fn 未知のoperation_errorはcodeと助言を失わず表示状態へ保持する() {
    let storage = FakeStorage::default();
    let mut state = ClientState::new(load_work_sessions(&storage).unwrap(), 0);
    let request_id = bootstrap_effect(state.request_bootstrap());
    let error = web_error("future_error", RetryAdvice::ManualCheck);

    state.apply_bootstrap_result(request_id, Err(ServerFailure::Operation(error.clone())));

    assert_eq!(state.display_error(), Some(&DisplayError::Operation(error)));
}

#[test]
fn manual_check_blockはsession破棄成功時だけ解消する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(TASK_ID));
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
        state.begin_record_session(TASK_ID),
        ClientEffect::RecordSession { .. }
    ));
}

#[test]
fn server_commit後のlocal削除失敗はserver成功とlocal失敗を別々に記録する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (request_id, _) = record_effect(state.begin_record_session(TASK_ID));
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

#[derive(Default)]
struct FakeStorage {
    value: RefCell<Option<String>>,
    fail_writes: Cell<bool>,
}

impl KeyValueStorage for FakeStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        assert_eq!(key, WORK_SESSIONS_STORAGE_KEY);
        Ok(self.value.borrow().clone())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        assert_eq!(key, WORK_SESSIONS_STORAGE_KEY);
        if self.fail_writes.get() {
            return Err(StorageError::WriteFailed);
        }
        *self.value.borrow_mut() = Some(value.to_owned());
        Ok(())
    }
}

fn state_with_sessions(storage: &FakeStorage, ids: &[&str]) -> ClientState {
    let mut sessions = load_work_sessions(storage).unwrap();
    sessions
        .replace_sessions(
            storage,
            ids.iter()
                .map(|id| WorkSession {
                    task_id: (*id).to_owned(),
                    task_name: "task".to_owned(),
                    started_at_epoch_ms: 0,
                    estimated_work_seconds_at_start: 900,
                    actual_work_seconds_at_start: 100,
                })
                .collect(),
        )
        .unwrap();
    ClientState::new(sessions, 0)
}

fn snapshot(logical_date: &str, observed_at_epoch_ms: i64) -> ServerSnapshot {
    ServerSnapshot {
        observed_at_epoch_ms,
        logical_date: logical_date.to_owned(),
        buffer_seconds: 60,
    }
}

fn row(task_id: &str, actual_work_seconds: i64) -> ScheduledTaskRow {
    ScheduledTaskRow {
        task: SessionTask {
            task_id: task_id.to_owned(),
            task_name: "task".to_owned(),
            estimated_work_seconds: 900,
            actual_work_seconds,
        },
        schedule_start_epoch_ms: 0,
        schedule_end_epoch_ms: 1,
        deadline_epoch_ms: None,
        is_leaf: true,
    }
}

fn web_error(code: &str, retry_advice: RetryAdvice) -> WebError {
    WebError {
        code: code.to_owned(),
        message: "safe".to_owned(),
        retry_advice,
    }
}

fn record_effect(effect: ClientEffect) -> (u64, schronu_web::RecordSessionRequest) {
    match effect {
        ClientEffect::RecordSession {
            request_id,
            request,
        } => (request_id, request),
        other => panic!("unexpected effect: {other:?}"),
    }
}

fn complete_effect(effect: ClientEffect) -> (u64, schronu_web::RecordSessionRequest) {
    match effect {
        ClientEffect::CompleteSession {
            request_id,
            request,
        } => (request_id, request),
        other => panic!("unexpected effect: {other:?}"),
    }
}

fn bootstrap_effect(effect: ClientEffect) -> u64 {
    match effect {
        ClientEffect::Bootstrap { request_id } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    }
}

fn list_effect(effect: ClientEffect) -> (u64, schronu_web::ListTasksRequest) {
    match effect {
        ClientEffect::ListTasks {
            request_id,
            request,
        } => (request_id, request),
        other => panic!("unexpected effect: {other:?}"),
    }
}

fn auto_effect(effect: ClientEffect) -> u64 {
    match effect {
        ClientEffect::AutoSession { request_id } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    }
}
