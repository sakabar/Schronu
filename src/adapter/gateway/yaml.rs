use crate::entity::datetime::parse_local_datetime;
use crate::entity::task::read_project_category;
use crate::entity::task::read_status;
use crate::entity::task::Status;
use crate::entity::task::{ImmutableTask, RepetitionAnchor, TaskHandle, TaskTreeError};
use chrono::LocalResult;
use chrono::TimeZone;
use chrono::{DateTime, Local};
use std::error::Error;
use std::fmt;
use uuid::Uuid;
use yaml_rust::Yaml;

pub fn yaml_to_immutable_task(yaml: &Yaml, now: DateTime<Local>) -> ImmutableTask {
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
        let child = yaml_to_immutable_task(child_yaml, now);
        children.push(child);
    }

    ImmutableTask::new_with_current_time(name, status, pending_until, children, now)
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
