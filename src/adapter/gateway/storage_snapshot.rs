#[allow(dead_code)]
mod error;
#[allow(dead_code)]
mod layout;
#[allow(dead_code)]
pub(super) mod manifest;

pub use error::SnapshotError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotSummary {
    revision: Option<uuid::Uuid>,
    file_count: usize,
}

impl SnapshotSummary {
    pub fn revision(&self) -> Option<uuid::Uuid> {
        self.revision
    }

    pub fn file_count(&self) -> usize {
        self.file_count
    }
}
