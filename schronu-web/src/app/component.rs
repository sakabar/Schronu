use crate::client::state::ActiveTab;
use crate::client::time_model::format_hh_mm_ss;
use dioxus::prelude::*;

#[cfg(all(feature = "web", target_arch = "wasm32"))]
mod browser;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
use browser::BrowserApp;

pub fn app() -> Element {
    rsx! {
        document::Stylesheet { href: asset!("/assets/main.css") }
        AppBody {}
    }
}

#[component]
pub(super) fn InteractiveShell(blocked: bool, children: Element) -> Element {
    rsx! {
        main {
            id: "schronu-web-ready",
            class: "shell",
            inert: blocked.then_some("true"),
            aria_busy: blocked,
            {children}
        }
    }
}

#[component]
pub(super) fn RestoringShell() -> Element {
    rsx! {
        InteractiveShell { blocked: false,
            p { role: "status", "画面を復元しています…" }
        }
    }
}

#[component]
pub(super) fn SessionChrome(active_tab: ActiveTab, children: Element) -> Element {
    if active_tab == ActiveTab::Session {
        rsx! { {children} }
    } else {
        rsx! {}
    }
}

#[component]
fn AppBody() -> Element {
    #[cfg(all(feature = "web", target_arch = "wasm32"))]
    {
        return rsx! { BrowserApp {} };
    }

    #[cfg(not(all(feature = "web", target_arch = "wasm32")))]
    rsx! { RestoringShell {} }
}

#[component]
pub(super) fn BufferPanel(value: i128) -> Element {
    let class = if value < 0 {
        "buffer-value is-negative"
    } else {
        "buffer-value"
    };
    let label = format_hh_mm_ss(value);
    rsx! {
        section {
            id: "schronu-buffer-ready",
            class: "buffer-panel",
            aria_label: "本日の余白",
            span { class: "buffer-label", "BUFFER" }
            strong { class, "{label}" }
        }
    }
}

#[component]
pub(super) fn LoadingOverlay() -> Element {
    rsx! {
        div {
            class: "loading-overlay",
            role: "status",
            aria_live: "polite",
            aria_busy: "true",
            div { class: "loading-indicator",
                span { class: "loading-spinner", aria_hidden: "true" }
                span { "通信中…" }
            }
        }
    }
}

#[component]
pub(super) fn BackgroundRefreshStatus(
    has_cached_list: bool,
    failed: bool,
    #[props(default)] on_retry: EventHandler<()>,
) -> Element {
    let message = match (has_cached_list, failed) {
        (true, false) => "前回の表示です。最新状態を確認中…",
        (true, true) => "前回の表示です。最新状態を確認できませんでした。",
        (false, false) => "最新状態を確認中…",
        (false, true) => "最新状態を確認できませんでした。",
    };
    rsx! {
        section { class: "background-refresh-status", role: if failed { "alert" } else { "status" },
            span { "{message}" }
            if failed {
                button {
                    r#type: "button",
                    onclick: move |_| on_retry.call(()),
                    "再試行"
                }
            }
        }
    }
}

#[component]
pub(super) fn NavigationTabs(active_tab: ActiveTab, on_switch: EventHandler<ActiveTab>) -> Element {
    rsx! {
        nav { class: "tabs", aria_label: "表示切替",
            TabButton {
                label: "セッション",
                selected: active_tab == ActiveTab::Session,
                onclick: move |_| on_switch.call(ActiveTab::Session),
            }
            TabButton {
                label: "一覧",
                selected: active_tab == ActiveTab::List,
                onclick: move |_| on_switch.call(ActiveTab::List),
            }
            TabButton {
                label: "発火履歴",
                selected: active_tab == ActiveTab::History,
                onclick: move |_| on_switch.call(ActiveTab::History),
            }
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
