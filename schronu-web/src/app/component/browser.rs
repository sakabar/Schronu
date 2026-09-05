use super::super::component_dispatch::{dispatch_action, dispatch_session_action};
use super::super::component_models::{browser_now_epoch_ms, BrowserPageModel};
use super::super::component_runtime::{ComponentAction, ComponentOrchestrator};
use super::super::history_view::HistoryView;
use super::super::list_view::ListView;
use super::super::session_view::SessionView;
use crate::client::state::ActiveTab;
use crate::client::time_model::format_hh_mm_ss;
use crate::client::work_sessions::BrowserLocalStorage;
use dioxus::prelude::*;

const TICK_MILLIS: u32 = 1_000;

#[component]
pub(super) fn BrowserApp() -> Element {
    let mut client = use_signal(ComponentOrchestrator::new);

    use_effect(move || {
        let effect = client
            .write()
            .mount(&BrowserLocalStorage, browser_now_epoch_ms());
        super::super::component_dispatch::dispatch_action_effect(client, effect);
    });

    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(TICK_MILLIS).await;
            dispatch_action(client, ComponentAction::Tick(browser_now_epoch_ms()));
        }
    });

    let model = {
        let client = client.read();
        let Some(state) = client.state() else {
            return loading_shell();
        };
        BrowserPageModel::from_state(state)
    };
    let BrowserPageModel {
        active_tab,
        tick_now_epoch_ms,
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
    } = model;

    rsx! {
        main { class: "shell",
            header { class: "toolbar", h1 { "Schronu" } }
            BufferPanel { value: buffer }
            nav { class: "tabs", aria_label: "表示切替",
                TabButton {
                    label: "セッション",
                    selected: active_tab == ActiveTab::Session,
                    onclick: move |_| dispatch_action(client, ComponentAction::SwitchTab(ActiveTab::Session)),
                }
                TabButton {
                    label: "一覧",
                    selected: active_tab == ActiveTab::List,
                    onclick: move |_| dispatch_action(client, ComponentAction::SwitchTab(ActiveTab::List)),
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
                        disabled: !can_confirm,
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
                    auto_session_in_flight,
                    on_auto_session: move |_| dispatch_action(client, ComponentAction::AutoSession),
                    on_action: move |action| dispatch_session_action(client, action),
                }
            } else {
                ListView {
                    dates,
                    rows,
                    active_task_ids,
                    tick_now_epoch_ms,
                    on_select_date: move |date| dispatch_action(client, ComponentAction::SelectDate(date)),
                    on_start_session: move |task| dispatch_action(client, ComponentAction::AddSession(task)),
                }
            }
            HistoryView { entries: history }
        }
    }
}

fn loading_shell() -> Element {
    rsx! {
        main { class: "shell",
            header { class: "toolbar", h1 { "Schronu" } }
            p { role: "status", "読み込み中…" }
        }
    }
}

#[component]
fn BufferPanel(value: Option<i128>) -> Element {
    let class = if value.is_some_and(|seconds| seconds < 0) {
        "buffer-value is-negative"
    } else {
        "buffer-value"
    };
    let label = value.map_or_else(|| "--:--:--".to_owned(), format_hh_mm_ss);
    rsx! {
        section { class: "buffer-panel", aria_label: "本日の余白",
            span { class: "buffer-label", "BUFFER" }
            strong { class, "{label}" }
        }
    }
}

#[component]
fn TabButton(label: &'static str, selected: bool, onclick: EventHandler<MouseEvent>) -> Element {
    let class = if selected {
        "tab-button is-selected"
    } else {
        "tab-button"
    };
    rsx! {
        button { class, r#type: "button", aria_pressed: selected, onclick, "{label}" }
    }
}
