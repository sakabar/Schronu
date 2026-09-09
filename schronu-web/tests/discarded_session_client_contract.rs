use schronu_web::client::state::{
    load_client_state, ActiveTab, ClientEffect, DiscardedSessionsViewState, ServerFailure,
};
use schronu_web::client::view_projection::project_session_cards;
use schronu_web::{web_error_codes, DiscardedSessionDay, RetryAdvice, WebSuccess};

#[test]
fn 破棄解除はserver成功後だけsessionを削除して一覧を再取得する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 1_000).unwrap();
    state.add_session_from_row(&storage, &row(TASK_ID, 300));
    state.tick(61_000);

    let ClientEffect::DiscardSession {
        request_id,
        request,
    } = state.begin_discard_session(&storage, TASK_ID)
    else {
        panic!("discard mutation expected");
    };
    assert_eq!(request.task_name_at_start, "task");
    assert_eq!(request.ended_at_epoch_ms, 61_000);
    assert_eq!(state.sessions().len(), 1);

    let follow_up =
        state.apply_discard_result(&storage, request_id, Ok(snapshot("2026-09-05", 61_000)));
    assert!(state.sessions().is_empty());
    assert!(matches!(follow_up, ClientEffect::ListTasks { .. }));
}

#[test]
fn 破棄解除のtransport不確実後は確認と再送で同じevent_idを使う() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 1_000).unwrap();
    state.add_session_from_row(&storage, &row(TASK_ID, 300));
    state.tick(61_000);
    let ClientEffect::DiscardSession {
        request_id,
        request,
    } = state.begin_discard_session(&storage, TASK_ID)
    else {
        panic!()
    };
    let event_id = request.event_id;
    let ended_at_epoch_ms = request.ended_at_epoch_ms;
    state.apply_discard_result(
        &storage,
        request_id,
        Err(ServerFailure::Transport("lost".to_owned())),
    );
    assert!(state.mutation_globally_blocked());
    assert_eq!(
        state.confirm_repository_checked(&storage),
        ClientEffect::None
    );
    state.tick(121_000);
    let ClientEffect::DiscardSession { request, .. } =
        state.begin_discard_session(&storage, TASK_ID)
    else {
        panic!()
    };
    assert_eq!(request.event_id, event_id);
    assert_eq!(request.ended_at_epoch_ms, ended_at_epoch_ms);
}

#[test]
fn 破棄解除のtransport不確実はreload後も同じpayloadで再送する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 1_000).unwrap();
    state.add_session_from_row(&storage, &row(TASK_ID, 300));
    state.tick(61_000);
    let ClientEffect::DiscardSession {
        request_id,
        request: first,
    } = state.begin_discard_session(&storage, TASK_ID)
    else {
        panic!()
    };
    state.apply_discard_result(
        &storage,
        request_id,
        Err(ServerFailure::Transport("lost".to_owned())),
    );

    let mut restored = load_client_state(&storage, 121_000).unwrap();
    restored.confirm_repository_checked(&storage);
    let ClientEffect::DiscardSession {
        request: retried, ..
    } = restored.begin_discard_session(&storage, TASK_ID)
    else {
        panic!()
    };

    assert_eq!(retried, first);
}

#[test]
fn 破棄解除の不確実markerは異なる完了操作へ流用しない() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    state.tick(61_000);
    let ClientEffect::DiscardSession {
        request_id,
        request: first,
    } = state.begin_discard_session(&storage, TASK_ID)
    else {
        panic!()
    };
    state.apply_discard_result(
        &storage,
        request_id,
        Err(ServerFailure::Transport("lost".to_owned())),
    );
    state.confirm_repository_checked(&storage);

    assert_eq!(
        state.begin_complete_session_without_recording(&storage, TASK_ID),
        ClientEffect::None
    );
    let ClientEffect::DiscardSession {
        request: retried, ..
    } = state.begin_discard_session(&storage, TASK_ID)
    else {
        panic!()
    };
    assert_eq!(retried, first);
}

