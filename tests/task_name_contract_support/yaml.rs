use std::error::Error;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

pub(super) struct TestStorageDir {
    pub(super) path: PathBuf,
}

impl TestStorageDir {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "schronu-task-name-yaml-contract-{}",
            Uuid::new_v4().hyphenated()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    pub(super) fn write_project(&self, yaml: &str) -> PathBuf {
        let project_dir = self.path.join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let project_yaml_path = project_dir.join("project.yaml");
        fs::write(&project_yaml_path, yaml).unwrap();
        project_yaml_path
    }
}

impl Drop for TestStorageDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(super) fn error_chain(error: &(dyn Error + 'static)) -> String {
    let mut messages = Vec::new();
    let mut current = Some(error);
    while let Some(source) = current {
        messages.push(source.to_string());
        current = source.source();
    }
    messages.join("\ncaused by: ")
}
