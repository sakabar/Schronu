use std::path::Path;
use std::sync::Arc;

use uuid::Uuid;

use super::cleanup::cleanup_stale_tombstones;
use super::layout::TransactionLayout;
use super::manifest::{
    content_checksum, ContentIntegrity, RawTransactionManifest, ValidatedEntry, ValidatedManifest,
};
use super::{
    acquire_transaction_lock, resolve_transactions_directory, sync_directory,
    validate_storage_relative_path, validate_transactions_directory, PreparedTransaction,
    StorageTransactionError, StorageTransactionIo, StorageTransactionOperation, TransactionPaths,
    TransactionState, WriteRequest,
};

struct PrepareContext<'a> {
    io: &'a dyn StorageTransactionIo,
    paths: &'a TransactionPaths,
    staged_files_dir_path: &'a Path,
    transaction_id: Uuid,
    revision: Uuid,
}

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
    let layout = TransactionLayout::new(storage_dir_path);
    let transactions_dir_path =
        resolve_transactions_directory(io.as_ref(), storage_dir_path, true)?
            .expect("transaction directory must exist after successful creation");
    let transaction_dir_path = layout.active_transaction_dir_path();
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
    let staged_files_dir_path = layout.staged_files_dir_path();
    if let Err(error) = io.create_dir_all(&staged_files_dir_path) {
        let _ = io.remove_dir_all(&transaction_dir_path);
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::CreateStagedFilesDirectory,
            &staged_files_dir_path,
            error,
        ));
    }

    let paths = TransactionPaths {
        storage_dir_path: storage_dir_path.to_path_buf(),
        transactions_dir_path,
        transaction_dir_path,
    };
    let context = PrepareContext {
        io: io.as_ref(),
        paths: &paths,
        staged_files_dir_path: &staged_files_dir_path,
        transaction_id,
        revision,
    };
    let manifest = prepare_contents(&context, writes, directories);
    let manifest = match manifest {
        Ok(manifest) => manifest,
        Err(error) => {
            let _ = io.remove_dir_all(&paths.transaction_dir_path);
            return Err(error);
        }
    };
    Ok(PreparedTransaction {
        state: TransactionState {
            paths,
            manifest,
            io,
            _transaction_lock: transaction_lock,
        },
    })
}

fn prepare_contents(
    context: &PrepareContext<'_>,
    writes: &[WriteRequest<'_>],
    directories: &[&Path],
) -> Result<ValidatedManifest, StorageTransactionError> {
    let mut entries = Vec::with_capacity(writes.len());
    for (index, write) in writes.iter().enumerate() {
        let target =
            validate_storage_relative_path(&context.paths.storage_dir_path, write.target_path)?;
        let staged_file = TransactionLayout::staged_file_relative_path(index);
        let staged_file_path =
            TransactionLayout::staged_file_path(&context.paths.transaction_dir_path, &staged_file);
        write_staged_file(
            context.io,
            write.target_path,
            &staged_file_path,
            write.bytes,
        )?;
        entries.push(ValidatedEntry::Write {
            target,
            staged_file,
            integrity: ContentIntegrity {
                content_length: write.bytes.len() as u64,
                checksum: content_checksum(write.bytes),
            },
        });
    }
    sync_directory(context.io, context.staged_files_dir_path)?;

    let directories = directories
        .iter()
        .map(|directory| validate_storage_relative_path(&context.paths.storage_dir_path, directory))
        .collect::<Result<Vec<_>, _>>()?;
    let manifest = ValidatedManifest {
        transaction_id: context.transaction_id,
        revision: context.revision,
        directories,
        entries,
    };
    let raw_manifest = RawTransactionManifest::from(&manifest);
    let manifest_path = TransactionLayout::manifest_path(&context.paths.transaction_dir_path);
    let manifest_bytes = serde_json::to_vec(&raw_manifest).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::SerializeManifest,
            &manifest_path,
            std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        )
    })?;
    context
        .io
        .create_new_file(&manifest_path)
        .map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::CreateManifest,
                &manifest_path,
                error,
            )
        })?;
    context
        .io
        .write_file(&manifest_path, &manifest_bytes)
        .map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::WriteManifest,
                &manifest_path,
                error,
            )
        })?;
    context.io.sync_file(&manifest_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::SyncManifest,
            &manifest_path,
            error,
        )
    })?;
    sync_directory(context.io, &context.paths.transaction_dir_path)?;
    sync_directory(context.io, &context.paths.transactions_dir_path)?;
    sync_directory(context.io, &context.paths.storage_dir_path)?;
    Ok(manifest)
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
