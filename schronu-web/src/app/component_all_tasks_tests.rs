use super::component_runtime::{ComponentAction, ComponentOrchestrator};
use super::effect_dispatcher::ClientResponse;
use crate::client::state::{ActiveTab, AllTasksStatus, ClientEffect, ListSelection};
use crate::client::view_state::{store_view_state, StoredListView, ViewState};
use crate::client::work_sessions::{KeyValueStorage, StorageError};
use crate::{AllTaskPage, ServerSnapshot, WebSuccess};
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
