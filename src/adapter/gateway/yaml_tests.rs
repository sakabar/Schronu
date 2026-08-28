#[cfg(test)]
use yaml_rust::YamlLoader;

#[cfg(test)]
use crate::entity::task::assert_task;

#[cfg(test)]
use crate::entity::task::ProjectCategory;

#[cfg(test)]
use uuid::uuid;

#[cfg(test)]
fn yaml_test_now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap()
}

#[cfg(test)]
fn yaml_encode_test_now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 8, 19, 0, 0, 0).unwrap()
}

#[cfg(test)]
fn new_test_task_attr(name: &str) -> TaskAttr {
    crate::test_support::new_task_attr_at(name, yaml_encode_test_now())
}

#[cfg(test)]
fn new_test_task_handle(name: &str) -> Result<TaskHandle, TaskTreeError> {
    crate::test_support::new_task_handle_at(name, yaml_encode_test_now())
}

#[test]
fn test_task_to_yaml_正常系1_デフォルトの値と同じ場合は出力しない() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_正常系2_再帰() {
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let mut task = new_test_task_handle("親タスク1").unwrap();
    task.set_orig_status(Status::Pending).unwrap();
    task.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap())
        .unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();

    let mut task_attr_child_1 = new_test_task_attr("子タスク1");
    task_attr_child_1.set_orig_status(Status::Pending);
    task_attr_child_1.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    task_attr_child_1.set_create_time(now);
    task_attr_child_1.set_start_time(now);
    let id_child_1: Uuid = uuid!("0aaee735-3e22-4216-8b59-d56d5caf29ee");
    task_attr_child_1.set_id(id_child_1);

    let mut task_attr_child_2 = new_test_task_attr("子タスク2");
    task_attr_child_2.set_orig_status(Status::Pending);
    task_attr_child_2.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    task_attr_child_2.set_create_time(now);
    task_attr_child_2.set_start_time(now);
    let id_child_2: Uuid = uuid!("7ffcba2f-80e0-4a44-aee9-d68e0d2d1256");
    task_attr_child_2.set_id(id_child_2);

    task.create_as_last_child(task_attr_child_1);
    task.create_as_last_child(task_attr_child_2);

    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: '親タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
status: pending
pending_until: '2023/04/01 12:00:00'
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
children:
  - name: '子タスク1'
    id: 0aaee735-3e22-4216-8b59-d56d5caf29ee
    status: pending
    pending_until: '2023/04/01 12:00:00'
    create_time: '2023/05/19 01:23:45'
    start_time: '2023/05/19 01:23:45'
  - name: '子タスク2'
    id: 7ffcba2f-80e0-4a44-aee9-d68e0d2d1256
    status: pending
    pending_until: '2023/04/01 12:00:00'
    create_time: '2023/05/19 01:23:45'
    start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_snapshot_to_yaml_root限定fieldとchildren順を保つ() {
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let mut task = new_test_task_handle("親タスク").unwrap();
    task.set_id(uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8"))
        .unwrap();
    task.set_priority(5).unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Sustaining))
        .unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();

    let mut first_child = new_test_task_attr("子タスク1");
    first_child.set_id(uuid!("0aaee735-3e22-4216-8b59-d56d5caf29ee"));
    first_child.set_priority(4);
    first_child.set_project_category_opt(Some(ProjectCategory::Sustaining));
    first_child.set_create_time(now);
    first_child.set_start_time(now);

    let mut second_child = new_test_task_attr("子タスク2");
    second_child.set_id(uuid!("7ffcba2f-80e0-4a44-aee9-d68e0d2d1256"));
    second_child.set_priority(3);
    second_child.set_project_category_opt(Some(ProjectCategory::Sustaining));
    second_child.set_create_time(now);
    second_child.set_start_time(now);

    task.create_as_last_child(first_child);
    task.create_as_last_child(second_child);

    let snapshot = task.snapshot().unwrap();
    let actual = task_snapshot_to_yaml(&snapshot);

    let s = "
name: '親タスク'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
priority: 5
category: sustaining
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
children:
  - name: '子タスク1'
    id: 0aaee735-3e22-4216-8b59-d56d5caf29ee
    create_time: '2023/05/19 01:23:45'
    start_time: '2023/05/19 01:23:45'
  - name: '子タスク2'
    id: 7ffcba2f-80e0-4a44-aee9-d68e0d2d1256
    create_time: '2023/05/19 01:23:45'
    start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_ユニークキー() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_project_category() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Sustaining))
        .unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
