#![cfg(feature = "server")]

use super::component::{app, InteractiveShell, NavigationTabs, SessionChrome};
#[cfg(feature = "web")]
use super::component_models::BrowserPageModel;
use super::component_runtime::{
    component_action_from_session_action, component_actions_from_session_action, initialize_client,
    reduce_component_action_at, ComponentAction, ComponentOrchestrator,
};
use super::effect_dispatcher::ClientResponse;
use super::session_view::{SessionAction, SessionActionKind};
use super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
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
fn component_actionはrank非0の手動session追加を拒否する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);

    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            1_000,
            ComponentAction::AddSession {
                task: task(RECORD_ID),
                is_leaf: false,
            },
        ),
        ClientEffect::None
    );

    assert!(state.sessions().is_empty());
    assert!(state.history().is_empty());
}

#[test]
fn native_ssrはbrowser_storageへ触れずloading_shellだけを描画する() {
    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);

    assert!(!html.contains("Schronu"), "{html}");
    assert!(html.contains("通信中…"), "{html}");
    assert!(html.contains("loading-overlay"), "{html}");
    assert!(html.contains("loading-spinner"), "{html}");
    assert!(html.contains("role=\"status\""), "{html}");
    assert!(html.contains("aria-live=\"polite\""), "{html}");
    assert!(html.contains("aria-busy=\"true\""), "{html}");
    assert!(!html.contains("schronu 今"), "{html}");
    assert!(!html.contains(">更新<"), "{html}");
}

#[test]
fn 共通chromeはsessionだけに表示する() {
    fn chrome_root(active_tab: ActiveTab) -> Element {
        rsx! {
            SessionChrome { active_tab,
                aside { class: "carry-lock-bar", "lock" }
                section { class: "buffer-panel", "buffer" }
            }
        }
    }

    for (active_tab, visible) in [
        (ActiveTab::Session, true),
        (ActiveTab::List, false),
        (ActiveTab::History, false),
    ] {
        let mut dom = VirtualDom::new_with_props(chrome_root, active_tab);
        dom.rebuild_in_place();
        let html = dioxus::ssr::render(&dom);
        assert_eq!(html.contains("carry-lock-bar"), visible, "{html}");
        assert_eq!(html.contains("buffer-panel"), visible, "{html}");
    }
}

fn interactive_shell_transition() -> Element {
    let mut blocked = use_signal(|| true);
    rsx! {
        InteractiveShell { blocked: blocked(),
            button { onclick: move |_| blocked.set(false), "response受理" }
        }
    }
}

#[test]
fn 通信完了時は製品mainからinert属性自体を除去する() {
    let mut dom = VirtualDom::new(interactive_shell_transition);
    let click_ids = rebuild_with_click_listeners(&mut dom);
    assert_eq!(click_ids.len(), 1);
    assert!(dioxus::ssr::render(&dom).contains(" inert"));

    dispatch_click(&dom, click_ids[0]);
    let mutations = dom.render_immediate_to_vec().edits;

    assert!(
        mutations.iter().any(|mutation| matches!(
            mutation,
            Mutation::SetAttribute {
                name: "inert",
                value: AttributeValue::None,
                ..
            }
        )),
        "{mutations:?}"
    );
    assert!(!dioxus::ssr::render(&dom).contains(" inert"));
}

#[test]
fn 製品orchestratorは全server_effectの完了まで通信中を保持する() {
    let mut orchestrator = ComponentOrchestrator::new();

    assert!(!orchestrator.server_effect_in_flight());
    orchestrator.begin_server_effect();
    assert!(orchestrator.server_effect_in_flight());
    orchestrator.begin_server_effect();
    orchestrator.finish_server_effect();
    assert!(orchestrator.server_effect_in_flight());
    orchestrator.finish_server_effect();
    assert!(!orchestrator.server_effect_in_flight());
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
        orchestrator.action(
            &storage,
            3_000,
            ComponentAction::Tick {
                wall_now_epoch_ms: 3_000,
            }
        ),
        ClientEffect::None
    );
    assert_eq!(orchestrator.state().unwrap().tick_now_epoch_ms(), 3_000);
}

#[test]
fn 最後のsessionを即時削除した時だけsessionから一覧tabへ移る() {
    let storage = MemoryStorage::default();
    let mut orchestrator = mounted_orchestrator(&storage);
    add_session(&mut orchestrator, &storage, RECORD_ID);
    add_session(&mut orchestrator, &storage, COMPLETE_ID);

    assert!(matches!(
        orchestrator.action(
            &storage,
            2_000,
            ComponentAction::DiscardSession(RECORD_ID.to_owned()),
        ),
        ClientEffect::ListTasks { .. }
    ));
    assert_eq!(
        orchestrator.state().unwrap().active_tab(),
        ActiveTab::Session,
        "sessionが残る場合は遷移しない"
    );

    assert!(matches!(
        orchestrator.action(
            &storage,
            2_001,
            ComponentAction::DiscardSession(COMPLETE_ID.to_owned()),
        ),
        ClientEffect::ListTasks { .. }
    ));
    assert_eq!(orchestrator.state().unwrap().active_tab(), ActiveTab::List);

    let other_storage = MemoryStorage::default();
    let mut other = mounted_orchestrator(&other_storage);
    add_session(&mut other, &other_storage, RECORD_ID);
    other.action(
        &other_storage,
        2_000,
        ComponentAction::SwitchTab(ActiveTab::History),
    );
    other.action(
        &other_storage,
        2_001,
        ComponentAction::DiscardSession(RECORD_ID.to_owned()),
    );
    assert_eq!(other.state().unwrap().active_tab(), ActiveTab::History);
}

