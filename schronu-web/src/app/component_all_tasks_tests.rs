use super::component_runtime::{ComponentAction, ComponentOrchestrator};
use super::effect_dispatcher::ClientResponse;
use crate::client::state::{ActiveTab, AllTasksStatus, ClientEffect, ListSelection};
use crate::client::view_state::{store_view_state, StoredListView, ViewState};
use crate::client::work_sessions::{KeyValueStorage, StorageError};
use crate::{AllTaskPage, AllTaskRow, ServerSnapshot, SessionTask, WebSuccess};
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Default)]
struct MemoryStorage(RefCell<HashMap<String, String>>);

impl KeyValueStorage for MemoryStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        Ok(self.0.borrow().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        self.0.borrow_mut().insert(key.to_owned(), value.to_owned());
        Ok(())
    }
}

fn snapshot(observed_at_epoch_ms: i64) -> ServerSnapshot {
    ServerSnapshot {
        observed_at_epoch_ms,
        logical_date: "2026-09-05".to_owned(),
        buffer_seconds: 0,
    }
}

fn all_row(segment_index: usize, task_id: &str, task_name: &str, is_leaf: bool) -> AllTaskRow {
    AllTaskRow {
        task: SessionTask {
            task_id: task_id.to_owned(),
            task_name: task_name.to_owned(),
            estimated_work_seconds: 900,
            actual_work_seconds: 300,
        },
        segment_index,
        schedule_date: "2026-09-06".to_owned(),
        deadline_epoch_ms: Some(1_000),
        deadline_label: "2026-09-06 07:00".to_owned(),
        misses_deadline: false,
        is_leaf,
    }
}

#[test]
fn reloadは日付別viewだけを復元しall選択とdataは復元しない() {
    let storage = MemoryStorage::default();
    store_view_state(
        &storage,
        &ViewState {
            snapshot: snapshot(1),
            list: Some(StoredListView {
                logical_date: "2026-09-06".to_owned(),
                rows: Vec::new(),
            }),
            active_tab: ActiveTab::List,
            task_name_filter: "date filter".to_owned(),
            date_input_text: String::new(),
        },
    )
    .unwrap();

    let mut orchestrator = ComponentOrchestrator::new();
    let bootstrap = orchestrator.mount(&storage, 2);
    assert!(orchestrator.effect_is_background(&bootstrap));
    let state = orchestrator.state().unwrap();
    assert_eq!(state.selected_logical_date(), Some("2026-09-06"));
    assert_eq!(state.list_selection(), ListSelection::Date);
    assert_eq!(state.all_tasks_status(), AllTasksStatus::NotLoaded);
    assert_eq!(orchestrator.task_name_filter(), "date filter");
}

#[test]
fn all取得はoverlay対象外で日付へ移動後もresponse_chainを継続する() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    let bootstrap = orchestrator.mount(&storage, 0);
    let bootstrap_id = match bootstrap {
        ClientEffect::Bootstrap { request_id } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };
    orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: bootstrap_id,
            result: Ok(snapshot(1)),
        },
    );

    let first = orchestrator.action(&storage, 0, ComponentAction::SelectAllTasks);
    assert!(orchestrator.effect_is_background(&first));
    assert!(!orchestrator.server_effect_in_flight());
    let (request_id, request) = match first {
        ClientEffect::ListAllTasks {
            request_id,
            request,
        } => (request_id, request),
        other => panic!("unexpected effect: {other:?}"),
    };

    let date = orchestrator.action(
        &storage,
        0,
        ComponentAction::SelectDate("2026-09-06".to_owned()),
    );
    assert!(matches!(date, ClientEffect::ListTasks { .. }));
    assert_eq!(
        orchestrator.state().unwrap().list_selection(),
        ListSelection::Date
    );

    let follow_up = orchestrator.apply_response(
        &storage,
        ClientResponse::ListAllTasks {
            request_id,
            request,
            result: Ok(WebSuccess {
                snapshot: snapshot(1),
                data: AllTaskPage {
                    rows: Vec::new(),
                    next_cursor: Some("snapshot:500".to_owned()),
                },
            }),
        },
    );
    assert!(matches!(follow_up, ClientEffect::ListAllTasks { .. }));
    assert!(orchestrator.effect_is_background(&follow_up));
    assert_eq!(
        orchestrator.state().unwrap().all_tasks_status(),
        AllTasksStatus::Loading
    );
}

