use super::super::web_session_write::WebSessionInputError;
use crate::adapter::gateway::storage_lock::StorageLockError;
use crate::application::interface::{BusyTimeSlotLoadError, TaskRepositoryError};
use crate::application::repository_transaction::RepositoryTransactionError;
use crate::application::task_use_case::ApplicationError;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebReadOverflowError {
    operation: &'static str,
    left: i64,
    right: i64,
}

impl WebReadOverflowError {
    pub(super) fn new(operation: &'static str, left: i64, right: i64) -> Self {
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
pub(in crate::adapter::controller) enum WebReadCoreError {
    Application(ApplicationError),
    Overflow(WebReadOverflowError),
}

impl From<WebReadOverflowError> for WebReadCoreError {
    fn from(error: WebReadOverflowError) -> Self {
        Self::Overflow(error)
    }
}

#[derive(Debug)]
pub(super) enum WebReadOperationError {
    BusyTimeSlots(BusyTimeSlotLoadError),
    Core(WebReadCoreError),
}

#[derive(Debug)]
pub enum WebReadError {
    BusyTimeSlots(BusyTimeSlotLoadError),
    Lock(StorageLockError),
    Repository(TaskRepositoryError),
    RepositorySaveFailed(TaskRepositoryError),
    RepositoryStateUncertain(TaskRepositoryError),
    RepositoryPoisoned,
    Application(ApplicationError),
    InvalidInput(WebSessionInputError),
    PathEncoding(PathBuf),
    Overflow(WebReadOverflowError),
}

impl WebReadError {
    pub(super) fn from_transaction(
        error: RepositoryTransactionError<StorageLockError, WebReadOperationError>,
    ) -> Self {
        match error {
            RepositoryTransactionError::Lock(error) => Self::Lock(error),
            RepositoryTransactionError::Load(error) => Self::Repository(error),
            RepositoryTransactionError::SaveFailed(error) => Self::RepositorySaveFailed(error),
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
            Self::RepositorySaveFailed(error) => {
                write!(formatter, "repository save failed before commit: {error}")
            }
            Self::RepositoryStateUncertain(error) => {
                write!(
                    formatter,
                    "repository state is uncertain after save: {error}"
                )
            }
            Self::RepositoryPoisoned => write!(
                formatter,
                "repository state is uncertain after a previous save failure"
            ),
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
            Self::RepositorySaveFailed(error) => Some(error),
            Self::RepositoryStateUncertain(error) => Some(error),
            Self::Application(error) => Some(error),
            Self::InvalidInput(error) => Some(error),
            Self::Overflow(error) => Some(error),
            Self::PathEncoding(_) | Self::RepositoryPoisoned => None,
        }
    }
}
