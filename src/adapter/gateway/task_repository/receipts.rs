use super::*;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Error as IoError, ErrorKind};

// This file belongs to task storage, including its backup/restore boundary.
// The transaction journal itself is temporary and cannot act as a receipt.
const RECEIPTS_FILE: &str = ".schronu-offline-receipts.json";

pub(crate) enum ReceiptStatus {
    New,
    AlreadyApplied,
    PayloadConflict,
}

impl TaskRepository {
    pub(super) fn receipts_path(&self) -> PathBuf {
        Path::new(&self.project_storage_dir_name).join(RECEIPTS_FILE)
    }

    // Called on an isolated mutation repository while the storage lock is held.
    // If validation or mutation subsequently fails, the candidate is discarded.
    pub(crate) fn stage_offline_receipt(
        &mut self,
        operation_id: Uuid,
        payload: &impl Serialize,
    ) -> Result<ReceiptStatus, TaskRepositoryError> {
        let payload = serde_json::to_value(payload).map_err(TaskRepositoryError::retryable_save)?;
        let path = self.receipts_path();
        let load_error = |source| {
            TaskRepositoryError::new(
                ApplicationRepositoryOperation::Load,
                FileRepositoryError::new(FileRepositoryOperation::ReadFile, &path, source),
            )
        };
        let mut receipts: BTreeMap<Uuid, Value> = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => {
                let bytes = fs::read(&path).map_err(load_error)?;
                serde_json::from_slice(&bytes)
                    .map_err(|error| load_error(IoError::new(ErrorKind::InvalidData, error)))?
            }
            Ok(_) => {
                return Err(load_error(IoError::new(
                    ErrorKind::InvalidData,
                    "offline receipts must be a regular file",
                )))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(load_error(error)),
        };
        if let Some(previous) = receipts.get(&operation_id) {
            return Ok(if previous == &payload {
                ReceiptStatus::AlreadyApplied
            } else {
                ReceiptStatus::PayloadConflict
            });
        }
        receipts.insert(operation_id, payload);
        let bytes = serde_json::to_vec(&receipts).map_err(TaskRepositoryError::retryable_save)?;
        *self.pending_offline_receipts.get_mut() = Some(bytes);
        Ok(ReceiptStatus::New)
    }
}
