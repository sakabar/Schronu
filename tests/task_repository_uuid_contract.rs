use schronu::adapter::gateway::task_repository::{DuplicateTaskIdError, TaskRepository};
use schronu::application::interface::TaskRepositoryTrait;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

struct TestStorageDir {
    path: PathBuf,
}

impl TestStorageDir {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!("schronu-uuid-contract-{}", Uuid::new_v4())),
        }
    }

    fn write_project(&self, directory_name: &str, name: &str, id: Uuid) -> PathBuf {
        let project_dir = self.path.join(directory_name);
        fs::create_dir_all(&project_dir).unwrap();
        let project_yaml_path = project_dir.join("project.yaml");
        fs::write(
            &project_yaml_path,
            format!("project:\n  name: {name}\n  id: {id}\n"),
        )
        .unwrap();
        project_yaml_path
    }
}

impl Drop for TestStorageDir {
    fn drop(&mut self) {
        if self.path.exists() {
            fs::remove_dir_all(&self.path).unwrap();
        }
    }
}

#[test]
fn duplicate_task_id_errorはcrate外からsource_chainの全typed情報を取得できる() {
    let storage_dir = TestStorageDir::new();
    let duplicate_id = Uuid::from_u128(0x2231);
    let first_path = storage_dir.write_project("first", "first", duplicate_id);
    let duplicate_path = storage_dir.write_project("second", "second", duplicate_id);
    let mut repository = TaskRepository::new(storage_dir.path.to_str().unwrap());

    let actual = repository.load().unwrap_err();

    let source = actual
        .source()
        .and_then(|source| source.downcast_ref::<DuplicateTaskIdError>())
        .expect("source must expose DuplicateTaskIdError outside the crate");
    assert_eq!(source.task_id(), duplicate_id);
    assert_eq!(source.first_project_yaml_file_path(), first_path);
    assert_eq!(source.first_task_path(), "project");
    assert_eq!(source.duplicate_project_yaml_file_path(), duplicate_path);
    assert_eq!(source.duplicate_task_path(), "project");
}
