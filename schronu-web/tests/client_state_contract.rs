use chrono::{Local, TimeZone};
use schronu_web::client::state::{
    load_client_state, ActiveTab, ClientEffect, Operation, Outcome, ServerFailure,
};
use schronu_web::client::view_projection::project_session_cards;
use schronu_web::client::work_sessions::{load_work_sessions, WorkSession};
use schronu_web::{web_error_codes, RecordSessionResult, RetryAdvice, SessionTask, WebSuccess};

mod client_state_support;
use client_state_support::*;

#[test]
fn 通信matrixとstorage_firstのlocal状態遷移を固定する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 1_000).unwrap();

    assert_eq!(state.active_tab(), ActiveTab::Session);
    assert_eq!(state.switch_tab(ActiveTab::List), ClientEffect::None);
    assert_eq!(state.switch_tab(ActiveTab::History), ClientEffect::None);
    assert_eq!(state.active_tab(), ActiveTab::History);
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
    assert!(
        state.history().is_empty(),
        "localStorage操作は発火履歴へ記録しない"
    );
}

#[test]
fn rank非0の一覧taskは手動sessionへ追加しない() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 2_000).unwrap();
    let mut task_row = row(TASK_ID, 300);
    task_row.is_leaf = false;

    assert_eq!(
        state.add_session_from_row(&storage, &task_row),
        ClientEffect::None
    );

    assert!(state.sessions().is_empty());
    assert!(storage.value.borrow().is_none());
    assert!(state.history().is_empty());
}

#[test]
fn bufferは成功したsession破棄で未作業時間を再計算する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 1_000_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1_000_000)));

    state.tick(1_010_000);
    assert_eq!(
        state.add_session_from_row(&storage, &row(TASK_ID, 0)),
        ClientEffect::None
    );
    state.tick(1_020_000);
    assert_eq!(
        state.add_session_from_row(&storage, &row(OTHER_TASK_ID, 0)),
        ClientEffect::None
    );
    state.tick(1_040_000);
    assert_eq!(state.display_buffer_seconds(), Some(50));

    storage.fail_writes.set(true);
    assert_eq!(state.discard_session(&storage, TASK_ID), ClientEffect::None);
    assert_eq!(state.sessions().len(), 2);
    assert_eq!(state.display_buffer_seconds(), Some(50));

    storage.fail_writes.set(false);
    assert!(matches!(
        state.discard_session(&storage, TASK_ID),
        ClientEffect::ListTasks { .. }
    ));
    assert_eq!(state.display_buffer_seconds(), Some(40));

    assert!(matches!(
        state.discard_session(&storage, OTHER_TASK_ID),
        ClientEffect::ListTasks { .. }
    ));
    assert_eq!(state.display_buffer_seconds(), Some(20));
}

#[test]
fn bufferはsessionの見積到達後に減算を再開する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 0)));
    state.add_session_from_row(&storage, &row(TASK_ID, 100));

    state.tick(800_000);
    assert_eq!(state.display_buffer_seconds(), Some(60));
    state.tick(800_999);
    assert_eq!(state.display_buffer_seconds(), Some(60));
    state.tick(801_000);
    assert_eq!(state.display_buffer_seconds(), Some(59));
}

#[test]
fn bufferは全sessionの見積到達後に実時間と同速で減算する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 0)));
    state.add_session_from_row(&storage, &row(TASK_ID, 800));
    state.tick(50_000);
    state.add_session_from_row(&storage, &row(OTHER_TASK_ID, 800));

    state.tick(120_000);
    assert_eq!(state.display_buffer_seconds(), Some(60));
    state.tick(160_000);
    assert_eq!(state.display_buffer_seconds(), Some(50));
}

#[test]
fn bufferは見積到達済みsessionの開始直後から減算する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 0)));
    state.add_session_from_row(&storage, &row(TASK_ID, 900));

    state.tick(1_000);
    assert_eq!(state.display_buffer_seconds(), Some(59));
}

