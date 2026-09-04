use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

pub(super) const TRANSACTION_DIRECTORY_NAME: &str = ".schronu-transactions";
const ACTIVE_TRANSACTION_DIRECTORY_NAME: &str = ".active";
const TRANSACTION_LOCK_FILE_NAME: &str = ".lock";

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
    ValidateStagedFile,
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

#[derive(Deserialize, Serialize)]
struct TransactionManifest {
    version: u32,
    transaction_id: Uuid,
    revision: Uuid,
    directories: Vec<PathBuf>,
    entries: Vec<ManifestEntry>,
}

#[derive(Deserialize, Serialize)]
struct ManifestEntry {
    target: PathBuf,
    #[serde(default, skip_serializing_if = "ManifestEntryOperation::is_write")]
    operation: ManifestEntryOperation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    staged_file: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ManifestEntryOperation {
    #[default]
    Write,
    Delete,
}

impl ManifestEntryOperation {
    fn is_write(operation: &Self) -> bool {
        *operation == Self::Write
    }
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

    fn symlink_metadata(&self, path: &Path) -> std::io::Result<fs::Metadata> {
        fs::symlink_metadata(path)
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

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
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
    _transaction_lock: TransactionLock,
}

struct TransactionLock {
    _file: File,
}

struct PreparedEntry {
    target: PathBuf,
    operation: ManifestEntryOperation,
    staged_file: Option<PathBuf>,
}

struct PreflightEntry {
    target_path: PathBuf,
    operation: ManifestEntryOperation,
    bytes: Option<Vec<u8>>,
    permissions: Option<fs::Permissions>,
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

        self.finish_committed(revision_path)
    }

    fn finish_committed(self, revision_path: &Path) -> Result<(), StorageTransactionError> {
        let preflight_entries = self.preflight_entries()?;
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
        for entry in &preflight_entries {
            match entry.operation {
                ManifestEntryOperation::Write => self.apply_bytes(
                    &entry.target_path,
                    entry
                        .bytes
                        .as_deref()
                        .expect("preflight write entry must contain staged bytes"),
                    entry.permissions.clone(),
                )?,
                ManifestEntryOperation::Delete => self.apply_delete(&entry.target_path)?,
            }
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

    fn preflight_entries(&self) -> Result<Vec<PreflightEntry>, StorageTransactionError> {
        self.entries
            .iter()
            .map(|entry| {
                if entry.operation == ManifestEntryOperation::Delete {
                    return Ok(PreflightEntry {
                        target_path: self.storage_dir_path.join(&entry.target),
                        operation: entry.operation,
                        bytes: None,
                        permissions: None,
                    });
                }
                let staged_file_path = self.transaction_dir_path.join(
                    entry
                        .staged_file
                        .as_ref()
                        .expect("validated write entry must contain a staged file"),
                );
                let metadata = self
                    .io
                    .symlink_metadata(&staged_file_path)
                    .map_err(|error| {
                        StorageTransactionError::new(
                            StorageTransactionOperation::ReadStagedFile,
                            &staged_file_path,
                            error,
                        )
                    })?;
                if !metadata.file_type().is_file() {
                    return Err(StorageTransactionError::new(
                        StorageTransactionOperation::ValidateStagedFile,
                        &staged_file_path,
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "staged transaction material must be a regular file",
                        ),
                    ));
                }
                let bytes = self.io.read_file(&staged_file_path).map_err(|error| {
                    StorageTransactionError::new(
                        StorageTransactionOperation::ReadStagedFile,
                        &staged_file_path,
                        error,
                    )
                })?;
                Ok(PreflightEntry {
                    target_path: self.storage_dir_path.join(&entry.target),
                    operation: entry.operation,
                    bytes: Some(bytes),
                    permissions: Some(metadata.permissions()),
                })
            })
            .collect()
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

    fn apply_delete(&self, target_path: &Path) -> Result<(), StorageTransactionError> {
        let parent_path = target_path.parent().ok_or_else(|| {
            StorageTransactionError::new(
                StorageTransactionOperation::RemoveLiveTarget,
                target_path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "transaction target must have a parent directory",
                ),
            )
        })?;
        match self.io.remove_file(target_path) {
            Ok(()) => sync_directory(self.io.as_ref(), parent_path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match self.io.sync_directory(parent_path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(StorageTransactionError::new(
                        StorageTransactionOperation::SyncDirectory,
                        parent_path,
                        error,
                    )),
                }
            }
            Err(error) => Err(StorageTransactionError::new(
                StorageTransactionOperation::RemoveLiveTarget,
                target_path,
                error,
            )),
        }
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
        if let Err(error) = self.io.create_new_file(&temporary_path) {
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(StorageTransactionError::new(
                    StorageTransactionOperation::CreateLiveTemporary,
                    &temporary_path,
                    error,
                ));
            }
            self.io.remove_file(&temporary_path).map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::RemoveLiveTemporary,
                    &temporary_path,
                    error,
                )
            })?;
            self.io.create_new_file(&temporary_path).map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::CreateLiveTemporary,
                    &temporary_path,
                    error,
                )
            })?;
        }
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
            match (entry.operation, entry.staged_file.as_deref()) {
                (ManifestEntryOperation::Write, Some(staged_file)) => {
                    validate_staged_file_path(&transaction_dir_path, staged_file)?;
                }
                (ManifestEntryOperation::Delete, None) => {}
                (ManifestEntryOperation::Write, None) => {
                    return Err(invalid_manifest_entry_error(
                        &manifest_path,
                        "write entry must contain a staged file",
                    ));
                }
                (ManifestEntryOperation::Delete, Some(_)) => {
                    return Err(invalid_manifest_entry_error(
                        &manifest_path,
                        "delete entry must not contain a staged file",
                    ));
                }
            }
            Ok(PreparedEntry {
                target,
                operation: entry.operation,
                staged_file: entry.staged_file,
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
                return Err(invalid_target_path_error(
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

fn invalid_manifest_entry_error(path: &Path, message: &'static str) -> StorageTransactionError {
    StorageTransactionError::new(
        StorageTransactionOperation::ValidateManifest,
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    )
}

fn validate_staged_file_path(
    transaction_dir_path: &Path,
    staged_file: &Path,
) -> Result<(), StorageTransactionError> {
    let components = staged_file.components().collect::<Vec<_>>();
    if !matches!(
        components.as_slice(),
        [Component::Normal(directory), Component::Normal(_)] if *directory == "files"
    ) {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::ValidateManifest,
            transaction_dir_path.join(staged_file),
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "staged file must be a direct child of the transaction files directory",
            ),
        ));
    }
    Ok(())
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

fn resolve_transactions_directory(
    io: &dyn StorageTransactionIo,
    storage_dir_path: &Path,
    create: bool,
) -> Result<Option<PathBuf>, StorageTransactionError> {
    let transactions_dir_path = storage_dir_path.join(TRANSACTION_DIRECTORY_NAME);
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

fn acquire_transaction_lock(
    transactions_dir_path: &Path,
) -> Result<TransactionLock, StorageTransactionError> {
    let lock_path = transactions_dir_path.join(TRANSACTION_LOCK_FILE_NAME);
    let file = open_transaction_lock_file(&lock_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::AcquireTransactionLock,
            &lock_path,
            error,
        )
    })?;
    file.try_lock_exclusive().map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::AcquireTransactionLock,
            &lock_path,
            error,
        )
    })?;
    Ok(TransactionLock { _file: file })
}

