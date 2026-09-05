use super::web_session_write::{
    prepare_add_actual_work_input, RecordSessionRequest, RecordSessionResult, WebSessionInputError,
};
use crate::adapter::gateway::free_time_manager::FreeTimeManager;
use crate::adapter::gateway::schronu_config::SchronuConfig;
use crate::adapter::gateway::storage_lock::{LockMode, StorageLock, StorageLockError};
use crate::adapter::gateway::task_repository::TaskRepository;
use crate::application::daily_capacity::try_logical_date;
use crate::application::interface::{
    BusyTimeSlotLoadError, FreeTimeManagerTrait, TaskRepositoryError, TaskRepositoryTrait,
};
use crate::application::repository_transaction::{
    run_repository_transaction, RepositoryTransactionError,
};
use crate::application::schedule_use_case::{get_schedule, ScheduledTaskView};
use crate::application::task_use_case::{
    add_actual_work, complete_task, get_focus, ApplicationError, CompleteTaskInput, TaskFactory,
};
use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ServerSnapshot {
    pub observed_at_epoch_ms: i64,
    pub logical_date: String,
    pub buffer_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionTaskDto {
    pub task_id: String,
    pub task_name: String,
    pub estimated_work_seconds: i64,
    pub actual_work_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScheduledTaskRowDto {
    pub task: SessionTaskDto,
    pub schedule_start_epoch_ms: i64,
    pub schedule_end_epoch_ms: i64,
    pub deadline_epoch_ms: Option<i64>,
    pub is_leaf: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebSuccess<T> {
    pub snapshot: ServerSnapshot,
    pub data: T,
}

pub struct WebService {
    storage_directory: PathBuf,
    task_repository: Option<TaskRepository>,
    free_time_manager: FreeTimeManager,
    config: SchronuConfig,
}

impl WebService {
    pub fn new(storage_directory: PathBuf, config: SchronuConfig) -> Self {
        let task_repository = storage_directory.to_str().map(TaskRepository::new);
        Self {
            storage_directory,
            task_repository,
            free_time_manager: FreeTimeManager::new(),
            config,
        }
    }

    pub fn bootstrap_at(
        &mut self,
        operation_now: DateTime<Local>,
    ) -> Result<ServerSnapshot, WebReadError> {
        self.run_at(operation_now, |repository, free_time_manager, offset| {
            build_server_snapshot_with_offset(repository, free_time_manager, operation_now, offset)
        })
    }

    pub fn list_tasks_at(
        &mut self,
        operation_now: DateTime<Local>,
        logical_date: NaiveDate,
    ) -> Result<WebSuccess<Vec<ScheduledTaskRowDto>>, WebReadError> {
        self.run_at(operation_now, |repository, free_time_manager, offset| {
            let schedule = get_schedule(repository).map_err(WebReadCoreError::Application)?;
            let data = build_scheduled_task_rows(&schedule, logical_date)?;
            let snapshot = build_server_snapshot_from_schedule(
                repository,
                free_time_manager,
                operation_now,
                &schedule,
                offset,
            )?;
            Ok(WebSuccess { snapshot, data })
        })
    }

    pub fn auto_session_at(
        &mut self,
        operation_now: DateTime<Local>,
    ) -> Result<WebSuccess<Option<SessionTaskDto>>, WebReadError> {
        self.run_at(operation_now, |repository, free_time_manager, offset| {
            let data = build_auto_session_dto(repository).map_err(WebReadCoreError::Application)?;
            let snapshot = build_server_snapshot_with_offset(
                repository,
                free_time_manager,
                operation_now,
                offset,
            )?;
            Ok(WebSuccess { snapshot, data })
        })
    }

    pub fn record_session_at(
        &mut self,
        operation_now: DateTime<Local>,
        request: RecordSessionRequest,
    ) -> Result<WebSuccess<RecordSessionResult>, WebReadError> {
        let input = prepare_add_actual_work_input(request, operation_now)
            .map_err(WebReadError::InvalidInput)?;
        self.run_mutation_at(operation_now, |repository, free_time_manager, offset| {
            let actual_work_seconds =
                add_actual_work(repository, input).map_err(WebReadCoreError::Application)?;
            let snapshot = build_server_snapshot_with_offset(
                repository,
                free_time_manager,
                operation_now,
                offset,
            )?;
            Ok((
                WebSuccess {
                    snapshot,
                    data: RecordSessionResult {
                        actual_work_seconds,
                    },
                },
                true,
            ))
        })
    }

    pub fn complete_session_at(
        &mut self,
        operation_now: DateTime<Local>,
        request: RecordSessionRequest,
    ) -> Result<ServerSnapshot, WebReadError> {
        let input = prepare_add_actual_work_input(request, operation_now)
            .map_err(WebReadError::InvalidInput)?;
        self.run_mutation_at(operation_now, |repository, free_time_manager, offset| {
            let mut next_id = uuid::Uuid::new_v4;
            let mut factory = TaskFactory::new(operation_now, &mut next_id);
            complete_task(
                repository,
                CompleteTaskInput {
                    task_id: input.task_id,
                    finished_at: operation_now,
                    additional_actual_work_seconds: input.additional_actual_work_seconds,
                    expected_actual_work_seconds: input.expected_actual_work_seconds,
                },
                &mut factory,
            )
            .map_err(WebReadCoreError::Application)?;
            let snapshot = build_server_snapshot_with_offset(
                repository,
                free_time_manager,
                operation_now,
                offset,
            )?;
            Ok((snapshot, true))
        })
    }

    fn busy_time_slots_path(&self) -> Result<&str, WebReadError> {
        self.config
            .busy_time_slots_yaml_path
            .to_str()
            .ok_or_else(|| {
                WebReadError::PathEncoding(self.config.busy_time_slots_yaml_path.clone())
            })
    }

    fn run_at<T>(
        &mut self,
        operation_now: DateTime<Local>,
        operation: impl FnOnce(
            &mut TaskRepository,
            &mut FreeTimeManager,
            i64,
        ) -> Result<T, WebReadCoreError>,
    ) -> Result<T, WebReadError> {
        self.run_transaction_at(operation_now, |repository, free_time_manager, offset| {
            operation(repository, free_time_manager, offset).map(|output| (output, false))
        })
    }

    fn run_transaction_at<T>(
        &mut self,
        operation_now: DateTime<Local>,
        operation: impl FnOnce(
            &mut TaskRepository,
            &mut FreeTimeManager,
            i64,
        ) -> Result<(T, bool), WebReadCoreError>,
    ) -> Result<T, WebReadError> {
        let busy_time_slots_path = self.busy_time_slots_path()?.to_owned();
        let end_of_day_offset_minutes = self.config.end_of_day_offset_minutes;
        let storage_directory = &self.storage_directory;
        let task_repository = self
            .task_repository
            .as_mut()
            .ok_or_else(|| WebReadError::PathEncoding(self.storage_directory.clone()))?;
        let free_time_manager = &mut self.free_time_manager;

        Self::run_with_repository_at(
            storage_directory,
            task_repository,
            free_time_manager,
            &busy_time_slots_path,
            end_of_day_offset_minutes,
            operation_now,
            operation,
        )
    }

    fn run_mutation_at<T>(
        &mut self,
        operation_now: DateTime<Local>,
        operation: impl FnOnce(
            &mut TaskRepository,
            &mut FreeTimeManager,
            i64,
        ) -> Result<(T, bool), WebReadCoreError>,
    ) -> Result<T, WebReadError> {
        let busy_time_slots_path = self.busy_time_slots_path()?.to_owned();
        let end_of_day_offset_minutes = self.config.end_of_day_offset_minutes;
        let storage_directory = &self.storage_directory;
        let storage_path = storage_directory
            .to_str()
            .ok_or_else(|| WebReadError::PathEncoding(storage_directory.clone()))?;
        let mut task_repository = TaskRepository::new(storage_path);
        let free_time_manager = &mut self.free_time_manager;

        Self::run_with_repository_at(
            storage_directory,
            &mut task_repository,
            free_time_manager,
            &busy_time_slots_path,
            end_of_day_offset_minutes,
            operation_now,
            operation,
        )
    }

    fn run_with_repository_at<T>(
        storage_directory: &std::path::Path,
        task_repository: &mut TaskRepository,
        free_time_manager: &mut FreeTimeManager,
        busy_time_slots_path: &str,
        end_of_day_offset_minutes: i64,
        operation_now: DateTime<Local>,
        operation: impl FnOnce(
            &mut TaskRepository,
            &mut FreeTimeManager,
            i64,
        ) -> Result<(T, bool), WebReadCoreError>,
    ) -> Result<T, WebReadError> {
        run_repository_transaction(
            task_repository,
            operation_now,
            || StorageLock::acquire(storage_directory, LockMode::Web),
            |repository| {
                free_time_manager
                    .load_busy_time_slots_from_file(busy_time_slots_path)
                    .map_err(WebReadOperationError::BusyTimeSlots)?;
                operation(repository, free_time_manager, end_of_day_offset_minutes)
                    .map_err(WebReadOperationError::Core)
            },
        )
        .map_err(WebReadError::from_transaction)
    }
}

#[cfg(test)]
pub(super) fn build_server_snapshot<R, F>(
    task_repository: &mut R,
    free_time_manager: &mut F,
    operation_now: DateTime<Local>,
) -> Result<ServerSnapshot, WebReadCoreError>
where
    R: TaskRepositoryTrait,
    F: FreeTimeManagerTrait,
{
    build_server_snapshot_with_offset(
        task_repository,
        free_time_manager,
        operation_now,
        crate::application::daily_capacity::END_OF_DAY_OFFSET_MINUTES,
    )
}

fn build_server_snapshot_with_offset<R, F>(
    task_repository: &mut R,
    free_time_manager: &mut F,
    operation_now: DateTime<Local>,
    end_of_day_offset_minutes: i64,
) -> Result<ServerSnapshot, WebReadCoreError>
where
    R: TaskRepositoryTrait,
    F: FreeTimeManagerTrait,
{
    let schedule = get_schedule(task_repository).map_err(WebReadCoreError::Application)?;
    build_server_snapshot_from_schedule(
        task_repository,
        free_time_manager,
        operation_now,
        &schedule,
        end_of_day_offset_minutes,
    )
}

fn build_server_snapshot_from_schedule<R, F>(
    task_repository: &mut R,
    free_time_manager: &mut F,
    operation_now: DateTime<Local>,
    schedule: &[ScheduledTaskView],
    end_of_day_offset_minutes: i64,
) -> Result<ServerSnapshot, WebReadCoreError>
where
    R: TaskRepositoryTrait,
    F: FreeTimeManagerTrait,
{
    let logical_date = try_logical_date(operation_now).map_err(WebReadCoreError::Application)?;
    let current_logical_date = try_logical_date(task_repository.get_last_synced_time())
        .map_err(WebReadCoreError::Application)?;
    let free_seconds = if logical_date == current_logical_date {
        let end = crate::application::daily_capacity::try_logical_date_end(
            logical_date,
            end_of_day_offset_minutes,
        )
        .map_err(WebReadCoreError::Application)?;
        if task_repository.get_last_synced_time() < end {
            free_time_manager.get_free_seconds(&task_repository.get_last_synced_time(), &end)
        } else {
            0
        }
    } else {
        let start = crate::application::daily_capacity::try_logical_date_start(logical_date)
            .map_err(WebReadCoreError::Application)?;
        let end = crate::application::daily_capacity::try_logical_date_end(
            logical_date,
            end_of_day_offset_minutes,
        )
        .map_err(WebReadCoreError::Application)?;
        free_time_manager.get_free_seconds(&start, &end)
    };
    let scheduled_segments = schedule
        .iter()
        .map(|segment| {
            try_logical_date(segment.scheduled_start)
                .map(|date| (date, segment.scheduled_work_seconds))
                .map_err(WebReadCoreError::Application)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let buffer_seconds = calculate_buffer_seconds(logical_date, free_seconds, &scheduled_segments)?;

    Ok(ServerSnapshot {
        observed_at_epoch_ms: operation_now.timestamp_millis(),
        logical_date: logical_date.format("%Y-%m-%d").to_string(),
        buffer_seconds,
    })
}

pub(super) fn build_scheduled_task_rows(
    schedule: &[ScheduledTaskView],
    logical_date: NaiveDate,
) -> Result<Vec<ScheduledTaskRowDto>, WebReadCoreError> {
    let mut dated_segments = schedule
        .iter()
        .map(|segment| {
            try_logical_date(segment.scheduled_start)
                .map(|date| (date, segment))
                .map_err(WebReadCoreError::Application)
        })
        .collect::<Result<Vec<_>, _>>()?;
    dated_segments.retain(|(date, _)| *date == logical_date);
    dated_segments.sort_by_key(|(_, segment)| segment.scheduled_start);

    Ok(dated_segments
        .into_iter()
        .map(|(_, segment)| ScheduledTaskRowDto {
            task: session_task_dto(
                segment.task.id.hyphenated().to_string(),
                segment.task.name.clone(),
                segment.task.estimated_work_seconds,
                segment.task.actual_work_seconds,
            ),
            schedule_start_epoch_ms: segment.scheduled_start.timestamp_millis(),
            schedule_end_epoch_ms: segment.scheduled_end.timestamp_millis(),
            deadline_epoch_ms: segment
                .task
                .deadline_time
                .map(|deadline| deadline.timestamp_millis()),
            is_leaf: segment.task.child_ids.is_empty(),
        })
        .collect())
}

pub(super) fn build_auto_session_dto(
    task_repository: &mut dyn TaskRepositoryTrait,
) -> Result<Option<SessionTaskDto>, ApplicationError> {
    get_focus(task_repository).map(|task| {
        task.map(|task| {
            session_task_dto(
                task.id.hyphenated().to_string(),
                task.name,
                task.estimated_work_seconds,
                task.actual_work_seconds,
            )
        })
    })
}

fn session_task_dto(
    task_id: String,
    task_name: String,
    estimated_work_seconds: i64,
    actual_work_seconds: i64,
) -> SessionTaskDto {
    SessionTaskDto {
        task_id,
        task_name,
        estimated_work_seconds,
        actual_work_seconds,
    }
}

pub(super) fn calculate_buffer_seconds(
    current_logical_date: NaiveDate,
    free_seconds: i64,
    scheduled_segments: &[(NaiveDate, i64)],
) -> Result<i64, WebReadOverflowError> {
    let scheduled_seconds = scheduled_segments
        .iter()
        .filter(|(date, _)| *date == current_logical_date)
        .try_fold(0_i64, |total, (_, seconds)| {
            total
                .checked_add(*seconds)
                .ok_or_else(|| WebReadOverflowError::new("scheduled_seconds_sum", total, *seconds))
        })?;
    free_seconds.checked_sub(scheduled_seconds).ok_or_else(|| {
        WebReadOverflowError::new("buffer_subtraction", free_seconds, scheduled_seconds)
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebReadOverflowError {
    operation: &'static str,
    left: i64,
    right: i64,
}

impl WebReadOverflowError {
    fn new(operation: &'static str, left: i64, right: i64) -> Self {
        Self {
            operation,
            left,
            right,
        }
    }

    pub fn operation(&self) -> &'static str {
        self.operation
    }

    pub fn operands(&self) -> (i64, i64) {
        (self.left, self.right)
    }
}

impl fmt::Display for WebReadOverflowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{0} overflow for operands {1} and {2}",
            self.operation, self.left, self.right
        )
    }
}

impl Error for WebReadOverflowError {}

#[derive(Debug)]
pub(super) enum WebReadCoreError {
    Application(ApplicationError),
    Overflow(WebReadOverflowError),
}

impl From<WebReadOverflowError> for WebReadCoreError {
    fn from(error: WebReadOverflowError) -> Self {
        Self::Overflow(error)
    }
}

#[derive(Debug)]
enum WebReadOperationError {
    BusyTimeSlots(BusyTimeSlotLoadError),
    Core(WebReadCoreError),
}

#[derive(Debug)]
pub enum WebReadError {
    BusyTimeSlots(BusyTimeSlotLoadError),
    Lock(StorageLockError),
    Repository(TaskRepositoryError),
    RepositoryStateUncertain(TaskRepositoryError),
    Application(ApplicationError),
    InvalidInput(WebSessionInputError),
    PathEncoding(PathBuf),
    Overflow(WebReadOverflowError),
}

impl WebReadError {
    fn from_transaction(
        error: RepositoryTransactionError<StorageLockError, WebReadOperationError>,
    ) -> Self {
        match error {
            RepositoryTransactionError::Lock(error) => Self::Lock(error),
            RepositoryTransactionError::Load(error) => Self::Repository(error),
            RepositoryTransactionError::StateUncertain(error) => {
                Self::RepositoryStateUncertain(error)
            }
            RepositoryTransactionError::Operation(WebReadOperationError::BusyTimeSlots(error)) => {
                Self::BusyTimeSlots(error)
            }
            RepositoryTransactionError::Operation(WebReadOperationError::Core(
                WebReadCoreError::Application(error),
            )) => Self::Application(error),
            RepositoryTransactionError::Operation(WebReadOperationError::Core(
                WebReadCoreError::Overflow(error),
            )) => Self::Overflow(error),
        }
    }
}

impl fmt::Display for WebReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BusyTimeSlots(error) => write!(formatter, "busy time slot load failed: {error}"),
            Self::Lock(error) => write!(formatter, "storage lock failed: {error}"),
            Self::Repository(error) => write!(formatter, "repository read failed: {error}"),
            Self::RepositoryStateUncertain(error) => {
                write!(
                    formatter,
                    "repository state is uncertain after save: {error}"
                )
            }
            Self::Application(error) => write!(formatter, "Web operation failed: {error}"),
            Self::InvalidInput(error) => write!(formatter, "invalid Web request: {error}"),
            Self::PathEncoding(path) => {
                write!(formatter, "path must be valid UTF-8: {}", path.display())
            }
            Self::Overflow(error) => error.fmt(formatter),
        }
    }
}

impl Error for WebReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::BusyTimeSlots(error) => Some(error),
            Self::Lock(error) => Some(error),
            Self::Repository(error) => Some(error),
            Self::RepositoryStateUncertain(error) => Some(error),
            Self::Application(error) => Some(error),
            Self::InvalidInput(error) => Some(error),
            Self::Overflow(error) => Some(error),
            Self::PathEncoding(_) => None,
        }
    }
}
