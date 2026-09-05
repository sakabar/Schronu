use std::collections::HashSet;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const WORK_SESSIONS_STORAGE_KEY: &str = "schronu_web.work_sessions.v1";
const STORAGE_VERSION: u64 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkSession {
    pub task_id: String,
    pub task_name: String,
    pub started_at_epoch_ms: i64,
    pub estimated_work_seconds_at_start: i64,
    pub actual_work_seconds_at_start: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkSessionsState {
    sessions: Vec<WorkSession>,
    warnings: Vec<String>,
    write_blocked: bool,
    needs_repair: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageError {
    Unavailable,
    ReadFailed,
    WriteFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkSessionsMutationError {
    WriteBlocked,
    InvalidSession,
    SerializationFailed,
    Storage(StorageError),
}

pub trait KeyValueStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError>;
    fn set(&self, key: &str, value: &str) -> Result<(), StorageError>;
}

pub fn load_work_sessions<S: KeyValueStorage>(
    storage: &S,
) -> Result<WorkSessionsState, StorageError> {
    let Some(raw) = storage.get(WORK_SESSIONS_STORAGE_KEY)? else {
        return Ok(empty_state());
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return Ok(blocked_state(
            "保存済みセッションを読み取れません。localStorageを手動で確認してください。",
        ));
    };
    let Some(object) = value.as_object() else {
        return Ok(blocked_state(
            "保存済みセッションの形式が不正です。localStorageを手動で確認してください。",
        ));
    };
    if object.get("version").and_then(Value::as_u64) != Some(STORAGE_VERSION) {
        return Ok(blocked_state(
            "保存済みセッションのversionに対応していません。localStorageを手動で確認してください。",
        ));
    }
    let Some(entries) = object.get("work_sessions").and_then(Value::as_array) else {
        return Ok(blocked_state(
            "保存済みセッションの形式が不正です。localStorageを手動で確認してください。",
        ));
    };

    let mut sessions = Vec::with_capacity(entries.len());
    let mut seen_task_ids = HashSet::with_capacity(entries.len());
    let mut warnings = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let session = serde_json::from_value::<WorkSession>(entry.clone()).ok();
        let task_id = session.as_ref().and_then(valid_task_id);
        let is_duplicate = task_id
            .as_ref()
            .is_some_and(|task_id| !seen_task_ids.insert(*task_id));
        if let Some(session) = session.filter(|_| task_id.is_some() && !is_duplicate) {
            sessions.push(session);
        } else {
            warnings.push(format!(
                "保存済みセッションの{}件目を不正なentryとして除外しました。",
                index + 1
            ));
        }
    }

    Ok(WorkSessionsState {
        sessions,
        needs_repair: !warnings.is_empty(),
        warnings,
        write_blocked: false,
    })
}

impl WorkSessionsState {
    pub fn sessions(&self) -> &[WorkSession] {
        &self.sessions
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn write_blocked(&self) -> bool {
        self.write_blocked
    }

    pub fn needs_repair(&self) -> bool {
        self.needs_repair
    }

    pub fn replace_sessions<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        candidate: Vec<WorkSession>,
    ) -> Result<(), WorkSessionsMutationError> {
        if self.write_blocked {
            return Err(WorkSessionsMutationError::WriteBlocked);
        }
        validate_sessions(&candidate)?;
        let serialized = serde_json::to_string(&StoredWorkSessions {
            version: STORAGE_VERSION,
            work_sessions: &candidate,
        })
        .map_err(|_| WorkSessionsMutationError::SerializationFailed)?;
        storage
            .set(WORK_SESSIONS_STORAGE_KEY, &serialized)
            .map_err(WorkSessionsMutationError::Storage)?;

        self.sessions = candidate;
        self.needs_repair = false;
        Ok(())
    }
}

#[derive(Serialize)]
struct StoredWorkSessions<'a> {
    version: u64,
    work_sessions: &'a [WorkSession],
}

fn validate_sessions(sessions: &[WorkSession]) -> Result<(), WorkSessionsMutationError> {
    let mut seen_task_ids = HashSet::with_capacity(sessions.len());
    if sessions
        .iter()
        .all(|session| valid_task_id(session).is_some_and(|task_id| seen_task_ids.insert(task_id)))
    {
        Ok(())
    } else {
        Err(WorkSessionsMutationError::InvalidSession)
    }
}

fn valid_task_id(session: &WorkSession) -> Option<Uuid> {
    if session.task_name.trim().is_empty()
        || session.estimated_work_seconds_at_start < 0
        || session.actual_work_seconds_at_start < 0
        || DateTime::<Utc>::from_timestamp_millis(session.started_at_epoch_ms).is_none()
    {
        return None;
    }
    Uuid::parse_str(&session.task_id).ok()
}

fn empty_state() -> WorkSessionsState {
    WorkSessionsState {
        sessions: Vec::new(),
        warnings: Vec::new(),
        write_blocked: false,
        needs_repair: false,
    }
}

fn blocked_state(warning: &str) -> WorkSessionsState {
    WorkSessionsState {
        sessions: Vec::new(),
        warnings: vec![warning.to_owned()],
        write_blocked: true,
        needs_repair: false,
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => write!(formatter, "localStorageを利用できません。"),
            Self::ReadFailed => write!(formatter, "localStorageの読み取りに失敗しました。"),
            Self::WriteFailed => write!(formatter, "localStorageの保存に失敗しました。"),
        }
    }
}

impl std::error::Error for StorageError {}

impl fmt::Display for WorkSessionsMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WriteBlocked => write!(formatter, "localStorageの書き込みは停止されています。"),
            Self::InvalidSession => write!(formatter, "セッションの内容が不正です。"),
            Self::SerializationFailed => {
                write!(formatter, "セッションの保存形式を生成できません。")
            }
            Self::Storage(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for WorkSessionsMutationError {}

#[cfg(feature = "web")]
#[derive(Clone, Copy, Debug, Default)]
pub struct BrowserLocalStorage;

#[cfg(feature = "web")]
impl KeyValueStorage for BrowserLocalStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        browser_storage(StorageError::ReadFailed)?
            .get_item(key)
            .map_err(|_| StorageError::ReadFailed)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        browser_storage(StorageError::WriteFailed)?
            .set_item(key, value)
            .map_err(|_| StorageError::WriteFailed)
    }
}

#[cfg(feature = "web")]
fn browser_storage(error: StorageError) -> Result<web_sys::Storage, StorageError> {
    web_sys::window()
        .ok_or(StorageError::Unavailable)?
        .local_storage()
        .map_err(|_| error)?
        .ok_or(StorageError::Unavailable)
}
