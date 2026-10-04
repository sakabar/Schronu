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
            in_progress_actual_work_seconds: Some(900),
            total_actual_work_seconds: 900,
            available_seconds: 43_200,
            recorded_percentage: Some(2),
        }),
        active_tab: ActiveTab::List,
        task_name_filter: "".to_owned(),
        date_input_text: "2026/9/8".to_owned(),
    };

    store_view_state(&storage, &state).unwrap();
    assert_eq!(load_view_state(&storage).state(), Some(&state));
}

#[test]
fn 旧view_state_v3の完了rowはtask種別を単発として復元する() {
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
            rows: vec![CompletedTaskRow {
                task_id: "00000000-0000-4000-8000-000000000001".to_owned(),
                task_name: "旧完了".to_owned(),
                project_name: "Schronu".to_owned(),
                completed_at_epoch_ms: 1_789_000_000_000,
                actual_work_seconds: 60,
                estimated_work_seconds: 60,
                task_display_kind: schronu_web::TaskDisplayKind::Fixed,
            }],
            in_progress_actual_work_seconds: None,
            total_actual_work_seconds: 60,
            available_seconds: 43_200,
            recorded_percentage: Some(0),
        }),
        active_tab: ActiveTab::List,
        task_name_filter: String::new(),
        date_input_text: "2026/9/8".to_owned(),
    };
    store_view_state(&storage, &state).unwrap();
    let mut raw: serde_json::Value =
        serde_json::from_str(storage.value.borrow().as_deref().unwrap()).unwrap();
    raw["list"]["rows"][0]
        .as_object_mut()
        .unwrap()
        .remove("task_display_kind");
    *storage.value.borrow_mut() = Some(raw.to_string());

    let loaded = load_view_state(&storage);
    let Some(StoredActiveList::Completed { rows, .. }) = &loaded.state().unwrap().list else {
        panic!("completed list expected");
    };
    assert_eq!(
        rows[0].task_display_kind,
        schronu_web::TaskDisplayKind::NonRepetitive
    );
    assert!(loaded.warning().is_none());
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
            in_progress_actual_work_seconds: None,
            total_actual_work_seconds: 0,
            available_seconds: 0,
            recorded_percentage: None,
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
fn view_state_v2はprojected行を予定modeへidentityを保って移行する() {
    let storage = MemoryStorage::default();
    let source_task_id = "00000000-0000-4000-8000-000000000010";
    let deadline_epoch_ms = 1_789_228_800_000_i64;
    *storage.value.borrow_mut() = Some(projected_v2_raw(
        source_task_id,
        &format!("{source_task_id}:{deadline_epoch_ms}"),
        deadline_epoch_ms,
    ));

    let loaded = load_view_state(&storage);
    let state = loaded.state().unwrap();

    assert_eq!(state.list_mode, ListMode::Scheduled);
    let Some(StoredActiveList::Scheduled { logical_date, rows }) = &state.list else {
        panic!("scheduled list expected");
    };
    assert_eq!(logical_date, "2026-09-12");
    assert_eq!(rows.len(), 1);
    assert!(rows[0].task.task_id.is_none());
    assert!(rows[0].defer_plan.is_none());
    assert!(matches!(
        &rows[0].occurrence,
        schronu_web::ScheduleOccurrence::Projected {
            occurrence_key,
            source_task_id: restored_source_task_id,
        } if occurrence_key == &format!("{source_task_id}:{deadline_epoch_ms}")
            && restored_source_task_id == source_task_id
    ));
    assert!(loaded.warning().is_none());
}

#[test]
fn view_state_v2は不正なprojected_identityを復元しない() {
    let storage = MemoryStorage::default();
    let source_task_id = "00000000-0000-4000-8000-000000000010";
    let deadline_epoch_ms = 1_789_228_800_000_i64;
    for raw in [
        projected_v2_raw(
            source_task_id,
            &format!("00000000-0000-4000-8000-000000000011:{deadline_epoch_ms}"),
            deadline_epoch_ms,
        ),
        projected_v2_raw(
            source_task_id,
            &format!("{source_task_id}:{deadline_epoch_ms}"),
            deadline_epoch_ms + 1,
        ),
    ] {
        *storage.value.borrow_mut() = Some(raw.clone());

        let loaded = load_view_state(&storage);

        assert!(loaded.state().is_none(), "{raw}");
        assert_eq!(
            loaded.warning(),
            Some("前回の画面状態が不正なため復元しませんでした。")
        );
        assert_eq!(storage.value.borrow().as_deref(), Some(raw.as_str()));
    }
}