#[test]
fn 破棄完了の不確実markerはreload後も同じrequestだけを再送する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    state.tick(61_000);
    let ClientEffect::CompleteSession {
        request_id,
        request: first,
    } = state.begin_complete_session_without_recording(&storage, TASK_ID)
    else {
        panic!()
    };
    state.apply_complete_result(
        &storage,
        request_id,
        Err(ServerFailure::Transport("lost".to_owned())),
    );

    let marker: serde_json::Value =
        serde_json::from_str(storage.safety_value.borrow().as_deref().unwrap()).unwrap();
    assert_eq!(marker["fixed_requests"][TASK_ID]["operation"], "complete");
    assert_eq!(
        marker["fixed_requests"][TASK_ID]["request"]["discard_event_id"],
        first.discard_event_id.as_deref().unwrap()
    );
    assert_eq!(
        marker["fixed_requests"][TASK_ID]["request"]["ended_at_epoch_ms"],
        61_000
    );

    let mut restored = load_client_state(&storage, 121_000).unwrap();
    restored.confirm_repository_checked(&storage);
    assert_eq!(
        restored.begin_discard_session(&storage, TASK_ID),
        ClientEffect::None
    );
    let ClientEffect::CompleteSession {
        request: retried, ..
    } = restored.begin_complete_session_without_recording(&storage, TASK_ID)
    else {
        panic!()
    };
    assert_eq!(retried, first);
}

#[test]
fn 記録の不確実markerもreload後は同じrequestだけを再送する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    state.tick(61_000);
    let (request_id, first) = record_effect(state.begin_record_session(&storage, TASK_ID));
    state.apply_record_result(
        &storage,
        request_id,
        Err(ServerFailure::Transport("lost".to_owned())),
    );

    let mut restored = load_client_state(&storage, 121_000).unwrap();
    restored.confirm_repository_checked(&storage);
    assert_eq!(
        restored.begin_complete_session(&storage, TASK_ID),
        ClientEffect::None
    );
    let (_, retried) = record_effect(restored.begin_record_session(&storage, TASK_ID));
    assert_eq!(retried, first);
}

#[test]
fn 不確実requestの再送が未commit確定なら四操作ともtimerと固定markerを解除する() {
    #[derive(Clone, Copy)]
    enum Case {
        Discard,
        Record,
        Complete,
        CompleteWithoutRecording,
    }
    fn uncertain() -> ServerFailure {
        ServerFailure::Transport("lost".to_owned())
    }
    fn definitive() -> ServerFailure {
        ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_SAVE_FAILED,
            RetryAdvice::Retry,
        ))
    }

    for case in [
        Case::Discard,
        Case::Record,
        Case::Complete,
        Case::CompleteWithoutRecording,
    ] {
        let storage = FakeStorage::default();
        let mut state = state_with_sessions(&storage, &[TASK_ID]);
        state.tick(61_000);
        let (request_id, is_discard) = match case {
            Case::Discard => match state.begin_discard_session(&storage, TASK_ID) {
                ClientEffect::DiscardSession { request_id, .. } => (request_id, true),
                other => panic!("unexpected effect: {other:?}"),
            },
            Case::Record => (
                record_effect(state.begin_record_session(&storage, TASK_ID)).0,
                false,
            ),
            Case::Complete => (
                complete_effect(state.begin_complete_session(&storage, TASK_ID)).0,
                false,
            ),
            Case::CompleteWithoutRecording => (
                complete_effect(state.begin_complete_session_without_recording(&storage, TASK_ID))
                    .0,
                false,
            ),
        };
        if is_discard {
            state.apply_discard_result(&storage, request_id, Err(uncertain()));
        } else if matches!(case, Case::Record) {
            state.apply_record_result(&storage, request_id, Err(uncertain()));
        } else {
            state.apply_complete_result(&storage, request_id, Err(uncertain()));
        }
        state.confirm_repository_checked(&storage);

        let retry_id = match case {
            Case::Discard => match state.begin_discard_session(&storage, TASK_ID) {
                ClientEffect::DiscardSession { request_id, .. } => request_id,
                other => panic!("unexpected effect: {other:?}"),
            },
            Case::Record => record_effect(state.begin_record_session(&storage, TASK_ID)).0,
            Case::Complete => complete_effect(state.begin_complete_session(&storage, TASK_ID)).0,
            Case::CompleteWithoutRecording => {
                complete_effect(state.begin_complete_session_without_recording(&storage, TASK_ID)).0
            }
        };
        if is_discard {
            state.apply_discard_result(&storage, retry_id, Err(definitive()));
        } else if matches!(case, Case::Record) {
            state.apply_record_result(&storage, retry_id, Err(definitive()));
        } else {
            state.apply_complete_result(&storage, retry_id, Err(definitive()));
        }
        state.tick(121_000);

        assert_eq!(project_session_cards(&state, 0)[0].remaining_seconds, 679);
        let different_operation = if is_discard {
            state.begin_complete_session_without_recording(&storage, TASK_ID)
        } else {
            state.begin_discard_session(&storage, TASK_ID)
        };
        assert_ne!(different_operation, ClientEffect::None);
    }
}

