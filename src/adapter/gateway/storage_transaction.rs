use serde::Serialize;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

pub(super) const TRANSACTION_DIRECTORY_NAME: &str = ".schronu-transactions";

#[derive(Debug)]
pub(super) struct StorageTransactionError {
    operation: StorageTransactionOperation,
    path: PathBuf,
    source: std::io::Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StorageTransactionOperation {
    Discard,
    CreateTransactionDirectory,
    CreateStagedFilesDirectory,
    ResolveTargetPath,
    ReadTargetMetadata,
    CreateStagedFile,
    SetStagedPermissions,
    WriteStagedFile,
    SyncStagedFile,
    SerializeManifest,
    CreateManifest,
    WriteManifest,
    SyncManifest,
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

#[derive(Serialize)]
struct TransactionManifest {
    version: u32,
    transaction_id: Uuid,
    revision: Uuid,
    entries: Vec<ManifestEntry>,
}

#[derive(Serialize)]
struct ManifestEntry {
    target: PathBuf,
    staged_file: PathBuf,
}

pub(super) trait StorageTransactionIo: Send + Sync {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::create_dir_all(path)
    }

    fn target_permissions(&self, path: &Path) -> std::io::Result<Option<fs::Permissions>> {
        match fs::metadata(path) {
            Ok(metadata) => Ok(Some(metadata.permissions())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn create_new_file(&self, path: &Path) -> std::io::Result<()> {
        File::options()
            .write(true)
            .create_new(true)
            .open(path)
            .map(drop)
    }

    fn set_permissions(&self, path: &Path, permissions: fs::Permissions) -> std::io::Result<()> {
        fs::set_permissions(path, permissions)
    }

    fn write_file(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        File::options().write(true).open(path)?.write_all(bytes)
    }

    fn sync_file(&self, path: &Path) -> std::io::Result<()> {
        File::open(path)?.sync_all()
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        File::open(path)?.sync_all()
    }
    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_dir_all(path)
    }
}

#[derive(Default)]
pub(super) struct FileSystemStorageTransactionIo;
impl StorageTransactionIo for FileSystemStorageTransactionIo {}

pub(super) struct PreparedTransaction {
    transaction_dir_path: PathBuf,
    io: Arc<dyn StorageTransactionIo>,
}

impl PreparedTransaction {
    pub(super) fn discard(self) -> Result<(), StorageTransactionError> {
        self.io
            .remove_dir_all(&self.transaction_dir_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::Discard,
                    self.transaction_dir_path,
                    error,
                )
            })
    }
}

pub(super) fn prepare(
    io: Arc<dyn StorageTransactionIo>,
    storage_dir_path: &Path,
    revision: Uuid,
    writes: &[WriteRequest<'_>],
) -> Result<PreparedTransaction, StorageTransactionError> {
    let transactions_dir_path = storage_dir_path.join(TRANSACTION_DIRECTORY_NAME);
    io.create_dir_all(&transactions_dir_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::CreateTransactionDirectory,
            &transactions_dir_path,
            error,
        )
    })?;
    let transaction_id = Uuid::new_v4();
    let transaction_dir_path = transactions_dir_path.join(transaction_id.hyphenated().to_string());
    let staged_files_dir_path = transaction_dir_path.join("files");
    if let Err(error) = io.create_dir_all(&staged_files_dir_path) {
        let _ = io.remove_dir_all(&transaction_dir_path);
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::CreateStagedFilesDirectory,
            &staged_files_dir_path,
            error,
        ));
    }

    let result = prepare_contents(
        io.as_ref(),
        storage_dir_path,
        &transactions_dir_path,
        &transaction_dir_path,
        &staged_files_dir_path,
        transaction_id,
        revision,
        writes,
    );
    if let Err(error) = result {
        let _ = io.remove_dir_all(&transaction_dir_path);
        return Err(error);
    }
    Ok(PreparedTransaction {
        transaction_dir_path,
        io,
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
) -> Result<(), StorageTransactionError> {
    let mut entries = Vec::with_capacity(writes.len());
    for (index, write) in writes.iter().enumerate() {
        let target = write
            .target_path
            .strip_prefix(storage_dir_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::ResolveTargetPath,
                    write.target_path,
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, error),
                )
            })?;
        let staged_file = PathBuf::from("files").join(index.to_string());
        let staged_file_path = transaction_dir_path.join(&staged_file);
        write_staged_file(io, write.target_path, &staged_file_path, write.bytes)?;
        entries.push(ManifestEntry {
            target: target.to_path_buf(),
            staged_file,
        });
    }
    sync_directory(io, staged_files_dir_path)?;

    let manifest = TransactionManifest {
        version: 1,
        transaction_id,
        revision,
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
    sync_directory(io, storage_dir_path)
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

fn sync_directory(
    io: &dyn StorageTransactionIo,
    path: &Path,
) -> Result<(), StorageTransactionError> {
    io.sync_directory(path).map_err(|error| {
        StorageTransactionError::new(StorageTransactionOperation::SyncDirectory, path, error)
    })
}

#[cfg(test)]
#[path = "storage_transaction_tests.rs"]
mod tests;