#[test]
fn restored_sessionの復元補正とsnapshot後の停止は見積到達時刻で打ち切る() {
    let storage = FakeStorage::default();
    let mut sessions = load_work_sessions(&storage).unwrap();
    sessions
        .replace_sessions(
            &storage,
            vec![WorkSession {
                task_id: TASK_ID.to_owned(),
                task_name: "task".to_owned(),
                started_at_epoch_ms: 0,
                estimated_work_seconds_at_start: 30,
                actual_work_seconds_at_start: 0,
            }],
        )
        .unwrap();
    let mut state = load_client_state(&storage, 20_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 20_000)));

    state.tick(35_000);
    assert_eq!(state.display_buffer_seconds(), Some(35));

    let (request_id, request) = list_effect(state.request_list("2026-09-05"));
    state.apply_list_result(
        request_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 40_000),
            data: Vec::new(),
        }),
    );
    state.tick(45_000);
    assert_eq!(state.display_buffer_seconds(), Some(25));
}

#[test]
fn restored_sessionの継続時間をserver_bufferから1回だけ差し引く() {
    let storage = FakeStorage::default();
    let mut sessions = load_work_sessions(&storage).unwrap();
    sessions
        .replace_sessions(
            &storage,
            vec![
                WorkSession {
                    task_id: TASK_ID.to_owned(),
                    task_name: "first".to_owned(),
                    started_at_epoch_ms: 1_000_000,
                    estimated_work_seconds_at_start: 900,
                    actual_work_seconds_at_start: 0,
                },
                WorkSession {
                    task_id: OTHER_TASK_ID.to_owned(),
                    task_name: "second".to_owned(),
                    started_at_epoch_ms: 1_010_000,
                    estimated_work_seconds_at_start: 900,
                    actual_work_seconds_at_start: 0,
                },
            ],
        )
        .unwrap();

    let mut state = load_client_state(&storage, 1_040_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1_030_000)));

    assert_eq!(state.display_buffer_seconds(), Some(30));
    state.discard_session(&storage, TASK_ID);
    assert_eq!(state.display_buffer_seconds(), Some(40));
    state.discard_session(&storage, OTHER_TASK_ID);
    assert_eq!(state.display_buffer_seconds(), Some(50));

    state.add_session_from_row(&storage, &row(TASK_ID, 0));
    state.tick(1_060_000);
    let (request_id, request) = list_effect(state.request_list("2026-09-05"));
    state.apply_list_result(
        request_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 1_050_000),
            data: Vec::new(),
        }),
    );
    assert_eq!(state.display_buffer_seconds(), Some(60));
}

#[test]
fn restored_sessionの復元補正は終了click時刻で打ち切る() {
    let storage = FakeStorage::default();
    let mut sessions = load_work_sessions(&storage).unwrap();
    sessions
        .replace_sessions(
            &storage,
            vec![WorkSession {
                task_id: TASK_ID.to_owned(),
                task_name: "task".to_owned(),
                started_at_epoch_ms: 1_000_000,
                estimated_work_seconds_at_start: 900,
                actual_work_seconds_at_start: 0,
            }],
        )
        .unwrap();
    let mut state = load_client_state(&storage, 1_020_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1_020_000)));

    state.tick(1_030_000);
    record_effect(state.begin_record_session(&storage, TASK_ID));
    state.tick(1_060_000);
    let (request_id, request) = list_effect(state.request_list("2026-09-05"));
    state.apply_list_result(
        request_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 1_050_000),
            data: Vec::new(),
        }),
    );

    assert_eq!(state.display_buffer_seconds(), Some(20));
}

#[test]
fn 終了処理中はclick時刻を保持してbufferを再開し失敗時に計測へ戻す() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 0)));
    state.tick(60_000);

    let (request_id, request) = record_effect(state.begin_record_session(&storage, TASK_ID));
    assert_eq!(request.ended_at_epoch_ms, Some(60_000));

    state.tick(65_000);
    assert_eq!(state.display_buffer_seconds(), Some(55));

    state.apply_record_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_SAVE_FAILED,
            RetryAdvice::Retry,
        ))),
    );
    assert_eq!(state.display_buffer_seconds(), Some(60));
}

