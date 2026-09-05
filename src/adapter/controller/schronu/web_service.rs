#[path = "web_service/error.rs"]
mod error;
#[path = "web_service/model.rs"]
mod model;
#[path = "web_service/read_model.rs"]
mod read_model;

pub use error::{WebReadError, WebReadOverflowError};
pub use model::{ScheduledTaskRowDto, ServerSnapshot, SessionTaskDto, WebSuccess};
pub(super) use read_model::{build_auto_session_dto, build_scheduled_task_rows};
#[cfg(test)]
pub(super) use read_model::{build_server_snapshot, calculate_buffer_seconds};

use super::web_session_write::{
    prepare_add_actual_work_input, prepare_complete_task_input, CompleteSessionRequest,
    RecordSessionRequest, RecordSessionResult,
};
use crate::adapter::gateway::free_time_manager::FreeTimeManager;
use crate::adapter::gateway::schronu_config::SchronuConfig;
use crate::adapter::gateway::storage_lock::{LockMode, StorageLock, StorageLockError};
use crate::adapter::gateway::task_repository::TaskRepository;
use crate::application::interface::FreeTimeManagerTrait;
use crate::application::repository_transaction::{
    run_repository_transaction, RepositoryTransactionError,
};
use crate::application::schedule_use_case::get_schedule;
use crate::application::task_use_case::{add_actual_work, complete_task, TaskFactory};
use chrono::{DateTime, Local, NaiveDate};
use error::{WebReadCoreError, WebReadOperationError};
use read_model::{build_server_snapshot_from_schedule, build_server_snapshot_with_offset};
use std::path::PathBuf;

pub struct WebService {
    storage_directory: PathBuf,
    task_repository: Option<TaskRepository>,
    free_time_manager: FreeTimeManager,
    config: SchronuConfig,
    repository_state_uncertain: bool,
    mutation_repository_factory: Box<dyn Fn(&str) -> TaskRepository>,
}

impl WebService {
    pub fn new(storage_directory: PathBuf, config: SchronuConfig) -> Self {
        let task_repository = storage_directory.to_str().map(TaskRepository::new);
        Self {
            storage_directory,
            task_repository,
            free_time_manager: FreeTimeManager::new(),
            config,
            repository_state_uncertain: false,
            mutation_repository_factory: Box::new(TaskRepository::new),
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
        request: CompleteSessionRequest,
    ) -> Result<ServerSnapshot, WebReadError> {
        let input = prepare_complete_task_input(request, operation_now)
            .map_err(WebReadError::InvalidInput)?;
        self.run_mutation_at(operation_now, |repository, free_time_manager, offset| {
            let mut next_id = uuid::Uuid::new_v4;
            let mut factory = TaskFactory::new(operation_now, &mut next_id);
            complete_task(repository, input, &mut factory)
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
        .map_err(WebReadError::from_transaction)
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
        self.ensure_mutation_available()?;
        let busy_time_slots_path = self.busy_time_slots_path()?.to_owned();
        let end_of_day_offset_minutes = self.config.end_of_day_offset_minutes;
        let storage_directory = &self.storage_directory;
        let storage_path = storage_directory
            .to_str()
            .ok_or_else(|| WebReadError::PathEncoding(storage_directory.clone()))?;
        let mut task_repository = (self.mutation_repository_factory)(storage_path);
        let free_time_manager = &mut self.free_time_manager;

        let result = Self::run_with_repository_at(
            storage_directory,
            &mut task_repository,
            free_time_manager,
            &busy_time_slots_path,
            end_of_day_offset_minutes,
            operation_now,
            operation,
        );
        self.finish_mutation_result(result)
    }

    fn ensure_mutation_available(&self) -> Result<(), WebReadError> {
        if self.repository_state_uncertain {
            Err(WebReadError::RepositoryPoisoned)
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    pub(super) fn set_mutation_repository_factory(
        &mut self,
        factory: impl Fn(&str) -> TaskRepository + 'static,
    ) {
        self.mutation_repository_factory = Box::new(factory);
    }

    fn finish_mutation_result<T>(
        &mut self,
        result: Result<T, RepositoryTransactionError<StorageLockError, WebReadOperationError>>,
    ) -> Result<T, WebReadError> {
        if matches!(result, Err(RepositoryTransactionError::StateUncertain(_))) {
            self.repository_state_uncertain = true;
        }
        result.map_err(WebReadError::from_transaction)
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
    ) -> Result<T, RepositoryTransactionError<StorageLockError, WebReadOperationError>> {
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
    }
}

#[cfg(test)]
mod save_error_contract_tests {
    use super::*;
    use crate::application::interface::{TaskRepositoryError, TaskRepositoryOperation};

    fn save_error(retryable: bool) -> TaskRepositoryError {
        let source = std::io::Error::other("injected save failure");
        if retryable {
            TaskRepositoryError::retryable_save(source)
        } else {
            TaskRepositoryError::new(TaskRepositoryOperation::Save, source)
        }
    }

    #[test]
    fn retryable_save失敗は分類を公開しserviceをpoisonしない() {
        let mut service = WebService::new(PathBuf::from("unused"), SchronuConfig::default());

        let error = service
            .finish_mutation_result::<()>(Err(RepositoryTransactionError::SaveFailed(save_error(
                true,
            ))))
            .unwrap_err();

        assert!(matches!(error, WebReadError::RepositorySaveFailed(_)));
        assert!(!service.repository_state_uncertain);
    }

    #[test]
    fn state_uncertain失敗はserviceをpoisonして後続mutationを拒否する() {
        let mut service = WebService::new(PathBuf::from("unused"), SchronuConfig::default());
        let error = service
            .finish_mutation_result::<()>(Err(RepositoryTransactionError::StateUncertain(
                save_error(false),
            )))
            .unwrap_err();

        assert!(matches!(error, WebReadError::RepositoryStateUncertain(_)));
        assert!(service.repository_state_uncertain);
        assert!(matches!(
            service.ensure_mutation_available(),
            Err(WebReadError::RepositoryPoisoned)
        ));
    }
}
