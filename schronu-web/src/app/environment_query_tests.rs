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