#[test]
fn server応答で最後のsessionが消えた時だけ一覧tabへ移る() {
    let multiple_storage = MemoryStorage::default();
    let mut multiple = mounted_orchestrator(&multiple_storage);
    add_session(&mut multiple, &multiple_storage, RECORD_ID);
    add_session(&mut multiple, &multiple_storage, COMPLETE_ID);
    let request_id = mutation_request_id(&multiple.action(
        &multiple_storage,
        2_000,
        ComponentAction::RecordSession(RECORD_ID.to_owned()),
    ));
    assert!(matches!(
        multiple.apply_response(
            &multiple_storage,
            ClientResponse::RecordSession {
                request_id,
                result: Ok(WebSuccess {
                    snapshot: snapshot(2_000),
                    data: RecordSessionResult {
                        actual_work_seconds: 1,
                    },
                }),
            },
        ),
        ClientEffect::ListTasks { .. }
    ));
    assert_eq!(multiple.state().unwrap().sessions().len(), 1);
    assert_eq!(multiple.state().unwrap().active_tab(), ActiveTab::Session);

    for complete in [false, true] {
        let storage = MemoryStorage::default();
        let mut orchestrator = mounted_orchestrator(&storage);
        add_session(&mut orchestrator, &storage, RECORD_ID);
        let effect = orchestrator.action(
            &storage,
            2_000,
            if complete {
                ComponentAction::CompleteSession(RECORD_ID.to_owned())
            } else {
                ComponentAction::RecordSession(RECORD_ID.to_owned())
            },
        );
        let request_id = mutation_request_id(&effect);
        let response = if complete {
            ClientResponse::CompleteSession {
                request_id,
                result: Ok(snapshot(2_000)),
            }
        } else {
            ClientResponse::RecordSession {
                request_id,
                result: Ok(WebSuccess {
                    snapshot: snapshot(2_000),
                    data: RecordSessionResult {
                        actual_work_seconds: 1,
                    },
                }),
            }
        };

        assert!(matches!(
            orchestrator.apply_response(&storage, response),
            ClientEffect::ListTasks { .. }
        ));
        assert!(orchestrator.state().unwrap().sessions().is_empty());
        assert_eq!(orchestrator.state().unwrap().active_tab(), ActiveTab::List);
    }
}

