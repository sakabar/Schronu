use crate::entity::datetime::parse_local_datetime;
use crate::entity::task::read_project_category;
use crate::entity::task::read_repetition_anchor;
use crate::entity::task::read_status;
use crate::entity::task::Status;
use crate::entity::task::{ImmutableTask, RepetitionAnchor, Task, TaskAttr};
use chrono::LocalResult;
use chrono::TimeZone;
use chrono::{DateTime, Local};
use std::error::Error;
use std::fmt;
use uuid::Uuid;
use yaml_rust::Yaml;

#[cfg(test)]
use yaml_rust::YamlLoader;

#[cfg(test)]
use crate::entity::task::assert_task;

#[cfg(test)]
use crate::entity::task::ProjectCategory;

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

    if let Ok(LocalResult::Single(pu)) =
        parse_local_datetime(&pending_until_str, "%Y/%m/%d %H:%M:%S")
    {
        pending_until = pu;
    }

    if let Ok(LocalResult::Single(pu)) = parse_local_datetime(&pending_until_str, "%Y/%m/%d %H:%M")
    {
        pending_until = pu;
    }

    if let Ok(LocalResult::Single(pu)) = parse_local_datetime(
        format!("{} 00:00", pending_until_str).as_str(),
        "%Y/%m/%d %H:%M",
    ) {
        pending_until = pu;
    }

    let mut children = vec![];

    for child_yaml in yaml["children"].as_vec().unwrap_or(&vec![]) {
        let child = yaml_to_immutable_task(child_yaml);
        children.push(child);
    }

    ImmutableTask::new_with_current_time(name, status, pending_until, children)
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct YamlConversionError {
    path: String,
    field: String,
    reason: String,
}

impl YamlConversionError {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            path: String::new(),
            field: String::new(),
            reason: reason.into(),
        }
    }

    fn at(path: &str, field: &str, reason: impl Into<String>) -> Self {
        Self {
            path: path.to_string(),
            field: field.to_string(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for YamlConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(
                formatter,
                "cannot convert project YAML to task: {}",
                self.reason
            )
        } else {
            write!(
                formatter,
                "cannot convert project YAML to task: {}.{}: {}",
                self.path, self.field, self.reason
            )
        }
    }
}

impl Error for YamlConversionError {}

#[allow(dead_code)]
fn transform_from_pending_until_str(pending_until_str: &str) -> DateTime<Local> {
    for format in ["%Y/%m/%d %H:%M:%S", "%Y/%m/%d %H:%M"] {
        if let Ok(LocalResult::Single(datetime)) = parse_local_datetime(pending_until_str, format) {
            return datetime;
        }
    }
    if let Ok(LocalResult::Single(datetime)) =
        parse_local_datetime(&format!("{pending_until_str} 00:00"), "%Y/%m/%d %H:%M")
    {
        return datetime;
    }
    DateTime::<Local>::MIN_UTC.into()
}

fn task_children_yaml(yaml: &Yaml) -> Result<&[Yaml], YamlConversionError> {
    match &yaml["children"] {
        Yaml::BadValue | Yaml::Null => Ok(&[]),
        Yaml::Array(children) => Ok(children),
        _ => Err(YamlConversionError::new(
            "children must be an array or null",
        )),
    }
}

