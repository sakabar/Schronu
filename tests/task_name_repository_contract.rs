use schronu::adapter::gateway::task_repository::TaskRepository;
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
        let path = std::env::temp_dir().join(format!(
            "schronu-task-name-yaml-contract-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn write_project(&self, yaml: &str) -> PathBuf {
        let project_dir = self.path.join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let project_yaml_path = project_dir.join("project.yaml");
        fs::write(&project_yaml_path, yaml).unwrap();
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
fn repository_loadはproject_yamlの実pathとchild名のcanonical診断を保持する() {
    let storage = TestStorageDir::new();
    let project_yaml_path = storage
        .write_project("project:\n  name: root\n  children:\n    - name: \"child\\u001Bname\"\n");
    let mut repository = TaskRepository::new(storage.path.to_str().unwrap());

    let actual = repository.load().unwrap_err();
    let diagnostic = error_chain(&actual);

    assert!(
        diagnostic.contains(project_yaml_path.to_str().unwrap()),
        "diagnostic must contain project YAML path: {diagnostic}"
    );
    assert!(
        diagnostic.contains("project.children[0].name: must not contain control characters"),
        "diagnostic must retain task path and canonical reason: {diagnostic}"
    );
}

fn error_chain(error: &(dyn Error + 'static)) -> String {
    let mut messages = Vec::new();
    let mut current = Some(error);
    while let Some(source) = current {
        messages.push(source.to_string());
        current = source.source();
    }
    messages.join("\ncaused by: ")
}
