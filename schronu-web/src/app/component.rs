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
