use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

mod commit;
mod io;
mod layout;
mod manifest;
mod prepare;

#[cfg(test)]
use io::TRANSACTION_LOCK_FILE_NAME;
use io::{acquire_transaction_lock, sync_directory, TransactionLock};
pub(super) use io::{FileSystemStorageTransactionIo, StorageTransactionIo};
#[cfg(test)]
pub(super) use layout::TRANSACTION_DIRECTORY_NAME;
use layout::{validate_storage_relative_path, ACTIVE_TRANSACTION_DIRECTORY_NAME};
use manifest::{
    content_checksum, invalid_manifest_entry_error, validate_content_integrity,
    validate_staged_file_path, ManifestEntry, ManifestEntryOperation, TransactionManifest,
};
#[cfg(test)]
pub(super) use prepare::prepare;
pub(super) use prepare::prepare_with_directories;

#[derive(Debug)]
pub(super) struct StorageTransactionError {
    operation: StorageTransactionOperation,
    path: PathBuf,
    source: std::io::Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StorageTransactionOperation {
    #[cfg(test)]
    Discard,
    CreateTransactionDirectory,
    AcquireActiveTransaction,
    ActiveTransaction,
    AcquireTransactionLock,
    ValidateTransactionDirectory,
    InspectActiveTransaction,
    ValidateCommitMarker,
    DiscardUncommitted,
    ReadManifest,
    ParseManifest,
    ValidateManifest,
    CreateStagedFilesDirectory,
    ResolveTargetPath,
    ValidateTargetPath,
    ReadTargetMetadata,
    ReadTargetContent,
    ValidateStagedFile,
    ValidateStagedContent,
    CreateStagedFile,
    SetStagedPermissions,
    WriteStagedFile,
    SyncStagedFile,
    SerializeManifest,
    CreateManifest,
    WriteManifest,
    SyncManifest,
    CreateCommitMarker,
    SyncCommitMarker,
    RenameCommitMarker,
    CreateTargetDirectory,
    ReadStagedFile,
    CreateLiveTemporary,
    RemoveLiveTemporary,
    SetLivePermissions,
    WriteLiveTemporary,
    SyncLiveTemporary,
    RenameLiveTarget,
    RemoveLiveTarget,
    RenameForCleanup,
    SyncDirectory,
}

impl StorageTransactionError {
    fn new(
        operation: StorageTransactionOperation,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for StorageTransactionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "storage transaction {:?} failed for {}: {}",
            self.operation,
            self.path.display(),
            self.source
        )
    }
}

impl Error for StorageTransactionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

pub(super) struct WriteRequest<'a> {
    pub(super) target_path: &'a Path,
    pub(super) bytes: &'a [u8],
}

pub(super) struct PreparedTransaction {
    storage_dir_path: PathBuf,
    transactions_dir_path: PathBuf,
    transaction_dir_path: PathBuf,
    transaction_id: Uuid,
    revision: Uuid,
    directories: Vec<PathBuf>,
    entries: Vec<PreparedEntry>,
    io: Arc<dyn StorageTransactionIo>,
    _transaction_lock: TransactionLock,
}

struct PreparedEntry {
    target: PathBuf,
    operation: ManifestEntryOperation,
    staged_file: Option<PathBuf>,
    content_length: Option<u64>,
    content_checksum: Option<String>,
}