#[allow(dead_code)]
fn yaml_to_task_legacy(yaml: &Yaml, now: DateTime<Local>) -> Result<Task, YamlConversionError> {
    if yaml.as_hash().is_none() {
        return Err(YamlConversionError::new("task node must be a mapping"));
    }

    let default_attr = TaskAttr::new("デフォルト用");
    let name: &str = yaml["name"].as_str().unwrap_or("");

    let status_str: &str = yaml["status"].as_str().unwrap_or("");
    let status: Status = read_status(status_str).unwrap_or(*default_attr.get_status());

    let is_on_other_side: bool = yaml["is_on_other_side"]
        .as_bool()
        .unwrap_or(*default_attr.get_is_on_other_side());
    let atomic: bool = yaml["atomic"]
        .as_bool()
        .unwrap_or(default_attr.get_atomic());

    let pending_until_str: &str = yaml["pending_until"].as_str().unwrap_or("");
    let pending_until = transform_from_pending_until_str(pending_until_str);

    let priority: i64 = yaml["priority"]
        .as_i64()
        .unwrap_or(default_attr.get_priority());
    let project_category_opt = yaml["category"].as_str().and_then(read_project_category);

    let create_time_str: &str = yaml["create_time"].as_str().unwrap_or("");
    let start_time_str: &str = yaml["start_time"].as_str().unwrap_or("");
    let end_time_str: &str = yaml["end_time"].as_str().unwrap_or("");
    let deadline_time_str: &str = yaml["deadline_time"].as_str().unwrap_or("");

    let estimated_work_seconds: i64 = yaml["estimated_work_seconds"]
        .as_i64()
        .unwrap_or(default_attr.get_estimated_work_seconds());
    let actual_work_seconds: i64 = yaml["actual_work_seconds"]
        .as_i64()
        .unwrap_or(default_attr.get_actual_work_seconds());

    let repetition_interval_days_opt: Option<i64> = yaml["repetition_interval_days"].as_i64();
    let repetition_anchor_str: &str = yaml["repetition_anchor"].as_str().unwrap_or("");
    let repetition_anchor = read_repetition_anchor(repetition_anchor_str);
    let days_in_advance: i64 = yaml["days_in_advance"]
        .as_i64()
        .unwrap_or(default_attr.get_days_in_advance());

    let mut parent_task: Task = Task::new(name);

    let id_str: &str = yaml["id"].as_str().unwrap_or("");
    if let Ok(id) = Uuid::parse_str(id_str) {
        parent_task.set_id(id);
    }

    parent_task.set_orig_status(status);
    parent_task.set_is_on_other_side(is_on_other_side);
    parent_task.set_atomic(atomic);
    parent_task.set_pending_until(pending_until);
    parent_task.set_priority(priority);
    parent_task.set_project_category_opt(project_category_opt);

    if let Ok(LocalResult::Single(create_time)) =
        parse_local_datetime(create_time_str, "%Y/%m/%d %H:%M:%S")
    {
        parent_task.set_create_time(create_time);
    }

    if let Ok(LocalResult::Single(start_time)) =
        parse_local_datetime(start_time_str, "%Y/%m/%d %H:%M:%S")
    {
        parent_task.set_start_time(start_time);
    }

    if let Ok(LocalResult::Single(end_time)) =
        parse_local_datetime(end_time_str, "%Y/%m/%d %H:%M:%S")
    {
        parent_task.set_end_time_opt(Some(end_time));
    }

    if let Ok(LocalResult::Single(deadline_time)) =
        parse_local_datetime(deadline_time_str, "%Y/%m/%d %H:%M:%S")
    {
        parent_task.set_deadline_time_opt(Some(deadline_time));
    }

    parent_task.set_estimated_work_seconds(estimated_work_seconds);
    parent_task.set_actual_work_seconds(actual_work_seconds);
    parent_task.set_repetition_interval_days_opt(repetition_interval_days_opt);
    parent_task.set_repetition_anchor(repetition_anchor);
    parent_task.set_days_in_advance(days_in_advance);

    // repetition_interval_daysを持つタスクがtodoのままだと、
    // show_all_tasks()する際にestimated_work_secondsを二重に数えてしまうことになるので
    // 便宜的に2037/12/31までpendingする
    if repetition_interval_days_opt.is_some() {
        let distant_future = Local.with_ymd_and_hms(2037, 12, 31, 23, 59, 59).unwrap();
        parent_task.set_pending_until(distant_future);
        parent_task.set_orig_status(Status::Pending);
    }

    parent_task.sync_clock(now);

    for child_yaml in task_children_yaml(yaml)? {
        let mut child_task = yaml_to_task_legacy(child_yaml, now)?;
        child_task
            .detach_insert_as_last_child_of(parent_task)
            .map_err(YamlConversionError::new)?;

        parent_task = child_task
            .parent()
            .ok_or_else(|| YamlConversionError::new("inserted child has no parent"))?;
    }

    Ok(parent_task)
}

fn yaml_field<'a>(yaml: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    yaml.as_hash()?.get(&Yaml::String(key.to_string()))
}

fn strict_error(path: &str, field: &str, reason: &str) -> YamlConversionError {
    YamlConversionError::at(path, field, reason)
}

fn strict_datetime(
    value: &Yaml,
    path: &str,
    field: &str,
    formats: &[&str],
) -> Result<DateTime<Local>, YamlConversionError> {
    let text = value
        .as_str()
        .ok_or_else(|| strict_error(path, field, "must be a string"))?;
    for format in formats {
        if let Ok(LocalResult::Single(datetime)) = parse_local_datetime(text, format) {
            return Ok(datetime);
        }
    }
    let accepted_formats = formats
        .iter()
        .map(|format| match *format {
            "%Y/%m/%d" => "YYYY/MM/DD",
            "%Y/%m/%d %H:%M" => "YYYY/MM/DD HH:MM",
            "%Y/%m/%d %H:%M:%S" => "YYYY/MM/DD HH:MM:SS",
            _ => *format,
        })
        .collect::<Vec<_>>()
        .join(" or ");
    Err(strict_error(
        path,
        field,
        &format!(
            "must be a valid local datetime in {} format",
            accepted_formats
        ),
    ))
}

