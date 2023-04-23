use crate::entity::task::read_status;
use crate::entity::task::Status;
use crate::entity::task::Task;
use chrono::TimeZone;
use chrono::{DateTime, Local};
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
    let expected = Task::new_with_name_status_children("タスク1".to_string(), Status::Done, vec![]);
    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_task_pending_untilキーが存在しない場合は1970として登録されること() {
    let s = "
name: 'タスク1'
status: 'pending'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);
    // 1970は過去なので、pendingではなくtodoとなる
    let expected = Task::new_with_name_status_children("タスク1".to_string(), Status::Todo, vec![]);
    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_task_pending_untilキーが存在する場合はそれが登録されて現在時刻と比較した上で代入されること_日時(
) {
    let s = "
name: 'タスク1'
status: 'pending'
pending_until: '2000/01/01 00:00'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);
    // 2000/01/01は過去なので、pendingではなくtodoとなる
    let expected = Task::new(
        "タスク1".to_string(),
        Status::Todo,
        Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap(),
        vec![],
    );
    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_task_pending_untilキーが存在する場合はそれが登録されて現在時刻と比較した上で代入されること_日付(
) {
    let s = "
name: 'タスク1'
status: 'pending'
pending_until: '2000/01/01'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);
    // 2000/01/01は過去なので、pendingではなくtodoとなる
    let expected = Task::new(
        "タスク1".to_string(),
        Status::Todo,
        Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap(),
        vec![],
    );
    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_task_pending_untilキーが存在する場合はそれが登録されて現在時刻と比較した上で代入されること_日時秒(
) {
    let s = "
name: 'タスク1'
status: 'pending'
pending_until: '2000/01/01 01:23:45'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_task(project_yaml);
    // 2000/01/01は過去なので、pendingではなくtodoとなる
    let expected = Task::new(
        "タスク1".to_string(),
        Status::Todo,
        Local.with_ymd_and_hms(2000, 1, 1, 1, 23, 45).unwrap(),
        vec![],
    );
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

    let pending_until_str: String = yaml["pending_until"].as_str().unwrap_or("").to_string();
    let mut pending_until: DateTime<Local> = DateTime::<Local>::MIN_UTC.into();

    match Local.datetime_from_str(&pending_until_str, "%Y/%m/%d %H:%M:%S") {
        Ok(pu) => {
            pending_until = pu;
        }
        Err(_) => {}
    }

    match Local.datetime_from_str(&pending_until_str, "%Y/%m/%d %H:%M") {
        Ok(pu) => {
            pending_until = pu;
        }
        Err(_) => {}
    }

    match Local.datetime_from_str(
        format!("{} 00:00", &pending_until_str).as_str(),
        "%Y/%m/%d %H:%M",
    ) {
        Ok(pu) => {
            pending_until = pu;
        }
        Err(_) => {}
    }

    let mut children = vec![];

    for child_yaml in yaml["children"].as_vec().unwrap_or(&vec![]) {
        let child = yaml_to_task(&child_yaml);
        children.push(child);
    }

    return Task::new_with_current_time(name, status, pending_until, children);
}