pub(super) fn recover(
    io: Arc<dyn StorageTransactionIo>,
    storage_dir_path: &Path,
) -> Result<(), StorageTransactionError> {
    let Some(transactions_dir_path) =
        resolve_transactions_directory(io.as_ref(), storage_dir_path, false)?
    else {
        return Ok(());
    };
    let _transaction_lock = acquire_transaction_lock(&transactions_dir_path)?;
    validate_transactions_directory(&transactions_dir_path)?;
    cleanup_stale_tombstones(io.as_ref(), &transactions_dir_path);
    let transaction_dir_path = transactions_dir_path.join(ACTIVE_TRANSACTION_DIRECTORY_NAME);
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

    let marker_path = transaction_dir_path.join("commit");
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
            let manifest_path = transaction_dir_path.join("manifest.json");
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
            let manifest: TransactionManifest =
                serde_json::from_slice(&manifest_bytes).map_err(|error| {
                    StorageTransactionError::new(
                        StorageTransactionOperation::ParseManifest,
                        &manifest_path,
                        std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                    )
                })?;
            let prepared = prepared_from_manifest(
                io,
                storage_dir_path,
                transactions_dir_path,
                transaction_dir_path,
                manifest_path,
                manifest,
                _transaction_lock,
            )?;
            return prepared.finish_committed(&storage_dir_path.join(".revision"));
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

#[allow(clippy::too_many_arguments)]
fn prepared_from_manifest(
    io: Arc<dyn StorageTransactionIo>,
    storage_dir_path: &Path,
    transactions_dir_path: PathBuf,
    transaction_dir_path: PathBuf,
    manifest_path: PathBuf,
    manifest: TransactionManifest,
    transaction_lock: TransactionLock,
) -> Result<PreparedTransaction, StorageTransactionError> {
    if manifest.version != 1 || manifest.transaction_id.is_nil() {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::ValidateManifest,
            manifest_path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "transaction manifest must use version 1 and a non-nil transaction id",
            ),
        ));
    }
    let directories = manifest
        .directories
        .into_iter()
        .map(|directory| {
            validate_storage_relative_path(storage_dir_path, &storage_dir_path.join(directory))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let entries = manifest
        .entries
        .into_iter()
        .map(|entry| {
            let target = validate_storage_relative_path(
                storage_dir_path,
                &storage_dir_path.join(entry.target),
            )?;
            if entry.operation == ManifestEntryOperation::Delete {
                validate_delete_target_ancestors(io.as_ref(), storage_dir_path, &target)?;
            }
            match (
                entry.operation,
                entry.staged_file.as_deref(),
                entry.content_length,
                entry.content_checksum.as_deref(),
            ) {
                (
                    ManifestEntryOperation::Write,
                    Some(staged_file),
                    Some(content_length),
                    Some(content_checksum),
                ) => {
                    validate_staged_file_path(&transaction_dir_path, staged_file)?;
                    validate_content_integrity(&manifest_path, content_length, content_checksum)?;
                }
                (ManifestEntryOperation::Delete, None, None, None) => {}
                (ManifestEntryOperation::Write, None, _, _) => {
                    return Err(invalid_manifest_entry_error(
                        &manifest_path,
                        "write entry must contain a staged file",
                    ));
                }
                (ManifestEntryOperation::Write, Some(_), _, _) => {
                    return Err(invalid_manifest_entry_error(
                        &manifest_path,
                        "write entry must contain content length and checksum",
                    ));
                }
                (ManifestEntryOperation::Delete, Some(_), _, _) => {
                    return Err(invalid_manifest_entry_error(
                        &manifest_path,
                        "delete entry must not contain a staged file",
                    ));
                }
                (ManifestEntryOperation::Delete, None, _, _) => {
                    return Err(invalid_manifest_entry_error(
                        &manifest_path,
                        "delete entry must not contain content integrity information",
                    ));
                }
            }
            Ok(PreparedEntry {
                target,
                operation: entry.operation,
                staged_file: entry.staged_file,
                content_length: entry.content_length,
                content_checksum: entry.content_checksum,
            })
        })
        .collect::<Result<Vec<_>, StorageTransactionError>>()?;
    Ok(PreparedTransaction {
        storage_dir_path: storage_dir_path.to_path_buf(),
        transactions_dir_path,
        transaction_dir_path,
        transaction_id: manifest.transaction_id,
        revision: manifest.revision,
        directories,
        entries,
        io,
        _transaction_lock: transaction_lock,
    })
}

fn validate_delete_target_ancestors(
    io: &dyn StorageTransactionIo,
    storage_dir_path: &Path,
    target: &Path,
) -> Result<(), StorageTransactionError> {
    let mut ancestor_path = storage_dir_path.to_path_buf();
    let Some(parent) = target.parent() else {
        return Ok(());
    };
    for component in parent.components() {
        let Component::Normal(name) = component else {
            unreachable!("validated transaction target must contain only normal components");
        };
        ancestor_path.push(name);
        match io.symlink_metadata(&ancestor_path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(layout::invalid_target_path_error(
                    &ancestor_path,
                    "delete target ancestors must be directories and must not be symbolic links",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(StorageTransactionError::new(
                    StorageTransactionOperation::ValidateTargetPath,
                    &ancestor_path,
                    error,
                ));
            }
        }
    }
    Ok(())
}

fn resolve_transactions_directory(
    io: &dyn StorageTransactionIo,
    storage_dir_path: &Path,
    create: bool,
) -> Result<Option<PathBuf>, StorageTransactionError> {
    let transactions_dir_path = storage_dir_path.join(layout::TRANSACTION_DIRECTORY_NAME);
    let (metadata, created) = match fs::symlink_metadata(&transactions_dir_path) {
        Ok(metadata) => (metadata, false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            io.create_dir_all(&transactions_dir_path).map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::CreateTransactionDirectory,
                    &transactions_dir_path,
                    error,
                )
            })?;
            (
                fs::symlink_metadata(&transactions_dir_path).map_err(|error| {
                    StorageTransactionError::new(
                        StorageTransactionOperation::ValidateTransactionDirectory,
                        &transactions_dir_path,
                        error,
                    )
                })?,
                true,
            )
        }
        Err(error) => {
            return Err(StorageTransactionError::new(
                StorageTransactionOperation::ValidateTransactionDirectory,
                &transactions_dir_path,
                error,
            ));
        }
    };
    validate_transactions_directory_metadata(&transactions_dir_path, &metadata)?;
    if create && !created {
        io.create_dir_all(&transactions_dir_path).map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::CreateTransactionDirectory,
                &transactions_dir_path,
                error,
            )
        })?;
        validate_transactions_directory(&transactions_dir_path)?;
    }
    Ok(Some(transactions_dir_path))
}

fn validate_transactions_directory(path: &Path) -> Result<(), StorageTransactionError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::ValidateTransactionDirectory,
            path,
            error,
        )
    })?;
    validate_transactions_directory_metadata(path, &metadata)
}

fn validate_transactions_directory_metadata(
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), StorageTransactionError> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::ValidateTransactionDirectory,
            path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "transaction root must be a directory and must not be a symbolic link",
            ),
        ));
    }
    Ok(())
}

fn cleanup_stale_tombstones(io: &dyn StorageTransactionIo, transactions_dir_path: &Path) {
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

#[cfg(test)]
#[path = "storage_transaction_tests.rs"]
mod tests;
