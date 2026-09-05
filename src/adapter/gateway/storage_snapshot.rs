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
