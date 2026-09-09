use super::component::{
    app, BackgroundRefreshStatus, BufferPanel, InteractiveShell, LoadingOverlay, NavigationTabs,
    SessionChrome,
};
#[cfg(feature = "web")]
use super::component_models::BrowserPageModel;
use super::component_runtime::{
    component_action_from_date_button, component_action_from_date_input,
    component_action_from_session_action, component_actions_from_session_action, initialize_client,
    reduce_component_action_at, reset_task_name_filter_after_session_add, ComponentAction,
    ComponentOrchestrator,
};
use super::effect_dispatcher::ClientResponse;
use super::session_view::{SessionAction, SessionActionKind};
use super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
use crate::client::date_input::DateInputState;
use crate::client::state::{ActiveTab, ClientEffect, ServerFailure};
use crate::client::view_state::{load_view_state, store_view_state, StoredListView, ViewState};
use crate::client::work_sessions::{KeyValueStorage, StorageError};
use crate::{
    web_error_codes, RecordSessionResult, RetryAdvice, ScheduledTaskRow, ServerSnapshot,
    SessionTask, WebError, WebSuccess,
};
use dioxus::dioxus_core::{AttributeValue, Mutation};
use dioxus::prelude::VirtualDom;
use dioxus::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct NavigationProps {
    active_tab: ActiveTab,
    events: Arc<Mutex<Vec<ActiveTab>>>,
}

#[test]
fn background更新はshellを塞がずstale状態と再試行を表示する() {
    fn refreshing() -> Element {
        rsx! {
            InteractiveShell { blocked: false,
                BackgroundRefreshStatus { has_cached_list: true, failed: false }
                button { "操作可能" }
            }
        }
    }
    let mut refreshing_dom = VirtualDom::new(refreshing);
    refreshing_dom.rebuild_in_place();
    let refreshing_html = dioxus::ssr::render(&refreshing_dom);
    assert!(refreshing_html.contains("前回の表示です。最新状態を確認中…"));
    assert!(!refreshing_html.contains(" inert"));
    assert!(!refreshing_html.contains("loading-overlay"));

    fn failed() -> Element {
        rsx! {
            BackgroundRefreshStatus {
                has_cached_list: true,
                failed: true,
                on_retry: move |_| {},
            }
        }
    }
    let mut failed_dom = VirtualDom::new(failed);
    failed_dom.rebuild_in_place();
    let failed_html = dioxus::ssr::render(&failed_dom);
    assert!(failed_html.contains("前回の表示です。最新状態を確認できませんでした。"));
    assert!(failed_html.contains(">再試行<"));
}

fn navigation_root(props: NavigationProps) -> dioxus::prelude::Element {
    rsx! {
        NavigationTabs {
            active_tab: props.active_tab,
            on_switch: move |tab| props.events.lock().unwrap().push(tab),
        }
    }
}

#[test]
fn 固定navigationは4tabの選択状態とcallbackを提供する() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut dom = VirtualDom::new_with_props(
        navigation_root,
        NavigationProps {
            active_tab: ActiveTab::History,
            events: Arc::clone(&events),
        },
    );
    let ids = rebuild_with_click_listeners(&mut dom);
    let html = dioxus::ssr::render(&dom);

    assert!(html.contains("<nav class=\"tabs\""), "{html}");
    assert_eq!(html.matches("class=\"tab-button").count(), 4, "{html}");
    for label in ["セッション", "一覧", "発火履歴", "集計"] {
        assert!(html.contains(label), "missing {label}: {html}");
    }
    assert!(
        html.contains(
            "class=\"tab-button is-selected\" type=\"button\" aria-pressed=true>発火履歴"
        ),
        "{html}"
    );

    for id in ids {
        dispatch_click(&dom, id);
    }
    assert_eq!(
        *events.lock().unwrap(),
        [ActiveTab::Summary, ActiveTab::History, ActiveTab::List, ActiveTab::Session]
    );
}

