use super::super::carry_lock_view::CarryLockBar;
use super::super::component_dispatch::{
    dispatch_action, dispatch_action_effect, dispatch_session_action,
};
use super::super::component_models::{
    browser_monotonic_now_ms, browser_now_epoch_ms, BrowserPageModel,
};
use super::super::component_runtime::{ComponentAction, ComponentOrchestrator};
use super::super::history_view::HistoryView;
use super::super::list_view::ListView;
use super::super::long_press_browser::BrowserLongPressScheduler;
use super::super::long_press_controller::LongPressSchedulerHandle;
use super::super::session_view::SessionView;
use super::{
    BackgroundRefreshStatus, BufferPanel, InteractiveShell, LoadingOverlay, NavigationTabs,
    SessionChrome,
};
use crate::client::state::{ActiveTab, ClientState};
use crate::client::work_sessions::BrowserLocalStorage;
use dioxus::prelude::*;

const TICK_MILLIS: u32 = 1_000;

#[component]
pub(super) fn BrowserApp() -> Element {
    let mut client = use_signal(ComponentOrchestrator::new);
    let long_press_scheduler =
        use_hook(|| LongPressSchedulerHandle::new(BrowserLongPressScheduler));

    use_effect(move || {
        let effect = client
            .write()
            .mount(&BrowserLocalStorage, browser_now_epoch_ms());
        super::super::component_dispatch::dispatch_action_effect(client, effect);
    });

    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(TICK_MILLIS).await;
            dispatch_action(
                client,
                ComponentAction::Tick {
                    wall_now_epoch_ms: browser_now_epoch_ms(),
                },
            );
        }
    });

    let model = {
        let client = client.read();
        client
            .state()
            .map(|state| BrowserPageModel::from_state_at(state, browser_monotonic_now_ms()))
    };
    let Some(model) = model else {
        return rsx! {
            InteractiveShell { blocked: false,
                p { role: "status", "画面を復元しています…" }
            }
        };
    };
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
    let mutations_locked = carry_lock.mutations_locked();
    let (
        server_effect_in_flight,
        background_refreshing,
        refresh_failed,
        server_actions_blocked,
        has_cached_list,
        date_input_text,
        date_input_error,
        filter_text,
    ) = {
        let client = client.read();
        (
            client.server_effect_in_flight(),
            client.background_refreshing(),
            client.refresh_failed(),
            client.server_actions_blocked(),
            client.state().is_some_and(ClientState::has_scheduled_list),
            client.date_input().text().to_owned(),
            client.date_input().error().map(|error| error.to_string()),
            client.task_name_filter().to_owned(),
        )
    };

    rsx! {
        InteractiveShell {
            blocked: server_effect_in_flight,
            SessionChrome { active_tab,
                CarryLockBar {
                    model: carry_lock,
                    scheduler: long_press_scheduler,
                    on_enable: move |_| dispatch_action(client, ComponentAction::EnableCarryLock),
                    on_arm: move |_| dispatch_action(client, ComponentAction::ArmCarryLock),
                    on_relock: move |_| dispatch_action(client, ComponentAction::RelockCarryLock),
                    on_disable: move |_| dispatch_action(client, ComponentAction::DisableCarryLock),
                }
                if let Some(buffer) = buffer {
                    BufferPanel { value: buffer }
                } else {
                    section { class: "buffer-panel", aria_label: "本日の余白",
                        span { class: "buffer-label", "BUFFER" }
                        strong { "未取得" }
                    }
                }
            }
            NavigationTabs {
                active_tab,
                on_switch: move |tab| dispatch_action(client, ComponentAction::SwitchTab(tab)),
            }
            if background_refreshing || refresh_failed {
                BackgroundRefreshStatus {
                    has_cached_list,
                    failed: refresh_failed,
                    on_retry: move |_| dispatch_action(client, ComponentAction::RetryRefresh),
                }
            }
            for warning in warnings {
                section { class: "error", role: "alert", p { "{warning}" } }
            }
            if let Some(warning) = safety_warning {
                section { class: "error", role: "alert",
                    p { "{warning}" }
                    button {
                        r#type: "button",
                        disabled: !can_confirm || mutations_locked,
                        onclick: move |_| dispatch_action(client, ComponentAction::ConfirmRepositoryChecked),
                        "repository確認済み"
                    }
                }
            }
            if let Some(error) = display_error {
                section { class: "error", role: "alert", p { "{error}" } }
            }
            if active_tab == ActiveTab::Session {
                if auto_session_empty && sessions.is_empty() {
                    p { role: "status", "自動選定できるタスクがありません。" }
                }
                SessionView {
                    sessions,
                    global_blocked,
                    mutations_locked,
                    auto_session_in_flight,
                    server_actions_blocked,
                    on_auto_session: move |_| dispatch_action(client, ComponentAction::AutoSession),
                    on_action: move |action| dispatch_session_action(client, action),
                }
            } else if active_tab == ActiveTab::List {
                ListView {
                    dates,
                    rows,
                    active_task_ids,
                    date_input_text,
                    date_input_error,
                    filter_text,
                    mutations_locked,
                    server_actions_blocked,
                    on_select_date: move |date| {
                        client.write().clear_date_input(&BrowserLocalStorage);
                        dispatch_action(client, ComponentAction::SelectDate(date));
                    },
                    on_date_input_change: move |text| {
                        client.write().edit_date_input(&BrowserLocalStorage, text);
                    },
                    on_submit_date_input: move |_| {
                        let action = client.write().submit_date_input(&BrowserLocalStorage);
                        if let Some(action) = action {
                            dispatch_action(client, action);
                        }
                    },
                    on_start_session: move |(task, is_leaf)| {
                        let effect = client.write().start_session_from_list(
                            &BrowserLocalStorage,
                            browser_monotonic_now_ms(),
                            task,
                            is_leaf,
                        );
                        dispatch_action_effect(client, effect);
                    },
                    on_filter_change: move |filter| {
                        client.write().edit_task_name_filter(&BrowserLocalStorage, filter);
                    },
                }
            } else {
                HistoryView { entries: history }
            }
        }
        if server_effect_in_flight {
            LoadingOverlay {}
        }
    }
}
