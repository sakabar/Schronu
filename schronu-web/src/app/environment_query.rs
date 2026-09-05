use crate::{TodayTextQuery, TodayWorkerHandle};
use schronu::adapter::controller::{resolve_project_storage_directory, TodayTextService};
use schronu::adapter::gateway::schronu_config::load_schronu_config;
use std::env;
use std::ffi::OsString;

pub fn worker_from_environment() -> TodayWorkerHandle {
    TodayWorkerHandle::spawn(EnvironmentTodayTextQuery::new)
}

struct EnvironmentTodayTextQuery {
    config_path: Option<OsString>,
    storage_directory: Option<OsString>,
    service: Option<TodayTextService>,
}

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

impl TodayTextQuery for EnvironmentTodayTextQuery {
    fn today_text(&mut self) -> Result<String, String> {
        self.service()?
            .render_at(chrono::Local::now())
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
#[path = "environment_query_tests.rs"]
mod tests;