#[test]
fn 不確実requestの再送commit後にlocal削除失敗しても確認時に固定markerを捨てる() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    state.tick(61_000);
    let ClientEffect::DiscardSession {
        request_id,
        request: first,
    } = state.begin_discard_session(&storage, TASK_ID)
    else {
        panic!()
    };
    state.apply_discard_result(
        &storage,
        request_id,
        Err(ServerFailure::Transport("lost".to_owned())),
    );
    state.confirm_repository_checked(&storage);
    let ClientEffect::DiscardSession { request_id, .. } =
        state.begin_discard_session(&storage, TASK_ID)
    else {
        panic!()
    };
    storage.fail_work_session_writes.set(true);
    state.apply_discard_result(&storage, request_id, Ok(snapshot("2026-09-05", 61_000)));
    storage.fail_work_session_writes.set(false);
    state.confirm_repository_checked(&storage);

    state.tick(121_000);
    state.add_session_from_row(&storage, &row(TASK_ID, 300));
    let ClientEffect::DiscardSession { request: next, .. } =
        state.begin_discard_session(&storage, TASK_ID)
    else {
        panic!()
    };
    assert_ne!(next.event_id, first.event_id);
    assert_eq!(next.ended_at_epoch_ms, 121_000);
}

#[test]
fn 旧discard_markerはpayloadを推測せずmanual_blockへ倒す() {
    let storage = FakeStorage::default();
    let _ = state_with_sessions(&storage, &[TASK_ID]);
    *storage.safety_value.borrow_mut() = Some(format!(
        r#"{{"version":1,"mutation_blocked":false,"committed_task_ids":["{TASK_ID}"],"discard_event_ids":{{"{TASK_ID}":"00000000-0000-4000-8000-000000000099"}},"discard_event_ended_at_epoch_ms":{{"{TASK_ID}":61000}}}}"#
    ));
    let mut state = load_client_state(&storage, 121_000).unwrap();

    assert!(state.mutation_globally_blocked());
    assert_eq!(
        state.begin_discard_session(&storage, TASK_ID),
        ClientEffect::None
    );
    state.confirm_repository_checked(&storage);
    assert_eq!(state.sessions().len(), 1);
}