fn yaml_to_task_strict(
    yaml: &Yaml,
    now: DateTime<Local>,
    path: &str,
) -> Result<Task, YamlConversionError> {
    if yaml.as_hash().is_none() {
        return Err(strict_error(path, "node", "must be a mapping"));
    }
    let name = yaml_field(yaml, "name")
        .ok_or_else(|| strict_error(path, "name", "is required"))?
        .as_str()
        .ok_or_else(|| strict_error(path, "name", "must be a string"))?;
    if name.trim().is_empty() {
        return Err(strict_error(path, "name", "must not be blank"));
    }
    let id = match yaml_field(yaml, "id") {
        None => None,
        Some(value) => Some(
            Uuid::parse_str(
                value
                    .as_str()
                    .ok_or_else(|| strict_error(path, "id", "must be a UUID"))?,
            )
            .map_err(|_| strict_error(path, "id", "must be a UUID"))?,
        ),
    };
    let status =
        match yaml_field(yaml, "status") {
            None => Status::Todo,
            Some(value) => read_status(value.as_str().ok_or_else(|| {
                strict_error(path, "status", "must be one of todo, pending, done")
            })?)
            .ok_or_else(|| strict_error(path, "status", "must be one of todo, pending, done"))?,
        };
    let boolean = |field| match yaml_field(yaml, field) {
        None => Ok(false),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| strict_error(path, field, "must be a boolean")),
    };
    let nonnegative = |field, default| match yaml_field(yaml, field) {
        None => Ok(default),
        Some(value) => match value.as_i64() {
            Some(value) if value >= 0 => Ok(value),
            _ => Err(strict_error(path, field, "must be a non-negative integer")),
        },
    };
    let category = match yaml_field(yaml, "category") {
        None | Some(Yaml::Null) => None,
        Some(value) => Some(
            read_project_category(value.as_str().ok_or_else(|| {
                strict_error(path, "category", "must be a known category or null")
            })?)
            .ok_or_else(|| strict_error(path, "category", "must be a known category or null"))?,
        ),
    };
    let anchor = match yaml_field(yaml, "repetition_anchor") {
        None => RepetitionAnchor::Deadline,
        Some(value) => match value.as_str().map(str::to_lowercase).as_deref() {
            Some("deadline") => RepetitionAnchor::Deadline,
            Some("completion") => RepetitionAnchor::Completion,
            _ => {
                return Err(strict_error(
                    path,
                    "repetition_anchor",
                    "must be deadline or completion",
                ))
            }
        },
    };
    let pending = match yaml_field(yaml, "pending_until") {
        None => DateTime::<Local>::MIN_UTC.into(),
        Some(value) => strict_datetime(
            value,
            path,
            "pending_until",
            &["%Y/%m/%d %H:%M:%S", "%Y/%m/%d %H:%M", "%Y/%m/%d"],
        )?,
    };
    let legacy_datetime = |field| match yaml_field(yaml, field) {
        None => Ok(None),
        Some(value) => strict_datetime(value, path, field, &["%Y/%m/%d %H:%M:%S"]).map(Some),
    };
    let optional_datetime = |field| match yaml_field(yaml, field) {
        None | Some(Yaml::Null) => Ok(None),
        Some(value) => strict_datetime(value, path, field, &["%Y/%m/%d %H:%M:%S"]).map(Some),
    };
    let interval = match yaml_field(yaml, "repetition_interval_days") {
        None => None,
        Some(value) => match value.as_i64() {
            Some(value) if value > 0 => Some(value),
            _ => {
                return Err(strict_error(
                    path,
                    "repetition_interval_days",
                    "must be a positive integer",
                ))
            }
        },
    };
    let children = match yaml_field(yaml, "children") {
        None | Some(Yaml::Null) => &[][..],
        Some(Yaml::Array(children)) => children.as_slice(),
        Some(_) => return Err(strict_error(path, "children", "must be an array or null")),
    };
    let mut task = Task::new(name);
    if let Some(id) = id {
        task.set_id(id);
    }
    task.set_orig_status(status);
    task.set_is_on_other_side(boolean("is_on_other_side")?);
    task.set_atomic(boolean("atomic")?);
    task.set_pending_until(pending);
    task.set_priority(match yaml_field(yaml, "priority") {
        None => 0,
        Some(value) => value
            .as_i64()
            .ok_or_else(|| strict_error(path, "priority", "must be an integer"))?,
    });
    task.set_project_category_opt(category);
    if let Some(create_time) = legacy_datetime("create_time")? {
        task.set_create_time(create_time);
    }
    if let Some(start_time) = legacy_datetime("start_time")? {
        task.set_start_time(start_time);
    }
    task.set_end_time_opt(optional_datetime("end_time")?);
    task.set_deadline_time_opt(optional_datetime("deadline_time")?);
    task.set_estimated_work_seconds(nonnegative("estimated_work_seconds", 900)?);
    task.set_actual_work_seconds(nonnegative("actual_work_seconds", 0)?);
    task.set_repetition_interval_days_opt(interval);
    task.set_repetition_anchor(anchor);
    task.set_days_in_advance(nonnegative("days_in_advance", 0)?);
    if interval.is_some() {
        task.set_pending_until(Local.with_ymd_and_hms(2037, 12, 31, 23, 59, 59).unwrap());
        task.set_orig_status(Status::Pending);
    }
    task.sync_clock(now);
    for (index, child_yaml) in children.iter().enumerate() {
        let mut child = yaml_to_task_strict(child_yaml, now, &format!("{path}.children[{index}]"))?;
        child
            .detach_insert_as_last_child_of(task.clone())
            .map_err(|reason| strict_error(path, "children", &reason))?;
    }
    Ok(task)
}

