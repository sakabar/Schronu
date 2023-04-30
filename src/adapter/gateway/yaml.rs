use crate::entity::task::read_status;
use crate::entity::task::Status;
use crate::entity::task::{ImmutableTask, Task};
use chrono::TimeZone;
use chrono::{DateTime, Local};
use uuid::Uuid;
use yaml_rust::Yaml;

#[cfg(test)]
use yaml_rust::YamlLoader;

#[cfg(test)]
use crate::entity::task::TaskAttr;

#[cfg(test)]
use crate::entity::task::assert_task;

#[cfg(test)]
use uuid::uuid;

#[test]
fn test_yaml_to_immutable_task_childrenキーが存在しない場合は空配列として登録されること() {
    let s = "
name: 'タスク1'
status: 'todo'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_immutable_task(project_yaml);
    let expected = ImmutableTask::new_with_name("タスク1".to_string());
    assert_eq!(actual, expected);
}

#[test]
fn test_yaml_to_immutable_task_childrenキーが存在して空配列の場合() {
    let s = "
name: 'タスク1'
children: []
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_immutable_task(project_yaml);
    let expected = ImmutableTask::new_with_name("タスク1".to_string());
    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_immutable_task_statusキーが存在しない場合はTodoとして登録されること() {
    let s = "
name: 'タスク1'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_immutable_task(project_yaml);
    let expected = ImmutableTask::new_with_name("タスク1".to_string());
    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_immutable_task_statusキーの値が不正な時はTodoとして登録されること() {
    let s = "
name: 'タスク1'
status: 'invalid_status'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_immutable_task(project_yaml);
    let expected = ImmutableTask::new_with_name("タスク1".to_string());
    assert_eq!(actual, expected);
}

#[test]
fn test_yaml_to_immutable_task_childrenキーが存在してnullの場合() {
    let s = "
name: 'タスク1'
status: 'done'
children:
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_immutable_task(project_yaml);
    let expected =
        ImmutableTask::new_with_name_status_children("タスク1".to_string(), Status::Done, vec![]);
    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_immutable_task_pending_untilキーが存在しない場合は1970として登録されること() {
    let s = "
name: 'タスク1'
status: 'pending'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_immutable_task(project_yaml);
    // 1970は過去なので、pendingではなくtodoとなる
    let expected =
        ImmutableTask::new_with_name_status_children("タスク1".to_string(), Status::Todo, vec![]);
    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_immutable_task_pending_untilキーが存在する場合はそれが登録されて現在時刻と比較した上で代入されること_日時(
) {
    let s = "
name: 'タスク1'
status: 'pending'
pending_until: '2000/01/01 00:00'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_immutable_task(project_yaml);
    // 2000/01/01は過去なので、pendingではなくtodoとなる
    let expected = ImmutableTask::new(
        "タスク1".to_string(),
        Status::Todo,
        Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap(),
        vec![],
    );
    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_immutable_task_pending_untilキーが存在する場合はそれが登録されて現在時刻と比較した上で代入されること_日付(
) {
    let s = "
name: 'タスク1'
status: 'pending'
pending_until: '2000/01/01'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_immutable_task(project_yaml);
    // 2000/01/01は過去なので、pendingではなくtodoとなる
    let expected = ImmutableTask::new(
        "タスク1".to_string(),
        Status::Todo,
        Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap(),
        vec![],
    );
    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_immutable_task_pending_untilキーが存在する場合はそれが登録されて現在時刻と比較した上で代入されること_日時秒(
) {
    let s = "
name: 'タスク1'
status: 'pending'
pending_until: '2000/01/01 01:23:45'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_immutable_task(project_yaml);
    // 2000/01/01は過去なので、pendingではなくtodoとなる
    let expected = ImmutableTask::new(
        "タスク1".to_string(),
        Status::Todo,
        Local.with_ymd_and_hms(2000, 1, 1, 1, 23, 45).unwrap(),
        vec![],
    );
    assert_eq!(actual, expected);
}

#[test]
fn test_yaml_to_immutable_task_再帰的にパーズできること() {
    let s = "
name: '親タスク'
children:
  - name: '子タスク'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_immutable_task(project_yaml);

    let child_task = ImmutableTask::new_with_name("子タスク".to_string());
    let parent_task =
        ImmutableTask::new_with_name_children("親タスク".to_string(), vec![child_task]);
    assert_eq!(actual, parent_task);
}

pub fn yaml_to_immutable_task(yaml: &Yaml) -> ImmutableTask {
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
        let child = yaml_to_immutable_task(&child_yaml);
        children.push(child);
    }

    return ImmutableTask::new_with_current_time(name, status, pending_until, children);
}

fn transform_from_pending_until_str(pending_until_str: &str) -> DateTime<Local> {
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

    pending_until
}

// Todo Result型を返すようにする
pub fn yaml_to_task(yaml: &Yaml, now: DateTime<Local>) -> Task {
    let name: &str = yaml["name"].as_str().unwrap_or("");

    let status_str: &str = yaml["status"].as_str().unwrap_or("");
    let status: Status = read_status(&status_str).unwrap_or(Status::Todo);

    let pending_until_str: &str = yaml["pending_until"].as_str().unwrap_or("");
    let pending_until = transform_from_pending_until_str(pending_until_str);

    let priority: i64 = yaml["priority"].as_i64().unwrap_or(0);

    let mut parent_task: Task = Task::new(name);

    let id_str: &str = yaml["id"].as_str().unwrap_or("");
    match Uuid::parse_str(id_str) {
        Ok(id) => {
            parent_task.set_id(id);
        }
        Err(_) => {}
    }

    parent_task.set_orig_status(status);
    parent_task.set_pending_until(pending_until);
    parent_task.set_priority(priority);

    parent_task.sync_clock(now);

    for child_yaml in yaml["children"].as_vec().unwrap_or(&vec![]) {
        let mut child_task = yaml_to_task(&child_yaml, now);
        child_task
            .detach_insert_as_last_child_of(parent_task)
            .unwrap();

        parent_task = child_task.parent().unwrap();
    }

    return parent_task;
}

#[test]
fn test_yaml_to_task_childrenキーが存在しない場合は空配列として登録されること() {
    let s = "
name: 'タスク1'
status: 'todo'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now);
    let expected = Task::new("タスク1");
    expected.sync_clock(now);

    assert!(
        &actual
            .try_eq_tree(&expected)
            .expect("data are not borrowed"),
        "actual and expected are not equal"
    );
}

#[test]
fn test_yaml_to_task_childrenキーが存在して空配列の場合() {
    let s = "
name: 'タスク1'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now);
    let expected = Task::new("タスク1");
    expected.sync_clock(now);

    assert!(
        &actual
            .try_eq_tree(&expected)
            .expect("data are not borrowed"),
        "actual and expected are not equal"
    );
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

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now);
    let expected = Task::new("タスク1");
    expected.sync_clock(now);

    assert!(
        &actual
            .try_eq_tree(&expected)
            .expect("data are not borrowed"),
        "actual and expected are not equal"
    );
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

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now);
    let expected = Task::new("タスク1");
    expected.sync_clock(now);
    expected.set_orig_status(Status::Todo);

    assert!(
        actual
            .try_eq_tree(&expected)
            .expect("data are not borrowed"),
        "actual and expected are not equal"
    );
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

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now);
    let expected = Task::new("タスク1");
    expected.sync_clock(now);

    expected.set_orig_status(Status::Done);
    assert_task(&actual, &expected);
}

