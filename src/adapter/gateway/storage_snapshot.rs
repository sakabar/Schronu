mod create;
mod error;
pub(in crate::adapter::gateway) mod io;
mod layout;
pub(super) mod manifest;
mod restore;
mod verify;

pub use create::create_snapshot;
#[cfg(test)]
pub(in crate::adapter::gateway) use create::{
    create_snapshot_after_parent_open, create_snapshot_at, create_snapshot_before_publish,
    create_snapshot_with_failure, create_snapshot_with_failure_observation,
    create_snapshot_with_limits,
};
pub use error::SnapshotError;
#[cfg(test)]
pub(in crate::adapter::gateway) use io::SnapshotFailurePoint;
pub use restore::restore_snapshot;
#[cfg(test)]
pub(in crate::adapter::gateway) use restore::{
    restore_snapshot_after_parent_open, restore_snapshot_before_publish,
    restore_snapshot_with_failure, restore_snapshot_with_failure_observation,
};
pub use verify::verify_snapshot;

use error::{SnapshotError as InternalSnapshotError, SnapshotLimitKind};
use std::path::Path;

const DEFAULT_RESOURCE_LIMITS: SnapshotResourceLimits = SnapshotResourceLimits::new(
    8 * 1024 * 1024,
    10_000,
    64 * 1024 * 1024,
    256 * 1024 * 1024,
    4_096,
    64,
);

#[derive(Clone, Copy)]
pub(in crate::adapter::gateway) struct SnapshotResourceLimits {
    manifest_bytes: u64,
    file_count: usize,
    file_bytes: u64,
    total_bytes: u64,
    path_bytes: usize,
    depth: usize,
}

impl SnapshotResourceLimits {
    pub(in crate::adapter::gateway) const fn new(
        manifest_bytes: u64,
        file_count: usize,
        file_bytes: u64,
        total_bytes: u64,
        path_bytes: usize,
        depth: usize,
    ) -> Self {
        Self {
            manifest_bytes,
            file_count,
            file_bytes,
            total_bytes,
            path_bytes,
            depth,
        }
    }

    #[cfg(test)]
    pub(in crate::adapter::gateway) const fn with_manifest_bytes(self, limit: u64) -> Self {
        Self {
            manifest_bytes: limit,
            ..self
        }
    }

    #[cfg(test)]
    pub(in crate::adapter::gateway) const fn with_file_count(self, limit: usize) -> Self {
        Self {
            file_count: limit,
            ..self
        }
    }

    #[cfg(test)]
    pub(in crate::adapter::gateway) const fn with_file_bytes(self, limit: u64) -> Self {
        Self {
            file_bytes: limit,
            ..self
        }
    }

    #[cfg(test)]
    pub(in crate::adapter::gateway) const fn with_total_bytes(self, limit: u64) -> Self {
        Self {
            total_bytes: limit,
            ..self
        }
    }

    #[cfg(test)]
    pub(in crate::adapter::gateway) const fn with_path_bytes(self, limit: usize) -> Self {
        Self {
            path_bytes: limit,
            ..self
        }
    }

    #[cfg(test)]
    pub(in crate::adapter::gateway) const fn with_depth(self, limit: usize) -> Self {
        Self {
            depth: limit,
            ..self
        }
    }

    fn check_path(self, path: &Path) -> Result<(), InternalSnapshotError> {
        let observed_bytes = path.to_str().map(str::len).unwrap_or(usize::MAX);
        self.check(
            path,
            SnapshotLimitKind::PathBytes,
            self.path_bytes as u64,
            u64::try_from(observed_bytes).unwrap_or(u64::MAX),
        )?;
        self.check(
            path,
            SnapshotLimitKind::PathDepth,
            self.depth as u64,
            path.components().count() as u64,
        )
    }

    fn check(
        self,
        path: &Path,
        kind: SnapshotLimitKind,
        limit: u64,
        observed: u64,
    ) -> Result<(), InternalSnapshotError> {
        if observed > limit {
            Err(InternalSnapshotError::limit(path, kind, limit, observed))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotSummary {
    revision: Option<uuid::Uuid>,
    file_count: usize,
}

impl SnapshotSummary {
    fn new(revision: Option<uuid::Uuid>, file_count: usize) -> Self {
        Self {
            revision,
            file_count,
        }
    }

    pub fn revision(&self) -> Option<uuid::Uuid> {
        self.revision
    }

    pub fn file_count(&self) -> usize {
        self.file_count
    }
}
