use schronu_web::client::state::ActiveTab;
use schronu_web::client::view_state::{
    load_view_state, store_view_state, StoredListView, ViewState, VIEW_STATE_STORAGE_KEY,
};
use schronu_web::client::work_sessions::{KeyValueStorage, StorageError};
use schronu_web::{ScheduledTaskRow, ServerSnapshot, SessionTask};
use std::cell::{Cell, RefCell};

#[derive(Default)]
struct MemoryStorage {
    value: RefCell<Option<String>>,
    fail_read: Cell<bool>,
    fail_write: Cell<bool>,
}

impl KeyValueStorage for MemoryStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        assert_eq!(key, VIEW_STATE_STORAGE_KEY);
        if self.fail_read.get() {
            return Err(StorageError::ReadFailed);
        }
        Ok(self.value.borrow().clone())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        assert_eq!(key, VIEW_STATE_STORAGE_KEY);
        if self.fail_write.get() {
            return Err(StorageError::WriteFailed);
        }
        *self.value.borrow_mut() = Some(value.to_owned());
        Ok(())
    }
}

#[test]
fn view_stateは最後の一覧と画面入力をversion付きで復元する() {
    let storage = MemoryStorage::default();
    let state = view_state(
        "2026-09-09",
        vec![row("00000000-0000-4000-8000-000000000001", "設計")],
    );

    store_view_state(&storage, &state).unwrap();
    let loaded = load_view_state(&storage);

    assert_eq!(loaded.state(), Some(&state));
    assert!(loaded.warning().is_none());
    let raw: serde_json::Value =
        serde_json::from_str(storage.value.borrow().as_deref().unwrap()).unwrap();
    assert_eq!(raw["version"], 1);
}

#[test]
fn view_state保存は最後の1日分を空一覧も含めてatomicに置換する() {
    let storage = MemoryStorage::default();
    store_view_state(
        &storage,
        &view_state(
            "2026-09-09",
            vec![row("00000000-0000-4000-8000-000000000001", "旧")],
        ),
    )
    .unwrap();
    let replacement = view_state("2026-09-10", Vec::new());

    store_view_state(&storage, &replacement).unwrap();

    assert_eq!(load_view_state(&storage).state(), Some(&replacement));
}

#[test]
fn view_stateの破損と未知versionと不正rowは全体を無視してwarningにする() {
    let storage = MemoryStorage::default();
    for raw in [
        "{",
        r#"{"version":2}"#,
        r#"{"version":1,"snapshot":{"observed_at_epoch_ms":0,"logical_date":"2026-09-09","buffer_seconds":0},"list":{"logical_date":"2026-09-09","rows":[{"task":{"task_id":"not-a-uuid","task_name":"task","estimated_work_seconds":1,"actual_work_seconds":0},"schedule_start_epoch_ms":0,"schedule_end_epoch_ms":1,"deadline_epoch_ms":null,"deadline_label":"","misses_deadline":false,"is_leaf":true}]},"active_tab":"list","task_name_filter":"","date_input_text":""}"#,
    ] {
        *storage.value.borrow_mut() = Some(raw.to_owned());
        let loaded = load_view_state(&storage);
        assert!(loaded.state().is_none(), "{raw}");
        assert!(loaded.warning().is_some(), "{raw}");
        assert_eq!(storage.value.borrow().as_deref(), Some(raw));
    }
}

#[test]
fn view_stateのread_write失敗は既存storage契約のerrorを保持する() {
    let storage = MemoryStorage::default();
    storage.fail_read.set(true);
    let loaded = load_view_state(&storage);
    assert!(loaded.state().is_none());
    assert!(loaded.warning().is_some());

    storage.fail_read.set(false);
    storage.fail_write.set(true);
    assert_eq!(
        store_view_state(&storage, &view_state("2026-09-09", Vec::new())),
        Err(StorageError::WriteFailed)
    );
}

fn view_state(logical_date: &str, rows: Vec<ScheduledTaskRow>) -> ViewState {
    ViewState {
        snapshot: ServerSnapshot {
            observed_at_epoch_ms: 1_789_000_000_000,
            logical_date: "2026-09-09".to_owned(),
            buffer_seconds: 60,
        },
        list: Some(StoredListView {
            logical_date: logical_date.to_owned(),
            rows,
        }),
        active_tab: ActiveTab::List,
        task_name_filter: "設計".to_owned(),
        date_input_text: "2026/9/9".to_owned(),
    }
}

fn row(task_id: &str, task_name: &str) -> ScheduledTaskRow {
    ScheduledTaskRow {
        task: SessionTask {
            task_id: task_id.to_owned(),
            task_name: task_name.to_owned(),
            estimated_work_seconds: 900,
            actual_work_seconds: 60,
        },
        schedule_start_epoch_ms: 1_789_000_000_000,
        schedule_end_epoch_ms: 1_789_000_900_000,
        deadline_epoch_ms: None,
        deadline_label: "____/__/__".to_owned(),
        misses_deadline: false,
        is_leaf: true,
    }
}