#[test]
fn 三終了操作は同じclick時刻をrequestへ保持する() {
    let record_storage = FakeStorage::default();
    let mut record_state = state_with_sessions(&record_storage, &[TASK_ID]);
    record_state.tick(60_000);
    let (_, record) = record_effect(record_state.begin_record_session(&record_storage, TASK_ID));

    let complete_storage = FakeStorage::default();
    let mut complete_state = state_with_sessions(&complete_storage, &[TASK_ID]);
    complete_state.tick(60_000);
    let (_, complete) =
        complete_effect(complete_state.begin_complete_session(&complete_storage, TASK_ID));

    let discard_storage = FakeStorage::default();
    let mut discard_state = state_with_sessions(&discard_storage, &[TASK_ID]);
    discard_state.tick(60_000);
    let (_, discard_complete) = complete_effect(
        discard_state.begin_complete_session_without_recording(&discard_storage, TASK_ID),
    );

    assert_eq!(record.ended_at_epoch_ms, Some(60_000));
    assert_eq!(complete.ended_at_epoch_ms, Some(60_000));
    assert_eq!(discard_complete.ended_at_epoch_ms, Some(60_000));
}

#[test]
fn 別sessionが計測中なら終了処理中もbufferを停止する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 0)));
    state.tick(60_000);
    record_effect(state.begin_record_session(&storage, TASK_ID));

    state.tick(65_000);

    assert_eq!(state.display_buffer_seconds(), Some(60));
}

#[test]
fn server_commit済みでlocal削除失敗したsessionはbufferを停止しない() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 0)));

    state.tick(60_000);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    storage.fail_work_session_writes.set(true);
    state.apply_record_result(
        &storage,
        request_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 60_000),
            data: RecordSessionResult {
                actual_work_seconds: 160,
            },
        }),
    );
    state.tick(90_000);

    assert!(state.is_session_committed_blocked(TASK_ID));
    assert_eq!(state.display_buffer_seconds(), Some(30));
}

#[test]
fn server_commit済みsessionはreload後もbuffer補正と再送の対象外にする() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 0)));

    state.tick(60_000);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    storage.fail_work_session_writes.set(true);
    state.apply_record_result(
        &storage,
        request_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 60_000),
            data: RecordSessionResult {
                actual_work_seconds: 160,
            },
        }),
    );

    let mut restored = load_client_state(&storage, 90_000).unwrap();
    let bootstrap_id = bootstrap_effect(restored.request_bootstrap());
    restored.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 60_000)));

    assert!(restored.is_session_committed_blocked(TASK_ID));
    assert_eq!(restored.display_buffer_seconds(), Some(30));
    assert_eq!(
        restored.begin_record_session(&storage, TASK_ID),
        ClientEffect::None
    );

    storage.fail_work_session_writes.set(false);
    restored.confirm_repository_checked(&storage);
    assert!(restored.sessions().is_empty());
    assert!(!restored.mutation_globally_blocked());
}

#[test]
fn server_commit済みidを永続化できなければlocal_sessionを削除しない() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    state.tick(60_000);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));

    storage.fail_safety_writes.set(true);
    state.apply_record_result(
        &storage,
        request_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 60_000),
            data: RecordSessionResult {
                actual_work_seconds: 160,
            },
        }),
    );

    assert_eq!(state.sessions().len(), 1);
    assert!(state.is_session_committed_blocked(TASK_ID));

    storage.fail_safety_writes.set(false);
    state.confirm_repository_checked(&storage);
    assert!(state.sessions().is_empty());
    assert!(!state.mutation_globally_blocked());
}