category: sustaining
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_project_categoryは子タスクには出力しない() {
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let mut task = new_test_task_handle("親タスク").unwrap();
    task.set_id(uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8"))
        .unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();

    let mut task_attr_child = new_test_task_attr("子タスク");
    task_attr_child.set_id(uuid!("0aaee735-3e22-4216-8b59-d56d5caf29ee"));
    task_attr_child.set_create_time(now);
    task_attr_child.set_start_time(now);
    task_attr_child.set_project_category_opt(Some(ProjectCategory::Sustaining));

    task.create_as_last_child(task_attr_child);

    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: '親タスク'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
children:
  - name: '子タスク'
    id: 0aaee735-3e22-4216-8b59-d56d5caf29ee
    create_time: '2023/05/19 01:23:45'
    start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_is_on_other_side() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_is_on_other_side(true).unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
is_on_other_side: true
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_atomic() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_atomic(true).unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
atomic: true
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_end_time_opt() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_is_on_other_side(true).unwrap();
    task.set_create_time(Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap())
        .unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2023, 5, 19, 2, 34, 56).unwrap())
        .unwrap();
    task.set_end_time_opt(Some(Local.with_ymd_and_hms(2023, 5, 19, 3, 45, 6).unwrap()))
        .unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
is_on_other_side: true
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 02:34:56'
end_time: '2023/05/19 03:45:06'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_deadline_time_opt() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_is_on_other_side(true).unwrap();
    task.set_create_time(Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap())
        .unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2023, 5, 19, 2, 34, 56).unwrap())
        .unwrap();
    task.set_deadline_time_opt(Some(Local.with_ymd_and_hms(2023, 5, 19, 3, 45, 6).unwrap()))
        .unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
is_on_other_side: true
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 02:34:56'
deadline_time: '2023/05/19 03:45:06'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_estimated_work_seconds() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_is_on_other_side(true).unwrap();
    task.set_create_time(Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap())
        .unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2023, 5, 19, 2, 34, 56).unwrap())
        .unwrap();
    task.set_estimated_work_seconds(1).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
is_on_other_side: true
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 02:34:56'
estimated_work_seconds: 1
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_actual_work_seconds() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_is_on_other_side(true).unwrap();
    task.set_create_time(Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap())
        .unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2023, 5, 19, 2, 34, 56).unwrap())
        .unwrap();
    task.set_actual_work_seconds(1).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
is_on_other_side: true
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 02:34:56'
actual_work_seconds: 1
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_repetition_interval() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_repetition_interval_days_opt(Some(7)).unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
repetition_interval_days: 7
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_repetition_anchor_completion() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_repetition_anchor(RepetitionAnchor::Completion)
        .unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
repetition_anchor: completion
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_repetition_anchor_deadlineは出力しない() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_repetition_anchor(RepetitionAnchor::Deadline)
        .unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_days_in_advance() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_days_in_advance(1).unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
days_in_advance: 1
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_yaml_to_immutable_task_childrenキーが存在しない場合は空配列として登録されること() {
    let s = "
name: 'タスク1'
status: 'todo'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let actual = yaml_to_immutable_task(project_yaml, yaml_test_now());
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

    let actual = yaml_to_immutable_task(project_yaml, yaml_test_now());
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

    let actual = yaml_to_immutable_task(project_yaml, yaml_test_now());
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

    let actual = yaml_to_immutable_task(project_yaml, yaml_test_now());
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

    let actual = yaml_to_immutable_task(project_yaml, yaml_test_now());
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

    let actual = yaml_to_immutable_task(project_yaml, yaml_test_now());
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

    let actual = yaml_to_immutable_task(project_yaml, yaml_test_now());
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

    let actual = yaml_to_immutable_task(project_yaml, yaml_test_now());
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

    let actual = yaml_to_immutable_task(project_yaml, yaml_test_now());
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

    let actual = yaml_to_immutable_task(project_yaml, yaml_test_now());

    let child_task = ImmutableTask::new_with_name("子タスク".to_string());
    let parent_task =
        ImmutableTask::new_with_name_children("親タスク".to_string(), vec![child_task]);
    assert_eq!(actual, parent_task);
}

#[test]
fn test_yaml_to_immutable_taskはcaller指定時刻を全nodeのpending判定へ共有する() {
    let s = "
name: '親タスク'
status: 'pending'
pending_until: '2001/01/01 00:00:00'
children:
  - name: '子タスク'
    status: 'pending'
    pending_until: '2001/01/01 00:00:00'
    children:
      - name: '孫タスク'
        status: 'pending'
        pending_until: '2001/01/01 00:00:00'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];
    let operation_now = Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();

    let actual = yaml_to_immutable_task(project_yaml, operation_now);

    assert_eq!(actual.get_status(), &Status::Pending);
    assert_eq!(actual.get_children()[0].get_status(), &Status::Pending);
    assert_eq!(
        actual.get_children()[0].get_children()[0].get_status(),
        &Status::Pending
    );
}

