use crate::entity::task::{TaskHandle, TaskTreeError};
use chrono::{DateTime, Local};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskRepositoryOperation {
    Load,
    Save,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryReloadOutcome {
    Reloaded,
    Cached,
}

#[derive(Debug)]
pub struct TaskRepositoryError {
    operation: TaskRepositoryOperation,
    source: Box<dyn Error + Send + Sync>,
}

impl TaskRepositoryError {
    pub fn new<E>(operation: TaskRepositoryOperation, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            operation,
            source: Box::new(source),
        }
    }

    pub fn operation(&self) -> TaskRepositoryOperation {
        self.operation
    }
}

impl fmt::Display for TaskRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "repository {:?} failed: {}",
            self.operation, self.source
        )
    }
}

impl Error for TaskRepositoryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

pub trait TaskRepositoryTrait {
    fn get_project_storage_dir_name(&self) -> &str;
    fn get_all_projects(&self) -> Vec<&TaskHandle>;
    fn load(&mut self) -> Result<(), TaskRepositoryError>;
    fn reload_if_changed(
        &mut self,
        now: DateTime<Local>,
    ) -> Result<RepositoryReloadOutcome, TaskRepositoryError> {
        self.sync_clock(now)
            .map_err(|error| TaskRepositoryError::new(TaskRepositoryOperation::Load, error))?;
        self.load()?;
        Ok(RepositoryReloadOutcome::Reloaded)
    }
    fn has_pending_changes(&self) -> Result<bool, TaskTreeError> {
        Ok(true)
    }
    fn save(&self) -> Result<(), TaskRepositoryError>;
    fn sync_clock(&mut self, now: DateTime<Local>) -> Result<(), TaskTreeError>;
    fn get_last_synced_time(&self) -> DateTime<Local>;
    fn get_highest_priority_project(&mut self) -> Option<&TaskHandle>;
    fn get_highest_priority_leaf_task_id(&mut self) -> Result<Option<Uuid>, TaskTreeError>;
    fn get_defer_candidate_leaf_task_id(
        &mut self,
        recent_days: i64,
    ) -> Result<Option<Uuid>, TaskTreeError>;
    fn get_by_id(&self, id: Uuid) -> Result<Option<TaskHandle>, TaskTreeError>;
    fn start_new_project(&mut self, root_task: TaskHandle) -> Result<(), TaskTreeError>;
}

pub trait FreeTimeManagerTrait {
    fn get_free_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64;
    fn get_busy_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64;
    fn register_busy_time_slot(
        &mut self,
        start: &DateTime<Local>,
        end: &DateTime<Local>,
    ) -> Result<(), BusyTimeSlotRegistrationError>;
    fn load_busy_time_slots_from_file(
        &mut self,
        busy_time_slots_file_path: &str,
    ) -> Result<(), BusyTimeSlotLoadError>;
}

#[derive(Debug)]
pub struct BusyTimeSlotLoadError {
    path: PathBuf,
    field_path: String,
    value: Option<String>,
    source: Box<dyn Error + Send + Sync>,
}

impl BusyTimeSlotLoadError {
    pub fn new<E>(
        path: impl Into<PathBuf>,
        field_path: impl Into<String>,
        value: Option<String>,
        source: E,
    ) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            path: path.into(),
            field_path: field_path.into(),
            value,
            source: Box::new(source),
        }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn field_path(&self) -> &str {
        &self.field_path
    }
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }
    pub fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl fmt::Display for BusyTimeSlotLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to load busy time slots at {}:{}",
            self.path.display(),
            self.field_path
        )?;
        if let Some(value) = &self.value {
            write!(f, " (value: {value})")?;
        }
        write!(f, ": {}", self.source)
    }
}
impl Error for BusyTimeSlotLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BusyTimeSlotRegistrationError;
impl fmt::Display for BusyTimeSlotRegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "different date between start and end")
    }
}
impl Error for BusyTimeSlotRegistrationError {}