#[test]
fn active_session中の新しいsnapshotを新たなbuffer基準にする() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 1_000_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1_000_000)));

    state.tick(1_010_000);
    state.add_session_from_row(&storage, &row(TASK_ID, 0));
    state.tick(1_030_000);
    let (request_id, request) = list_effect(state.request_list("2026-09-05"));
    state.apply_list_result(
        request_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: schronu_web::ServerSnapshot {
                observed_at_epoch_ms: 1_030_000,
                logical_date: "2026-09-05".to_owned(),
                buffer_seconds: 30,
            },
            data: Vec::new(),
        }),
    );
    state.tick(1_040_000);

    assert_eq!(state.display_buffer_seconds(), Some(30));
    state.discard_session(&storage, TASK_ID);
    assert_eq!(state.display_buffer_seconds(), Some(20));
}

#[test]
fn active_session中にbusy_timeを跨いだsnapshotは壁時計時間をcreditしない() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 1_000_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1_000_000)));

    state.tick(1_010_000);
    state.add_session_from_row(&storage, &row(TASK_ID, 0));
    state.tick(1_030_000);
    let (request_id, request) = list_effect(state.request_list("2026-09-05"));
    state.apply_list_result(
        request_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: schronu_web::ServerSnapshot {
                observed_at_epoch_ms: 1_030_000,
                logical_date: "2026-09-05".to_owned(),
                buffer_seconds: 50,
            },
            data: Vec::new(),
        }),
    );
    state.tick(1_040_000);

    assert_eq!(state.display_buffer_seconds(), Some(50));
}

#[test]
fn 複数session中の記録snapshotを新たなbuffer基準にする() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 1_000_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1_000_000)));

    state.tick(1_010_000);
    state.add_session_from_row(&storage, &row(OTHER_TASK_ID, 0));
    state.tick(1_020_000);
    state.add_session_from_row(&storage, &row(TASK_ID, 0));
    state.tick(1_030_000);
    let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    state.apply_record_result(
        &storage,
        request_id,
        Ok(WebSuccess {
            snapshot: schronu_web::ServerSnapshot {
                observed_at_epoch_ms: 1_030_000,
                logical_date: "2026-09-05".to_owned(),
                buffer_seconds: 40,
            },
            data: RecordSessionResult {
                actual_work_seconds: 110,
            },
        }),
    );
    state.tick(1_040_000);

    assert_eq!(state.display_buffer_seconds(), Some(40));
    state.discard_session(&storage, OTHER_TASK_ID);
    assert_eq!(state.display_buffer_seconds(), Some(30));
}

#[test]
fn logical_date変更snapshotを新たなbuffer基準にする() {
    let boundary = Local
        .with_ymd_and_hms(2026, 9, 6, 6, 0, 0)
        .single()
        .expect("06:00 must be an unambiguous local time")
        .timestamp_millis();
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, boundary - 10_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(
        bootstrap_id,
        Ok(schronu_web::ServerSnapshot {
            observed_at_epoch_ms: boundary - 10_000,
            logical_date: "2026-09-05".to_owned(),
            buffer_seconds: 60,
        }),
    );

    state.tick(boundary - 5_000);
    state.add_session_from_row(&storage, &row(TASK_ID, 0));
    state.tick(boundary + 10_000);
    let (request_id, request) = list_effect(state.request_list("2026-09-06"));
    state.apply_list_result(
        request_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: schronu_web::ServerSnapshot {
                observed_at_epoch_ms: boundary + 10_000,
                logical_date: "2026-09-06".to_owned(),
                buffer_seconds: 50,
            },
            data: Vec::new(),
        }),
    );
    state.tick(boundary + 20_000);

    assert_eq!(state.display_buffer_seconds(), Some(50));
    state.discard_session(&storage, TASK_ID);
    assert_eq!(state.display_buffer_seconds(), Some(40));
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
        ended_at_epoch_ms: Some(62_999),
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
        Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
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
fn 完了effectは計測の記録方針と履歴種別を保持する() {
    let recording_storage = FakeStorage::default();
    let mut recording_state = state_with_sessions(&recording_storage, &[TASK_ID]);
    let (_, recording_request) =
        complete_effect(recording_state.begin_complete_session(&recording_storage, TASK_ID));
    assert!(recording_request.record_elapsed_seconds);

    let discard_storage = FakeStorage::default();
    let mut discard_state = state_with_sessions(&discard_storage, &[TASK_ID]);
    let (request_id, discard_request) = complete_effect(
        discard_state.begin_complete_session_without_recording(&discard_storage, TASK_ID),
    );
    assert!(!discard_request.record_elapsed_seconds);

    discard_state.apply_complete_result(
        &discard_storage,
        request_id,
        Ok(snapshot("2026-09-05", 1)),
    );
    assert!(discard_state.history().iter().any(|entry| {
        entry.invocation.operation() == Operation::CompleteSessionWithoutRecording
    }));
    assert!(discard_state.sessions().is_empty());

    let failed_storage = FakeStorage::default();
    let mut failed_state = state_with_sessions(&failed_storage, &[TASK_ID]);
    let (failed_request_id, _) = complete_effect(
        failed_state.begin_complete_session_without_recording(&failed_storage, TASK_ID),
    );
    failed_state.apply_complete_result(
        &failed_storage,
        failed_request_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::ACTUAL_WORK_CONFLICT,
            RetryAdvice::ManualCheck,
        ))),
    );
    assert_eq!(failed_state.sessions().len(), 1);
    assert!(failed_state.history().iter().any(|entry| {
        entry.invocation.operation() == Operation::CompleteSessionWithoutRecording
            && entry.outcome == Outcome::Failure
    }));
}