#[test]
fn test_yaml_to_taskは欠落した生成時刻と開始時刻へoperation時刻を全nodeで共有する() {
    let docs = YamlLoader::load_from_str(
        "
name: 親
children:
  - name: 子
",
    )
    .unwrap();
    let operation_now = Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();

    let actual = yaml_to_task(&docs[0], operation_now).unwrap();
    let child = actual.get_children().unwrap().remove(0);

    assert_eq!(actual.get_create_time().unwrap(), operation_now);
    assert_eq!(actual.get_start_time().unwrap(), operation_now);
    assert_eq!(child.get_create_time().unwrap(), operation_now);
    assert_eq!(child.get_start_time().unwrap(), operation_now);
}

#[test]
fn test_yaml_to_task_childrenが配列でなければerrorを返す() {
    let docs = YamlLoader::load_from_str(
        "
name: '親タスク'
children: '子タスクではない文字列'
",
    )
    .unwrap();

    let actual = yaml_to_task(&docs[0], Local::now());

    assert!(matches!(
        actual,
        Err(YamlConversionError { ref path, ref field, ref reason })
            if path == "project" && field == "children" && reason == "must be an array or null"
    ));
}

#[test]
fn test_yaml_to_task_childがmappingでなければerrorを返す() {
    let docs = YamlLoader::load_from_str(
        "
name: '親タスク'
children:
  - 42
",
    )
    .unwrap();

    let actual = yaml_to_task(&docs[0], Local::now());

    assert!(matches!(
        actual,
        Err(YamlConversionError { ref path, ref field, ref reason })
            if path == "project.children[0]" && field == "node" && reason == "must be a mapping"
    ));
}

#[test]
fn test_yaml_to_task_存在する不正fieldはtask_path付きerrorを返す() {
    let docs = YamlLoader::load_from_str(
        "
name: 親
id: not-a-uuid
children:
  - name: 子
    estimated_work_seconds: -1
",
    )
    .unwrap();

    let actual = yaml_to_task(&docs[0], Local::now()).unwrap_err();

    assert_eq!(
        actual.to_string(),
        "cannot convert project YAML to task: project.id: must be a UUID"
    );
}

#[test]
fn test_yaml_to_task_子の不正fieldは子のpath付きerrorを返す() {
    let docs = YamlLoader::load_from_str(
        "
name: 親
children:
  - name: 子
    estimated_work_seconds: -1
",
    )
    .unwrap();

    let actual = yaml_to_task(&docs[0], Local::now()).unwrap_err();

    assert_eq!(
        actual.to_string(),
        "cannot convert project YAML to task: project.children[0].estimated_work_seconds: must be a non-negative integer"
    );
}

