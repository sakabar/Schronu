use super::super::carry_lock_view::CarryLockBar;
use super::super::component_dispatch::{dispatch_action, dispatch_session_action};
use super::super::component_models::{
    browser_monotonic_now_ms, browser_now_epoch_ms, BrowserPageModel,
};
use super::super::component_runtime::{
    component_action_from_date_button, component_action_from_date_input, ComponentAction,
    ComponentOrchestrator,
};
use super::super::history_view::HistoryView;
use super::super::list_view::ListView;
use super::super::long_press_browser::BrowserLongPressScheduler;
use super::super::long_press_controller::LongPressSchedulerHandle;
use super::super::session_view::SessionView;
use super::{
    initial_load_phase, BufferPanel, InitialLoadPhase, InitialLoadView, InteractiveShell,
    LoadingOverlay, NavigationTabs, SessionChrome,
};
use crate::client::date_input::DateInputState;
use crate::client::state::ActiveTab;
use crate::client::work_sessions::BrowserLocalStorage;
use dioxus::prelude::*;

const TICK_MILLIS: u32 = 1_000;

#[component]
pub(super) fn BrowserApp() -> Element {
    let mut client = use_signal(ComponentOrchestrator::new);
    let mut date_input = use_signal(DateInputState::default);
    let mut task_name_filter = use_signal(String::new);
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

    let (snapshot_loaded, initial_error, server_effect_in_flight) = {
        let client = client.read();
        let snapshot_loaded = client
            .state()
            .is_some_and(|state| state.snapshot().is_some());
        let initial_error = client
            .state()
            .and_then(|state| state.display_error())
            .map(|error| error.message().to_owned());
        (
            snapshot_loaded,
            initial_error,
            client.server_effect_in_flight(),
        )
    };
    match initial_load_phase(
        snapshot_loaded,
        server_effect_in_flight,
        initial_error.as_deref(),
    ) {
        InitialLoadPhase::Loading => {
            return rsx! { InitialLoadView { in_flight: true, error: None } };
        }
        InitialLoadPhase::Error => {
            return rsx! { InitialLoadView { in_flight: false, error: initial_error } };
        }
        InitialLoadPhase::Ready => {}
    }
    let model = {
        let client = client.read();
        BrowserPageModel::from_state_at(
            client
                .state()
                .expect("ready phase requires initialized client state"),
            browser_monotonic_now_ms(),
        )
    };
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
    let mutations_locked = carry_lock.mutations_locked();
    let server_effect_in_flight = client.read().server_effect_in_flight();
    let date_input_text = date_input.read().text().to_owned();
    let date_input_error = date_input.read().error().map(|error| error.to_string());
    let filter_text = task_name_filter.read().clone();
    let buffer = buffer.expect("ready phase requires a server snapshot");

    rsx! {
        InteractiveShell {
            blocked: server_effect_in_flight,
            SessionChrome { active_tab,
                CarryLockBar {
                    model: carry_lock,
                    scheduler: long_press_scheduler,
                    on_enable: move |_| dispatch_action(client, ComponentAction::EnableCarryLock),
                    on_arm: move |_| dispatch_action(client, ComponentAction::ArmCarryLock),
                    on_disable: move |_| dispatch_action(client, ComponentAction::DisableCarryLock),
                }
                BufferPanel { value: buffer }
            }
            NavigationTabs {
                active_tab,
                on_switch: move |tab| dispatch_action(client, ComponentAction::SwitchTab(tab)),
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
                    on_select_date: move |date| {
                        let action = component_action_from_date_button(&mut date_input.write(), date);
                        dispatch_action(client, action);
                    },
                    on_date_input_change: move |text| date_input.write().edit(text),
                    on_submit_date_input: move |_| {
                        let Some(current_logical_date) = current_logical_date.as_deref() else {
                            return;
                        };
                        if let Some(action) = component_action_from_date_input(
                            &mut date_input.write(),
                            current_logical_date,
                        ) {
                            dispatch_action(client, action);
                        }
                    },
                    on_start_session: move |(task, is_leaf)| dispatch_action(
                        client,
                        ComponentAction::AddSession { task, is_leaf },
                    ),
                    on_filter_change: move |filter| task_name_filter.set(filter),
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
