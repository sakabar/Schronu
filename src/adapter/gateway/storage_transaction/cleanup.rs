use std::fs;
use std::path::Path;

use uuid::Uuid;

use super::{
    PreparedTransaction, StorageTransactionError, StorageTransactionIo, StorageTransactionOperation,
};

pub(super) fn cleanup_committed_transaction(
    transaction: &PreparedTransaction,
) -> Result<(), StorageTransactionError> {
    let cleanup_dir_path = transaction.transactions_dir_path.join(format!(
        ".cleanup-{}",
        transaction.transaction_id.hyphenated()
    ));
    transaction
        .io
        .rename(&transaction.transaction_dir_path, &cleanup_dir_path)
        .map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::RenameForCleanup,
                &transaction.transaction_dir_path,
                error,
            )
        })?;
    if transaction
        .io
        .sync_directory(&transaction.transactions_dir_path)
        .is_ok()
    {
        let _ = transaction.io.remove_dir_all(&cleanup_dir_path);
        let _ = transaction
            .io
            .sync_directory(&transaction.transactions_dir_path);
    }
    let _ = transaction.io.sync_directory(&transaction.storage_dir_path);
    Ok(())
}

pub(super) fn cleanup_stale_tombstones(
    io: &dyn StorageTransactionIo,
    transactions_dir_path: &Path,
) {
    let Ok(entries) = fs::read_dir(transactions_dir_path) else {
        return;
    };
    let mut removed = false;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(uuid_text) = name.strip_prefix(".cleanup-") else {
            continue;
        };
        let Ok(uuid) = Uuid::parse_str(uuid_text) else {
            continue;
        };
        if uuid.hyphenated().to_string() != uuid_text {
            continue;
        }
        let Ok(metadata) = io.symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_dir() {
            continue;
        }
        if io.remove_dir_all(&path).is_ok() {
            removed = true;
        }
    }
    if removed {
        let _ = io.sync_directory(transactions_dir_path);
    }
}
