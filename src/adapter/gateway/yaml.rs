use crate::entity::datetime::parse_local_datetime;
use crate::entity::task::read_project_category;
use crate::entity::task::read_status;
use crate::entity::task::Status;
use crate::entity::task::{RepetitionAnchor, TaskAttr, TaskHandle, TaskSnapshot, TaskTreeError};
use chrono::LocalResult;
use chrono::TimeZone;
use chrono::{DateTime, Duration, Local};
use linked_hash_map::LinkedHashMap;
use std::error::Error;
use std::fmt;
use uuid::Uuid;
use yaml_rust::Yaml;

pub(crate) fn task_snapshot_to_yaml(snapshot: &TaskSnapshot) -> Yaml {
    task_snapshot_to_yaml_recursive(snapshot, true)
}

fn task_snapshot_to_yaml_recursive(snapshot: &TaskSnapshot, is_project_root: bool) -> Yaml {
    let default_attr = TaskAttr::with_identity(
        "デフォルト用",
        Uuid::nil(),
        DateTime::<Local>::MIN_UTC.into(),
    );
    let task = snapshot.attr();

    let mut task_hash = LinkedHashMap::new();

    task_hash.insert(
        Yaml::String(String::from("name")),
        Yaml::String(task.get_name().to_string()),
    );

    task_hash.insert(
        Yaml::String(String::from("id")),
        Yaml::String(task.get_id().to_string()),
    );

    let orig_status = task.get_orig_status();
    if orig_status != default_attr.get_orig_status() {
        task_hash.insert(
            Yaml::String(String::from("status")),
            Yaml::String(orig_status.to_string()),
        );
    }

    let is_on_other_side = task.get_is_on_other_side();
    if is_on_other_side != default_attr.get_is_on_other_side() {
        task_hash.insert(
            Yaml::String(String::from("is_on_other_side")),
            Yaml::Boolean(*is_on_other_side),
        );
    }

    let atomic = task.get_atomic();
    if atomic != default_attr.get_atomic() {
        task_hash.insert(Yaml::String(String::from("atomic")), Yaml::Boolean(atomic));
    }

    let fixed_start = task.get_fixed_start();
    if fixed_start
        || matches_legacy_fixed_start_shape(
            *task.get_start_time(),
            *task.get_deadline_time_opt(),
            task.get_estimated_work_seconds(),
        )
    {
        // 旧判定式に一致するfalseは省略すると、次回読込時にtrueへ戻ってしまう。
        task_hash.insert(
            Yaml::String(String::from("fixed_start")),
            Yaml::Boolean(fixed_start),
        );
    }

    let pending_until = task.get_pending_until();
    if pending_until != default_attr.get_pending_until() {
        let pending_until_string = pending_until.format("%Y/%m/%d %H:%M:%S").to_string();
        task_hash.insert(
            Yaml::String(String::from("pending_until")),
            Yaml::String(pending_until_string),
        );
    }

    let priority = task.get_priority();
    if is_project_root && priority != default_attr.get_priority() {
        task_hash.insert(
            Yaml::String(String::from("priority")),
            Yaml::Integer(priority),
        );
    }

    if is_project_root {
        if let Some(project_category) = task.get_project_category_opt() {
            task_hash.insert(
                Yaml::String(String::from("category")),
                Yaml::String(project_category.to_string()),
            );
        }
    }

    let create_time = task.get_create_time();
    let create_time_string = create_time.format("%Y/%m/%d %H:%M:%S").to_string();
    task_hash.insert(
        Yaml::String(String::from("create_time")),
        Yaml::String(create_time_string),
    );

    let start_time = task.get_start_time();
    let start_time_string = start_time.format("%Y/%m/%d %H:%M:%S").to_string();
    task_hash.insert(
        Yaml::String(String::from("start_time")),
        Yaml::String(start_time_string),
    );

    let end_time_opt = task.get_end_time_opt();
    if let Some(end_time) = end_time_opt {
        let end_time_string = end_time.format("%Y/%m/%d %H:%M:%S").to_string();
        task_hash.insert(
            Yaml::String(String::from("end_time")),
            Yaml::String(end_time_string),
        );
    }

    let deadline_time_opt = task.get_deadline_time_opt();
    if let Some(deadline_time) = deadline_time_opt {
        let deadline_time_string = deadline_time.format("%Y/%m/%d %H:%M:%S").to_string();
        task_hash.insert(
            Yaml::String(String::from("deadline_time")),
            Yaml::String(deadline_time_string),
        );
    }

    let estimated_work_seconds = task.get_estimated_work_seconds();
    if estimated_work_seconds != default_attr.get_estimated_work_seconds() {
        task_hash.insert(
            Yaml::String(String::from("estimated_work_seconds")),
            Yaml::Integer(estimated_work_seconds),
        );
    }

    let actual_work_seconds = task.get_actual_work_seconds();
    if actual_work_seconds != default_attr.get_actual_work_seconds() {
        task_hash.insert(
            Yaml::String(String::from("actual_work_seconds")),
            Yaml::Integer(actual_work_seconds),
        );
    }

    let repetition_interval_days_opt = task.get_repetition_interval_days_opt();
    if let Some(repetition_interval_days) = repetition_interval_days_opt {
        task_hash.insert(
            Yaml::String(String::from("repetition_interval_days")),
            Yaml::Integer(repetition_interval_days),
        );
    }

    let repetition_anchor = task.get_repetition_anchor();
    if repetition_anchor != default_attr.get_repetition_anchor() {
        task_hash.insert(
            Yaml::String(String::from("repetition_anchor")),
            Yaml::String(repetition_anchor.to_string()),
        );
    }

    let days_in_advance = task.get_days_in_advance();
    if days_in_advance != default_attr.get_days_in_advance() {
        task_hash.insert(
            Yaml::String(String::from("days_in_advance")),
            Yaml::Integer(days_in_advance),
        );
    }

    let mut children = vec![];
    for child_task in snapshot.children() {
        let child_yaml = task_snapshot_to_yaml_recursive(child_task, false);
        children.push(child_yaml);
    }

    if !children.is_empty() {
        task_hash.insert(
            Yaml::String(String::from("children")),
            Yaml::Array(children),
        );
    }

    Yaml::Hash(task_hash)
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

fn map_task_tree_error(error: TaskTreeError) -> YamlConversionError {
    YamlConversionError::new(error.to_string())
}

fn yaml_field<'a>(yaml: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    yaml.as_hash()?.get(&Yaml::String(key.to_string()))
}