#[test]
fn semanticに不正なfixed_request_markerは全体blockへ倒す() {
    let invalid_markers = [
        format!(r#"{{"operation":"record","request":{{"task_id":"{TASK_ID}","started_at_epoch_ms":0,"ended_at_epoch_ms":1000,"expected_actual_work_seconds":0}}}}"#),
        r#"{"operation":"record","request":{"task_id":"not-a-uuid","started_at_epoch_ms":0,"ended_at_epoch_ms":1000,"expected_actual_work_seconds":0}}"#.to_owned(),
        format!(r#"{{"operation":"record","request":{{"task_id":"{TASK_ID}","started_at_epoch_ms":0,"ended_at_epoch_ms":1000,"expected_actual_work_seconds":-1}}}}"#),
        format!(r#"{{"operation":"complete","request":{{"task_id":"{TASK_ID}","started_at_epoch_ms":2000,"ended_at_epoch_ms":1000,"expected_actual_work_seconds":0,"record_elapsed_seconds":false,"discard_event_id":"not-a-uuid","task_name_at_start":"task"}}}}"#),
        format!(r#"{{"operation":"complete","request":{{"task_id":"{TASK_ID}","started_at_epoch_ms":0,"ended_at_epoch_ms":1000,"expected_actual_work_seconds":0,"record_elapsed_seconds":false,"discard_event_id":"00000000-0000-4000-8000-000000000099","task_name_at_start":"   "}}}}"#),
        format!(r#"{{"operation":"discard","request":{{"event_id":"00000000-0000-4000-8000-000000000099","task_id":"{TASK_ID}","task_name_at_start":"task","started_at_epoch_ms":0,"ended_at_epoch_ms":9223372036854775807}}}}"#),
    ];

    for marker in invalid_markers {
        let storage = FakeStorage::default();
        let _ = state_with_sessions(&storage, &[TASK_ID]);
        *storage.safety_value.borrow_mut() = Some(format!(
            r#"{{"version":1,"mutation_blocked":false,"committed_task_ids":["{TASK_ID}"],"fixed_requests":{{"{TASK_ID}":{marker}}}}}"#
        ));
        let mut state = load_client_state(&storage, 121_000).unwrap();
        assert!(state.mutation_globally_blocked(), "marker: {marker}");
        state.confirm_repository_checked(&storage);
        assert_eq!(state.sessions().len(), 1, "marker: {marker}");
    }
}

#[test]
fn repository確認はcommit済みsessionを除去して未確定discard_markerだけを保持する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
    state.tick(61_000);
    let ClientEffect::DiscardSession {
        request_id: committed_id,
        ..
    } = state.begin_discard_session(&storage, TASK_ID)
    else {
        panic!()
    };
    let ClientEffect::DiscardSession {
        request_id: uncertain_id,
        request: uncertain_request,
    } = state.begin_discard_session(&storage, OTHER_TASK_ID)
    else {
        panic!()
    };
    storage.fail_work_session_writes.set(true);
    state.apply_discard_result(&storage, committed_id, Ok(snapshot("2026-09-05", 61_000)));
    storage.fail_work_session_writes.set(false);
    state.apply_discard_result(
        &storage,
        uncertain_id,
        Err(ServerFailure::Transport("lost".to_owned())),
    );

    let mut restored = load_client_state(&storage, 121_000).unwrap();
    restored.confirm_repository_checked(&storage);
    assert_eq!(restored.sessions().len(), 1);
    assert_eq!(restored.sessions()[0].task_id, OTHER_TASK_ID);
    let ClientEffect::DiscardSession {
        request: retried, ..
    } = restored.begin_discard_session(&storage, OTHER_TASK_ID)
    else {
        panic!()
    };
    assert_eq!(retried, uncertain_request);
}

#[test]
fn task単位の手動確認errorから破棄解除で退出できる() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let (record_id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
    state.apply_record_result(
        &storage,
        record_id,
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_SAVE_FAILED,
            RetryAdvice::ManualCheck,
        ))),
    );
    assert!(state.is_session_manual_check_blocked(TASK_ID));

    let ClientEffect::DiscardSession { request_id, .. } =
        state.begin_discard_session(&storage, TASK_ID)
    else {
        panic!("discard must remain available");
    };
    state.apply_discard_result(&storage, request_id, Ok(snapshot("2026-09-05", 1_000)));

    assert!(state.sessions().is_empty());
    assert!(!state.is_session_manual_check_blocked(TASK_ID));
    assert_eq!(state.display_error(), None);
}

#[test]
fn 集計tabは選択日だけをreadして成功結果を保持する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 1_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1_000)));
    let ClientEffect::ListDiscardedSessions {
        request_id,
        request,
    } = state.switch_tab(ActiveTab::Summary)
    else {
        panic!()
    };
    assert_eq!(request.logical_date, "2026-09-05");
    assert_eq!(
        state.discarded_sessions_view_state(),
        &DiscardedSessionsViewState::Loading {
            logical_date: "2026-09-05".to_owned()
        }
    );
    state.apply_discarded_sessions_result(
        request_id,
        "2026-09-05",
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 2_000),
            data: DiscardedSessionDay {
                logical_date: "2026-09-05".to_owned(),
                total_seconds: 60,
                task_totals: vec![],
                events: vec![],
            },
        }),
    );
    assert_eq!(state.discarded_sessions().unwrap().total_seconds, 60);
}

