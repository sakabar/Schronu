use schronu_web::client::state::{ActiveTab, ListMode};
use schronu_web::client::view_state::{
    load_view_state, store_view_state, StoredActiveList, ViewState, ViewStateStoreError,
    VIEW_STATE_STORAGE_KEY,
};
use schronu_web::client::work_sessions::{KeyValueStorage, StorageError};
use schronu_web::{CompletedTaskRow, ScheduledTaskRow, ServerSnapshot, SessionTask};
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
    assert_eq!(raw["version"], 3);
}

#[test]
fn view_state_v3は完了modeと空成功を含むactive_listを復元する() {
    let storage = MemoryStorage::default();
    let state = ViewState {
        snapshot: ServerSnapshot {
            observed_at_epoch_ms: 1_789_000_000_000,
            logical_date: "2026-09-09".to_owned(),
            buffer_seconds: 60,
        },
        list_mode: ListMode::Completed,
        list: Some(StoredActiveList::Completed {
            logical_date: "2026-09-08".to_owned(),
            rows: Vec::new(),
        }),
        active_tab: ActiveTab::List,
        task_name_filter: "".to_owned(),
        date_input_text: "2026/9/8".to_owned(),
    };

    store_view_state(&storage, &state).unwrap();
    assert_eq!(load_view_state(&storage).state(), Some(&state));
}

#[test]
fn view_state_v3は明示された将来の完了一覧日を保持する() {
    let storage = MemoryStorage::default();
    let state = ViewState {
        snapshot: ServerSnapshot {
            observed_at_epoch_ms: 1_789_000_000_000,
            logical_date: "2026-09-09".to_owned(),
            buffer_seconds: 60,
        },
        list_mode: ListMode::Completed,
        list: Some(StoredActiveList::Completed {
            logical_date: "9999-12-31".to_owned(),
            rows: Vec::new(),
        }),
        active_tab: ActiveTab::List,
        task_name_filter: String::new(),
        date_input_text: "9999/12/31".to_owned(),
    };

    store_view_state(&storage, &state).unwrap();
    assert_eq!(load_view_state(&storage).state(), Some(&state));
}

#[test]
fn view_state_v2は予定modeとscheduled_listへ移行する() {
    let storage = MemoryStorage::default();
    *storage.value.borrow_mut() = Some(
        serde_json::json!({
            "version": 2,
            "snapshot": {
                "observed_at_epoch_ms": 1_789_000_000_000_i64,
                "logical_date": "2026-09-09",
                "buffer_seconds": 60
            },
            "list": {
                "logical_date": "2026-09-09",
                "rows": [row("00000000-0000-4000-8000-000000000001", "旧予定")]
            },
            "active_tab": "list",
            "task_name_filter": "旧",
            "date_input_text": "2026/9/9"
        })
        .to_string(),
    );

    let loaded = load_view_state(&storage);
    let state = loaded.state().unwrap();
    assert_eq!(state.list_mode, ListMode::Scheduled);
    assert!(matches!(
        state.list,
        Some(StoredActiveList::Scheduled { .. })
    ));
    assert!(loaded.warning().is_none());
}

#[test]
fn view_state_v3は不正な完了rowを全体不正として扱う() {
    let storage = MemoryStorage::default();
    let mut state = ViewState {
        snapshot: ServerSnapshot {
            observed_at_epoch_ms: 1_789_000_000_000,
            logical_date: "2026-09-09".to_owned(),
            buffer_seconds: 60,
        },
        list_mode: ListMode::Completed,
        list: Some(StoredActiveList::Completed {
            logical_date: "2026-09-09".to_owned(),
            rows: vec![CompletedTaskRow {
                task_id: "bad".to_owned(),
                task_name: "完了".to_owned(),
                project_name: "project".to_owned(),
                completed_at_epoch_ms: 1_789_000_000_000,
                actual_work_seconds: 1,
                estimated_work_seconds: 1,
            }],
        }),
        active_tab: ActiveTab::List,
        task_name_filter: String::new(),
        date_input_text: String::new(),
    };

    assert_eq!(
        store_view_state(&storage, &state),
        Err(ViewStateStoreError::InvalidState)
    );
    if let Some(StoredActiveList::Completed { rows, .. }) = &mut state.list {
        rows[0].task_id = "00000000-0000-4000-8000-000000000001".to_owned();
        rows[0].actual_work_seconds = -1;
    }
    assert_eq!(
        store_view_state(&storage, &state),
        Err(ViewStateStoreError::InvalidState)
    );
    if let Some(StoredActiveList::Completed { rows, .. }) = &mut state.list {
        rows[0].actual_work_seconds = 1;
        rows[0].project_name = "  ".to_owned();
    }
    assert_eq!(
        store_view_state(&storage, &state),
        Err(ViewStateStoreError::InvalidState)
    );
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
        r#"{"version":3}"#,
        r#"{"version":1,"snapshot":{"observed_at_epoch_ms":1789000000000,"logical_date":"2026-09-09","buffer_seconds":0},"list":null,"active_tab":"list","task_name_filter":"","date_input_text":""}"#,
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
        Err(ViewStateStoreError::Storage(StorageError::WriteFailed))
    );
}

#[test]
fn view_stateの構築不正はstorage失敗と区別する() {
    let storage = MemoryStorage::default();
    let mut state = view_state(
        "2026-09-09",
        vec![row("00000000-0000-4000-8000-000000000001", "task")],
    );
    let Some(StoredActiveList::Scheduled { rows, .. }) = state.list.as_mut() else {
        panic!("scheduled list expected");
    };
    rows[0].task.task_name.clear();

    assert_eq!(
        store_view_state(&storage, &state),
        Err(ViewStateStoreError::InvalidState)
    );
    assert!(storage.value.borrow().is_none());
}

#[test]
fn view_stateは希望日時以上の期限制限日時を拒否する() {
    let storage = MemoryStorage::default();
    let mut state = view_state(
        "2026-09-09",
        vec![row("00000000-0000-4000-8000-000000000001", "task")],
    );
    let Some(StoredActiveList::Scheduled { rows, .. }) = state.list.as_mut() else {
        panic!("scheduled list expected");
    };
    let plan = &mut rows[0].defer_plan;
    plan.mode = schronu_web::DeferMode::DeadlineLimited;
    plan.effective_pending_until_epoch_ms = Some(plan.requested_pending_until_epoch_ms);

    assert_eq!(
        store_view_state(&storage, &state),
        Err(ViewStateStoreError::InvalidState)
    );
}

fn view_state(logical_date: &str, rows: Vec<ScheduledTaskRow>) -> ViewState {
    ViewState {
        snapshot: ServerSnapshot {
            observed_at_epoch_ms: 1_789_000_000_000,
            logical_date: "2026-09-09".to_owned(),
            buffer_seconds: 60,
        },
        list_mode: ListMode::Scheduled,
        list: Some(StoredActiveList::Scheduled {
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
        task_display_kind: schronu_web::TaskDisplayKind::NonRepetitive,
        deadline_display_kind: schronu_web::DeadlineDisplayKind::None,
        is_leaf: true,
        defer_plan: schronu_web::DeferPlan {
            mode: schronu_web::DeferMode::Normal,
            requested_pending_until_epoch_ms: 1_789_086_400_000,
            effective_pending_until_epoch_ms: None,
            repetition_interval_days: None,
        },
    }
}