#[test]
fn 完了実績競合は初回clickの計測を保持して最新実績で再送する() {
    for record_elapsed_seconds in [true, false] {
        let storage = FakeStorage::default();
        let mut state = state_with_sessions(&storage, &[TASK_ID]);
        state.tick(6_500);
        let (first_request_id, original_request) = if record_elapsed_seconds {
            complete_effect(state.begin_complete_session(&storage, TASK_ID))
        } else {
            complete_effect(state.begin_complete_session_without_recording(&storage, TASK_ID))
        };

        state.apply_complete_result(
            &storage,
            first_request_id,
            Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
        );

        assert!(!state.is_session_manual_check_blocked(TASK_ID));
        assert!(state.display_error().is_none());
        state.tick(20_000);
        let card = project_session_cards(&state, 540).remove(0);
        assert_eq!(card.remaining_seconds, 794, "初回clickでcardを停止する");
        let conflict = card.completion_conflict.unwrap();
        assert_eq!(conflict.current_actual_work_seconds, 250);
        assert_eq!(conflict.measured_elapsed_seconds, 6);
        assert_eq!(conflict.record_elapsed_seconds, record_elapsed_seconds);

        let (retry_request_id, retry_request) =
            complete_effect(state.confirm_completion_conflict(&storage, TASK_ID));
        assert_ne!(retry_request_id, first_request_id);
        assert_eq!(
            retry_request,
            schronu_web::CompleteSessionRequest {
                expected_actual_work_seconds: 250,
                ..original_request.clone()
            }
        );
        assert_eq!(
            state.confirm_completion_conflict(&storage, TASK_ID),
            ClientEffect::None,
            "in-flight中に二重送信しない"
        );

        state.apply_complete_result(
            &storage,
            retry_request_id,
            Err(ServerFailure::Operation(actual_work_conflict(Some(300)))),
        );
        let updated = project_session_cards(&state, 540).remove(0);
        assert_eq!(
            updated
                .completion_conflict
                .unwrap()
                .current_actual_work_seconds,
            300
        );
        assert_eq!(state.history().len(), 2);
        assert!(state
            .history()
            .iter()
            .all(|entry| entry.outcome == Outcome::Failure));

        let (final_request_id, final_request) =
            complete_effect(state.confirm_completion_conflict(&storage, TASK_ID));
        assert_eq!(final_request.expected_actual_work_seconds, 300);
        state.apply_complete_result(
            &storage,
            final_request_id,
            Ok(snapshot("2026-09-05", 21_000)),
        );
        assert!(state.sessions().is_empty());
        assert!(state.display_error().is_none());
    }
}