#[test]
fn test_yaml_to_task_存在する型違いと不正enumはerrorを返す() {
    for (yaml, expected) in [
        (
            "name: task\nis_on_other_side: true\natomic: nope",
            "project.atomic: must be a boolean",
        ),
        (
            "name: task\nstatus: unknown",
            "project.status: must be one of todo, pending, done",
        ),
        (
            "name: task\ncategory: unknown",
            "project.category: must be a known category or null",
        ),
        (
            "name: task\nrepetition_anchor: unknown",
            "project.repetition_anchor: must be deadline or completion",
        ),
        (
            "name: task\nrepetition_interval_days: 0",
            "project.repetition_interval_days: must be a positive integer",
        ),
        (
            "name: task\ncreate_time: invalid",
            "project.create_time: must be a valid local datetime in YYYY/MM/DD HH:MM:SS format",
        ),
    ] {
        let docs = YamlLoader::load_from_str(yaml).unwrap();
        let actual = yaml_to_task(&docs[0], Local::now()).unwrap_err();
        assert_eq!(
            actual.to_string(),
            format!("cannot convert project YAML to task: {expected}")
        );
    }
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
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    expected.sync_clock(now).unwrap();

    assert!(
        &actual.eq_tree(&expected).expect("data are not borrowed"),
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
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    expected.sync_clock(now).unwrap();

    assert!(
        &actual.eq_tree(&expected).expect("data are not borrowed"),
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
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    expected.sync_clock(now).unwrap();

    assert!(
        &actual.eq_tree(&expected).expect("data are not borrowed"),
        "actual and expected are not equal"
    );
}

#[test]
#[allow(non_snake_case)]
fn test_yaml_to_task_statusキーの値が不正な時はerrorを返すこと() {
    let s = "
name: 'タスク1'
status: 'invalid_status'
children: []
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap_err();
    assert_eq!(
        actual.to_string(),
        "cannot convert project YAML to task: project.status: must be one of todo, pending, done"
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
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    expected.sync_clock(now).unwrap();

    expected.set_orig_status(Status::Done).unwrap();
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
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    expected.set_priority(5).unwrap();
    expected.sync_clock(now).unwrap();

    assert_task(&actual, &expected);
}

#[test]
fn test_yaml_to_task_priorityキー_異常の値の場合はerrorを返すこと() {
    let s = "
name: 'タスク1'
status: 'todo'
priority: 'invalid'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap_err();
    assert_eq!(
        actual.to_string(),
        "cannot convert project YAML to task: project.priority: must be an integer"
    );
}

#[test]
fn test_yaml_to_task_categoryキー_正常系() {
    let s = "
name: 'タスク1'
status: 'todo'
category: sustaining
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    expected
        .set_project_category_opt(Some(ProjectCategory::Sustaining))
        .unwrap();
    expected.sync_clock(now).unwrap();

    assert_task(&actual, &expected);
}

#[test]
fn test_yaml_to_task_categoryキーがnullの場合はnone() {
    let s = "
name: 'タスク1'
status: 'todo'
category:
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();

    assert_eq!(actual.get_project_category_opt().unwrap(), None);
}

#[test]
fn test_yaml_to_task_categoryキーが存在しない場合はnone() {
    let s = "
name: 'タスク1'
status: 'todo'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();

    assert_eq!(actual.get_project_category_opt().unwrap(), None);
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
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let mut expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id).unwrap();
    expected.sync_clock(now).unwrap();

    assert!(
        &actual.eq_tree(&expected).expect("data are not borrowed"),
        "actual and expected are not equal"
    );

    assert_eq!(&actual.get_id().unwrap(), &expected.get_id().unwrap());
}

#[test]
fn test_yaml_to_task_is_on_other_side_正常系() {
    let s = "
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
name: 'タスク1'
is_on_other_side: true
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let mut expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id).unwrap();
    expected.set_is_on_other_side(true).unwrap();
    expected.sync_clock(now).unwrap();

    assert!(
        &actual.eq_tree(&expected).expect("data are not borrowed"),
        "actual and expected are not equal"
    );

    assert_eq!(&actual.get_id().unwrap(), &expected.get_id().unwrap());
}

#[test]
fn test_yaml_to_task_atomic_正常系() {
    let s = "
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
name: 'タスク1'
atomic: true
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let mut expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id).unwrap();
    expected.set_atomic(true).unwrap();
    expected.sync_clock(now).unwrap();

    assert!(
        &actual.eq_tree(&expected).expect("data are not borrowed"),
        "actual and expected are not equal"
    );

    assert_eq!(&actual.get_id().unwrap(), &expected.get_id().unwrap());
}

#[test]
fn test_yaml_to_task_atomic未指定ならfalse() {
    let s = "
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
name: 'タスク1'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();

    assert!(!actual.get_atomic().unwrap());
}

#[test]
fn test_yaml_to_task_create_time_正常系() {
    let s = "
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
name: 'タスク1'
create_time: '2023/05/19 01:23:45'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let mut expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id).unwrap();
    expected.set_create_time(now).unwrap();
    expected.sync_clock(now).unwrap();

    assert!(
        &actual.eq_tree(&expected).expect("data are not borrowed"),
        "actual and expected are not equal"
    );

    assert_eq!(&actual.get_id().unwrap(), &expected.get_id().unwrap());
    assert_eq!(
        &actual.get_create_time().unwrap(),
        &expected.get_create_time().unwrap()
    );
}

#[test]
fn test_yaml_to_task_start_time_正常系() {
    let s = "
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
name: 'タスク1'
start_time: '2023/05/19 01:23:45'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let mut expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id).unwrap();
    expected.set_start_time(now).unwrap();
    expected.sync_clock(now).unwrap();

    assert!(
        &actual.eq_tree(&expected).expect("data are not borrowed"),
        "actual and expected are not equal"
    );

    assert_eq!(&actual.get_id().unwrap(), &expected.get_id().unwrap());
    assert_eq!(
        &actual.get_start_time().unwrap(),
        &expected.get_start_time().unwrap()
    );
}

