#![allow(dead_code)]

use schronu_web::client::state::{load_client_state, ClientEffect, ClientState};
use schronu_web::client::work_sessions::{
    load_work_sessions, KeyValueStorage, StorageError, WorkSession, WORK_SESSIONS_STORAGE_KEY,
};
use schronu_web::{RetryAdvice, ScheduledTaskRow, ServerSnapshot, SessionTask, WebError};
use std::cell::{Cell, RefCell};

pub const TASK_ID: &str = "00000000-0000-4000-8000-000000000001";
pub const OTHER_TASK_ID: &str = "00000000-0000-4000-8000-000000000002";

#[derive(Default)]
pub struct FakeStorage {
    pub value: RefCell<Option<String>>,
    pub safety_value: RefCell<Option<String>>,
    pub fail_writes: Cell<bool>,
    pub fail_work_session_writes: Cell<bool>,
}

impl KeyValueStorage for FakeStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        match key {
            WORK_SESSIONS_STORAGE_KEY => Ok(self.value.borrow().clone()),
            "schronu_web.mutation_safety.v1" => Ok(self.safety_value.borrow().clone()),
            other => panic!("unexpected key: {other}"),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        if self.fail_writes.get() {
            return Err(StorageError::WriteFailed);
        }
        if key == WORK_SESSIONS_STORAGE_KEY && self.fail_work_session_writes.get() {
            return Err(StorageError::WriteFailed);
        }
        match key {
            WORK_SESSIONS_STORAGE_KEY => *self.value.borrow_mut() = Some(value.to_owned()),
            "schronu_web.mutation_safety.v1" => {
                *self.safety_value.borrow_mut() = Some(value.to_owned());
            }
            other => panic!("unexpected key: {other}"),
        }
        Ok(())
    }
}

pub fn state_with_sessions(storage: &FakeStorage, ids: &[&str]) -> ClientState {
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
    load_client_state(storage, 0).unwrap()
}

pub fn snapshot(logical_date: &str, observed_at_epoch_ms: i64) -> ServerSnapshot {
    ServerSnapshot {
        observed_at_epoch_ms,
        logical_date: logical_date.to_owned(),
        buffer_seconds: 60,
    }
}

pub fn row(task_id: &str, actual_work_seconds: i64) -> ScheduledTaskRow {
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

pub fn web_error(code: &str, retry_advice: RetryAdvice) -> WebError {
    WebError {
        code: code.to_owned(),
        message: "safe".to_owned(),
        retry_advice,
    }
}

pub fn record_effect(effect: ClientEffect) -> (u64, schronu_web::RecordSessionRequest) {
    match effect {
        ClientEffect::RecordSession {
            request_id,
            request,
        } => (request_id, request),
        other => panic!("unexpected effect: {other:?}"),
    }
}

pub fn complete_effect(effect: ClientEffect) -> (u64, schronu_web::RecordSessionRequest) {
    match effect {
        ClientEffect::CompleteSession {
            request_id,
            request,
        } => (request_id, request),
        other => panic!("unexpected effect: {other:?}"),
    }
}

pub fn bootstrap_effect(effect: ClientEffect) -> u64 {
    match effect {
        ClientEffect::Bootstrap { request_id } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    }
}

pub fn list_effect(effect: ClientEffect) -> (u64, schronu_web::ListTasksRequest) {
    match effect {
        ClientEffect::ListTasks {
            request_id,
            request,
        } => (request_id, request),
        other => panic!("unexpected effect: {other:?}"),
    }
}

pub fn auto_effect(effect: ClientEffect) -> u64 {
    match effect {
        ClientEffect::AutoSession { request_id } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    }
}
