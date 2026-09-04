use serde::Serialize;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

pub(super) const TRANSACTION_DIRECTORY_NAME: &str = ".schronu-transactions";
const ACTIVE_TRANSACTION_DIRECTORY_NAME: &str = ".active";

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
    CreateCommitMarker,
    SyncCommitMarker,
    RenameCommitMarker,
    CreateTargetDirectory,
    ReadStagedFile,
    CreateLiveTemporary,
    SetLivePermissions,
    WriteLiveTemporary,
    SyncLiveTemporary,
    RenameLiveTarget,
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

#[derive(Serialize)]
struct TransactionManifest {
    version: u32,
    transaction_id: Uuid,
    revision: Uuid,
    directories: Vec<PathBuf>,
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

    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        fs::create_dir(path)
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

    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn sync_file(&self, path: &Path) -> std::io::Result<()> {
        File::open(path)?.sync_all()
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        File::open(path)?.sync_all()
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_dir_all(path)
    }
}

#[derive(Default)]
pub(super) struct FileSystemStorageTransactionIo;
impl StorageTransactionIo for FileSystemStorageTransactionIo {}

pub(super) struct PreparedTransaction {
    storage_dir_path: PathBuf,
    transactions_dir_path: PathBuf,
    transaction_dir_path: PathBuf,
    transaction_id: Uuid,
    revision: Uuid,
    directories: Vec<PathBuf>,
    entries: Vec<PreparedEntry>,
    io: Arc<dyn StorageTransactionIo>,
}

struct PreparedEntry {
    target_path: PathBuf,
    staged_file_path: PathBuf,
}

impl PreparedTransaction {
    #[cfg(test)]
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

