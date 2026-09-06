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

#[test]
fn 完了成功は記録方針にかかわらず対象taskの全segmentだけを一覧から除去する() {
    let mut remaining_task_ids_by_policy = Vec::new();
    for record_elapsed_seconds in [true, false] {
        let storage = FakeStorage::default();
        let mut state = state_with_sessions(&storage, &[TASK_ID]);
        apply_list_rows(
            &mut state,
            "2026-09-05",
            1,
            vec![row(TASK_ID, 0), row(OTHER_TASK_ID, 0), row(TASK_ID, 0)],
        );

        let effect = if record_elapsed_seconds {
            state.begin_complete_session(&storage, TASK_ID)
        } else {
            state.begin_complete_session_without_recording(&storage, TASK_ID)
        };
        let (request_id, _) = complete_effect(effect);
        state.apply_complete_result(&storage, request_id, Ok(snapshot("2026-09-05", 2)));

        assert_eq!(state.selected_logical_date(), Some("2026-09-05"));
        remaining_task_ids_by_policy.push(
            state
                .scheduled_rows()
                .iter()
                .map(|scheduled| scheduled.task.task_id.clone())
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(
        remaining_task_ids_by_policy,
        vec![vec![OTHER_TASK_ID], vec![OTHER_TASK_ID]]
    );
}

#[test]
fn 日付境界の完了成功は新snapshotと日付buttonを適用し表示中の一覧を維持する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let other_row = row(OTHER_TASK_ID, 10);
    apply_list_rows(
        &mut state,
        "2026-09-05",
        1,
        vec![row(TASK_ID, 0), other_row.clone()],
    );
    let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));

    state.apply_complete_result(&storage, request_id, Ok(snapshot("2026-09-06", 2)));

    assert_eq!(state.snapshot().unwrap().logical_date, "2026-09-06");
    assert_eq!(state.date_buttons()[0].logical_date, "2026-09-06");
    assert_eq!(state.selected_logical_date(), Some("2026-09-05"));
    assert_eq!(state.scheduled_rows(), &[other_row]);
}

#[test]
fn 完了失敗は記録方針にかかわらず一覧を保持する() {
    for record_elapsed_seconds in [true, false] {
        let storage = FakeStorage::default();
        let mut state = state_with_sessions(&storage, &[TASK_ID]);
        apply_list_rows(
            &mut state,
            "2026-09-05",
            1,
            vec![row(TASK_ID, 0), row(OTHER_TASK_ID, 0), row(TASK_ID, 0)],
        );
        let expected_rows = state.scheduled_rows().to_vec();

        let effect = if record_elapsed_seconds {
            state.begin_complete_session(&storage, TASK_ID)
        } else {
            state.begin_complete_session_without_recording(&storage, TASK_ID)
        };
        let (request_id, _) = complete_effect(effect);
        state.apply_complete_result(
            &storage,
            request_id,
            Err(ServerFailure::Operation(web_error(
                web_error_codes::REPOSITORY_SAVE_FAILED,
                RetryAdvice::Retry,
            ))),
        );

        assert_eq!(state.selected_logical_date(), Some("2026-09-05"));
        assert_eq!(state.scheduled_rows(), expected_rows);
    }
}

#[test]
fn server完了後のlocal_session削除失敗でも対象taskを一覧から除去する() {
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
    state.apply_complete_result(&storage, request_id, Ok(snapshot("2026-09-05", 2)));

    assert!(state.is_session_committed_blocked(TASK_ID));
    assert_eq!(state.scheduled_rows().len(), 1);
    assert_eq!(state.scheduled_rows()[0].task.task_id, OTHER_TASK_ID);
}

#[test]
fn 完了前のlist応答は後着しても完了taskを復活させない() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let initial_other_row = row(OTHER_TASK_ID, 10);
    apply_list_rows(
        &mut state,
        "2026-09-05",
        1,
        vec![row(TASK_ID, 0), initial_other_row.clone()],
    );
    let expected_rows = vec![initial_other_row];
    let (old_list_request_id, old_list_request) = list_effect(state.request_list("2026-09-05"));
    let (complete_request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    state.apply_complete_result(&storage, complete_request_id, Ok(snapshot("2026-09-05", 3)));

    state.apply_list_result(
        old_list_request_id,
        &old_list_request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 2),
            data: vec![row(TASK_ID, 100), row(OTHER_TASK_ID, 200)],
        }),
    );

    assert_eq!(state.scheduled_rows(), expected_rows);
}

#[test]
fn 完了後に開始した新しいlist応答は通常どおり適用する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    apply_list_rows(&mut state, "2026-09-05", 1, vec![row(TASK_ID, 0)]);
    let (complete_request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    state.apply_complete_result(&storage, complete_request_id, Ok(snapshot("2026-09-05", 2)));

    apply_list_rows(
        &mut state,
        "2026-09-05",
        3,
        vec![row(TASK_ID, 0), row(OTHER_TASK_ID, 0)],
    );

    assert_eq!(state.scheduled_rows().len(), 2);
    assert_eq!(state.scheduled_rows()[0].task.task_id, TASK_ID);
    assert_eq!(state.scheduled_rows()[1].task.task_id, OTHER_TASK_ID);
}

#[test]
fn 記録とlocalなsession破棄は一覧を変更しない() {
    let record_storage = FakeStorage::default();
    let mut record_state = state_with_sessions(&record_storage, &[TASK_ID]);
    apply_list_rows(
        &mut record_state,
        "2026-09-05",
        1,
        vec![row(TASK_ID, 0), row(OTHER_TASK_ID, 0)],
    );
    let expected_record_rows = record_state.scheduled_rows().to_vec();
    let (record_request_id, _) =
        record_effect(record_state.begin_record_session(&record_storage, TASK_ID));
    record_state.apply_record_result(
        &record_storage,
        record_request_id,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-05", 2),
            data: RecordSessionResult {
                actual_work_seconds: 101,
            },
        }),
    );
    assert_eq!(record_state.scheduled_rows(), expected_record_rows);

    let discard_storage = FakeStorage::default();
    let mut discard_state = state_with_sessions(&discard_storage, &[TASK_ID]);
    apply_list_rows(
        &mut discard_state,
        "2026-09-05",
        1,
        vec![row(TASK_ID, 0), row(OTHER_TASK_ID, 0)],
    );
    let expected_discard_rows = discard_state.scheduled_rows().to_vec();
    discard_state.discard_session(&discard_storage, TASK_ID);
    assert_eq!(discard_state.scheduled_rows(), expected_discard_rows);
}
