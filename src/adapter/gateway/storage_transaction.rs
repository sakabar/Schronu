use serde::Serialize;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(super) const TRANSACTION_DIRECTORY_NAME: &str = ".schronu-transactions";

#[derive(Debug)]
pub(super) struct StorageTransactionError {
    operation: &'static str,
    path: PathBuf,
    source: std::io::Error,
}

impl StorageTransactionError {
    fn new(operation: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
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
            "storage transaction {} failed for {}: {}",
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

pub(super) struct PreparedTransaction {
    transaction_dir_path: PathBuf,
}

pub(super) trait StorageTransactionIo: Send + Sync {
    fn prepare(
        &self,
        storage_dir_path: &Path,
        revision: Uuid,
        writes: &[WriteRequest<'_>],
    ) -> Result<PreparedTransaction, StorageTransactionError>;
}

#[derive(Default)]
pub(super) struct FileSystemStorageTransactionIo;

impl StorageTransactionIo for FileSystemStorageTransactionIo {
    fn prepare(
        &self,
        storage_dir_path: &Path,
        revision: Uuid,
        writes: &[WriteRequest<'_>],
    ) -> Result<PreparedTransaction, StorageTransactionError> {
        prepare(storage_dir_path, revision, writes)
    }
}

impl PreparedTransaction {
    pub(super) fn discard(self) -> Result<(), StorageTransactionError> {
        fs::remove_dir_all(&self.transaction_dir_path).map_err(|error| {
            StorageTransactionError::new("discard", self.transaction_dir_path, error)
        })?;
        Ok(())
    }
}

pub(super) fn prepare(
    storage_dir_path: &Path,
    revision: Uuid,
    writes: &[WriteRequest<'_>],
) -> Result<PreparedTransaction, StorageTransactionError> {
    let transactions_dir_path = storage_dir_path.join(TRANSACTION_DIRECTORY_NAME);
    fs::create_dir_all(&transactions_dir_path).map_err(|error| {
        StorageTransactionError::new(
            "create transaction directory",
            &transactions_dir_path,
            error,
        )
    })?;

    let transaction_id = Uuid::new_v4();
    let transaction_dir_path = transactions_dir_path.join(transaction_id.hyphenated().to_string());
    let staged_files_dir_path = transaction_dir_path.join("files");
    fs::create_dir_all(&staged_files_dir_path).map_err(|error| {
        StorageTransactionError::new(
            "create staged files directory",
            &staged_files_dir_path,
            error,
        )
    })?;

    let result = prepare_contents(
        storage_dir_path,
        &transactions_dir_path,
        &transaction_dir_path,
        &staged_files_dir_path,
        transaction_id,
        revision,
        writes,
    );
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&transaction_dir_path);
        return Err(error);
    }

    Ok(PreparedTransaction {
        transaction_dir_path,
    })
}

#[allow(clippy::too_many_arguments)]
fn prepare_contents(
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
                    "resolve target path",
                    write.target_path,
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, error),
                )
            })?;
        let staged_file = PathBuf::from("files").join(index.to_string());
        let staged_file_path = transaction_dir_path.join(&staged_file);
        write_staged_file(write.target_path, &staged_file_path, write.bytes)?;
        entries.push(ManifestEntry {
            target: target.to_path_buf(),
            staged_file,
        });
    }

    sync_directory(staged_files_dir_path)?;

    let manifest = TransactionManifest {
        version: 1,
        transaction_id,
        revision,
        entries,
    };
    let manifest_path = transaction_dir_path.join("manifest.json");
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(|error| {
        StorageTransactionError::new(
            "serialize manifest",
            &manifest_path,
            std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        )
    })?;
    let mut manifest_file = File::options()
        .write(true)
        .create_new(true)
        .open(&manifest_path)
        .map_err(|error| StorageTransactionError::new("create manifest", &manifest_path, error))?;
    manifest_file
        .write_all(&manifest_bytes)
        .map_err(|error| StorageTransactionError::new("write manifest", &manifest_path, error))?;
    manifest_file
        .sync_all()
        .map_err(|error| StorageTransactionError::new("sync manifest", &manifest_path, error))?;

    sync_directory(transaction_dir_path)?;
    sync_directory(transactions_dir_path)?;
    sync_directory(storage_dir_path)
}

fn write_staged_file(
    target_path: &Path,
    staged_file_path: &Path,
    bytes: &[u8],
) -> Result<(), StorageTransactionError> {
    let existing_permissions = match fs::metadata(target_path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(StorageTransactionError::new(
                "read target metadata",
                target_path,
                error,
            ));
        }
    };
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(staged_file_path)
        .map_err(|error| {
            StorageTransactionError::new("create staged file", staged_file_path, error)
        })?;
    if let Some(permissions) = existing_permissions {
        file.set_permissions(permissions).map_err(|error| {
            StorageTransactionError::new("set staged file permissions", staged_file_path, error)
        })?;
    }
    file.write_all(bytes).map_err(|error| {
        StorageTransactionError::new("write staged file", staged_file_path, error)
    })?;
    file.sync_all()
        .map_err(|error| StorageTransactionError::new("sync staged file", staged_file_path, error))
}

fn sync_directory(path: &Path) -> Result<(), StorageTransactionError> {
    let directory = File::open(path)
        .map_err(|error| StorageTransactionError::new("open directory", path, error))?;
    directory
        .sync_all()
        .map_err(|error| StorageTransactionError::new("sync directory", path, error))
}

#[cfg(test)]
#[path = "storage_transaction_tests.rs"]
mod tests;