#[test]
fn test_yaml_to_task_priorityキー_正常系() {
    let s = "
name: 'タスク1'
status: 'todo'
priority: 5
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now);
    let expected = Task::new("タスク1");
    expected.set_priority(5);
    expected.sync_clock(now);

    assert_task(&actual, &expected);
}

#[test]
fn test_yaml_to_task_priorityキー_異常の値の場合はデフォルト値となること() {
    let s = "
name: 'タスク1'
status: 'todo'
priority: 'invalid'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now);
    let expected = Task::new("タスク1");
    expected.set_priority(0);
    expected.sync_clock(now);

    assert!(
        &actual
            .try_eq_tree(&expected)
            .expect("data are not borrowed"),
        "actual and expected are not equal"
    );
}

#[test]
fn test_yaml_to_task_idキー_正常系() {
    let s = "
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
name: 'タスク1'
status: 'todo'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now);
    let mut expected = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id);
    expected.sync_clock(now);

    assert!(
        &actual
            .try_eq_tree(&expected)
            .expect("data are not borrowed"),
        "actual and expected are not equal"
    );

    assert_eq!(&actual.get_id(), &expected.get_id());
}

#[test]
fn test_yaml_to_task_再帰的にパーズできること_親子() {
    let s = "
name: '親タスク'
children:
  - name: '子タスク'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now);

    let parent_task = Task::new("親タスク");
    parent_task.sync_clock(now);
    let mut task_attr = TaskAttr::new("子タスク");
    task_attr.sync_clock(now);
    parent_task.create_as_last_child(task_attr);

    assert_task(&actual, &parent_task);
}

#[test]
fn test_yaml_to_task_再帰的にパーズできること_親子孫() {
    let s = "
name: '親タスク'
children:
  - name: '子タスク1'
    children:
      - name: '孫タスク'
  - name: '子タスク2'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual_task = yaml_to_task(project_yaml, now);

    let parent_task = Task::new("親タスク");
    parent_task.sync_clock(now);

    let child_task_1 = parent_task.create_as_last_child(TaskAttr::new("子タスク1"));
    child_task_1.sync_clock(now);

    let grand_child_task = child_task_1.create_as_last_child(TaskAttr::new("孫タスク"));
    grand_child_task.sync_clock(now);

    let _child_task_2 = parent_task.create_as_last_child(TaskAttr::new("子タスク2"));
    _child_task_2.sync_clock(now);

    assert_task(&actual_task, &grand_child_task);
}
