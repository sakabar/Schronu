use crate::entity::discarded_session::{
    DiscardedSessionEvent, DiscardedSessionReason, DiscardedSessionSource,
    PersistedDiscardedSessionEvent,
};
use chrono::{Datelike, NaiveDate};
use linked_hash_map::LinkedHashMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use yaml_rust::{Yaml, YamlEmitter, YamlLoader};

const JOURNAL_VERSION: i64 = 1;

#[derive(Debug)]
pub(super) struct DiscardedSessionJournalYamlError {
    path: PathBuf,
    detail: String,
}

impl DiscardedSessionJournalYamlError {
    fn new(path: &Path, detail: impl Into<String>) -> Self {
        Self {
            path: path.to_path_buf(),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for DiscardedSessionJournalYamlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "discarded session journal parse failed for {}: {}",
            self.path.display(),
            self.detail
        )
    }
}

impl Error for DiscardedSessionJournalYamlError {}

pub(super) fn parse_journal(
    path: &Path,
    bytes: &[u8],
) -> Result<Vec<DiscardedSessionEvent>, DiscardedSessionJournalYamlError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| DiscardedSessionJournalYamlError::new(path, error.to_string()))?;
    let documents = YamlLoader::load_from_str(text)
        .map_err(|error| DiscardedSessionJournalYamlError::new(path, error.to_string()))?;
    if documents.len() != 1 {
        return Err(DiscardedSessionJournalYamlError::new(
            path,
            "document must contain exactly one YAML document",
        ));
    }
    let root = documents
        .first()
        .and_then(Yaml::as_hash)
        .ok_or_else(|| DiscardedSessionJournalYamlError::new(path, "document must be a mapping"))?;
    let version = mapping_value(root, "version")
        .and_then(Yaml::as_i64)
        .ok_or_else(|| DiscardedSessionJournalYamlError::new(path, "version must be an integer"))?;
    if version != JOURNAL_VERSION {
        return Err(DiscardedSessionJournalYamlError::new(
            path,
            format!("unsupported version {version}"),
        ));
    }
    let events = mapping_value(root, "events")
        .and_then(Yaml::as_vec)
        .ok_or_else(|| DiscardedSessionJournalYamlError::new(path, "events must be an array"))?;
    events
        .iter()
        .enumerate()
        .map(|(index, event)| parse_event(path, index, event))
        .collect()
}

fn parse_event(
    path: &Path,
    index: usize,
    yaml: &Yaml,
) -> Result<DiscardedSessionEvent, DiscardedSessionJournalYamlError> {
    let map = yaml.as_hash().ok_or_else(|| {
        DiscardedSessionJournalYamlError::new(path, format!("events[{index}] must be a mapping"))
    })?;
    let string = |key: &str| -> Result<&str, DiscardedSessionJournalYamlError> {
        mapping_value(map, key)
            .and_then(Yaml::as_str)
            .ok_or_else(|| {
                DiscardedSessionJournalYamlError::new(
                    path,
                    format!("events[{index}].{key} must be a string"),
                )
            })
    };
    let integer = |key: &str| -> Result<i64, DiscardedSessionJournalYamlError> {
        mapping_value(map, key)
            .and_then(Yaml::as_i64)
            .ok_or_else(|| {
                DiscardedSessionJournalYamlError::new(
                    path,
                    format!("events[{index}].{key} must be an integer"),
                )
            })
    };
    let event_id = Uuid::parse_str(string("event_id")?).map_err(|error| {
        DiscardedSessionJournalYamlError::new(path, format!("events[{index}].event_id: {error}"))
    })?;
    let task_id = Uuid::parse_str(string("task_id")?).map_err(|error| {
        DiscardedSessionJournalYamlError::new(path, format!("events[{index}].task_id: {error}"))
    })?;
    let logical_date =
        NaiveDate::parse_from_str(string("logical_date")?, "%Y-%m-%d").map_err(|error| {
            DiscardedSessionJournalYamlError::new(
                path,
                format!("events[{index}].logical_date: {error}"),
            )
        })?;
    let source = DiscardedSessionSource::parse(string("source")?).ok_or_else(|| {
        DiscardedSessionJournalYamlError::new(path, format!("events[{index}].source is unknown"))
    })?;
    let reason = DiscardedSessionReason::parse(string("reason")?).ok_or_else(|| {
        DiscardedSessionJournalYamlError::new(path, format!("events[{index}].reason is unknown"))
    })?;
    let event = DiscardedSessionEvent::from_persisted(PersistedDiscardedSessionEvent {
        event_id,
        task_id,
        task_name_at_start: string("task_name_at_start")?.to_string(),
        started_at_epoch_ms: integer("started_at_epoch_ms")?,
        ended_at_epoch_ms: integer("ended_at_epoch_ms")?,
        logical_date,
        source,
        reason,
    })
    .map_err(|error| {
        DiscardedSessionJournalYamlError::new(path, format!("events[{index}]: {error}"))
    })?
    .ok_or_else(|| {
        DiscardedSessionJournalYamlError::new(
            path,
            format!("events[{index}] must be at least one second"),
        )
    })?;
    let (year, month) = journal_month_from_path(path)?;
    if event.logical_date().year() != year || event.logical_date().month() != month {
        return Err(DiscardedSessionJournalYamlError::new(
            path,
            format!("events[{index}] belongs to a different logical month"),
        ));
    }
    Ok(event)
}

