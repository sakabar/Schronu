use schronu_web::client::state::{load_client_state, ClientEffect, DisplayError, ServerFailure};
use schronu_web::client::work_sessions::{
    KeyValueStorage, StorageError, WORK_SESSIONS_STORAGE_KEY,
};
use schronu_web::{web_error_codes, CompleteSessionResponse, RetryAdvice, SessionTask, WebSuccess};
use std::cell::Cell;

mod client_state_support;
use client_state_support::*;

const PARENT_ID: &str = "00000000-0000-4000-8000-000000000003";

#[derive(Default)]
struct CountingStorage {
    inner: FakeStorage,
    session_writes: Cell<usize>,
}

impl KeyValueStorage for CountingStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        self.inner.get(key)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        if key == WORK_SESSIONS_STORAGE_KEY {
            self.session_writes.set(self.session_writes.get() + 1);
        }
        self.inner.set(key, value)
    }
}

fn handoff() -> CompleteSessionResponse {
    WebSuccess {
        snapshot: snapshot("2026-09-05", 65_000),
        data: Some(SessionTask {
            task_id: PARENT_ID.to_owned(),
            task_name: "handoff parent".to_owned(),
            estimated_work_seconds: 600,
            actual_work_seconds: 120,
        }),
    }
}

#[test]
fn completion_atomically_replaces_child_with_parent_at_receipt_time() {
    for record in [true, false] {
        let storage = CountingStorage::default();
        let mut state = state_with_sessions(&storage.inner, &[TASK_ID, OTHER_TASK_ID]);
        let other = state.sessions()[1].clone();
        state.tick(60_000);
        let effect = if record {
            state.begin_complete_session(&storage, TASK_ID)
        } else {
            state.begin_complete_session_without_recording(&storage, TASK_ID)
        };
        let (request_id, _) = complete_effect(effect);
        state.tick(70_123);
        let result = state.apply_complete_result(&storage, request_id, Ok(handoff()));
        assert!(matches!(result, ClientEffect::ListTasks { .. }));
        assert_eq!(storage.session_writes.get(), 1);
        assert_eq!(state.sessions().len(), 2);
        assert_eq!(state.sessions()[0], other);
        let parent = &state.sessions()[1];
        assert_eq!(parent.task_id, PARENT_ID);
        assert_eq!(parent.task_name, "handoff parent");
        assert_eq!(parent.started_at_epoch_ms, 70_123);
        assert_eq!(parent.estimated_work_seconds_at_start, 600);
        assert_eq!(parent.actual_work_seconds_at_start, 120);
        let restored = load_client_state(&storage, 80_000).unwrap();
        assert_eq!(restored.sessions(), state.sessions());
        assert_eq!(
            state.apply_complete_result(&storage, request_id, Ok(handoff())),
            ClientEffect::None
        );
        assert_eq!(storage.session_writes.get(), 1);
        assert_eq!(state.sessions()[1].started_at_epoch_ms, 70_123);
    }
}

#[test]
fn completion_preserves_existing_parent_session() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID, PARENT_ID]);
    let parent = state.sessions()[1].clone();
    let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
    state.tick(70_000);
    state.apply_complete_result(&storage, request_id, Ok(handoff()));
    assert_eq!(state.sessions(), &[parent]);
}

#[test]
fn handoff_storage_failure_keeps_sessions_and_blocks_resubmission() {
    for fail_marker in [false, true] {
        let storage = FakeStorage::default();
        let mut state = state_with_sessions(&storage, &[TASK_ID, OTHER_TASK_ID]);
        let before = state.sessions().to_vec();
        let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));
        storage.fail_safety_writes.set(fail_marker);
        storage.fail_work_session_writes.set(!fail_marker);
        state.tick(70_000);
        state.apply_complete_result(&storage, request_id, Ok(handoff()));
        assert_eq!(state.sessions(), before);
        assert!(state.mutation_globally_blocked());
        assert!(matches!(
            state.display_error(),
            Some(DisplayError::LocalStorage {
                committed_on_server: true,
                ..
            })
        ));
        assert_eq!(
            state.begin_complete_session(&storage, TASK_ID),
            ClientEffect::None
        );
        storage.fail_safety_writes.set(false);
        storage.fail_work_session_writes.set(false);
        let restored = load_client_state(&storage, 80_000).unwrap();
        assert!(restored.mutation_globally_blocked());
        assert_eq!(restored.sessions(), before);
    }
}

#[test]
fn completion_errors_never_start_parent_and_conflict_retry_starts_at_success() {
    for record in [true, false] {
        for error in [
            ServerFailure::Transport("disconnected".to_owned()),
            ServerFailure::Operation(web_error(
                web_error_codes::REPOSITORY_STATE_UNCERTAIN,
                RetryAdvice::ManualCheck,
            )),
            ServerFailure::Operation(web_error(
                web_error_codes::REPOSITORY_SAVE_FAILED,
                RetryAdvice::Retry,
            )),
            ServerFailure::Operation(actual_work_conflict(Some(200))),
        ] {
            let storage = FakeStorage::default();
            let mut state = state_with_sessions(&storage, &[TASK_ID]);
            state.tick(60_000);
            let effect = if record {
                state.begin_complete_session(&storage, TASK_ID)
            } else {
                state.begin_complete_session_without_recording(&storage, TASK_ID)
            };
            let (request_id, request) = complete_effect(effect);
            state.apply_complete_result(&storage, request_id, Err(error.clone()));
            assert_eq!(state.sessions().len(), 1);
            assert_eq!(state.sessions()[0].task_id, TASK_ID);
            if matches!(error, ServerFailure::Operation(ref error) if error.code == web_error_codes::ACTUAL_WORK_CONFLICT)
            {
                state.tick(90_000);
                let (retry_id, retry) =
                    complete_effect(state.confirm_completion_conflict(&storage, TASK_ID));
                assert_ne!(retry_id, request_id);
                assert_eq!(retry.ended_at_epoch_ms, request.ended_at_epoch_ms);
                assert_eq!(
                    state.apply_complete_result(&storage, request_id, Ok(handoff())),
                    ClientEffect::None
                );
                assert_eq!(state.sessions()[0].task_id, TASK_ID);
                state.tick(95_123);
                state.apply_complete_result(&storage, retry_id, Ok(handoff()));
                assert_eq!(state.sessions().len(), 1);
                assert_eq!(state.sessions()[0].task_id, PARENT_ID);
                assert_eq!(state.sessions()[0].started_at_epoch_ms, 95_123);
            }
        }
    }
}
