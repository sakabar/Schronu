use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;

#[path = "task_name_contract_support/yaml.rs"]
mod yaml_support;

use yaml_support::{error_chain, TestStorageDir};

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
