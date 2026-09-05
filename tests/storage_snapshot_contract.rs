use chrono::{Local, Timelike};
use schronu::adapter::gateway::storage_snapshot::{create_snapshot, restore_snapshot};
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;
use schronu::entity::task::TaskHandle;
use std::fs;
use std::process::Command;
use uuid::Uuid;

#[test]
fn restored_storageは既存cli検証製品経路を通る() {
    let root = std::env::temp_dir().join(format!(
        "schronu-storage-snapshot-contract-{}",
        Uuid::new_v4().hyphenated()
    ));
    let source = root.join("source");
    let snapshot = root.join("snapshot");
    let restored = root.join("restored");
    fs::create_dir_all(&source).unwrap();
    let now = Local::now().with_nanosecond(0).unwrap();
    let task = TaskHandle::with_identity("restored-project", Uuid::new_v4(), now).unwrap();
    let mut repository = TaskRepository::new(source.to_str().unwrap());
    repository.sync_clock(now).unwrap();
    repository.start_new_project(task).unwrap();
    repository.save().unwrap();
    create_snapshot(&source, &snapshot).unwrap();
    fs::remove_dir_all(&source).unwrap();

    restore_snapshot(&snapshot, &restored).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_schronu"))
        .arg("検証")
        .env("SCHRONU_STORAGE_DIR", &restored)
        .output()
        .unwrap();

    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "検証: OK\n");
    fs::remove_dir_all(root).unwrap();
}
