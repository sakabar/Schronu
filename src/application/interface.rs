use crate::entity::task::Task;
use chrono::{DateTime, Local};
use std::error::Error;
use std::fmt;
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
    fn get_all_projects(&self) -> Vec<&Task>;
    fn load(&mut self) -> Result<(), TaskRepositoryError>;
    fn reload_if_changed(
        &mut self,
        now: DateTime<Local>,
    ) -> Result<RepositoryReloadOutcome, TaskRepositoryError> {
        self.sync_clock(now);
        self.load()?;
        Ok(RepositoryReloadOutcome::Reloaded)
    }
    fn save(&self) -> Result<(), TaskRepositoryError>;
    fn sync_clock(&mut self, now: DateTime<Local>);
    fn get_last_synced_time(&self) -> DateTime<Local>;
    fn get_highest_priority_project(&mut self) -> Option<&Task>;
    fn get_highest_priority_leaf_task_id(&mut self) -> Option<Uuid>;
    fn get_defer_candidate_leaf_task_id(&mut self, recent_days: i64) -> Option<Uuid>;
    fn get_by_id(&self, id: Uuid) -> Option<Task>;
    fn start_new_project(&mut self, root_task: Task);
}

pub trait FreeTimeManagerTrait {
    fn get_free_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64;
    fn get_busy_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64;
    fn register_busy_time_slot(&mut self, start: &DateTime<Local>, end: &DateTime<Local>);
    fn load_busy_time_slots_from_file(
        &mut self,
        busy_time_slots_file_path: &str,
        now: &DateTime<Local>,
    );
}
