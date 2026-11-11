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
        super::task_id_copy::CopyTaskIdProvider {
            AppBody {}
        }
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

// All variants remain available for build-time configuration.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TimeDisplayMode {
    Sleep,
    Buffer,
    Hidden,
}

pub(super) const TIME_DISPLAY_MODE: TimeDisplayMode = TimeDisplayMode::Sleep;

#[component]
pub(super) fn BufferPanel(
    value: Option<i128>,
    #[props(default = TIME_DISPLAY_MODE)] mode: TimeDisplayMode,
) -> Element {
    const BASE_SLEEP_MINUTES: i128 = 420;

    let display_name = match mode {
        TimeDisplayMode::Sleep => "睡眠時間",
        TimeDisplayMode::Buffer => "余裕時間",
        TimeDisplayMode::Hidden => return rsx! {},
    };
    let Some(value) = value else {
        return rsx! {
            section { class: "buffer-panel", aria_label: display_name,
                span { class: "buffer-label", "{display_name}" }
                strong { "未取得" }
            }
        };
    };
    let class = if value < 0 {
        "buffer-value is-negative"
    } else {
        "buffer-value"
    };
    let label = match mode {
        TimeDisplayMode::Sleep => format_hh_mm_ss(value + BASE_SLEEP_MINUTES * 60),
        _ if value > 0 => format!("+{}", format_hh_mm_ss(value)),
        _ => format_hh_mm_ss(value),
    };
    let panel_label = match mode {
        TimeDisplayMode::Sleep if value < 0 => {
            let deficit = format_hh_mm_ss(value);
            format!(
                "睡眠時間 {label}、基準より{}不足",
                deficit.trim_start_matches('-')
            )
        }
        TimeDisplayMode::Sleep => display_name.to_owned(),
        _ => format!("{display_name} {label}"),
    };
    rsx! {
        section {
            id: "schronu-buffer-ready",
            class: "buffer-panel",
            aria_label: panel_label,
            span { class: "buffer-label", "{display_name}" }
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

#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
const NAVIGATION_TABS: [(ActiveTab, &str); 4] = [
    (ActiveTab::Session, "セッション"),
    (ActiveTab::List, "一覧"),
    (ActiveTab::Load, "負荷"),
    (ActiveTab::Create, "作成"),
];

#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
fn adjacent_tab(active_tab: ActiveTab, key: &str) -> Option<ActiveTab> {
    let offset = match key {
        "ArrowLeft" => NAVIGATION_TABS.len() - 1,
        "ArrowRight" => 1,
        _ => return None,
    };
    let current = NAVIGATION_TABS
        .iter()
        .position(|&(tab, _)| tab == active_tab)?;
    Some(NAVIGATION_TABS[(current + offset) % NAVIGATION_TABS.len()].0)
}

#[cfg(test)]
#[test]
fn arrow_navigation_wraps_in_display_order() {
    use ActiveTab::{Create, List, Load, Session};

    for (current, left, right) in [
        (Session, Create, List),
        (List, Session, Load),
        (Load, List, Create),
        (Create, Load, Session),
    ] {
        assert_eq!(adjacent_tab(current, "ArrowLeft"), Some(left));
        assert_eq!(adjacent_tab(current, "ArrowRight"), Some(right));
        assert_eq!(adjacent_tab(current, "ArrowUp"), None);
    }
}

#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
#[component]
pub(super) fn NavigationTabs(active_tab: ActiveTab, on_switch: EventHandler<ActiveTab>) -> Element {
    rsx! {
        nav { class: "tabs", aria_label: "表示切替",
            for (tab, label) in NAVIGATION_TABS {
                TabButton {
                    key: "{label}",
                    label,
                    selected: active_tab == tab,
                    onclick: move |_| on_switch.call(tab),
                }
            }
        }
    }
}

#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
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
