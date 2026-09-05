mod component;
mod today_endpoint;

pub use component::app;
pub use today_endpoint::today_text;

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
