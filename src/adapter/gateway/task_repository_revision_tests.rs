use super::TaskRepository;
use crate::application::interface::TaskRepositoryTrait;
use crate::entity::task::TaskHandle;
use chrono::{Local, TimeZone};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

struct TestStorage {
    path: PathBuf,
}

impl TestStorage {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "schronu-repository-revision-{}",
            Uuid::new_v4().hyphenated()
        ));
        fs::create_dir(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestStorage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn repository_revisionは確定済みstorage_snapshotだけをread_onlyで返す() {
    let storage = TestStorage::new();
    let now = Local.with_ymd_and_hms(2026, 9, 6, 12, 0, 0).unwrap();
    let mut repository = TaskRepository::new(storage.path().to_str().unwrap());
    repository.sync_clock(now).unwrap();
    repository.load().unwrap();
    assert_eq!(repository.repository_revision(), None);

    let root = TaskHandle::with_identity("revision project", Uuid::new_v4(), now).unwrap();
    repository.start_new_project(root.clone()).unwrap();
    repository.save().unwrap();
    let first_revision = repository.repository_revision().unwrap();

    repository.save().unwrap();
    assert_eq!(repository.repository_revision(), Some(first_revision));

    root.set_estimated_work_seconds(30 * 60).unwrap();
    repository.save().unwrap();
    let second_revision = repository.repository_revision().unwrap();
    assert_ne!(second_revision, first_revision);

    let mut reloaded = TaskRepository::new(storage.path().to_str().unwrap());
    reloaded.sync_clock(now).unwrap();
    reloaded.load().unwrap();
    assert_eq!(reloaded.repository_revision(), Some(second_revision));
}
