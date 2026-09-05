use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::cleanup::cleanup_stale_tombstones;
use super::layout::TransactionLayout;
use super::manifest::{
    invalid_manifest_entry_error, validate_content_integrity, validate_staged_file_path,
    ContentIntegrity, ManifestEntryOperation, RawTransactionManifest, ValidatedEntry,
    ValidatedManifest,
};
#[cfg(test)]
use super::PreparedTransaction;
use super::{
    acquire_transaction_lock, resolve_transactions_directory, sync_directory,
    validate_delete_target_ancestors, validate_storage_relative_path,
    validate_transactions_directory, CommittedTransaction, StorageTransactionError,
    StorageTransactionIo, StorageTransactionOperation, TransactionLock, TransactionPaths,
    TransactionState,
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
    validate_transactions_directory(&transactions_dir_path)?;
    cleanup_stale_tombstones(io.as_ref(), &transactions_dir_path);
    let transaction_dir_path = layout.active_transaction_dir_path();
    match fs::symlink_metadata(&transaction_dir_path) {
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
            let manifest_path = TransactionLayout::manifest_path(&transaction_dir_path);
            let manifest_metadata = io.symlink_metadata(&manifest_path).map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::ReadManifest,
                    &manifest_path,
                    error,
                )
            })?;
            if !manifest_metadata.file_type().is_file() {
                return Err(StorageTransactionError::new(
                    StorageTransactionOperation::ValidateManifest,
                    &manifest_path,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "transaction manifest must be a regular file",
                    ),
                ));
            }
            let manifest_bytes = io.read_file(&manifest_path).map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::ReadManifest,
                    &manifest_path,
                    error,
                )
            })?;
            let manifest: RawTransactionManifest = serde_json::from_slice(&manifest_bytes)
                .map_err(|error| {
                    StorageTransactionError::new(
                        StorageTransactionOperation::ParseManifest,
                        &manifest_path,
                        std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                    )
                })?;
            let state = transaction_state_from_manifest(
                io,
                TransactionPaths {
                    storage_dir_path: storage_dir_path.to_path_buf(),
                    transactions_dir_path,
                    transaction_dir_path,
                },
                manifest,
                _transaction_lock,
            )?;
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
    Ok(PreparedTransaction {
        state: transaction_state_from_manifest(io, paths, manifest, transaction_lock)?,
    })
}

fn transaction_state_from_manifest(
    io: Arc<dyn StorageTransactionIo>,
    paths: TransactionPaths,
    manifest: RawTransactionManifest,
    transaction_lock: TransactionLock,
) -> Result<TransactionState, StorageTransactionError> {
    let manifest_path = TransactionLayout::manifest_path(&paths.transaction_dir_path);
    let RawTransactionManifest {
        version,
        transaction_id,
        revision,
        directories,
        entries,
    } = manifest;
    let layout = TransactionLayout::new(&paths.storage_dir_path);
    if version != 1 || transaction_id.is_nil() {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::ValidateManifest,
            manifest_path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "transaction manifest must use version 1 and a non-nil transaction id",
            ),
        ));
    }
    let directories = directories
        .into_iter()
        .map(|directory| {
            validate_storage_relative_path(&paths.storage_dir_path, &layout.target_path(&directory))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let entries = entries
        .into_iter()
        .map(|entry| {
            let target = validate_storage_relative_path(
                &paths.storage_dir_path,
                &layout.target_path(&entry.target),
            )?;
            if entry.operation == ManifestEntryOperation::Delete {
                validate_delete_target_ancestors(io.as_ref(), &paths.storage_dir_path, &target)?;
            }
            match (
                entry.operation,
                entry.staged_file,
                entry.content_length,
                entry.content_checksum,
            ) {
                (
                    ManifestEntryOperation::Write,
                    Some(staged_file),
                    Some(content_length),
                    Some(content_checksum),
                ) => {
                    validate_staged_file_path(&paths.transaction_dir_path, &staged_file)?;
                    validate_content_integrity(&manifest_path, content_length, &content_checksum)?;
                    Ok(ValidatedEntry::Write {
                        target,
                        staged_file,
                        integrity: ContentIntegrity {
                            content_length,
                            checksum: content_checksum,
                        },
                    })
                }
                (ManifestEntryOperation::Delete, None, None, None) => {
                    Ok(ValidatedEntry::Delete { target })
                }
                (ManifestEntryOperation::Write, None, _, _) => Err(invalid_manifest_entry_error(
                    &manifest_path,
                    "write entry must contain a staged file",
                )),
                (ManifestEntryOperation::Write, Some(_), _, _) => {
                    Err(invalid_manifest_entry_error(
                        &manifest_path,
                        "write entry must contain content length and checksum",
                    ))
                }
                (ManifestEntryOperation::Delete, Some(_), _, _) => {
                    Err(invalid_manifest_entry_error(
                        &manifest_path,
                        "delete entry must not contain a staged file",
                    ))
                }
                (ManifestEntryOperation::Delete, None, _, _) => Err(invalid_manifest_entry_error(
                    &manifest_path,
                    "delete entry must not contain content integrity information",
                )),
            }
        })
        .collect::<Result<Vec<_>, StorageTransactionError>>()?;
    let manifest = ValidatedManifest {
        transaction_id,
        revision,
        directories,
        entries,
    };
    Ok(TransactionState {
        paths,
        manifest,
        io,
        _transaction_lock: transaction_lock,
    })
}
