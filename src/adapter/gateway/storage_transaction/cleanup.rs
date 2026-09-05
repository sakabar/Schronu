use std::path::Path;

use super::layout::TransactionLayout;
use super::{
    CommittedTransaction, StorageTransactionError, StorageTransactionIo,
    StorageTransactionOperation,
};

pub(super) fn cleanup_committed_transaction(
    transaction: &CommittedTransaction,
) -> Result<(), StorageTransactionError> {
    let layout = TransactionLayout::new(&transaction.state.paths.storage_dir_path);
    let cleanup_dir_path = layout.cleanup_dir_path(transaction.state.manifest.transaction_id);
    transaction
        .state
        .io
        .rename(
            &transaction.state.paths.transaction_dir_path,
            &cleanup_dir_path,
        )
        .map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::RenameForCleanup,
                &transaction.state.paths.transaction_dir_path,
                error,
            )
        })?;
    if transaction
        .state
        .io
        .sync_directory(&transaction.state.paths.transactions_dir_path)
        .is_ok()
    {
        let _ = transaction.state.io.remove_dir_all(&cleanup_dir_path);
        let _ = transaction
            .state
            .io
            .sync_directory(&transaction.state.paths.transactions_dir_path);
    }
    let _ = transaction
        .state
        .io
        .sync_directory(&transaction.state.paths.storage_dir_path);
    Ok(())
}

pub(super) fn cleanup_stale_tombstones(
    io: &dyn StorageTransactionIo,
    transactions_dir_path: &Path,
) {
    let Ok(entries) = io.read_directory_paths(transactions_dir_path) else {
        return;
    };
    let mut removed = false;
    for path in entries {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if TransactionLayout::cleanup_transaction_id(name).is_none() {
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