#[test]
fn 日付入力actionは正規化して選択し日付buttonは入力をclearする() {
    let mut date_input = DateInputState::default();
    date_input.edit("9/16".to_owned());

    let action = component_action_from_date_input(&mut date_input, "2026-09-16")
        .expect("valid date input must create one action");
    assert!(matches!(
        action,
        ComponentAction::SelectDate(ref date) if date == "2026-09-16"
    ));
    assert_eq!(date_input.text(), "2026/9/16");

    let action =
        component_action_from_date_button(&mut date_input, "2026-09-17".to_owned());
    assert!(matches!(
        action,
        ComponentAction::SelectDate(ref date) if date == "2026-09-17"
    ));
    assert_eq!(date_input.text(), "");
    assert_eq!(date_input.error(), None);
}

#[test]
fn session操作は対応するcomponent_actionへ変換する() {
    for (kind, expected) in [
        (SessionActionKind::Discard, "discard"),
        (SessionActionKind::Record, "record"),
        (SessionActionKind::Complete, "complete"),
        (
            SessionActionKind::CompleteWithoutRecording,
            "complete_without_recording",
        ),
        (
            SessionActionKind::ResumeCompletionConflict,
            "resume_conflict",
        ),
        (
            SessionActionKind::ConfirmCompletionConflict,
            "confirm_conflict",
        ),
    ] {
        let action = component_action_from_session_action(SessionAction {
            task_id: "task".to_owned(),
            kind,
        });
        let actual = match action {
            ComponentAction::DiscardSession(task_id) if task_id == "task" => "discard",
            ComponentAction::RecordSession(task_id) if task_id == "task" => "record",
            ComponentAction::CompleteSession(task_id) if task_id == "task" => "complete",
            ComponentAction::CompleteSessionWithoutRecording(task_id) if task_id == "task" => {
                "complete_without_recording"
            }
            ComponentAction::ResumeCompletionConflict(task_id) if task_id == "task" => {
                "resume_conflict"
            }
            ComponentAction::ConfirmCompletionConflict(task_id) if task_id == "task" => {
                "confirm_conflict"
            }
            _ => "unexpected",
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn 四終了操作はbrowser時刻のtick後にdispatchする() {
    for kind in [
        SessionActionKind::Record,
        SessionActionKind::Complete,
        SessionActionKind::CompleteWithoutRecording,
    ] {
        let actions = component_actions_from_session_action(
            SessionAction {
                task_id: "task".to_owned(),
                kind,
            },
            60_000,
        );

        assert!(matches!(
            actions.first(),
            Some(ComponentAction::Tick {
                wall_now_epoch_ms: 60_000
            })
        ));
        assert_eq!(actions.len(), 2);
    }

    let discard = component_actions_from_session_action(
        SessionAction {
            task_id: "task".to_owned(),
            kind: SessionActionKind::Discard,
        },
        60_000,
    );
    assert!(matches!(discard.as_slice(), [ComponentAction::Tick { wall_now_epoch_ms: 60_000 }, ComponentAction::DiscardSession(task_id)] if task_id == "task"));
}

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
        ComponentAction::Tick {
            wall_now_epoch_ms: 2_000,
        },
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
        ComponentAction::ConfirmRepositoryChecked,
    ] {
        assert_eq!(
            reduce_component_action_at(&mut state, &storage, 2_000, action),
            ClientEffect::None
        );
    }

    assert!(matches!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_000,
            ComponentAction::SelectDate("2026-09-05".to_owned())
        ),
        ClientEffect::ListTasks { request, .. } if request.logical_date == "2026-09-05"
    ));

    let discard_storage = MemoryStorage::default();
    let (mut discard_state, _) = initialize_client(&discard_storage, 1_000);
    reduce_component_action_at(&mut discard_state, &discard_storage, 1_000, ComponentAction::AddSession { task: task(RECORD_ID), is_leaf: true });
    assert!(matches!(
        reduce_component_action_at(&mut discard_state, &discard_storage, 2_000, ComponentAction::DiscardSession(RECORD_ID.to_owned())),
        ClientEffect::DiscardSession { .. }
    ));
    assert!(matches!(
        reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::AutoSession),
        ClientEffect::AutoSession { .. }
    ));

    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_000,
            ComponentAction::AddSession {
                task: task(RECORD_ID),
                is_leaf: true,
            }
        ),
        ClientEffect::None
    );
    assert!(matches!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_000,
            ComponentAction::RecordSession(RECORD_ID.to_owned())
        ),
        ClientEffect::RecordSession { request, .. } if request.task_id == RECORD_ID
    ));

    let other_storage = MemoryStorage::default();
    let (mut other_state, _) = initialize_client(&other_storage, 1_000);
    reduce_component_action_at(
        &mut other_state,
        &other_storage,
        1_000,
        ComponentAction::AddSession {
            task: task(COMPLETE_ID),
            is_leaf: true,
        },
    );
    assert!(matches!(
        reduce_component_action_at(
            &mut other_state,
            &other_storage,
            1_000,
            ComponentAction::CompleteSession(COMPLETE_ID.to_owned())
        ),
        ClientEffect::CompleteSession { request, .. } if request.task_id == COMPLETE_ID
            && request.record_elapsed_seconds
    ));

    let discard_complete_storage = MemoryStorage::default();
    let (mut discard_complete_state, _) = initialize_client(&discard_complete_storage, 1_000);
    reduce_component_action_at(
        &mut discard_complete_state,
        &discard_complete_storage,
        1_000,
        ComponentAction::AddSession {
            task: task(COMPLETE_ID),
            is_leaf: true,
        },
    );
    assert!(matches!(
        reduce_component_action_at(
            &mut discard_complete_state,
            &discard_complete_storage,
            1_000,
            ComponentAction::CompleteSessionWithoutRecording(COMPLETE_ID.to_owned())
        ),
        ClientEffect::CompleteSession { request, .. } if request.task_id == COMPLETE_ID
            && !request.record_elapsed_seconds
    ));
}

