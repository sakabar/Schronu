use std::cell::{Cell, RefCell};

use schronu_web::client::work_sessions::{
    load_work_sessions, KeyValueStorage, StorageError, WorkSession, WorkSessionsMutationError,
    WORK_SESSIONS_STORAGE_KEY,
};
use serde_json::json;

#[derive(Default)]
struct FakeStorage {
    value: RefCell<Option<String>>,
    get_fails: Cell<bool>,
    set_fails: Cell<bool>,
    set_calls: Cell<usize>,
}

impl FakeStorage {
    fn with_value(value: impl Into<String>) -> Self {
        Self {
            value: RefCell::new(Some(value.into())),
            ..Self::default()
        }
    }
}

impl KeyValueStorage for FakeStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        assert_eq!(key, WORK_SESSIONS_STORAGE_KEY);
        if self.get_fails.get() {
            Err(StorageError::ReadFailed)
        } else {
            Ok(self.value.borrow().clone())
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        assert_eq!(key, WORK_SESSIONS_STORAGE_KEY);
        self.set_calls.set(self.set_calls.get() + 1);
        if self.set_fails.get() {
            Err(StorageError::WriteFailed)
        } else {
            self.value.replace(Some(value.to_owned()));
            Ok(())
        }
    }
}

fn session(task_id: &str, task_name: &str) -> WorkSession {
    WorkSession {
        task_id: task_id.to_owned(),
        task_name: task_name.to_owned(),
        started_at_epoch_ms: 1_788_565_500_000,
        estimated_work_seconds_at_start: 900,
        actual_work_seconds_at_start: 300,
    }
}

#[test]
fn missing_key_loads_an_empty_writable_state_without_warnings() {
    let storage = FakeStorage::default();

    let state = load_work_sessions(&storage).unwrap();

    assert!(state.sessions.is_empty());
    assert!(state.warnings.is_empty());
    assert!(!state.write_blocked);
    assert!(!state.needs_repair);
    assert_eq!(storage.set_calls.get(), 0);
}

#[test]
fn sessions_roundtrip_in_a_versioned_top_level_object() {
    let storage = FakeStorage::default();
    let mut state = load_work_sessions(&storage).unwrap();
    let expected = vec![session("550e8400-e29b-41d4-a716-446655440000", "設計")];

    state.replace_sessions(&storage, expected.clone()).unwrap();

    assert_eq!(state.sessions, expected);
    assert_eq!(storage.set_calls.get(), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(storage.value.borrow().as_ref().unwrap())
            .unwrap(),
        json!({"version": 1, "work_sessions": expected})
    );
    assert_eq!(load_work_sessions(&storage).unwrap().sessions, expected);
}

#[test]
fn corrupt_json_wrong_shape_and_wrong_version_are_write_blocked_without_rewrite() {
    for raw in [
        "{secret invalid json",
        r#"{"version":1,"work_sessions":"not-an-array"}"#,
        r#"{"version":2,"work_sessions":[]}"#,
    ] {
        let storage = FakeStorage::with_value(raw);
        let state = load_work_sessions(&storage).unwrap();

        assert!(state.sessions.is_empty());
        assert!(!state.warnings.is_empty());
        assert!(state.warnings.iter().all(|warning| !warning.contains(raw)));
        assert!(state.write_blocked);
        assert!(!state.needs_repair);
        assert_eq!(storage.set_calls.get(), 0);
        assert_eq!(storage.value.borrow().as_deref(), Some(raw));
    }
}

#[test]
fn invalid_and_duplicate_entries_are_filtered_without_writing_on_load() {
    let valid_one = session("550e8400-e29b-41d4-a716-446655440000", "設計");
    let valid_two = session("67e55044-10b1-426f-9247-bb680e5fe0c8", "実装");
    let raw = json!({
        "version": 1,
        "work_sessions": [
            valid_one,
            session("not-a-uuid", "UUID不正"),
            session("123e4567-e89b-12d3-a456-426614174000", "   "),
            {"task_id":"123e4567-e89b-12d3-a456-426614174001","task_name":"負見積","started_at_epoch_ms":0,"estimated_work_seconds_at_start":-1,"actual_work_seconds_at_start":0},
            {"task_id":"123e4567-e89b-12d3-a456-426614174002","task_name":"負実績","started_at_epoch_ms":0,"estimated_work_seconds_at_start":1,"actual_work_seconds_at_start":-1},
            {"task_id":"123e4567-e89b-12d3-a456-426614174003","task_name":"epoch不正","started_at_epoch_ms":i64::MAX,"estimated_work_seconds_at_start":1,"actual_work_seconds_at_start":0},
            session("550E8400-E29B-41D4-A716-446655440000", "重複"),
            valid_two,
        ]
    });
    let storage = FakeStorage::with_value(raw.to_string());

    let state = load_work_sessions(&storage).unwrap();

    assert_eq!(
        state.sessions,
        [
            session("550e8400-e29b-41d4-a716-446655440000", "設計"),
            session("67e55044-10b1-426f-9247-bb680e5fe0c8", "実装")
        ]
    );
    assert!(!state.write_blocked);
    assert!(state.needs_repair);
    assert!(!state.warnings.is_empty());
    assert_eq!(storage.set_calls.get(), 0);
}

#[test]
fn next_successful_mutation_repairs_filtered_storage_once() {
    let raw = json!({
        "version": 1,
        "work_sessions": [
            session("550e8400-e29b-41d4-a716-446655440000", "設計"),
            session("invalid", "破損")
        ]
    });
    let storage = FakeStorage::with_value(raw.to_string());
    let mut state = load_work_sessions(&storage).unwrap();
    let repaired = vec![session("550e8400-e29b-41d4-a716-446655440000", "設計")];

    state.replace_sessions(&storage, repaired.clone()).unwrap();

    assert_eq!(state.sessions, repaired);
    assert!(!state.needs_repair);
    assert_eq!(storage.set_calls.get(), 1);
    assert_eq!(
        load_work_sessions(&storage).unwrap().warnings,
        Vec::<String>::new()
    );
}

#[test]
fn failed_write_leaves_memory_unchanged() {
    let storage = FakeStorage::default();
    let mut state = load_work_sessions(&storage).unwrap();
    let original = vec![session("550e8400-e29b-41d4-a716-446655440000", "設計")];
    state.replace_sessions(&storage, original.clone()).unwrap();
    storage.set_fails.set(true);

    assert_eq!(
        state.replace_sessions(
            &storage,
            vec![session("67e55044-10b1-426f-9247-bb680e5fe0c8", "実装")]
        ),
        Err(WorkSessionsMutationError::Storage(
            StorageError::WriteFailed
        ))
    );
    assert_eq!(state.sessions, original);
}

#[test]
fn write_blocked_state_rejects_mutation_without_touching_storage() {
    let storage = FakeStorage::with_value("invalid");
    let mut state = load_work_sessions(&storage).unwrap();

    assert_eq!(
        state.replace_sessions(
            &storage,
            vec![session("550e8400-e29b-41d4-a716-446655440000", "設計")]
        ),
        Err(WorkSessionsMutationError::WriteBlocked)
    );
    assert!(state.sessions.is_empty());
    assert_eq!(storage.set_calls.get(), 0);
}

#[test]
fn storage_read_failure_is_typed() {
    let storage = FakeStorage::default();
    storage.get_fails.set(true);

    assert_eq!(load_work_sessions(&storage), Err(StorageError::ReadFailed));
}