#[test]
fn 完了実績競合からの再開は確認待ちを除外してstorageを原子的に更新する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    state.tick(6_500);
    let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    state.apply_complete_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
    );
    state.tick(20_000);

    storage.fail_work_session_writes.set(true);
    assert_eq!(
        state.resume_completion_conflict(&storage, TASK_ID),
        ClientEffect::None
    );
    assert_eq!(state.sessions()[0].started_at_epoch_ms, 0);
    assert_eq!(state.sessions()[0].actual_work_seconds_at_start, 100);
    assert!(project_session_cards(&state, 540)[0]
        .completion_conflict
        .is_some());
    assert!(matches!(
        state.display_error(),
        Some(schronu_web::client::state::DisplayError::LocalStorage {
            committed_on_server: false,
            ..
        })
    ));

    storage.fail_work_session_writes.set(false);
    state.resume_completion_conflict(&storage, TASK_ID);
    assert_eq!(state.sessions()[0].started_at_epoch_ms, 13_500);
    assert_eq!(state.sessions()[0].actual_work_seconds_at_start, 250);
    assert!(project_session_cards(&state, 540)[0]
        .completion_conflict
        .is_none());
    assert!(state.display_error().is_none());
}

#[test]
fn 完了実績競合の再開は安全marker解除成功後だけsessionを更新する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    state.tick(6_500);
    let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    storage.fail_safety_writes.set(true);
    state.apply_complete_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
    );
    state.tick(20_000);

    state.resume_completion_conflict(&storage, TASK_ID);

    assert_eq!(state.sessions()[0].started_at_epoch_ms, 0);
    assert_eq!(state.sessions()[0].actual_work_seconds_at_start, 100);
    assert!(project_session_cards(&state, 540)[0]
        .completion_conflict
        .is_some());
    assert!(matches!(
        state.display_error(),
        Some(schronu_web::client::state::DisplayError::LocalStorage {
            committed_on_server: false,
            ..
        })
    ));
    let blocked_after_reload = load_client_state(&storage, 20_000).unwrap();
    assert!(blocked_after_reload.mutation_globally_blocked());
    assert_eq!(
        blocked_after_reload.sessions()[0].started_at_epoch_ms,
        13_500
    );
    assert_eq!(
        blocked_after_reload.sessions()[0].actual_work_seconds_at_start,
        250
    );

    storage.fail_safety_writes.set(false);
    state.resume_completion_conflict(&storage, TASK_ID);

    assert_eq!(state.sessions()[0].started_at_epoch_ms, 13_500);
    assert_eq!(state.sessions()[0].actual_work_seconds_at_start, 250);
    let restored = load_client_state(&storage, 20_000).unwrap();
    assert!(!restored.mutation_globally_blocked());
    assert_eq!(restored.sessions(), state.sessions());
}

#[test]
fn 完了実績競合の再開はsession保存失敗時に安全markerを維持する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    state.tick(6_500);
    let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    storage.fail_safety_writes.set(true);
    state.apply_complete_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
    );
    state.tick(20_000);
    storage.fail_safety_writes.set(false);
    storage.fail_work_session_writes.set(true);

    state.resume_completion_conflict(&storage, TASK_ID);

    assert_eq!(state.sessions()[0].started_at_epoch_ms, 0);
    assert_eq!(state.sessions()[0].actual_work_seconds_at_start, 100);
    assert!(project_session_cards(&state, 540)[0]
        .completion_conflict
        .is_some());
    let restored = load_client_state(&storage, 20_000).unwrap();
    assert!(restored.mutation_globally_blocked());
    assert_eq!(restored.sessions()[0].started_at_epoch_ms, 0);
    assert_eq!(restored.sessions()[0].actual_work_seconds_at_start, 100);
}

