use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use super::io::{FileWriteError, FileWriteStage};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SnapshotOperation {
    AcquireLock,
    Create,
    Encode,
    Decode,
    Read,
    RepositoryLoad,
    Sync,
    Validate,
    Write,
}

#[derive(Debug)]
pub struct SnapshotError {
    operation: SnapshotOperation,
    path: PathBuf,
    source: Box<dyn Error + Send + Sync>,
}

impl SnapshotError {
    pub(super) fn new<E>(operation: SnapshotOperation, path: impl Into<PathBuf>, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            operation,
            path: path.into(),
            source: Box::new(source),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn file_write(path: impl Into<PathBuf>, source: FileWriteError) -> Self {
        let operation = match source.stage() {
            FileWriteStage::Write => SnapshotOperation::Write,
            FileWriteStage::Sync => SnapshotOperation::Sync,
        };
        Self::new(operation, path, source)
    }

    pub(super) fn followup_failure<E>(primary: Self, action: &'static str, followup: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        let Self {
            operation,
            path,
            source,
        } = primary;
        Self {
            operation,
            path,
            source: Box::new(SnapshotFollowupError {
                primary: source,
                action,
                followup: Box::new(followup),
            }),
        }
    }
}

#[derive(Debug)]
struct SnapshotFollowupError {
    primary: Box<dyn Error + Send + Sync>,
    action: &'static str,
    followup: Box<dyn Error + Send + Sync>,
}

impl fmt::Display for SnapshotFollowupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}; {} failed: {}",
            self.primary, self.action, self.followup
        )
    }
}

impl Error for SnapshotFollowupError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.followup.as_ref())
    }
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "storage snapshot {:?} failed for {}: {}",
            self.operation,
            self.path.display(),
            self.source
        )
    }
}

impl Error for SnapshotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}