#[cfg(unix)]
fn open_transaction_lock_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "transaction lock path must be a regular file",
        ));
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_transaction_lock_file(_path: &Path) -> std::io::Result<File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "transaction locking is supported only on Unix platforms",
    ))
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

fn validate_storage_relative_path(
    storage_dir_path: &Path,
    target_path: &Path,
) -> Result<PathBuf, StorageTransactionError> {
    let relative_path = target_path
        .strip_prefix(storage_dir_path)
        .map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::ResolveTargetPath,
                target_path,
                std::io::Error::new(std::io::ErrorKind::InvalidInput, error),
            )
        })?;
    if relative_path.as_os_str().is_empty() {
        return Err(invalid_target_path_error(
            target_path,
            "transaction target must not be the storage root",
        ));
    }

    let mut validated = PathBuf::new();
    for (index, component) in relative_path.components().enumerate() {
        match component {
            Component::Normal(name) => {
                if index == 0 && name == TRANSACTION_DIRECTORY_NAME {
                    return Err(invalid_target_path_error(
                        target_path,
                        "transaction target must not use the reserved transaction namespace",
                    ));
                }
                validated.push(name);
            }
            Component::CurDir => {
                return Err(invalid_target_path_error(
                    target_path,
                    "transaction target must use normalized path components",
                ));
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(invalid_target_path_error(
                    target_path,
                    "transaction target must remain within the storage directory",
                ));
            }
        }
    }
    Ok(validated)
}

fn invalid_target_path_error(path: &Path, message: &'static str) -> StorageTransactionError {
    StorageTransactionError::new(
        StorageTransactionOperation::ValidateTargetPath,
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidInput, message),
    )
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
