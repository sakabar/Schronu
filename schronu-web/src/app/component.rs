use super::today_text;
#[cfg(feature = "web")]
use crate::REFRESH_INTERVAL;
use crate::{RefreshState, RefreshTrigger};
use dioxus::prelude::*;

pub fn app() -> Element {
    let state = use_signal(RefreshState::new);

    use_effect(move || start_refresh(state, RefreshTrigger::Initial));

    #[cfg(feature = "web")]
    use_future(move || async move {
        loop {
            let interval_millis = REFRESH_INTERVAL
                .as_millis()
                .try_into()
                .expect("refresh interval must fit in u32 milliseconds");
            gloo_timers::future::TimeoutFuture::new(interval_millis).await;
            start_refresh(state, RefreshTrigger::Interval);
        }
    });

    let snapshot = state.read();
    let text = snapshot.text().map(ToOwned::to_owned);
    let error = snapshot.error().map(ToOwned::to_owned);
    let is_refreshing = snapshot.is_refreshing();
    drop(snapshot);

    rsx! {
        document::Stylesheet { href: asset!("/assets/main.css") }
        main { class: "shell",
            header { class: "toolbar",
                h1 { "schronu 今" }
                button {
                    r#type: "button",
                    disabled: is_refreshing,
                    onclick: move |_| start_refresh(state, RefreshTrigger::Manual),
                    if is_refreshing { "更新中" } else { "更新" }
                }
            }
            if let Some(error) = error {
                section { class: "error", role: "alert",
                    p { "{error}" }
                    button {
                        r#type: "button",
                        disabled: is_refreshing,
                        onclick: move |_| start_refresh(state, RefreshTrigger::Manual),
                        "再試行"
                    }
                }
            }
            if let Some(text) = text {
                pre { class: "today-text", "{text}" }
            } else if !is_refreshing {
                p { "表示する内容がありません。" }
            }
        }
    }
}

fn start_refresh(mut state: Signal<RefreshState>, trigger: RefreshTrigger) {
    if !state.write().begin_refresh(trigger) {
        return;
    }
    spawn(async move {
        let result = today_text().await.map_err(|error| error.to_string());
        state.write().complete_refresh(result);
    });
}
