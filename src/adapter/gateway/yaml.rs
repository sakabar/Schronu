use crate::entity::task::read_status;
use crate::entity::task::Status;
use crate::entity::task::Task;
use yaml_rust::Yaml;

#[cfg(test)]
use yaml_rust::YamlLoader;

#[test]
fn test_yaml_to_task_childrenキーが存在しない場合は空配列として登録されること() {
    let s = "
name: 'タスク1'
status: 'todo'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);
    let expected = Task::new_with_name("タスク1".to_string());
    assert_eq!(actual, expected);
}

#[test]
fn test_yaml_to_task_childrenキーが存在して空配列の場合() {
    let s = "
name: 'タスク1'
children: []
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);
    let expected = Task::new_with_name("タスク1".to_string());
    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_task_statusキーが存在しない場合はTodoとして登録されること() {
    let s = "
name: 'タスク1'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);
    let expected = Task::new_with_name("タスク1".to_string());
    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_task_statusキーの値が不正な時はTodoとして登録されること() {
    let s = "
name: 'タスク1'
status: 'invalid_status'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);
    let expected = Task::new_with_name("タスク1".to_string());
    assert_eq!(actual, expected);
}

#[test]
fn test_yaml_to_task_childrenキーが存在してnullの場合() {
    let s = "
name: 'タスク1'
status: 'done'
children:
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);
    let expected = Task::new("タスク1".to_string(), Status::Done, vec![]);
    assert_eq!(actual, expected);
}

#[test]
fn test_yaml_to_task_再帰的にパーズできること() {
    let s = "
name: '親タスク'
children:
  - name: '子タスク'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);

    let child_task = Task::new_with_name("子タスク".to_string());
    let parent_task = Task::new_with_name_children("親タスク".to_string(), vec![child_task]);
    assert_eq!(actual, parent_task);
}

pub fn yaml_to_task(yaml: &Yaml) -> Task {
    let name: String = yaml["name"].as_str().unwrap_or("").to_string();
    let status_str: String = yaml["status"].as_str().unwrap_or("").to_string();
    let status: Status = read_status(&status_str).unwrap_or(Status::Todo);

    let mut children = vec![];

    for child_yaml in yaml["children"].as_vec().unwrap_or(&vec![]) {
        let child = yaml_to_task(&child_yaml);
        children.push(child);
    }

    return Task::new(name, status, children);
}