#[test]
fn 一覧のセッション追加後はセッションtabへ切り替えられserver通信を発生させない() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    let mut task_name_filter = "  実装  ".to_owned();
    let mut date_input = DateInputState::default();
    date_input.edit("2026/9/16".to_owned());

    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::SwitchTab(ActiveTab::List),
    );
    let previous_session_count = state.sessions().len();
    let effect = reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    reset_task_name_filter_after_session_add(
        &mut task_name_filter,
        previous_session_count,
        state.sessions().len(),
    );

    assert_eq!(effect, ClientEffect::None);
    assert_eq!(state.sessions().len(), 1);
    assert_eq!(state.active_tab(), ActiveTab::Session);
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::Normal
    );
    assert!(state.history().is_empty());
    assert!(task_name_filter.is_empty());
    assert_eq!(date_input.text(), "2026/9/16");
    assert_eq!(date_input.error(), None);
}

#[test]
fn 製品orchestratorの一覧追加は成功時だけ検索を解除し日付状態を保つ() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    let bootstrap = orchestrator.mount(&storage, 1_000);
    let bootstrap_request_id = match bootstrap {
        ClientEffect::Bootstrap { request_id } => request_id,
        _ => panic!("mount must request bootstrap"),
    };
    orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: bootstrap_request_id,
            result: Ok(ServerSnapshot {
                observed_at_epoch_ms: 1_000,
                logical_date: "2026-09-05".to_owned(),
                buffer_seconds: 60,
            }),
        },
    );
    orchestrator.action(
        &storage,
        1_000,
        ComponentAction::SwitchTab(ActiveTab::List),
    );
    let list_effect = orchestrator.action(
        &storage,
        1_000,
        ComponentAction::SelectDate("2026-09-16".to_owned()),
    );
    let request_id = match list_effect {
        ClientEffect::ListTasks { request_id, .. } => request_id,
        _ => panic!("date selection must request the list"),
    };
    orchestrator.apply_response(
        &storage,
        ClientResponse::ListTasks {
            request_id,
            requested_date: "2026-09-16".to_owned(),
            result: Ok(WebSuccess {
                snapshot: ServerSnapshot {
                    observed_at_epoch_ms: 1_000,
                    logical_date: "2026-09-05".to_owned(),
                    buffer_seconds: 60,
                },
                data: Vec::new(),
            }),
        },
    );
    let mut date_input = DateInputState::default();
    date_input.edit("不正".to_owned());
    assert_eq!(date_input.submit("2026-09-05"), None);
    assert!(date_input.error().is_some());
    orchestrator.edit_task_name_filter(&storage, "実装".to_owned());
    let previous_history_len = orchestrator.state().unwrap().history().len();

    let effect = orchestrator.start_session_from_list(
        &storage,
        1_000,
        task(RECORD_ID),
        true,
    );

    let state = orchestrator.state().unwrap();
    assert_eq!(effect, ClientEffect::None);
    assert_eq!(state.active_tab(), ActiveTab::Session);
    assert_eq!(state.selected_logical_date(), Some("2026-09-16"));
    assert_eq!(state.history().len(), previous_history_len);
    assert!(orchestrator.task_name_filter().is_empty());
    assert_eq!(date_input.text(), "不正");
    assert!(date_input.error().is_some());
}

