use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug)]
pub(super) enum FileWriteStage {
    Write,
    Sync,
}

#[derive(Debug)]
pub(super) struct FileWriteError {
    stage: FileWriteStage,
    source: std::io::Error,
}

impl FileWriteError {
    pub(super) fn write(source: std::io::Error) -> Self {
        Self {
            stage: FileWriteStage::Write,
            source,
        }
    }

    pub(super) fn sync(source: std::io::Error) -> Self {
        Self {
            stage: FileWriteStage::Sync,
            source,
        }
    }

    pub(super) fn stage(&self) -> FileWriteStage {
        self.stage
    }
}

impl fmt::Display for FileWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl Error for FileWriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotLimitKind {
    ManifestBytes,
    FileCount,
    FileBytes,
    PayloadBytes,
    PathBytes,
    PathDepth,
}

#[derive(Debug)]
pub struct SnapshotError {
    operation: SnapshotOperation,
    path: PathBuf,
    source: Box<dyn Error + Send + Sync>,
    limit: Option<SnapshotLimitDetails>,
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
            limit: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn limit_kind(&self) -> Option<SnapshotLimitKind> {
        self.limit.as_ref().map(|details| details.kind)
    }

    pub fn limit_value(&self) -> Option<u64> {
        self.limit.as_ref().map(|details| details.limit)
    }

    pub fn observed_value(&self) -> Option<u64> {
        self.limit.as_ref().map(|details| details.observed)
    }

    pub fn limit_path(&self) -> Option<&Path> {
        self.limit
            .as_ref()
            .and_then(|details| details.path.as_deref())
    }

    pub(super) fn file_write(path: impl Into<PathBuf>, source: FileWriteError) -> Self {
        let operation = match source.stage() {
            FileWriteStage::Write => SnapshotOperation::Write,
            FileWriteStage::Sync => SnapshotOperation::Sync,
        };
        Self::new(operation, path, source)
    }

    pub(super) fn limit(
        path: impl Into<PathBuf>,
        kind: SnapshotLimitKind,
        limit: u64,
        observed: u64,
        limit_path: Option<PathBuf>,
    ) -> Self {
        Self {
            operation: SnapshotOperation::Validate,
            path: path.into(),
            source: Box::new(SnapshotLimitError {
                kind,
                limit,
                observed,
            }),
            limit: Some(SnapshotLimitDetails {
                kind,
                limit,
                observed,
                path: limit_path,
            }),
        }
    }

    pub(super) fn followup_failure<E>(primary: Self, action: &'static str, followup: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        let Self {
            operation,
            path,
            source,
            limit,
        } = primary;
        Self {
            operation,
            path,
            source: Box::new(SnapshotFollowupError {
                primary: source,
                action,
                followup: Box::new(followup),
            }),
            limit,
        }
    }
}

#[derive(Clone, Debug)]
struct SnapshotLimitDetails {
    kind: SnapshotLimitKind,
    limit: u64,
    observed: u64,
    path: Option<PathBuf>,
}

#[derive(Debug)]
struct SnapshotLimitError {
    kind: SnapshotLimitKind,
    limit: u64,
    observed: u64,
}

impl fmt::Display for SnapshotLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "snapshot resource limit {:?} exceeded: limit={}, observed={}",
            self.kind, self.limit, self.observed
        )
    }
}

impl Error for SnapshotLimitError {}

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
