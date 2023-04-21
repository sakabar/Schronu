use crate::entity::task::Task;
use yaml_rust::Yaml;

#[test]
fn test_yaml_to_task__childrenキーが存在しない場合は空配列として登録されること() {
    let s = "name: 'タスク1'";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);
    let expected = Task::new("タスク1".to_string(), vec![]);
    assert_eq!(actual, expected);
}

#[test]
fn test_yaml_to_task__childrenキーが存在して空配列の場合() {
    let s = "
name: 'タスク1'
children: []
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);
    let expected = Task::new("タスク1".to_string(), vec![]);
    assert_eq!(actual, expected);
}

#[test]
fn test_yaml_to_task__childrenキーが存在してnullの場合() {
    let s = "
name: 'タスク1'
children:
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);
    let expected = Task::new("タスク1".to_string(), vec![]);
    assert_eq!(actual, expected);
}

#[test]
fn test_yaml_to_task__再帰的にパーズできること() {
    let s = "
name: '親タスク'
children:
  - name: '子タスク'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);

    let child_task = Task::new("子タスク".to_string(), vec![]);
    let parent_task = Task::new("親タスク".to_string(), vec![child_task]);
    assert_eq!(actual, parent_task);
}

pub fn yaml_to_task(yaml: &Yaml) -> Task {
    let name: String = yaml["name"].as_str().unwrap_or("").to_string();
    let mut children = vec![];

    for child_yaml in yaml["children"].as_vec().unwrap_or(&vec![]) {
        let child = yaml_to_task(&child_yaml);
        children.push(child);
    }

    return Task::new(name, children);
}
