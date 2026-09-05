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
    create_snapshot_after_parent_open, create_snapshot_at, finalize_publication,
};
pub use error::SnapshotError;
pub use restore::restore_snapshot;
#[cfg(test)]
pub(in crate::adapter::gateway) use restore::restore_snapshot_after_parent_open;
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
