use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

mod cleanup;
mod commit;
mod io;
mod layout;
mod manifest;
mod prepare;
mod recovery;

#[cfg(test)]
use io::TRANSACTION_LOCK_FILE_NAME;
use io::{acquire_transaction_lock, sync_directory, TransactionLock};
pub(super) use io::{FileSystemStorageTransactionIo, StorageTransactionIo};
#[cfg(test)]
use layout::ACTIVE_TRANSACTION_DIRECTORY_NAME;
#[cfg(test)]
pub(super) use layout::TRANSACTION_DIRECTORY_NAME;
use layout::{validate_storage_relative_path, TransactionLayout};
use manifest::{
    content_checksum, invalid_manifest_entry_error, validate_content_integrity,
    validate_staged_file_path, ManifestEntry, ManifestEntryOperation, TransactionManifest,
};
#[cfg(test)]
pub(super) use prepare::prepare;
pub(super) use prepare::prepare_with_directories;
#[cfg(test)]
use recovery::prepared_from_manifest;
pub(super) use recovery::recover;

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
    let transactions_dir_path = TransactionLayout::new(storage_dir_path).transactions_dir_path();
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

#[cfg(test)]
#[path = "storage_transaction_tests.rs"]
mod tests;