    pub(super) fn commit(self, revision_path: &Path) -> Result<(), StorageTransactionError> {
        let marker_temporary_path = self.transaction_dir_path.join("commit.tmp");
        let marker_path = self.transaction_dir_path.join("commit");
        self.io
            .create_new_file(&marker_temporary_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::CreateCommitMarker,
                    &marker_temporary_path,
                    error,
                )
            })?;
        self.io.sync_file(&marker_temporary_path).map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::SyncCommitMarker,
                &marker_temporary_path,
                error,
            )
        })?;
        self.io
            .rename(&marker_temporary_path, &marker_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::RenameCommitMarker,
                    &marker_path,
                    error,
                )
            })?;
        sync_directory(self.io.as_ref(), &self.transaction_dir_path)?;

        for directory in &self.directories {
            let directory_path = self.storage_dir_path.join(directory);
            self.io.create_dir_all(&directory_path).map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::CreateTargetDirectory,
                    directory_path,
                    error,
                )
            })?;
        }
        for entry in &self.entries {
            self.apply_staged_file(entry)?;
        }
        self.apply_revision(revision_path)?;

        let cleanup_dir_path = self
            .transactions_dir_path
            .join(format!(".cleanup-{}", self.transaction_id.hyphenated()));
        self.io
            .rename(&self.transaction_dir_path, &cleanup_dir_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::RenameForCleanup,
                    &self.transaction_dir_path,
                    error,
                )
            })?;
        if self.io.sync_directory(&self.transactions_dir_path).is_ok() {
            let _ = self.io.remove_dir_all(&cleanup_dir_path);
            let _ = self.io.sync_directory(&self.transactions_dir_path);
        }
        let _ = self.io.sync_directory(&self.storage_dir_path);
        Ok(())
    }

    fn apply_staged_file(&self, entry: &PreparedEntry) -> Result<(), StorageTransactionError> {
        let bytes = self
            .io
            .read_file(&entry.staged_file_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::ReadStagedFile,
                    &entry.staged_file_path,
                    error,
                )
            })?;
        let permissions = self
            .io
            .target_permissions(&entry.staged_file_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::ReadTargetMetadata,
                    &entry.staged_file_path,
                    error,
                )
            })?;
        self.apply_bytes(&entry.target_path, &bytes, permissions)
    }

    fn apply_revision(&self, revision_path: &Path) -> Result<(), StorageTransactionError> {
        let permissions = self.io.target_permissions(revision_path).map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::ReadTargetMetadata,
                revision_path,
                error,
            )
        })?;
        self.apply_bytes(
            revision_path,
            format!("{}\n", self.revision).as_bytes(),
            permissions,
        )
    }

    fn apply_bytes(
        &self,
        target_path: &Path,
        bytes: &[u8],
        permissions: Option<fs::Permissions>,
    ) -> Result<(), StorageTransactionError> {
        let parent_path = target_path.parent().ok_or_else(|| {
            StorageTransactionError::new(
                StorageTransactionOperation::CreateTargetDirectory,
                target_path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "transaction target must have a parent directory",
                ),
            )
        })?;
        self.io.create_dir_all(parent_path).map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::CreateTargetDirectory,
                parent_path,
                error,
            )
        })?;
        let file_name = target_path.file_name().ok_or_else(|| {
            StorageTransactionError::new(
                StorageTransactionOperation::CreateLiveTemporary,
                target_path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "transaction target must have a file name",
                ),
            )
        })?;
        let temporary_path = parent_path.join(format!(
            ".{}.{}.tmp",
            file_name.to_string_lossy(),
            self.transaction_id.hyphenated()
        ));
        self.io.create_new_file(&temporary_path).map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::CreateLiveTemporary,
                &temporary_path,
                error,
            )
        })?;
        if let Some(permissions) = permissions {
            self.io
                .set_permissions(&temporary_path, permissions)
                .map_err(|error| {
                    StorageTransactionError::new(
                        StorageTransactionOperation::SetLivePermissions,
                        &temporary_path,
                        error,
                    )
                })?;
        }
        self.io
            .write_file(&temporary_path, bytes)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::WriteLiveTemporary,
                    &temporary_path,
                    error,
                )
            })?;
        self.io.sync_file(&temporary_path).map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::SyncLiveTemporary,
                &temporary_path,
                error,
            )
        })?;
        self.io
            .rename(&temporary_path, target_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::RenameLiveTarget,
                    target_path,
                    error,
                )
            })?;
        sync_directory(self.io.as_ref(), parent_path)
    }
}

#[cfg(test)]
pub(super) fn prepare(
    io: Arc<dyn StorageTransactionIo>,
    storage_dir_path: &Path,
    revision: Uuid,
    writes: &[WriteRequest<'_>],
) -> Result<PreparedTransaction, StorageTransactionError> {
    prepare_with_directories(io, storage_dir_path, revision, writes, &[])
}

pub(super) fn prepare_with_directories(
    io: Arc<dyn StorageTransactionIo>,
    storage_dir_path: &Path,
    revision: Uuid,
    writes: &[WriteRequest<'_>],
    directories: &[&Path],
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
    let transaction_dir_path = transactions_dir_path.join(ACTIVE_TRANSACTION_DIRECTORY_NAME);
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
        .map(|(index, write)| PreparedEntry {
            target_path: write.target_path.to_path_buf(),
            staged_file_path: transaction_dir_path.join("files").join(index.to_string()),
        })
        .collect();
    Ok(PreparedTransaction {
        storage_dir_path: storage_dir_path.to_path_buf(),
        transactions_dir_path,
        transaction_dir_path,
        transaction_id,
        revision,
        directories,
        entries,
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
    directories: &[&Path],
) -> Result<Vec<PathBuf>, StorageTransactionError> {
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

    let directories = directories
        .iter()
        .map(|directory| {
            directory
                .strip_prefix(storage_dir_path)
                .map(Path::to_path_buf)
                .map_err(|error| {
                    StorageTransactionError::new(
                        StorageTransactionOperation::ResolveTargetPath,
                        *directory,
                        std::io::Error::new(std::io::ErrorKind::InvalidInput, error),
                    )
                })
        })
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