#[test]
fn 製品orchestratorの一覧追加は保存失敗時に検索を保つ() {
    let storage = MemoryStorage::failing_writes();
    let mut orchestrator = ComponentOrchestrator::new();
    orchestrator.mount(&storage, 1_000);
    orchestrator.action(
        &storage,
        1_000,
        ComponentAction::SwitchTab(ActiveTab::List),
    );
    orchestrator.edit_task_name_filter(&storage, "保存失敗".to_owned());

    let effect = orchestrator.start_session_from_list(
        &storage,
        1_000,
        task(RECORD_ID),
        true,
    );

    assert_eq!(effect, ClientEffect::None);
    assert!(orchestrator.state().unwrap().sessions().is_empty());
    assert_eq!(orchestrator.task_name_filter(), "保存失敗");
}

#[test]
fn 一覧のセッション追加が拒否された場合は一覧tabに留まる() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);

    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::SwitchTab(ActiveTab::List),
    );

    for (task, is_leaf) in [(task(RECORD_ID), true), (task(COMPLETE_ID), false)] {
        let mut task_name_filter = "絞り込み中".to_owned();
        let previous_session_count = state.sessions().len();
        let effect = reduce_component_action_at(
            &mut state,
            &storage,
            1_000,
            ComponentAction::AddSession { task, is_leaf },
        );
        reset_task_name_filter_after_session_add(
            &mut task_name_filter,
            previous_session_count,
            state.sessions().len(),
        );

        assert_eq!(effect, ClientEffect::None);
        assert_eq!(state.sessions().len(), 1);
        assert_eq!(state.active_tab(), ActiveTab::List);
        assert_eq!(task_name_filter, "絞り込み中");
    }

    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    let mut task_name_filter = "ロック中".to_owned();
    let previous_session_count = state.sessions().len();
    let effect = reduce_component_action_at(
        &mut state,
        &storage,
        1_001,
        ComponentAction::AddSession {
            task: task(COMPLETE_ID),
            is_leaf: true,
        },
    );
    reset_task_name_filter_after_session_add(
        &mut task_name_filter,
        previous_session_count,
        state.sessions().len(),
    );

    assert_eq!(effect, ClientEffect::None);
    assert_eq!(state.sessions().len(), 1);
    assert_eq!(state.active_tab(), ActiveTab::List);
    assert_eq!(task_name_filter, "ロック中");
}