#[test]
fn 現在実績のない旧完了競合はmanual_checkのままsession破棄で解消する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    state.apply_complete_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(actual_work_conflict(None))),
    );

    assert!(state.is_session_manual_check_blocked(TASK_ID));
    assert!(project_session_cards(&state, 540)[0]
        .completion_conflict
        .is_none());
    state.discard_session(&storage, TASK_ID);
    assert!(state.sessions().is_empty());
}

#[test]
fn 完了実績競合は同じ完了taskの古いerrorを確認uiへ置き換える() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (retryable_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    state.apply_complete_result(
        &storage,
        retryable_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_SAVE_FAILED,
            RetryAdvice::Retry,
        ))),
    );
    assert!(state.display_error().is_some());

    let (conflict_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    state.apply_complete_result(
        &storage,
        conflict_id,
        Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
    );

    assert!(state.display_error().is_none());
    assert!(project_session_cards(&state, 540)[0]
        .completion_conflict
        .is_some());

    storage.fail_work_session_writes.set(true);
    state.resume_completion_conflict(&storage, TASK_ID);
    assert!(matches!(
        state.display_error(),
        Some(schronu_web::client::state::DisplayError::LocalStorage {
            committed_on_server: false,
            task_id: Some(task_id),
        }) if task_id == TASK_ID
    ));
    storage.fail_work_session_writes.set(false);
    let (second_conflict_id, _) =
        complete_effect(state.confirm_completion_conflict(&storage, TASK_ID));
    state.apply_complete_result(
        &storage,
        second_conflict_id,
        Err(ServerFailure::Operation(actual_work_conflict(Some(300)))),
    );

    assert!(state.display_error().is_none());
    assert_eq!(
        project_session_cards(&state, 540)[0]
            .completion_conflict
            .unwrap()
            .current_actual_work_seconds,
        300
    );
}

#[test]
fn 完了実績競合は別taskのrepository不確実errorを消さない() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
    let (conflict_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    let (uncertain_id, _) = complete_effect(state.begin_complete_session(&storage, OTHER_TASK_ID));
    state.apply_complete_result(
        &storage,
        uncertain_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_STATE_UNCERTAIN,
            RetryAdvice::ManualCheck,
        ))),
    );
    state.apply_complete_result(
        &storage,
        conflict_id,
        Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
    );

    assert!(matches!(
        state.display_error(),
        Some(schronu_web::client::state::DisplayError::Operation {
            error,
            task_id: Some(task_id),
            ..
        }) if error.code == web_error_codes::REPOSITORY_STATE_UNCERTAIN
            && task_id == OTHER_TASK_ID
    ));
    assert!(project_session_cards(&state, 540)
        .into_iter()
        .find(|card| card.task_id == TASK_ID)
        .unwrap()
        .completion_conflict
        .is_some());
}

#[test]
fn 完了実績競合の確認中と再送中はbufferを初回click時刻で停止する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 0)));
    state.tick(6_500);
    let (first_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    state.apply_complete_result(
        &storage,
        first_id,
        Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
    );

    state.tick(20_000);
    assert_eq!(state.display_buffer_seconds(), Some(60));
    let (second_id, _) = complete_effect(state.confirm_completion_conflict(&storage, TASK_ID));
    state.tick(30_000);
    assert_eq!(state.display_buffer_seconds(), Some(60));
    state.apply_complete_result(
        &storage,
        second_id,
        Err(ServerFailure::Operation(actual_work_conflict(Some(300)))),
    );
    state.tick(40_000);
    assert_eq!(state.display_buffer_seconds(), Some(60));

    state.resume_completion_conflict(&storage, TASK_ID);
    assert_eq!(state.sessions()[0].started_at_epoch_ms, 33_500);
    assert_eq!(state.display_buffer_seconds(), Some(27));
    state.tick(50_000);
    assert_eq!(state.display_buffer_seconds(), Some(27));
}

#[test]
fn 完了実績競合の確認中は開始時見積の到達後もbufferを停止する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 0)));
    state.tick(6_500);
    let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    state.apply_complete_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
    );

    state.tick(900_000);

    assert_eq!(state.display_buffer_seconds(), Some(60));
}

