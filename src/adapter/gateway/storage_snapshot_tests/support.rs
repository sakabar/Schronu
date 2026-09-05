use crate::adapter::gateway::task_repository::TaskRepository;
use crate::application::interface::TaskRepositoryTrait;
use crate::entity::task::TaskHandle;
use chrono::{DateTime, Local};
use std::fs;
use std::path::Path;

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "schronu-storage-snapshot-{label}-{}",
            Uuid::new_v4().hyphenated()
        ));
        fs::create_dir(&path).unwrap();
        Self { path }
    }

    fn child(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if self.path.exists() {
            fs::remove_dir_all(&self.path).unwrap();
        }
    }
}

fn create_saved_repository(storage: &Path, now: DateTime<Local>) -> (Uuid, PathBuf) {
    fs::create_dir_all(storage).unwrap();
    let task_id = Uuid::new_v4();
    let task = TaskHandle::with_identity("snapshot-project", task_id, now).unwrap();
    let mut repository = TaskRepository::new(storage.to_str().unwrap());
    repository.sync_clock(now).unwrap();
    repository.start_new_project(task).unwrap();
    repository.save().unwrap();
    let project_yaml = fs::read_dir(storage)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("project.yaml"))
        .find(|path| path.is_file())
        .unwrap();
    (task_id, project_yaml)
}
