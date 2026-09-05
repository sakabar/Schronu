use std::path::Path;
use std::sync::Arc;

use super::cleanup::cleanup_stale_tombstones;
use super::io::{
    acquire_transaction_lock, resolve_transactions_directory, sync_directory,
    validate_transactions_directory,
};
use super::layout::TransactionLayout;
use super::manifest::{read_validated_manifest, ValidatedManifest};
#[cfg(test)]
use super::manifest::{validate_raw_manifest, RawTransactionManifest};
#[cfg(test)]
use super::PreparedTransaction;
use super::{
    CommittedTransaction, StorageTransactionError, StorageTransactionIo,
    StorageTransactionOperation, TransactionLock, TransactionPaths, TransactionState,
};

pub(in crate::adapter::gateway) fn recover(
    io: Arc<dyn StorageTransactionIo>,
    storage_dir_path: &Path,
) -> Result<(), StorageTransactionError> {
    let layout = TransactionLayout::new(storage_dir_path);
    let Some(transactions_dir_path) =
        resolve_transactions_directory(io.as_ref(), storage_dir_path, false)?
    else {
        return Ok(());
    };
    let _transaction_lock = acquire_transaction_lock(&transactions_dir_path)?;
    validate_transactions_directory(io.as_ref(), &transactions_dir_path)?;
    cleanup_stale_tombstones(io.as_ref(), &transactions_dir_path);
    let transaction_dir_path = layout.active_transaction_dir_path();
    match io.symlink_metadata(&transaction_dir_path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(StorageTransactionError::new(
                StorageTransactionOperation::InspectActiveTransaction,
                &transaction_dir_path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "active transaction must be a directory",
                ),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(StorageTransactionError::new(
                StorageTransactionOperation::InspectActiveTransaction,
                &transaction_dir_path,
                error,
            ));
        }
    }

    let marker_path = TransactionLayout::commit_marker_path(&transaction_dir_path);
    match io.symlink_metadata(&marker_path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(StorageTransactionError::new(
                    StorageTransactionOperation::ValidateCommitMarker,
                    marker_path,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "transaction commit marker must be a regular file",
                    ),
                ));
            }
            let manifest =
                read_validated_manifest(io.as_ref(), storage_dir_path, &transaction_dir_path)?;
            let state = transaction_state_from_validated_manifest(
                io,
                TransactionPaths {
                    storage_dir_path: storage_dir_path.to_path_buf(),
                    transactions_dir_path,
                    transaction_dir_path,
                },
                manifest,
                _transaction_lock,
            );
            return CommittedTransaction { state }.roll_forward();
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(StorageTransactionError::new(
                StorageTransactionOperation::InspectActiveTransaction,
                marker_path,
                error,
            ));
        }
    }

    io.remove_dir_all(&transaction_dir_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::DiscardUncommitted,
            &transaction_dir_path,
            error,
        )
    })?;
    sync_directory(io.as_ref(), &transactions_dir_path)?;
    Ok(())
}

#[cfg(test)]
pub(super) fn prepared_from_manifest(
    io: Arc<dyn StorageTransactionIo>,
    paths: TransactionPaths,
    manifest: RawTransactionManifest,
    transaction_lock: TransactionLock,
) -> Result<PreparedTransaction, StorageTransactionError> {
    let manifest = validate_raw_manifest(
        io.as_ref(),
        &paths.storage_dir_path,
        &paths.transaction_dir_path,
        manifest,
    )?;
    Ok(PreparedTransaction {
        state: transaction_state_from_validated_manifest(io, paths, manifest, transaction_lock),
    })
}

fn transaction_state_from_validated_manifest(
    io: Arc<dyn StorageTransactionIo>,
    paths: TransactionPaths,
    manifest: ValidatedManifest,
    transaction_lock: TransactionLock,
) -> TransactionState {
    TransactionState {
        paths,
        manifest,
        io,
        _transaction_lock: transaction_lock,
    }
}