#[test]
fn 見積到達後の完了実績競合は初回click以前のbuffer減算を維持する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 0)));
    state.tick(900_000);
    assert_eq!(state.display_buffer_seconds(), Some(-40));
    let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    state.apply_complete_result(
        &storage,
        request_id,
        Err(ServerFailure::Operation(actual_work_conflict(Some(250)))),
    );

    state.tick(950_000);

    assert_eq!(state.display_buffer_seconds(), Some(-40));
}

#[test]
fn 計測破棄完了は多重送信と不確実な再送を防ぐ() {
    let transport_storage = FakeStorage::default();
    let mut transport_state = state_with_sessions(&transport_storage, &[TASK_ID, OTHER_TASK_ID]);
    let (transport_request_id, _) = complete_effect(
        transport_state.begin_complete_session_without_recording(&transport_storage, TASK_ID),
    );
    assert_eq!(
        transport_state.begin_complete_session_without_recording(&transport_storage, TASK_ID),
        ClientEffect::None
    );
    transport_state.apply_complete_result(
        &transport_storage,
        transport_request_id,
        Err(ServerFailure::Transport("detail".to_owned())),
    );
    assert!(transport_state.mutation_globally_blocked());
    let mut restored = load_client_state(&transport_storage, 0).unwrap();
    assert!(restored.mutation_globally_blocked());
    assert_eq!(
        restored.begin_complete_session_without_recording(&transport_storage, OTHER_TASK_ID),
        ClientEffect::None
    );

    let manual_storage = FakeStorage::default();
    let mut manual_state = state_with_sessions(&manual_storage, &[TASK_ID]);
    let (manual_request_id, _) = complete_effect(
        manual_state.begin_complete_session_without_recording(&manual_storage, TASK_ID),
    );
    manual_state.apply_complete_result(
        &manual_storage,
        manual_request_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::ACTUAL_WORK_CONFLICT,
            RetryAdvice::ManualCheck,
        ))),
    );
    assert!(manual_state.is_session_manual_check_blocked(TASK_ID));
    assert_eq!(
        manual_state.begin_complete_session_without_recording(&manual_storage, TASK_ID),
        ClientEffect::None
    );

    let committed_storage = FakeStorage::default();
    let mut committed_state = state_with_sessions(&committed_storage, &[TASK_ID]);
    let (committed_request_id, _) = complete_effect(
        committed_state.begin_complete_session_without_recording(&committed_storage, TASK_ID),
    );
    committed_storage.fail_work_session_writes.set(true);
    committed_state.apply_complete_result(
        &committed_storage,
        committed_request_id,
        Ok(snapshot("2026-09-05", 1)),
    );
    assert_eq!(committed_state.sessions().len(), 1);
    assert!(committed_state.is_session_committed_blocked(TASK_ID));
    assert_eq!(
        committed_state.begin_complete_session_without_recording(&committed_storage, TASK_ID),
        ClientEffect::None
    );
    assert!(load_client_state(&committed_storage, 0)
        .unwrap()
        .mutation_globally_blocked());
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
fn commit成否不明ならrepository確認までclick時刻で停止する() {
    for uncertain_result in [
        ServerFailure::Transport("detail".to_owned()),
        ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_STATE_UNCERTAIN,
            RetryAdvice::ManualCheck,
        )),
    ] {
        let storage = FakeStorage::default();
        let mut state = state_with_sessions(&storage, &[TASK_ID]);
        let bootstrap_id = bootstrap_effect(state.request_bootstrap());
        state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 0)));
        state.tick(60_000);
        let (request_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));

        state.tick(65_000);
        state.apply_record_result(&storage, request_id, Err(uncertain_result));
        state.tick(70_000);

        assert_eq!(state.display_buffer_seconds(), Some(50));
        assert!(state.can_confirm_repository_checked());
        state.confirm_repository_checked(&storage);
        assert_eq!(state.display_buffer_seconds(), Some(60));
    }
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
