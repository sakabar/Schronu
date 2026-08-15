use crate::application::interface::{TaskRepositoryError, TaskRepositoryTrait};
use chrono::{DateTime, Local};

#[derive(Debug)]
pub enum RepositoryTransactionError<LockError, OperationError> {
    Lock(LockError),
    Load(TaskRepositoryError),
    Operation(OperationError),
    StateUncertain(TaskRepositoryError),
}

pub fn run_repository_transaction<T, Lock, LockError, OperationError>(
    repository: &mut dyn TaskRepositoryTrait,
    now: DateTime<Local>,
    acquire_lock: impl FnOnce() -> Result<Lock, LockError>,
    operation: impl FnOnce(&mut dyn TaskRepositoryTrait) -> Result<T, OperationError>,
) -> Result<T, RepositoryTransactionError<LockError, OperationError>> {
    let _lock = acquire_lock().map_err(RepositoryTransactionError::Lock)?;
    repository
        .reload_if_changed(now)
        .map_err(RepositoryTransactionError::Load)?;
    let output = operation(repository).map_err(RepositoryTransactionError::Operation)?;
    if repository.has_pending_changes() {
        repository
            .save()
            .map_err(RepositoryTransactionError::StateUncertain)?;
    }
    Ok(output)
}