#[test]
fn 一覧のセッション保存失敗時は一覧tabに留まる() {
    let storage = MemoryStorage::failing_writes();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::SwitchTab(ActiveTab::List),
    );

    let mut task_name_filter = "保存失敗".to_owned();
    let previous_session_count = state.sessions().len();
    let effect = reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    reset_task_name_filter_after_session_add(
        &mut task_name_filter,
        previous_session_count,
        state.sessions().len(),
    );

    assert_eq!(effect, ClientEffect::None);
    assert!(state.sessions().is_empty());
    assert_eq!(state.active_tab(), ActiveTab::List);
    assert_eq!(task_name_filter, "保存失敗");
}

#[test]
fn 持ち歩きロックはguard対象actionの期限を延長し最初のsession追加成功時は再ロックする() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );

    for action in [
        ComponentAction::AutoSession,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
        ComponentAction::DiscardSession(RECORD_ID.to_owned()),
        ComponentAction::RecordSession(RECORD_ID.to_owned()),
        ComponentAction::CompleteSession(RECORD_ID.to_owned()),
        ComponentAction::CompleteSessionWithoutRecording(RECORD_ID.to_owned()),
        ComponentAction::ResumeCompletionConflict(RECORD_ID.to_owned()),
        ComponentAction::ConfirmCompletionConflict(RECORD_ID.to_owned()),
        ComponentAction::ConfirmRepositoryChecked,
    ] {
        assert_eq!(
            reduce_component_action_at(&mut state, &storage, 1_001, action),
            ClientEffect::None
        );
    }

    for action in [
        ComponentAction::AutoSession,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
        ComponentAction::DiscardSession(RECORD_ID.to_owned()),
        ComponentAction::RecordSession(RECORD_ID.to_owned()),
        ComponentAction::CompleteSession(RECORD_ID.to_owned()),
        ComponentAction::CompleteSessionWithoutRecording(RECORD_ID.to_owned()),
        ComponentAction::ResumeCompletionConflict(RECORD_ID.to_owned()),
        ComponentAction::ConfirmCompletionConflict(RECORD_ID.to_owned()),
        ComponentAction::ConfirmRepositoryChecked,
    ] {
        let action_storage = MemoryStorage::default();
        let (mut action_state, _) = initialize_client(&action_storage, 1_000);
        reduce_component_action_at(
            &mut action_state,
            &action_storage,
            1_000,
            ComponentAction::EnableCarryLock,
        );
        reduce_component_action_at(
            &mut action_state,
            &action_storage,
            2_000,
            ComponentAction::ArmCarryLock,
        );

        let adds_first_session =
            matches!(&action, ComponentAction::AddSession { is_leaf: true, .. });
        let _ = reduce_component_action_at(&mut action_state, &action_storage, 2_001, action);

        let expected = if adds_first_session {
            crate::client::carry_lock::CarryLockMode::Locked
        } else {
            crate::client::carry_lock::CarryLockMode::ArmedUntil(17_001)
        };
        assert_eq!(action_state.carry_lock_mode(), expected);
    }

    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);
    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_001,
            ComponentAction::SwitchTab(ActiveTab::List)
        ),
        ClientEffect::None
    );
    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_002,
            ComponentAction::Tick {
                wall_now_epoch_ms: 2_002,
            }
        ),
        ClientEffect::None
    );
    assert!(matches!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_002,
            ComponentAction::SelectDate("2026-09-05".to_owned())
        ),
        ClientEffect::ListTasks { .. }
    ));
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(17_000),
        "閲覧操作では無操作期限を延長しない"
    );
    assert!(matches!(
        reduce_component_action_at(&mut state, &storage, 2_003, ComponentAction::AutoSession),
        ClientEffect::AutoSession { .. }
    ));
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(17_003)
    );
    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            10_000,
            ComponentAction::AddSession {
                task: task(RECORD_ID),
                is_leaf: true,
            }
        ),
        ClientEffect::None
    );
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::Locked,
        "最初のsession追加成功後は期限延長より即時再ロックを優先する"
    );
    reduce_component_action_at(
        &mut state,
        &storage,
        2_004,
        ComponentAction::DisableCarryLock,
    );

    let failing_storage = MemoryStorage::failing_writes();
    let (mut failing_state, _) = initialize_client(&failing_storage, 1_000);
    reduce_component_action_at(
        &mut failing_state,
        &failing_storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(
        &mut failing_state,
        &failing_storage,
        2_000,
        ComponentAction::ArmCarryLock,
    );
    reduce_component_action_at(
        &mut failing_state,
        &failing_storage,
        3_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    assert_eq!(
        failing_state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(18_000),
        "localStorage失敗でもdispatch時点から期限を延長する"
    );
}

#[test]
fn 一時許可中に一覧から最初のsessionを追加すると即時再ロックする() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);

    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_001,
            ComponentAction::AddSession {
                task: task(RECORD_ID),
                is_leaf: true,
            },
        ),
        ClientEffect::None
    );

    assert_eq!(state.sessions().len(), 1);
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::Locked
    );
}

