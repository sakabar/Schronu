#![cfg(feature = "server")]

use super::component::app;
use super::component_runtime::{
    initialize_client, reduce_component_action, ComponentAction, ComponentOrchestrator,
};
use super::effect_dispatcher::ClientResponse;
use crate::client::state::{ActiveTab, ClientEffect};
use crate::client::work_sessions::{KeyValueStorage, StorageError};
use crate::ServerSnapshot;
use crate::SessionTask;
use dioxus::prelude::VirtualDom;
use std::cell::RefCell;
use std::collections::HashMap;

#[test]
fn 初期化はstorage失敗時もbootstrapを一度だけ要求する() {
    let storage = MemoryStorage::failing_reads();

    let (state, effect) = initialize_client(&storage, 1_000);

    assert_eq!(effect, ClientEffect::Bootstrap { request_id: 1 });
    assert!(state.storage_write_blocked());
    assert!(state.mutation_globally_blocked());
    assert!(state.sessions().is_empty());
}

#[test]
fn component_actionは仕様の五操作だけをserver_effectへ変換する() {
    let storage = MemoryStorage::default();
    let (mut state, bootstrap) = initialize_client(&storage, 1_000);
    assert!(matches!(bootstrap, ClientEffect::Bootstrap { .. }));

    for action in [
        ComponentAction::SwitchTab(ActiveTab::List),
        ComponentAction::Tick(2_000),
        ComponentAction::AddSession(task(RECORD_ID)),
        ComponentAction::DiscardSession(RECORD_ID.to_owned()),
        ComponentAction::ConfirmRepositoryChecked,
    ] {
        assert_eq!(
            reduce_component_action(&mut state, &storage, action),
            ClientEffect::None
        );
    }

    assert!(matches!(
        reduce_component_action(
            &mut state,
            &storage,
            ComponentAction::SelectDate("2026-09-05".to_owned())
        ),
        ClientEffect::ListTasks { request, .. } if request.logical_date == "2026-09-05"
    ));
    assert!(matches!(
        reduce_component_action(&mut state, &storage, ComponentAction::AutoSession),
        ClientEffect::AutoSession { .. }
    ));

    assert_eq!(
        reduce_component_action(
            &mut state,
            &storage,
            ComponentAction::AddSession(task(RECORD_ID))
        ),
        ClientEffect::None
    );
    assert!(matches!(
        reduce_component_action(
            &mut state,
            &storage,
            ComponentAction::RecordSession(RECORD_ID.to_owned())
        ),
        ClientEffect::RecordSession { request, .. } if request.task_id == RECORD_ID
    ));

    let other_storage = MemoryStorage::default();
    let (mut other_state, _) = initialize_client(&other_storage, 1_000);
    reduce_component_action(
        &mut other_state,
        &other_storage,
        ComponentAction::AddSession(task(COMPLETE_ID)),
    );
    assert!(matches!(
        reduce_component_action(
            &mut other_state,
            &other_storage,
            ComponentAction::CompleteSession(COMPLETE_ID.to_owned())
        ),
        ClientEffect::CompleteSession { request, .. } if request.task_id == COMPLETE_ID
            && request.record_elapsed_seconds
    ));

    let discard_complete_storage = MemoryStorage::default();
    let (mut discard_complete_state, _) = initialize_client(&discard_complete_storage, 1_000);
    reduce_component_action(
        &mut discard_complete_state,
        &discard_complete_storage,
        ComponentAction::AddSession(task(COMPLETE_ID)),
    );
    assert!(matches!(
        reduce_component_action(
            &mut discard_complete_state,
            &discard_complete_storage,
            ComponentAction::CompleteSessionWithoutRecording(COMPLETE_ID.to_owned())
        ),
        ClientEffect::CompleteSession { request, .. } if request.task_id == COMPLETE_ID
            && !request.record_elapsed_seconds
    ));
}

#[test]
fn native_ssrはbrowser_storageへ触れずloading_shellだけを描画する() {
    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);

    assert!(html.contains("Schronu"), "{html}");
    assert!(html.contains("読み込み中"), "{html}");
    assert!(!html.contains("schronu 今"), "{html}");
    assert!(!html.contains(">更新<"), "{html}");
}

#[test]
fn 製品orchestratorはmountを一度に制限しresponseとtickを同じstateへ適用する() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();

    let first = orchestrator.mount(&storage, 1_000);
    assert!(matches!(first, ClientEffect::Bootstrap { request_id: 1 }));
    assert_eq!(orchestrator.mount(&storage, 2_000), ClientEffect::None);

    orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: 1,
            result: Ok(ServerSnapshot {
                observed_at_epoch_ms: 1_000,
                logical_date: "2026-09-05".to_owned(),
                buffer_seconds: 60,
            }),
        },
    );
    assert_eq!(
        orchestrator
            .state()
            .unwrap()
            .snapshot()
            .unwrap()
            .buffer_seconds,
        60
    );
    assert_eq!(
        orchestrator.action(&storage, ComponentAction::Tick(3_000)),
        ClientEffect::None
    );
    assert_eq!(orchestrator.state().unwrap().tick_now_epoch_ms(), 3_000);
}

fn task(task_id: &str) -> SessionTask {
    SessionTask {
        task_id: task_id.to_owned(),
        task_name: format!("task {task_id}"),
        estimated_work_seconds: 600,
        actual_work_seconds: 0,
    }
}

const RECORD_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const COMPLETE_ID: &str = "123e4567-e89b-12d3-a456-426614174001";

#[derive(Default)]
struct MemoryStorage {
    values: RefCell<HashMap<String, String>>,
    fail_reads: bool,
}

impl MemoryStorage {
    fn failing_reads() -> Self {
        Self {
            values: RefCell::new(HashMap::new()),
            fail_reads: true,
        }
    }
}

impl KeyValueStorage for MemoryStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        if self.fail_reads {
            return Err(StorageError::ReadFailed);
        }
        Ok(self.values.borrow().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        self.values
            .borrow_mut()
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }
}
