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
        main { class: "shell",
            header { class: "toolbar", h1 { "Schronu" } }
            p { role: "status", "読み込み中…" }
        }
    }
}
