use super::component::{
    app, initial_load_phase, BufferPanel, InitialLoadPhase, InitialLoadView, InteractiveShell,
    LoadingOverlay, NavigationTabs, SessionChrome,
};
#[cfg(feature = "web")]
use super::component_models::BrowserPageModel;
use super::component_runtime::{
    component_action_from_date_button, component_action_from_date_input,
    component_action_from_session_action, component_actions_from_session_action, initialize_client,
    reduce_component_action_at, ComponentAction, ComponentOrchestrator,
};
use super::effect_dispatcher::ClientResponse;
use super::session_view::{SessionAction, SessionActionKind};
use super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
use crate::client::date_input::DateInputState;
use crate::client::state::{ActiveTab, ClientEffect, ServerFailure};
use crate::client::work_sessions::{KeyValueStorage, StorageError};
use crate::{
    web_error_codes, RecordSessionResult, RetryAdvice, ServerSnapshot, SessionTask, WebError,
    WebSuccess,
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

fn navigation_root(props: NavigationProps) -> dioxus::prelude::Element {
    rsx! {
        NavigationTabs {
            active_tab: props.active_tab,
            on_switch: move |tab| props.events.lock().unwrap().push(tab),
        }
    }
}

#[test]
fn 固定navigationは3tabの選択状態とcallbackを提供する() {
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
    assert_eq!(html.matches("class=\"tab-button").count(), 3, "{html}");
    for label in ["セッション", "一覧", "発火履歴"] {
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
        [ActiveTab::History, ActiveTab::List, ActiveTab::Session]
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
fn 三終了操作はbrowser時刻のtick後にdispatchする() {
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
    assert!(matches!(
        discard.as_slice(),
        [ComponentAction::DiscardSession(task_id)] if task_id == "task"
    ));
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
        ComponentAction::DiscardSession(RECORD_ID.to_owned()),
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

    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::SwitchTab(ActiveTab::List),
    );
    let effect = reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );

    assert_eq!(effect, ClientEffect::None);
    assert_eq!(state.sessions().len(), 1);
    assert_eq!(state.active_tab(), ActiveTab::Session);
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
        let effect = reduce_component_action_at(
            &mut state,
            &storage,
            1_000,
            ComponentAction::AddSession { task, is_leaf },
        );

        assert_eq!(effect, ClientEffect::None);
        assert_eq!(state.sessions().len(), 1);
        assert_eq!(state.active_tab(), ActiveTab::List);
    }

    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    let effect = reduce_component_action_at(
        &mut state,
        &storage,
        1_001,
        ComponentAction::AddSession {
            task: task(COMPLETE_ID),
            is_leaf: true,
        },
    );

    assert_eq!(effect, ClientEffect::None);
    assert_eq!(state.sessions().len(), 1);
    assert_eq!(state.active_tab(), ActiveTab::List);
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

    let effect = reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );

    assert_eq!(effect, ClientEffect::None);
    assert!(state.sessions().is_empty());
    assert_eq!(state.active_tab(), ActiveTab::List);
}

#[test]
fn 持ち歩きロックはguard対象actionのたびに無操作期限を延長する() {
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

        let _ = reduce_component_action_at(&mut action_state, &action_storage, 2_001, action);

        assert_eq!(
            action_state.carry_lock_mode(),
            crate::client::carry_lock::CarryLockMode::ArmedUntil(17_001),
            "成功・失敗や実変更の有無によらずdispatch時点から15秒延長する"
        );
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
        crate::client::carry_lock::CarryLockMode::ArmedUntil(25_000),
        "次のguard対象actionから無操作期限を再計算する"
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
        current_logical_date,
        history,
        warnings,
        safety_warning,
        display_error,
        global_blocked,
        can_confirm,
        auto_session_in_flight,
        auto_session_empty,
        carry_lock,
    } = model;
    let _ = (
        active_tab,
        buffer,
        sessions,
        rows,
        active_task_ids,
        dates,
        current_logical_date,
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
