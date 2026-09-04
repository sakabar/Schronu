#[cfg(feature = "web")]
use crate::REFRESH_INTERVAL;
use crate::{RefreshState, RefreshTrigger};
use dioxus::prelude::*;

#[cfg(feature = "server")]
use crate::{TodayTextQuery, TodayWorkerHandle};

#[cfg(feature = "server")]
use schronu::adapter::controller::{resolve_project_storage_directory, TodayTextService};

#[cfg(feature = "server")]
use schronu::adapter::gateway::schronu_config::load_schronu_config;

#[cfg(feature = "server")]
use std::env;

#[cfg(feature = "server")]
use std::ffi::OsString;

#[cfg(feature = "server")]
use dioxus::fullstack::axum::Extension;

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

#[server(endpoint = "today_text")]
pub async fn today_text() -> Result<String, ServerFnError> {
    #[cfg(feature = "server")]
    {
        let Extension(worker) =
            dioxus::fullstack::FullstackContext::extract::<Extension<TodayWorkerHandle>, _>()
                .await
                .map_err(|error| ServerFnError::new(error.to_string()))?;
        return worker.request_async().await.map_err(ServerFnError::new);
    }

    #[cfg(not(feature = "server"))]
    unreachable!("server function body only runs on the server")
}

#[cfg(feature = "server")]
pub fn worker_from_environment() -> TodayWorkerHandle {
    TodayWorkerHandle::spawn(EnvironmentTodayTextQuery::new)
}

#[cfg(feature = "server")]
struct EnvironmentTodayTextQuery {
    config_path: Option<OsString>,
    storage_directory: Option<OsString>,
    service: Option<TodayTextService>,
}

#[cfg(feature = "server")]
impl EnvironmentTodayTextQuery {
    fn new() -> Self {
        Self::with_environment(
            env::var_os("SCHRONU_CONFIG_PATH"),
            env::var_os("SCHRONU_STORAGE_DIR"),
        )
    }

    fn with_environment(
        config_path: Option<OsString>,
        storage_directory: Option<OsString>,
    ) -> Self {
        Self {
            config_path,
            storage_directory,
            service: None,
        }
    }

    fn service(&mut self) -> Result<&mut TodayTextService, String> {
        if self.service.is_none() {
            let config = load_schronu_config(self.config_path.clone())?;
            let directory = resolve_project_storage_directory(self.storage_directory.clone())?;
            self.service = Some(TodayTextService::new(directory, config));
        }
        self.service
            .as_mut()
            .ok_or_else(|| "today text service was not initialized".to_owned())
    }
}

#[cfg(feature = "server")]
impl TodayTextQuery for EnvironmentTodayTextQuery {
    fn today_text(&mut self) -> Result<String, String> {
        self.service()?
            .render_at(chrono::Local::now())
            .map_err(|error| error.to_string())
    }
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::EnvironmentTodayTextQuery;
    use std::ffi::OsString;
    use std::fs;

    #[test]
    fn service_initialization_can_recover_after_config_is_repaired() {
        let root =
            std::env::temp_dir().join(format!("schronu-web-config-retry-{}", std::process::id()));
        fs::create_dir_all(&root).expect("test directory must be created");
        let config_path = root.join("schronu.yaml");
        let storage_directory = root.join("tasks");
        let mut query = EnvironmentTodayTextQuery::with_environment(
            Some(config_path.clone().into_os_string()),
            Some(OsString::from(storage_directory)),
        );

        assert!(query.service().is_err());
        fs::write(&config_path, "{}\n").expect("config must be repaired");
        assert!(query.service().is_ok());

        fs::remove_dir_all(root).expect("test directory must be removed");
    }
}
