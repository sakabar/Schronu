use crate::application::interface::{TaskRepositoryError, TaskRepositoryTrait};
use chrono::{DateTime, Local};

#[derive(Debug)]
pub enum RepositoryTransactionError<LockError, OperationError> {
    Lock(LockError),
    Load(TaskRepositoryError),
    Operation(OperationError),
    StateUncertain(TaskRepositoryError),
}

pub fn run_repository_transaction<R, T, Lock, LockError, OperationError>(
    repository: &mut R,
    now: DateTime<Local>,
    acquire_lock: impl FnOnce() -> Result<Lock, LockError>,
    operation: impl FnOnce(&mut R) -> Result<(T, bool), OperationError>,
) -> Result<T, RepositoryTransactionError<LockError, OperationError>>
where
    R: TaskRepositoryTrait + ?Sized,
{
    let _lock = acquire_lock().map_err(RepositoryTransactionError::Lock)?;
    repository
        .reload_if_changed(now)
        .map_err(RepositoryTransactionError::Load)?;
    let (output, should_save) =
        operation(repository).map_err(RepositoryTransactionError::Operation)?;
    if should_save {
        repository
            .save()
            .map_err(RepositoryTransactionError::StateUncertain)?;
    }
    Ok(output)
}