#[test]
fn sessionが0件から1件へ増えない追加では一時許可を維持する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    reduce_component_action_at(
        &mut state,
        &storage,
        1_001,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);

    reduce_component_action_at(
        &mut state,
        &storage,
        2_001,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    assert_eq!(state.sessions().len(), 1);
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(17_001)
    );

    reduce_component_action_at(
        &mut state,
        &storage,
        2_002,
        ComponentAction::AddSession {
            task: task(COMPLETE_ID),
            is_leaf: true,
        },
    );
    assert_eq!(state.sessions().len(), 2);
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(17_002)
    );

    let failing_storage = MemoryStorage::failing_writes();
    let (mut failing_state, _) = initialize_client(&failing_storage, 1_000);
    reduce_component_action_at(
        &mut failing_state,
        &failing_storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(
        &mut failing_state,
        &failing_storage,
        2_000,
        ComponentAction::ArmCarryLock,
    );
    reduce_component_action_at(
        &mut failing_state,
        &failing_storage,
        2_001,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    assert!(failing_state.sessions().is_empty());
    assert_eq!(
        failing_state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(17_001)
    );
}

#[test]
fn armed期限判定はactionのmonotonic時刻を使い期限切れ操作を遮断する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);

    assert_eq!(
        reduce_component_action_at(&mut state, &storage, 17_000, ComponentAction::AutoSession),
        ClientEffect::None
    );
    assert!(state.carry_lock_locked());
}

#[test]
fn armedはmonotonic時刻が期限へ到達した時点でlockedへ戻る() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);

    reduce_component_action_at(
        &mut state,
        &storage,
        16_999,
        ComponentAction::Tick {
            wall_now_epoch_ms: i64::MAX,
        },
    );
    assert!(!state.carry_lock_locked());

    reduce_component_action_at(
        &mut state,
        &storage,
        17_000,
        ComponentAction::Tick {
            wall_now_epoch_ms: i64::MIN,
        },
    );
    assert!(state.carry_lock_locked());
}

