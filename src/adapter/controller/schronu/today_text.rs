use super::renderer::render_plain_display_model;
use super::runtime::{select_focus_task_id, FocusSelectionMode};
use super::view::{build_show_all_tasks_display_with_config, TaskListDisplayOrder};
use crate::adapter::gateway::free_time_manager::FreeTimeManager;
use crate::adapter::gateway::schronu_config::SchronuConfig;
use crate::adapter::gateway::storage_lock::{LockMode, StorageLock, StorageLockError};
use crate::adapter::gateway::task_repository::TaskRepository;
use crate::application::interface::{
    BusyTimeSlotLoadError, FreeTimeManagerTrait, TaskRepositoryError,
};
use crate::application::repository_transaction::{
    run_repository_transaction, RepositoryTransactionError,
};
use crate::application::task_use_case::ApplicationError;
use chrono::{DateTime, Local};
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

pub struct TodayTextService {
    storage_directory: PathBuf,
    task_repository: Option<TaskRepository>,
    free_time_manager: FreeTimeManager,
    config: SchronuConfig,
}

impl TodayTextService {
    pub fn new(storage_directory: PathBuf, config: SchronuConfig) -> Self {
        let task_repository = storage_directory.to_str().map(TaskRepository::new);
        Self {
            storage_directory,
            task_repository,
            free_time_manager: FreeTimeManager::new(),
            config,
        }
    }

    pub fn render_at(&mut self, now: DateTime<Local>) -> Result<String, TodayTextError> {
        let busy_time_slots_path =
            self.config
                .busy_time_slots_yaml_path
                .to_str()
                .ok_or_else(|| {
                    TodayTextError::PathEncoding(self.config.busy_time_slots_yaml_path.clone())
                })?;
        let task_repository = self
            .task_repository
            .as_mut()
            .ok_or_else(|| TodayTextError::PathEncoding(self.storage_directory.clone()))?;
        let storage_directory = &self.storage_directory;
        let free_time_manager = &mut self.free_time_manager;
        let config = &self.config;
        let display = run_repository_transaction(
            task_repository,
            now,
            || StorageLock::acquire(storage_directory, LockMode::Web),
            |repository| {
                free_time_manager
                    .load_busy_time_slots_from_file(busy_time_slots_path)
                    .map_err(TodayTextOperationError::BusyTimeSlots)?;
                let mut focused_task_id_opt =
                    select_focus_task_id(repository, &FocusSelectionMode::highest_priority())
                        .map_err(TodayTextOperationError::Application)?;
                let display = build_show_all_tasks_display_with_config(
                    &mut focused_task_id_opt,
                    repository,
                    free_time_manager,
                    &Some("今".to_string()),
                    TaskListDisplayOrder::ScheduledStartDesc,
                    config,
                )
                .map_err(TodayTextOperationError::Application)?;
                Ok((display, false))
            },
        )
        .map_err(TodayTextError::from_transaction)?;

        let mut bytes = Vec::new();
        render_plain_display_model(&mut bytes, &display).map_err(TodayTextError::Render)?;
        String::from_utf8(bytes).map_err(TodayTextError::Encoding)
    }
}

#[derive(Debug)]
enum TodayTextOperationError {
    BusyTimeSlots(BusyTimeSlotLoadError),
    Application(ApplicationError),
}

#[derive(Debug)]
pub enum TodayTextError {
    BusyTimeSlots(BusyTimeSlotLoadError),
    Lock(StorageLockError),
    Repository(TaskRepositoryError),
    Application(ApplicationError),
    Render(std::io::Error),
    Encoding(std::string::FromUtf8Error),
    PathEncoding(PathBuf),
}

impl TodayTextError {
    fn from_transaction(
        error: RepositoryTransactionError<StorageLockError, TodayTextOperationError>,
    ) -> Self {
        match error {
            RepositoryTransactionError::Lock(error) => Self::Lock(error),
            RepositoryTransactionError::Load(error)
            | RepositoryTransactionError::StateUncertain(error) => Self::Repository(error),
            RepositoryTransactionError::Operation(TodayTextOperationError::BusyTimeSlots(
                error,
            )) => Self::BusyTimeSlots(error),
            RepositoryTransactionError::Operation(TodayTextOperationError::Application(error)) => {
                Self::Application(error)
            }
        }
    }
}

impl fmt::Display for TodayTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BusyTimeSlots(error) => write!(formatter, "busy time slot load failed: {error}"),
            Self::Lock(error) => write!(formatter, "storage lock failed: {error}"),
            Self::Repository(error) => write!(formatter, "repository read failed: {error}"),
            Self::Application(error) => write!(formatter, "today view generation failed: {error}"),
            Self::Render(error) => write!(formatter, "today text rendering failed: {error}"),
            Self::Encoding(error) => write!(formatter, "today text encoding failed: {error}"),
            Self::PathEncoding(path) => {
                write!(formatter, "path must be valid UTF-8: {}", path.display())
            }
        }
    }
}

impl Error for TodayTextError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::BusyTimeSlots(error) => Some(error),
            Self::Lock(error) => Some(error),
            Self::Repository(error) => Some(error),
            Self::Application(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::Encoding(error) => Some(error),
            Self::PathEncoding(_) => None,
        }
    }
}