#[test]
fn view_state_v3はactive_modeの日付buttonを構築できない境界を保存と読込で拒否する() {
    let storage = MemoryStorage::default();
    for (mode, logical_date) in [
        (ListMode::Scheduled, "9999-12-31"),
        (ListMode::Completed, "0000-01-01"),
    ] {
        let state = boundary_view_state(mode, logical_date);
        assert_eq!(
            store_view_state(&storage, &state),
            Err(ViewStateStoreError::InvalidState)
        );

        let mut raw = serde_json::to_value(&state).unwrap();
        raw.as_object_mut()
            .unwrap()
            .insert("version".to_owned(), serde_json::json!(3));
        *storage.value.borrow_mut() = Some(raw.to_string());
        let loaded = load_view_state(&storage);
        assert!(loaded.state().is_none(), "{mode:?} {logical_date}");
        assert!(loaded.warning().is_some(), "{mode:?} {logical_date}");
    }
}

#[test]
fn view_state_v2も予定button上限を越えるsnapshotを復元しない() {
    let storage = MemoryStorage::default();
    *storage.value.borrow_mut() = Some(
        serde_json::json!({
            "version": 2,
            "snapshot": {
                "observed_at_epoch_ms": 1_789_000_000_000_i64,
                "logical_date": "9999-12-31",
                "buffer_seconds": 60
            },
            "list": null,
            "active_tab": "list",
            "task_name_filter": "",
            "date_input_text": ""
        })
        .to_string(),
    );

    let loaded = load_view_state(&storage);
    assert!(loaded.state().is_none());
    assert!(loaded.warning().is_some());
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
                task_display_kind: Default::default(),
            }],
            in_progress_actual_work_seconds: None,
            total_actual_work_seconds: 1,
            available_seconds: 10,
            recorded_percentage: Some(10),
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
fn view_state_v3は不整合な完了summaryを拒否する() {
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
            rows: Vec::new(),
            in_progress_actual_work_seconds: None,
            total_actual_work_seconds: -1,
            available_seconds: 10,
            recorded_percentage: Some(0),
        }),
        active_tab: ActiveTab::List,
        task_name_filter: String::new(),
        date_input_text: String::new(),
    };
    assert_eq!(
        store_view_state(&storage, &state),
        Err(ViewStateStoreError::InvalidState)
    );

    if let Some(StoredActiveList::Completed {
        total_actual_work_seconds,
        recorded_percentage,
        ..
    }) = &mut state.list
    {
        *total_actual_work_seconds = 0;
        *recorded_percentage = Some(1);
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
    let plan = rows[0].defer_plan.as_mut().unwrap();
    plan.mode = schronu_web::DeferMode::DeadlineLimited;
    plan.effective_pending_until_epoch_ms = Some(plan.requested_pending_until_epoch_ms);

    assert_eq!(
        store_view_state(&storage, &state),
        Err(ViewStateStoreError::InvalidState)
    );
}

#[test]
fn view_stateはprojected行をactionable_idなしで保存復元する() {
    let storage = MemoryStorage::default();
    let mut projected = row("00000000-0000-4000-8000-000000000001", "筋トレ(9/12)");
    projected.task.task_id = None;
    projected.occurrence = schronu_web::ScheduleOccurrence::Projected {
        occurrence_key: "00000000-0000-4000-8000-000000000010:1789228800000".to_owned(),
        source_task_id: "00000000-0000-4000-8000-000000000010".to_owned(),
    };
    projected.deadline_epoch_ms = Some(1_789_228_800_000);
    projected.defer_plan = None;
    let state = view_state("2026-09-12", vec![projected]);

    store_view_state(&storage, &state).unwrap();
    let loaded = load_view_state(&storage).into_state().unwrap();

    let Some(StoredActiveList::Scheduled { rows, .. }) = loaded.list else {
        panic!("scheduled list expected");
    };
    let restored = &rows[0];
    assert!(restored.task.task_id.is_none());
    assert_eq!(
        restored.occurrence.occurrence_key(),
        Some("00000000-0000-4000-8000-000000000010:1789228800000")
    );
}