#[test]
fn wall_clock変動にかかわらずarmedはmonotonic_15秒境界で失効する() {
    let storage = MemoryStorage::default();
    let (mut tick_state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut tick_state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(
        &mut tick_state,
        &storage,
        2_000,
        ComponentAction::ArmCarryLock,
    );
    reduce_component_action_at(
        &mut tick_state,
        &storage,
        2_500,
        ComponentAction::Tick {
            wall_now_epoch_ms: 50_000,
        },
    );
    assert_eq!(
        tick_state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(17_000)
    );
    reduce_component_action_at(
        &mut tick_state,
        &storage,
        3_000,
        ComponentAction::Tick {
            wall_now_epoch_ms: -50_000,
        },
    );
    assert_eq!(tick_state.tick_now_epoch_ms(), -50_000);
    assert!(!tick_state.carry_lock_locked());

    reduce_component_action_at(
        &mut tick_state,
        &storage,
        16_999,
        ComponentAction::Tick {
            wall_now_epoch_ms: i64::MAX,
        },
    );
    assert!(!tick_state.carry_lock_locked());

    reduce_component_action_at(
        &mut tick_state,
        &storage,
        17_000,
        ComponentAction::Tick {
            wall_now_epoch_ms: i64::MIN,
        },
    );
    assert!(tick_state.carry_lock_locked());
}

#[test]
fn armedはmonotonic時計が後退した時点でlockedへ戻る() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);
    reduce_component_action_at(
        &mut state,
        &storage,
        2_500,
        ComponentAction::Tick {
            wall_now_epoch_ms: 2_500,
        },
    );

    reduce_component_action_at(
        &mut state,
        &storage,
        2_499,
        ComponentAction::Tick {
            wall_now_epoch_ms: 2_499,
        },
    );

    assert!(state.carry_lock_locked());
}

#[test]
fn armedの即時再lock_actionは次の変更操作を遮断する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);

    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_001,
            ComponentAction::RelockCarryLock,
        ),
        ClientEffect::None
    );
    assert!(state.carry_lock_locked());
    assert_eq!(
        reduce_component_action_at(&mut state, &storage, 2_002, ComponentAction::AutoSession),
        ClientEffect::None
    );
}

#[test]
#[cfg(feature = "web")]
fn carry_lock_warningはbrowser_page_modelのwarningsへ合流する() {
    let storage = MemoryStorage::failing_carry_lock_reads();
    let (state, _) = initialize_client(&storage, 1_000);

    let model = BrowserPageModel::from_state(&state);

    assert!(model
        .warnings
        .iter()
        .any(|warning| warning.contains("持ち歩きロック")));
    let BrowserPageModel {
        active_tab,
        buffer,
        sessions,
        rows,
        active_task_ids,
        dates,
        history,
        warnings,
        safety_warning,
        display_error,
        global_blocked,
        can_confirm,
        auto_session_in_flight,
        auto_session_empty,
        carry_lock,
        discarded_summary: _,
    } = model;
    let _ = (
        active_tab,
        buffer,
        sessions,
        rows,
        active_task_ids,
        dates,
        history,
        warnings,
        safety_warning,
        display_error,
        global_blocked,
        can_confirm,
        auto_session_in_flight,
        auto_session_empty,
        carry_lock,
    );
}

#[test]
#[cfg(feature = "web")]
fn 集計read_errorはsummary内だけに表示してglobal_bannerと重複しない() {
    let storage = MemoryStorage::default();
    let (mut state, effect) = initialize_client(&storage, 1_000);
    let ClientEffect::Bootstrap { request_id } = effect else {
        panic!()
    };
    state.apply_bootstrap_result(
        request_id,
        Ok(ServerSnapshot {
            observed_at_epoch_ms: 1_000,
            logical_date: "2026-09-10".to_owned(),
            buffer_seconds: 0,
        }),
    );
    let ClientEffect::ListDiscardedSessions { request_id, .. } =
        state.switch_tab(ActiveTab::Summary)
    else {
        panic!()
    };
    state.apply_discarded_sessions_result(
        request_id,
        "2026-09-10",
        Err(ServerFailure::Operation(WebError {
            code: web_error_codes::REPOSITORY_UNAVAILABLE.to_owned(),
            message: "集計を取得できませんでした。".to_owned(),
            retry_advice: RetryAdvice::Retry,
            current_actual_work_seconds: None,
        })),
    );

    let model = BrowserPageModel::from_state(&state);
    assert!(matches!(
        model.discarded_summary,
        crate::client::state::DiscardedSessionsViewState::Error { .. }
    ));
    assert_eq!(model.display_error, None);
}
