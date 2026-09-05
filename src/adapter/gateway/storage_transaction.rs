use std::error::Error;
use std::fmt;
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use uuid::Uuid;

mod cleanup;
mod commit;
mod io;
mod layout;
mod manifest;
mod prepare;
mod recovery;

use io::TransactionLock;
pub(super) use io::{FileSystemStorageTransactionIo, StorageTransactionIo};
use layout::validate_storage_relative_path;
#[cfg(test)]
use layout::ACTIVE_TRANSACTION_DIRECTORY_NAME;
#[cfg(test)]
pub(super) use layout::TRANSACTION_DIRECTORY_NAME;
use manifest::ValidatedManifest;
#[cfg(test)]
use manifest::{
    ManifestEntryOperation, RawManifestEntry as ManifestEntry,
    RawTransactionManifest as TransactionManifest,
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
    state: TransactionState,
}

struct CommittedTransaction {
    state: TransactionState,
}

struct TransactionState {
    paths: TransactionPaths,
    manifest: ValidatedManifest,
    io: Arc<dyn StorageTransactionIo>,
    _transaction_lock: TransactionLock,
}

struct TransactionPaths {
    storage_dir_path: PathBuf,
    transactions_dir_path: PathBuf,
    transaction_dir_path: PathBuf,
}

#[cfg(test)]
#[path = "storage_transaction_tests.rs"]
mod tests;