#[test]
fn 集計readは別日の旧結果を隠して失敗を再試行可能な状態にする() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 1_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1_000)));
    let ClientEffect::ListDiscardedSessions { request_id, .. } =
        state.request_discarded_sessions("2026-09-05")
    else {
        panic!()
    };
    state.apply_discarded_sessions_result(
        request_id,
        "2026-09-05",
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 2_000),
            data: DiscardedSessionDay {
                logical_date: "2026-09-05".to_owned(),
                total_seconds: 60,
                task_totals: vec![],
                events: vec![],
            },
        }),
    );

    let ClientEffect::ListDiscardedSessions { request_id, .. } =
        state.request_discarded_sessions("2026-09-06")
    else {
        panic!()
    };
    assert!(state.discarded_sessions().is_none());
    assert!(matches!(
        state.discarded_sessions_view_state(),
        DiscardedSessionsViewState::Loading { logical_date }
            if logical_date == "2026-09-06"
    ));
    state.apply_discarded_sessions_result(
        request_id,
        "2026-09-06",
        Err(ServerFailure::Operation(web_error(
            web_error_codes::REPOSITORY_UNAVAILABLE,
            RetryAdvice::Retry,
        ))),
    );

    assert!(matches!(
        state.discarded_sessions_view_state(),
        DiscardedSessionsViewState::Error {
            logical_date,
            message,
        } if logical_date == "2026-09-06" && message == "safe"
    ));
    assert!(state.discarded_sessions().is_none());
    let ClientEffect::ListDiscardedSessions { request, .. } =
        state.request_discarded_sessions("2026-09-06")
    else {
        panic!()
    };
    assert_eq!(request.logical_date, "2026-09-06");
    assert!(state.display_error().is_none());
    assert!(matches!(
        state.discarded_sessions_view_state(),
        DiscardedSessionsViewState::Loading { .. }
    ));
}

#[test]
fn 集計で共有日付を変えた後は一覧tabが同じ日を再取得する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 1_000).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-05", 1_000)));
    let (list_id, _) = list_effect(state.request_list("2026-09-05"));
    state.apply_list_result(
        list_id,
        "2026-09-05",
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 2_000),
            data: vec![row(TASK_ID, 0)],
        }),
    );
    let ClientEffect::ListDiscardedSessions { request_id, .. } =
        state.request_discarded_sessions("2026-09-06")
    else {
        panic!()
    };
    state.apply_discarded_sessions_result(
        request_id,
        "2026-09-06",
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 3_000),
            data: DiscardedSessionDay {
                logical_date: "2026-09-06".to_owned(),
                total_seconds: 0,
                task_totals: vec![],
                events: vec![],
            },
        }),
    );

    let ClientEffect::ListTasks { request, .. } = state.switch_tab(ActiveTab::List) else {
        panic!("shared logical date needs a fresh list");
    };
    assert_eq!(request.logical_date, "2026-09-06");
    assert!(!state.has_scheduled_list());
    assert!(state.scheduled_rows().is_empty());
}

mod client_state_support;
use client_state_support::*;
