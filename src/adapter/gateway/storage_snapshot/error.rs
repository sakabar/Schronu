use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SnapshotOperation {
    AcquireLock,
    Create,
    Encode,
    Decode,
    Read,
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