#[test]
fn test_yaml_to_task_end_time_opt_正常系() {
    let s = "
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
name: 'タスク1'
end_time: '2023/05/19 01:23:45'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let mut expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id).unwrap();
    expected.set_end_time_opt(Some(now)).unwrap();
    expected.sync_clock(now).unwrap();

    assert!(
        &actual.eq_tree(&expected).expect("data are not borrowed"),
        "actual and expected are not equal"
    );

    assert_eq!(&actual.get_id().unwrap(), &expected.get_id().unwrap());
}

#[test]
fn test_yaml_to_task_deadline_time_opt_正常系() {
    let s = "
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
name: 'タスク1'
deadline_time: '2023/05/19 01:23:45'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let mut expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id).unwrap();
    expected.set_deadline_time_opt(Some(now)).unwrap();
    expected.sync_clock(now).unwrap();

    assert!(
        &actual.eq_tree(&expected).expect("data are not borrowed"),
        "actual and expected are not equal"
    );

    assert_eq!(&actual.get_id().unwrap(), &expected.get_id().unwrap());
}

#[test]
fn test_yaml_to_task_estimated_work_secondsキー_正常系() {
    let s = "
name: 'タスク1'
status: 'todo'
estimated_work_seconds: 5
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    expected.set_estimated_work_seconds(5).unwrap();
    expected.sync_clock(now).unwrap();

    assert_task(&actual, &expected);
}

#[test]
fn test_yaml_to_task_actual_work_secondsキー_正常系() {
    let s = "
name: 'タスク1'
status: 'todo'
actual_work_seconds: 5
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    expected.set_actual_work_seconds(5).unwrap();
    expected.sync_clock(now).unwrap();

    assert_task(&actual, &expected);
}

#[test]
fn test_yaml_to_task_repetition_interval_daysキー_正常系() {
    let s = "
name: 'タスク1'
status: 'todo'
repetition_interval_days: 7
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    expected.set_repetition_interval_days_opt(Some(7)).unwrap();

    // 2037/12/31までpendingになる
    let distant_future = Local.with_ymd_and_hms(2037, 12, 31, 23, 59, 59).unwrap();
    expected.set_orig_status(Status::Pending).unwrap();
    expected.set_pending_until(distant_future).unwrap();

    expected.sync_clock(now).unwrap();

    assert_task(&actual, &expected);
}

#[test]
fn test_yaml_to_task_repetition_anchor_completionキー_正常系() {
    let s = "
name: 'タスク1'
status: 'todo'
repetition_anchor: completion
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    expected
        .set_repetition_anchor(RepetitionAnchor::Completion)
        .unwrap();
    expected.sync_clock(now).unwrap();

    assert_task(&actual, &expected);
}

#[test]
fn test_yaml_to_task_repetition_anchor未指定ならdeadline() {
    let s = "
name: 'タスク1'
status: 'todo'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    expected
        .set_repetition_anchor(RepetitionAnchor::Deadline)
        .unwrap();
    expected.sync_clock(now).unwrap();

    assert_task(&actual, &expected);
}

#[test]
fn test_yaml_to_task_repetition_anchor不正値ならerror() {
    let s = "
name: 'タスク1'
status: 'todo'
repetition_anchor: invalid
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap_err();
    assert_eq!(actual.to_string(), "cannot convert project YAML to task: project.repetition_anchor: must be deadline or completion");
}

#[test]
fn test_yaml_to_task_days_in_advanceキー_正常系() {
    let s = "
name: 'タスク1'
status: 'todo'
days_in_advance: 1
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = crate::test_support::new_task_handle_at("タスク1", now).unwrap();
    expected.set_days_in_advance(1).unwrap();
    expected.sync_clock(now).unwrap();

    assert_task(&actual, &expected);
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
    let actual = yaml_to_task(project_yaml, now).unwrap();

    let parent_task = crate::test_support::new_task_handle_at("親タスク", now).unwrap();
    parent_task.sync_clock(now).unwrap();
    let mut task_attr = crate::test_support::new_task_attr_at("子タスク", now);
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
    let actual_task = yaml_to_task(project_yaml, now).unwrap();

    let parent_task = crate::test_support::new_task_handle_at("親タスク", now).unwrap();
    parent_task.sync_clock(now).unwrap();

    let child_task_1 =
        parent_task.create_as_last_child(crate::test_support::new_task_attr_at("子タスク1", now));
    child_task_1.sync_clock(now).unwrap();

    let grand_child_task =
        child_task_1.create_as_last_child(crate::test_support::new_task_attr_at("孫タスク", now));
    grand_child_task.sync_clock(now).unwrap();

    let child_task_2 =
        parent_task.create_as_last_child(crate::test_support::new_task_attr_at("子タスク2", now));
    child_task_2.sync_clock(now).unwrap();

    assert_task(&actual_task, &grand_child_task);
}
