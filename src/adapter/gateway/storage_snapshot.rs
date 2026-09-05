mod create;
mod error;
pub(in crate::adapter::gateway) mod io;
mod layout;
pub(super) mod manifest;

pub use create::create_snapshot;
#[cfg(test)]
pub(in crate::adapter::gateway) use create::create_snapshot_at;
pub use error::SnapshotError;

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