#[test]
fn all検索はmemoryにだけ保持し日付検索と独立する() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    let bootstrap_id = match orchestrator.mount(&storage, 0) {
        ClientEffect::Bootstrap { request_id } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };
    let _ = orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: bootstrap_id,
            result: Ok(snapshot(1)),
        },
    );

    orchestrator.edit_task_name_filter(&storage, "date filter".to_owned());
    let _ = orchestrator.action(&storage, 0, ComponentAction::SelectAllTasks);
    orchestrator.edit_task_name_filter(&storage, "all filter".to_owned());
    assert_eq!(orchestrator.task_name_filter(), "all filter");

    let _ = orchestrator.action(
        &storage,
        0,
        ComponentAction::SelectDate("2026-09-06".to_owned()),
    );
    assert_eq!(orchestrator.task_name_filter(), "date filter");

    let _ = orchestrator.action(&storage, 0, ComponentAction::SelectAllTasks);
    assert_eq!(orchestrator.task_name_filter(), "all filter");

    let mut reloaded = ComponentOrchestrator::new();
    let _ = reloaded.mount(&storage, 0);
    let _ = reloaded.action(&storage, 0, ComponentAction::SelectAllTasks);
    assert_eq!(reloaded.task_name_filter(), "");
}

#[test]
fn all検索変更は表示上限を500へ戻す() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    let _ = orchestrator.mount(&storage, 0);
    let _ = orchestrator.action(&storage, 0, ComponentAction::SelectAllTasks);

    assert_eq!(orchestrator.all_tasks_visible_limit(), 500);
    orchestrator.show_more_all_tasks();
    assert_eq!(orchestrator.all_tasks_visible_limit(), 1_000);
    orchestrator.edit_task_name_filter(&storage, "設計".to_owned());
    assert_eq!(orchestrator.all_tasks_visible_limit(), 500);
}

#[test]
fn allからsession追加成功した時だけall検索をclearする() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    let bootstrap_id = match orchestrator.mount(&storage, 0) {
        ClientEffect::Bootstrap { request_id } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };
    let _ = orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: bootstrap_id,
            result: Ok(snapshot(1)),
        },
    );
    orchestrator.edit_task_name_filter(&storage, "date filter".to_owned());
    let _ = orchestrator.action(&storage, 0, ComponentAction::SelectAllTasks);
    orchestrator.edit_task_name_filter(&storage, "all filter".to_owned());

    let _ = orchestrator.start_session_from_list(
        &storage,
        0,
        SessionTask {
            task_id: "00000000-0000-4000-8000-000000000001".to_owned(),
            task_name: "task".to_owned(),
            estimated_work_seconds: 900,
            actual_work_seconds: 0,
        },
        true,
    );
    assert_eq!(orchestrator.state().unwrap().sessions().len(), 1);
    assert_eq!(orchestrator.task_name_filter(), "");
    assert_eq!(
        orchestrator.state().unwrap().active_tab(),
        ActiveTab::Session
    );

    let _ = orchestrator.action(
        &storage,
        0,
        ComponentAction::SelectDate("2026-09-06".to_owned()),
    );
    assert_eq!(orchestrator.task_name_filter(), "date filter");
}

#[test]
fn all行は日付labelとsegment_index_keyを使い先送りを持たない() {
    use crate::client::view_projection::project_all_task_rows;

    let rows = project_all_task_rows(&[
        all_row(12, "same", "設計", true),
        all_row(13, "same", "設計", true),
        all_row(14, "parent", "親", false),
    ]);
    assert_eq!(rows[0].row_key, "all:12");
    assert_eq!(rows[0].schedule_label, "2026/09/06(日)");
    assert!(rows[0].defer_plan.is_none());
    assert_eq!(rows[1].row_key, "all:13");
    assert!(!rows[2].is_leaf);
}