fn mapping_value<'a>(map: &'a yaml_rust::yaml::Hash, key: &str) -> Option<&'a Yaml> {
    map.get(&Yaml::String(key.to_string()))
}

fn journal_month_from_path(path: &Path) -> Result<(i32, u32), DiscardedSessionJournalYamlError> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| {
            DiscardedSessionJournalYamlError::new(path, "file name must be YYYY-MM.yaml")
        })?;
    let date = NaiveDate::parse_from_str(&format!("{stem}-01"), "%Y-%m-%d").map_err(|_| {
        DiscardedSessionJournalYamlError::new(path, "file name must be YYYY-MM.yaml")
    })?;
    Ok((date.year(), date.month()))
}

pub(super) fn serialize_journal(
    path: &Path,
    events: &[DiscardedSessionEvent],
) -> Result<Vec<u8>, DiscardedSessionJournalYamlError> {
    let mut root = LinkedHashMap::new();
    root.insert(
        Yaml::String("version".to_string()),
        Yaml::Integer(JOURNAL_VERSION),
    );
    root.insert(
        Yaml::String("events".to_string()),
        Yaml::Array(events.iter().map(event_to_yaml).collect()),
    );
    let mut output = String::new();
    YamlEmitter::new(&mut output)
        .dump(&Yaml::Hash(root))
        .map_err(|error| DiscardedSessionJournalYamlError::new(path, error.to_string()))?;
    output.push('\n');
    Ok(output.into_bytes())
}

fn event_to_yaml(event: &DiscardedSessionEvent) -> Yaml {
    let mut map = LinkedHashMap::new();
    insert_string(&mut map, "event_id", event.event_id().to_string());
    insert_string(&mut map, "task_id", event.task_id().to_string());
    insert_string(
        &mut map,
        "task_name_at_start",
        event.task_name_at_start().to_string(),
    );
    map.insert(
        Yaml::String("started_at_epoch_ms".to_string()),
        Yaml::Integer(event.started_at_epoch_ms()),
    );
    map.insert(
        Yaml::String("ended_at_epoch_ms".to_string()),
        Yaml::Integer(event.ended_at_epoch_ms()),
    );
    insert_string(
        &mut map,
        "logical_date",
        event.logical_date().format("%Y-%m-%d").to_string(),
    );
    insert_string(&mut map, "source", event.source().as_str().to_string());
    insert_string(&mut map, "reason", event.reason().as_str().to_string());
    Yaml::Hash(map)
}

fn insert_string(map: &mut LinkedHashMap<Yaml, Yaml>, key: &str, value: String) {
    map.insert(Yaml::String(key.to_string()), Yaml::String(value));
}
