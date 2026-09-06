use crate::client::state::ActiveTab;
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

#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
#[component]
pub(super) fn InteractiveShell(blocked: bool, children: Element) -> Element {
    rsx! {
        main {
            class: "shell",
            inert: blocked.then_some("true"),
            aria_busy: blocked,
            {children}
        }
    }
}

#[component]
fn AppBody() -> Element {
    #[cfg(all(feature = "web", target_arch = "wasm32"))]
    {
        return rsx! { BrowserApp {} };
    }

    #[cfg(not(all(feature = "web", target_arch = "wasm32")))]
    rsx! {
        main { class: "shell", aria_busy: "true",
            header { class: "toolbar", h1 { "Schronu" } }
        }
        LoadingOverlay {}
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