#[test]
fn session削除失敗と他tabでは一覧へ強制遷移しない() {
    let failure_storage = MemoryStorage::default();
    let mut failure = mounted_orchestrator(&failure_storage);
    add_session(&mut failure, &failure_storage, RECORD_ID);
    let request_id = mutation_request_id(&failure.action(
        &failure_storage,
        2_000,
        ComponentAction::RecordSession(RECORD_ID.to_owned()),
    ));
    assert_eq!(
        failure.apply_response(
            &failure_storage,
            ClientResponse::RecordSession {
                request_id,
                result: Err(ServerFailure::Transport("切断".to_owned())),
            },
        ),
        ClientEffect::None
    );
    assert_eq!(failure.state().unwrap().active_tab(), ActiveTab::Session);
    assert_eq!(failure.state().unwrap().sessions().len(), 1);

    let storage_failure = MemoryStorage::default();
    let mut blocked = mounted_orchestrator(&storage_failure);
    add_session(&mut blocked, &storage_failure, RECORD_ID);
    let request_id = mutation_request_id(&blocked.action(
        &storage_failure,
        2_000,
        ComponentAction::RecordSession(RECORD_ID.to_owned()),
    ));
    storage_failure.set_fail_writes(true);
    assert!(matches!(
        blocked.apply_response(
            &storage_failure,
            ClientResponse::RecordSession {
                request_id,
                result: Ok(WebSuccess {
                    snapshot: snapshot(2_000),
                    data: RecordSessionResult {
                        actual_work_seconds: 1,
                    },
                }),
            },
        ),
        ClientEffect::ListTasks { .. }
    ));
    assert_eq!(blocked.state().unwrap().active_tab(), ActiveTab::Session);
    assert_eq!(blocked.state().unwrap().sessions().len(), 1);

    storage_failure.set_fail_writes(false);
    assert_eq!(
        blocked.action(
            &storage_failure,
            2_001,
            ComponentAction::ConfirmRepositoryChecked,
        ),
        ClientEffect::None
    );
    assert!(blocked.state().unwrap().sessions().is_empty());
    assert_eq!(blocked.state().unwrap().active_tab(), ActiveTab::List);

    let conflict_storage = MemoryStorage::default();
    let mut conflict = mounted_orchestrator(&conflict_storage);
    add_session(&mut conflict, &conflict_storage, COMPLETE_ID);
    let request_id = mutation_request_id(&conflict.action(
        &conflict_storage,
        2_000,
        ComponentAction::CompleteSession(COMPLETE_ID.to_owned()),
    ));
    assert_eq!(
        conflict.apply_response(
            &conflict_storage,
            ClientResponse::CompleteSession {
                request_id,
                result: Err(ServerFailure::Operation(WebError {
                    code: web_error_codes::ACTUAL_WORK_CONFLICT.to_owned(),
                    message: "競合".to_owned(),
                    retry_advice: RetryAdvice::ManualCheck,
                    current_actual_work_seconds: Some(1),
                })),
            },
        ),
        ClientEffect::None
    );
    assert_eq!(conflict.state().unwrap().active_tab(), ActiveTab::Session);
    assert_eq!(conflict.state().unwrap().sessions().len(), 1);
    assert_eq!(
        conflict.action(
            &conflict_storage,
            2_500,
            ComponentAction::ResumeCompletionConflict(COMPLETE_ID.to_owned()),
        ),
        ClientEffect::None
    );
    assert_eq!(conflict.state().unwrap().active_tab(), ActiveTab::Session);
    assert_eq!(conflict.state().unwrap().sessions().len(), 1);

    let other_storage = MemoryStorage::default();
    let mut other = mounted_orchestrator(&other_storage);
    add_session(&mut other, &other_storage, COMPLETE_ID);
    let request_id = mutation_request_id(&other.action(
        &other_storage,
        2_000,
        ComponentAction::CompleteSession(COMPLETE_ID.to_owned()),
    ));
    other.action(
        &other_storage,
        2_001,
        ComponentAction::SwitchTab(ActiveTab::History),
    );
    assert!(matches!(
        other.apply_response(
            &other_storage,
            ClientResponse::CompleteSession {
                request_id,
                result: Ok(snapshot(2_000)),
            },
        ),
        ClientEffect::ListTasks { .. }
    ));
    assert!(other.state().unwrap().sessions().is_empty());
    assert_eq!(other.state().unwrap().active_tab(), ActiveTab::History);
}

fn mounted_orchestrator(storage: &MemoryStorage) -> ComponentOrchestrator {
    let mut orchestrator = ComponentOrchestrator::new();
    assert!(matches!(
        orchestrator.mount(storage, 1_000),
        ClientEffect::Bootstrap { request_id: 1 }
    ));
    orchestrator.apply_response(
        storage,
        ClientResponse::Bootstrap {
            request_id: 1,
            result: Ok(snapshot(1_000)),
        },
    );
    orchestrator
}

fn add_session(orchestrator: &mut ComponentOrchestrator, storage: &MemoryStorage, task_id: &str) {
    assert_eq!(
        orchestrator.action(
            storage,
            1_500,
            ComponentAction::AddSession {
                task: task(task_id),
                is_leaf: true,
            },
        ),
        ClientEffect::None
    );
}

fn mutation_request_id(effect: &ClientEffect) -> u64 {
    match effect {
        ClientEffect::RecordSession { request_id, .. }
        | ClientEffect::CompleteSession { request_id, .. } => *request_id,
        effect => panic!("mutation effectを期待しました: {effect:?}"),
    }
}

fn snapshot(observed_at_epoch_ms: i64) -> ServerSnapshot {
    ServerSnapshot {
        observed_at_epoch_ms,
        logical_date: "2026-09-05".to_owned(),
        buffer_seconds: 60,
    }
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
    fail_writes: Cell<bool>,
    fail_carry_lock_reads: bool,
}

impl MemoryStorage {
    fn failing_reads() -> Self {
        Self {
            values: RefCell::new(HashMap::new()),
            fail_reads: true,
            fail_writes: Cell::new(false),
            fail_carry_lock_reads: false,
        }
    }

    fn failing_writes() -> Self {
        Self {
            values: RefCell::new(HashMap::new()),
            fail_reads: false,
            fail_writes: Cell::new(true),
            fail_carry_lock_reads: false,
        }
    }

    #[cfg(feature = "web")]
    fn failing_carry_lock_reads() -> Self {
        Self {
            values: RefCell::new(HashMap::new()),
            fail_reads: false,
            fail_writes: Cell::new(false),
            fail_carry_lock_reads: true,
        }
    }

    fn set_fail_writes(&self, fail_writes: bool) {
        self.fail_writes.set(fail_writes);
    }
}

impl KeyValueStorage for MemoryStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        if self.fail_reads {
            return Err(StorageError::ReadFailed);
        }
        if key == "schronu_web.carry_lock.v1" && self.fail_carry_lock_reads {
            return Err(StorageError::ReadFailed);
        }
        Ok(self.values.borrow().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        if self.fail_writes.get() {
            return Err(StorageError::WriteFailed);
        }
        self.values
            .borrow_mut()
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }
}