fn matches_legacy_fixed_start_shape(
    start_time: DateTime<Local>,
    deadline_time: Option<DateTime<Local>>,
    estimated_work_seconds: i64,
) -> bool {
    Duration::try_seconds(estimated_work_seconds)
        .and_then(|duration| start_time.checked_add_signed(duration))
        .is_some_and(|expected_deadline| deadline_time == Some(expected_deadline))
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
) -> Result<TaskHandle, YamlConversionError> {
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
    let mut task =
        TaskHandle::with_identity(name, uuid::Uuid::new_v4(), now).map_err(map_task_tree_error)?;
    if let Some(id) = id {
        task.set_id(id).map_err(map_task_tree_error)?;
    }
    task.set_orig_status(status).map_err(map_task_tree_error)?;
    task.set_is_on_other_side(boolean("is_on_other_side")?)
        .map_err(map_task_tree_error)?;
    task.set_atomic(boolean("atomic")?)
        .map_err(map_task_tree_error)?;
    task.set_pending_until(pending)
        .map_err(map_task_tree_error)?;
    task.set_priority(match yaml_field(yaml, "priority") {
        None => 0,
        Some(value) => value
            .as_i64()
            .ok_or_else(|| strict_error(path, "priority", "must be an integer"))?,
    })
    .map_err(map_task_tree_error)?;
    task.set_project_category_opt(category)
        .map_err(map_task_tree_error)?;
    if let Some(create_time) = legacy_datetime("create_time")? {
        task.set_create_time(create_time)
            .map_err(map_task_tree_error)?;
    }
    if let Some(start_time) = legacy_datetime("start_time")? {
        task.set_start_time(start_time)
            .map_err(map_task_tree_error)?;
    }
    task.set_end_time_opt(optional_datetime("end_time")?)
        .map_err(map_task_tree_error)?;
    task.set_deadline_time_opt(optional_datetime("deadline_time")?)
        .map_err(map_task_tree_error)?;
    task.set_estimated_work_seconds(nonnegative("estimated_work_seconds", 900)?)
        .map_err(map_task_tree_error)?;
    let fixed_start = match yaml_field(yaml, "fixed_start") {
        Some(value) => value
            .as_bool()
            .ok_or_else(|| strict_error(path, "fixed_start", "must be a boolean"))?,
        None => matches_legacy_fixed_start_shape(
            task.get_start_time().map_err(map_task_tree_error)?,
            task.get_deadline_time_opt().map_err(map_task_tree_error)?,
            task.get_estimated_work_seconds()
                .map_err(map_task_tree_error)?,
        ),
    };
    task.set_fixed_start(fixed_start)
        .map_err(map_task_tree_error)?;
    task.set_actual_work_seconds(nonnegative("actual_work_seconds", 0)?)
        .map_err(map_task_tree_error)?;
    task.set_repetition_interval_days_opt(interval)
        .map_err(map_task_tree_error)?;
    task.set_repetition_anchor(anchor)
        .map_err(map_task_tree_error)?;
    task.set_days_in_advance(nonnegative("days_in_advance", 0)?)
        .map_err(map_task_tree_error)?;
    if interval.is_some() {
        task.set_pending_until(Local.with_ymd_and_hms(2037, 12, 31, 23, 59, 59).unwrap())
            .map_err(map_task_tree_error)?;
        task.set_orig_status(Status::Pending)
            .map_err(map_task_tree_error)?;
    }
    task.sync_clock(now).map_err(map_task_tree_error)?;
    for (index, child_yaml) in children.iter().enumerate() {
        let mut child = yaml_to_task_strict(child_yaml, now, &format!("{path}.children[{index}]"))?;
        child
            .reparent_to(&task)
            .map_err(|reason| strict_error(path, "children", &reason.to_string()))?;
    }
    Ok(task)
}

pub(crate) fn yaml_to_task(
    yaml: &Yaml,
    now: DateTime<Local>,
) -> Result<TaskHandle, YamlConversionError> {
    yaml_to_task_strict(yaml, now, "project")
}

#[cfg(test)]
include!("yaml_tests.rs");
