use std::path::{Path, PathBuf};
use std::sync::Arc;

use uuid::Uuid;

use super::cleanup::cleanup_stale_tombstones;
use super::{
    acquire_transaction_lock, content_checksum, resolve_transactions_directory, sync_directory,
    validate_storage_relative_path, validate_transactions_directory, ManifestEntry,
    ManifestEntryOperation, PreparedEntry, PreparedTransaction, StorageTransactionError,
    StorageTransactionIo, StorageTransactionOperation, TransactionManifest, WriteRequest,
    ACTIVE_TRANSACTION_DIRECTORY_NAME,
};

#[cfg(test)]
pub(in crate::adapter::gateway) fn prepare(
    io: Arc<dyn StorageTransactionIo>,
    storage_dir_path: &Path,
    revision: Uuid,
    writes: &[WriteRequest<'_>],
) -> Result<PreparedTransaction, StorageTransactionError> {
    prepare_with_directories(io, storage_dir_path, revision, writes, &[])
}

pub(in crate::adapter::gateway) fn prepare_with_directories(
    io: Arc<dyn StorageTransactionIo>,
    storage_dir_path: &Path,
    revision: Uuid,
    writes: &[WriteRequest<'_>],
    directories: &[&Path],
) -> Result<PreparedTransaction, StorageTransactionError> {
    let transactions_dir_path =
        resolve_transactions_directory(io.as_ref(), storage_dir_path, true)?
            .expect("transaction directory must exist after successful creation");
    let transaction_dir_path = transactions_dir_path.join(ACTIVE_TRANSACTION_DIRECTORY_NAME);
    let transaction_lock = acquire_transaction_lock(&transactions_dir_path).map_err(|error| {
        if error.source.kind() == std::io::ErrorKind::WouldBlock {
            StorageTransactionError::new(
                StorageTransactionOperation::ActiveTransaction,
                &transaction_dir_path,
                error.source,
            )
        } else {
            error
        }
    })?;
    validate_transactions_directory(&transactions_dir_path)?;
    cleanup_stale_tombstones(io.as_ref(), &transactions_dir_path);
    let transaction_id = Uuid::new_v4();
    if let Err(error) = io.create_dir(&transaction_dir_path) {
        let operation = if error.kind() == std::io::ErrorKind::AlreadyExists {
            StorageTransactionOperation::ActiveTransaction
        } else {
            StorageTransactionOperation::AcquireActiveTransaction
        };
        return Err(StorageTransactionError::new(
            operation,
            &transaction_dir_path,
            error,
        ));
    }
    let staged_files_dir_path = transaction_dir_path.join("files");
    if let Err(error) = io.create_dir_all(&staged_files_dir_path) {
        let _ = io.remove_dir_all(&transaction_dir_path);
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::CreateStagedFilesDirectory,
            &staged_files_dir_path,
            error,
        ));
    }

    let directories = prepare_contents(
        io.as_ref(),
        storage_dir_path,
        &transactions_dir_path,
        &transaction_dir_path,
        &staged_files_dir_path,
        transaction_id,
        revision,
        writes,
        directories,
    );
    let directories = match directories {
        Ok(directories) => directories,
        Err(error) => {
            let _ = io.remove_dir_all(&transaction_dir_path);
            return Err(error);
        }
    };
    let entries = writes
        .iter()
        .enumerate()
        .map(|(index, write)| {
            Ok(PreparedEntry {
                target: validate_storage_relative_path(storage_dir_path, write.target_path)?,
                operation: ManifestEntryOperation::Write,
                staged_file: Some(PathBuf::from("files").join(index.to_string())),
                content_length: Some(write.bytes.len() as u64),
                content_checksum: Some(content_checksum(write.bytes)),
            })
        })
        .collect::<Result<Vec<_>, StorageTransactionError>>()?;
    Ok(PreparedTransaction {
        storage_dir_path: storage_dir_path.to_path_buf(),
        transactions_dir_path,
        transaction_dir_path,
        transaction_id,
        revision,
        directories,
        entries,
        io,
        _transaction_lock: transaction_lock,
    })
}

#[allow(clippy::too_many_arguments)]
fn prepare_contents(
    io: &dyn StorageTransactionIo,
    storage_dir_path: &Path,
    transactions_dir_path: &Path,
    transaction_dir_path: &Path,
    staged_files_dir_path: &Path,
    transaction_id: Uuid,
    revision: Uuid,
    writes: &[WriteRequest<'_>],
    directories: &[&Path],
) -> Result<Vec<PathBuf>, StorageTransactionError> {
    let mut entries = Vec::with_capacity(writes.len());
    for (index, write) in writes.iter().enumerate() {
        let target = validate_storage_relative_path(storage_dir_path, write.target_path)?;
        let staged_file = PathBuf::from("files").join(index.to_string());
        let staged_file_path = transaction_dir_path.join(&staged_file);
        write_staged_file(io, write.target_path, &staged_file_path, write.bytes)?;
        entries.push(ManifestEntry {
            target,
            operation: ManifestEntryOperation::Write,
            staged_file: Some(staged_file),
            content_length: Some(write.bytes.len() as u64),
            content_checksum: Some(content_checksum(write.bytes)),
        });
    }
    sync_directory(io, staged_files_dir_path)?;

    let directories = directories
        .iter()
        .map(|directory| validate_storage_relative_path(storage_dir_path, directory))
        .collect::<Result<Vec<_>, _>>()?;
    let manifest = TransactionManifest {
        version: 1,
        transaction_id,
        revision,
        directories: directories.clone(),
        entries,
    };
    let manifest_path = transaction_dir_path.join("manifest.json");
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::SerializeManifest,
            &manifest_path,
            std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        )
    })?;
    io.create_new_file(&manifest_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::CreateManifest,
            &manifest_path,
            error,
        )
    })?;
    io.write_file(&manifest_path, &manifest_bytes)
        .map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::WriteManifest,
                &manifest_path,
                error,
            )
        })?;
    io.sync_file(&manifest_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::SyncManifest,
            &manifest_path,
            error,
        )
    })?;
    sync_directory(io, transaction_dir_path)?;
    sync_directory(io, transactions_dir_path)?;
    sync_directory(io, storage_dir_path)?;
    Ok(directories)
}

fn write_staged_file(
    io: &dyn StorageTransactionIo,
    target_path: &Path,
    staged_file_path: &Path,
    bytes: &[u8],
) -> Result<(), StorageTransactionError> {
    let existing_permissions = io.target_permissions(target_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::ReadTargetMetadata,
            target_path,
            error,
        )
    })?;
    io.create_new_file(staged_file_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::CreateStagedFile,
            staged_file_path,
            error,
        )
    })?;
    if let Some(permissions) = existing_permissions {
        io.set_permissions(staged_file_path, permissions)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::SetStagedPermissions,
                    staged_file_path,
                    error,
                )
            })?;
    }
    io.write_file(staged_file_path, bytes).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::WriteStagedFile,
            staged_file_path,
            error,
        )
    })?;
    io.sync_file(staged_file_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::SyncStagedFile,
            staged_file_path,
            error,
        )
    })
}
