use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use uuid::Uuid;

use super::cleanup::cleanup_stale_tombstones;
use super::io::{
    acquire_transaction_lock, resolve_transactions_directory, sync_directory,
    validate_delete_target, validate_transactions_directory,
};
use super::layout::{invalid_target_path_error, validate_storage_relative_path, TransactionLayout};
use super::manifest::{
    content_checksum, ContentIntegrity, RawTransactionManifest, ValidatedEntry, ValidatedManifest,
};
use super::{
    PreparedTransaction, ReplacementRequest, StorageTransactionError, StorageTransactionIo,
    StorageTransactionOperation, TransactionPaths, TransactionState, WriteRequest,
};

struct PrepareContext<'a> {
    io: &'a dyn StorageTransactionIo,
    paths: &'a TransactionPaths,
    staged_files_dir_path: &'a Path,
    transaction_id: Uuid,
    revision: Uuid,
}

struct PrepareRequest<'a> {
    writes: &'a [WriteRequest<'a>],
    file_permissions: Option<&'a [std::fs::Permissions]>,
    directories: &'a [&'a Path],
    directory_permissions: Option<&'a [std::fs::Permissions]>,
    deletes: &'a [&'a Path],
    preserve_existing_permissions: bool,
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
    prepare_with_directories_and_deletes(io, storage_dir_path, revision, writes, directories, &[])
}

pub(in crate::adapter::gateway) fn prepare_with_directories_and_deletes(
    io: Arc<dyn StorageTransactionIo>,
    storage_dir_path: &Path,
    revision: Uuid,
    writes: &[WriteRequest<'_>],
    directories: &[&Path],
    deletes: &[&Path],
) -> Result<PreparedTransaction, StorageTransactionError> {
    prepare_impl(
        io,
        storage_dir_path,
        revision,
        PrepareRequest {
            writes,
            file_permissions: None,
            directories,
            directory_permissions: None,
            deletes,
            preserve_existing_permissions: true,
        },
    )
}

pub(in crate::adapter::gateway) fn prepare_replacing_with_directories_and_deletes(
    io: Arc<dyn StorageTransactionIo>,
    storage_dir_path: &Path,
    revision: Uuid,
    request: ReplacementRequest<'_>,
) -> Result<PreparedTransaction, StorageTransactionError> {
    prepare_impl(
        io,
        storage_dir_path,
        revision,
        PrepareRequest {
            writes: request.writes,
            file_permissions: Some(request.file_permissions),
            directories: request.directories,
            directory_permissions: Some(request.directory_permissions),
            deletes: request.deletes,
            preserve_existing_permissions: false,
        },
    )
}

fn prepare_impl(
    io: Arc<dyn StorageTransactionIo>,
    storage_dir_path: &Path,
    revision: Uuid,
    request: PrepareRequest<'_>,
) -> Result<PreparedTransaction, StorageTransactionError> {
    validate_permission_counts(storage_dir_path, &request)?;
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
    validate_transactions_directory(io.as_ref(), &transactions_dir_path)?;
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
    let manifest = prepare_contents(
        &context,
        request.writes,
        request.file_permissions,
        request.directories,
        request.directory_permissions,
        request.deletes,
        request.preserve_existing_permissions,
    );
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

fn validate_permission_counts(
    storage_dir_path: &Path,
    request: &PrepareRequest<'_>,
) -> Result<(), StorageTransactionError> {
    let file_permissions_match = request
        .file_permissions
        .is_none_or(|permissions| permissions.len() == request.writes.len());
    let directory_permissions_match = request
        .directory_permissions
        .is_none_or(|permissions| permissions.len() == request.directories.len());
    if file_permissions_match && directory_permissions_match {
        return Ok(());
    }
    Err(StorageTransactionError::new(
        StorageTransactionOperation::ValidateTargetPath,
        storage_dir_path,
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "replacement permissions must match transaction target counts",
        ),
    ))
}

fn prepare_contents(
    context: &PrepareContext<'_>,
    writes: &[WriteRequest<'_>],
    file_permissions: Option<&[std::fs::Permissions]>,
    directories: &[&Path],
    directory_permissions: Option<&[std::fs::Permissions]>,
    deletes: &[&Path],
    preserve_existing_permissions: bool,
) -> Result<ValidatedManifest, StorageTransactionError> {
    let mut entries = Vec::with_capacity(writes.len() + deletes.len());
    let mut targets = HashSet::with_capacity(writes.len() + deletes.len());
    for (index, write) in writes.iter().enumerate() {
        let target =
            validate_storage_relative_path(&context.paths.storage_dir_path, write.target_path)?;
        ensure_unique_target(&mut targets, target.clone(), write.target_path)?;
        let staged_file = TransactionLayout::staged_file_relative_path(index);
        let staged_file_path =
            TransactionLayout::staged_file_path(&context.paths.transaction_dir_path, &staged_file);
        write_staged_file(
            context.io,
            write.target_path,
            &staged_file_path,
            write.bytes,
            preserve_existing_permissions,
            file_permissions.map(|permissions| permissions[index].clone()),
        )?;
        entries.push(ValidatedEntry::Write {
            target,
            staged_file,
            mode: file_permissions.and_then(|permissions| permission_mode(&permissions[index])),
            integrity: ContentIntegrity {
                content_length: write.bytes.len() as u64,
                checksum: content_checksum(write.bytes),
            },
        });
    }
    for delete in deletes {
        let target = validate_delete_target(context.io, &context.paths.storage_dir_path, delete)?;
        ensure_unique_target(&mut targets, target.clone(), delete)?;
        entries.push(ValidatedEntry::Delete { target });
    }
    sync_directory(context.io, context.staged_files_dir_path)?;

    let directories = directories
        .iter()
        .map(|directory| validate_storage_relative_path(&context.paths.storage_dir_path, directory))
        .collect::<Result<Vec<_>, _>>()?;
    let directory_modes = directory_permissions
        .map(|permissions| permissions.iter().map(permission_mode).collect())
        .unwrap_or_else(|| vec![None; directories.len()]);
    let manifest = ValidatedManifest {
        transaction_id: context.transaction_id,
        revision: context.revision,
        replace_target_directories: !preserve_existing_permissions,
        directories,
        directory_modes,
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

fn ensure_unique_target(
    targets: &mut HashSet<std::path::PathBuf>,
    target: std::path::PathBuf,
    operation_path: &Path,
) -> Result<(), StorageTransactionError> {
    if targets.insert(target) {
        Ok(())
    } else {
        Err(invalid_target_path_error(
            operation_path,
            "transaction targets must be unique",
        ))
    }
}

fn write_staged_file(
    io: &dyn StorageTransactionIo,
    target_path: &Path,
    staged_file_path: &Path,
    bytes: &[u8],
    preserve_existing_permissions: bool,
    permission_override: Option<std::fs::Permissions>,
) -> Result<(), StorageTransactionError> {
    let permissions = if let Some(permissions) = permission_override {
        Some(permissions)
    } else if preserve_existing_permissions {
        io.target_permissions(target_path).map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::ReadTargetMetadata,
                target_path,
                error,
            )
        })?
    } else {
        None
    };
    io.create_new_file(staged_file_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::CreateStagedFile,
            staged_file_path,
            error,
        )
    })?;
    if let Some(permissions) = permissions {
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

#[cfg(unix)]
fn permission_mode(permissions: &std::fs::Permissions) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;

    Some(permissions.mode() & 0o7777)
}

#[cfg(not(unix))]
fn permission_mode(_permissions: &std::fs::Permissions) -> Option<u32> {
    None
}