#[test]
fn view_stateは非canonicalなprojected_occurrence_keyを全体ごと復元しない() {
    let storage = MemoryStorage::default();
    let source_task_id = "00000000-0000-4000-8000-000000000010";
    let deadline_epoch_ms = 1_789_228_800_000_i64;
    let mut projected = row("00000000-0000-4000-8000-000000000001", "筋トレ(9/12)");
    projected.task.task_id = None;
    projected.occurrence = schronu_web::ScheduleOccurrence::Projected {
        occurrence_key: format!("{source_task_id}:{deadline_epoch_ms}"),
        source_task_id: source_task_id.to_owned(),
    };
    projected.deadline_epoch_ms = Some(deadline_epoch_ms);
    projected.defer_plan = None;
    let state = view_state("2026-09-12", vec![projected]);
    store_view_state(&storage, &state).unwrap();
    let valid_raw = storage.value.borrow().clone().unwrap();

    for (occurrence_key, row_deadline_epoch_ms) in [
        ("garbage".to_owned(), deadline_epoch_ms),
        (
            format!("00000000-0000-4000-8000-000000000011:{deadline_epoch_ms}"),
            deadline_epoch_ms,
        ),
        (format!("{source_task_id}:not-an-epoch"), deadline_epoch_ms),
        (
            format!("{source_task_id}:9223372036854775807"),
            deadline_epoch_ms,
        ),
        (format!("{source_task_id}:1789228800001"), deadline_epoch_ms),
        (
            format!("{source_task_id}:+1789228800000"),
            deadline_epoch_ms,
        ),
        (
            format!("{source_task_id}:01789228800000"),
            deadline_epoch_ms,
        ),
        (format!("{source_task_id}:-0"), 0),
    ] {
        let mut value: serde_json::Value = serde_json::from_str(&valid_raw).unwrap();
        value["list"]["rows"][0]["occurrence"]["occurrence_key"] =
            serde_json::Value::String(occurrence_key);
        value["list"]["rows"][0]["deadline_epoch_ms"] = row_deadline_epoch_ms.into();
        let raw = serde_json::to_string(&value).unwrap();
        *storage.value.borrow_mut() = Some(raw.clone());

        let loaded = load_view_state(&storage);

        assert!(loaded.state().is_none(), "{raw}");
        assert!(loaded.warning().is_some(), "{raw}");
    }
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

fn boundary_view_state(list_mode: ListMode, logical_date: &str) -> ViewState {
    ViewState {
        snapshot: ServerSnapshot {
            observed_at_epoch_ms: 1_789_000_000_000,
            logical_date: logical_date.to_owned(),
            buffer_seconds: 60,
        },
        list_mode,
        list: None,
        active_tab: ActiveTab::List,
        task_name_filter: String::new(),
        date_input_text: String::new(),
    }
}

fn row(task_id: &str, task_name: &str) -> ScheduledTaskRow {
    ScheduledTaskRow {
        task: SessionTask {
            task_id: task_id.to_owned(),
            task_name: task_name.to_owned(),
            estimated_work_seconds: 900,
            actual_work_seconds: 60,
        }
        .into(),
        occurrence: schronu_web::ScheduleOccurrence::Actual {
            task_id: task_id.to_owned(),
        },
        schedule_start_epoch_ms: 1_789_000_000_000,
        schedule_end_epoch_ms: 1_789_000_900_000,
        deadline_epoch_ms: None,
        deadline_label: "____/__/__".to_owned(),
        misses_deadline: false,
        task_display_kind: schronu_web::TaskDisplayKind::NonRepetitive,
        deadline_display_kind: schronu_web::DeadlineDisplayKind::None,
        is_leaf: true,
        defer_plan: Some(schronu_web::DeferPlan {
            mode: schronu_web::DeferMode::Normal,
            requested_pending_until_epoch_ms: 1_789_086_400_000,
            effective_pending_until_epoch_ms: None,
            repetition_interval_days: None,
        }),
    }
}

fn projected_v2_raw(
    source_task_id: &str,
    occurrence_key: &str,
    row_deadline_epoch_ms: i64,
) -> String {
    serde_json::json!({
        "version": 2,
        "snapshot": {
            "observed_at_epoch_ms": 1_789_000_000_000_i64,
            "logical_date": "2026-09-09",
            "buffer_seconds": 60
        },
        "list": {
            "logical_date": "2026-09-12",
            "rows": [{
                "task": {
                    "task_id": null,
                    "task_name": "筋トレ(9/12)",
                    "estimated_work_seconds": 900,
                    "actual_work_seconds": 0
                },
                "occurrence": {
                    "kind": "projected",
                    "occurrence_key": occurrence_key,
                    "source_task_id": source_task_id
                },
                "schedule_start_epoch_ms": 1_789_228_000_000_i64,
                "schedule_end_epoch_ms": 1_789_228_800_000_i64,
                "deadline_epoch_ms": row_deadline_epoch_ms,
                "deadline_label": "09/12",
                "misses_deadline": false,
                "task_display_kind": "repetitive",
                "deadline_display_kind": "today",
                "is_leaf": true,
                "defer_plan": null
            }]
        },
        "active_tab": "list",
        "task_name_filter": "筋トレ",
        "date_input_text": "2026/9/12"
    })
    .to_string()
}