pub(crate) fn yaml_to_task(yaml: &Yaml, now: DateTime<Local>) -> Result<Task, YamlConversionError> {
    yaml_to_task_strict(yaml, now, "project")
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
    let actual = yaml_to_task(project_yaml, now).unwrap();
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
    let actual = yaml_to_task(project_yaml, now).unwrap();
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
    let actual = yaml_to_task(project_yaml, now).unwrap();
    let expected = Task::new("タスク1");
    expected.set_priority(5);
    expected.sync_clock(now);

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
    let expected = Task::new("タスク1");
    expected.set_project_category_opt(Some(ProjectCategory::Sustaining));
    expected.sync_clock(now);

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

    assert_eq!(actual.get_project_category_opt(), None);
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

    assert_eq!(actual.get_project_category_opt(), None);
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
    let mut expected = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id);
    expected.set_is_on_other_side(true);
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
    let mut expected = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id);
    expected.set_atomic(true);
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
fn test_yaml_to_task_atomic未指定ならfalse() {
    let s = "
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
name: 'タスク1'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0];

    let now = Local::now();
    let actual = yaml_to_task(project_yaml, now).unwrap();

    assert!(!actual.get_atomic());
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
    let mut expected = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id);
    expected.set_create_time(now);
    expected.sync_clock(now);

    assert!(
        &actual
            .try_eq_tree(&expected)
            .expect("data are not borrowed"),
        "actual and expected are not equal"
    );

    assert_eq!(&actual.get_id(), &expected.get_id());
    assert_eq!(&actual.get_create_time(), &expected.get_create_time());
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
    let mut expected = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id);
    expected.set_start_time(now);
    expected.sync_clock(now);

    assert!(
        &actual
            .try_eq_tree(&expected)
            .expect("data are not borrowed"),
        "actual and expected are not equal"
    );

    assert_eq!(&actual.get_id(), &expected.get_id());
    assert_eq!(&actual.get_start_time(), &expected.get_start_time());
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
    let mut expected = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id);
    expected.set_end_time_opt(Some(now));
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
    let mut expected = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    expected.set_id(id);
    expected.set_deadline_time_opt(Some(now));
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
    let expected = Task::new("タスク1");
    expected.set_estimated_work_seconds(5);
    expected.sync_clock(now);

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
    let expected = Task::new("タスク1");
    expected.set_actual_work_seconds(5);
    expected.sync_clock(now);

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
    let expected = Task::new("タスク1");
    expected.set_repetition_interval_days_opt(Some(7));

    // 2037/12/31までpendingになる
    let distant_future = Local.with_ymd_and_hms(2037, 12, 31, 23, 59, 59).unwrap();
    expected.set_orig_status(Status::Pending);
    expected.set_pending_until(distant_future);

    expected.sync_clock(now);

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
    let expected = Task::new("タスク1");
    expected.set_repetition_anchor(RepetitionAnchor::Completion);
    expected.sync_clock(now);

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
    let expected = Task::new("タスク1");
    expected.set_repetition_anchor(RepetitionAnchor::Deadline);
    expected.sync_clock(now);

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
    let expected = Task::new("タスク1");
    expected.set_days_in_advance(1);
    expected.sync_clock(now);

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
    let actual_task = yaml_to_task(project_yaml, now).unwrap();

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
