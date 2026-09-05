use super::{StorageTransactionError, StorageTransactionOperation};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

pub(in crate::adapter::gateway) const TRANSACTION_DIRECTORY_NAME: &str = ".schronu-transactions";
pub(super) const ACTIVE_TRANSACTION_DIRECTORY_NAME: &str = ".active";
const STAGED_FILES_DIRECTORY_NAME: &str = "files";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const COMMIT_MARKER_FILE_NAME: &str = "commit";
const TEMPORARY_COMMIT_MARKER_FILE_NAME: &str = "commit.tmp";
pub(super) const TRANSACTION_LOCK_FILE_NAME: &str = ".lock";
const REVISION_FILE_NAME: &str = ".revision";
const CLEANUP_DIRECTORY_PREFIX: &str = ".cleanup-";

pub(super) struct TransactionLayout<'a> {
    storage_dir_path: &'a Path,
}

impl<'a> TransactionLayout<'a> {
    pub(super) fn new(storage_dir_path: &'a Path) -> Self {
        Self { storage_dir_path }
    }

    pub(super) fn transactions_dir_path(&self) -> PathBuf {
        self.storage_dir_path.join(TRANSACTION_DIRECTORY_NAME)
    }

    pub(super) fn active_transaction_dir_path(&self) -> PathBuf {
        self.transactions_dir_path()
            .join(ACTIVE_TRANSACTION_DIRECTORY_NAME)
    }

    pub(super) fn staged_files_dir_path(&self) -> PathBuf {
        self.active_transaction_dir_path()
            .join(STAGED_FILES_DIRECTORY_NAME)
    }

    pub(super) fn staged_file_relative_path(index: usize) -> PathBuf {
        PathBuf::from(STAGED_FILES_DIRECTORY_NAME).join(index.to_string())
    }

    pub(super) fn is_staged_files_directory_name(name: &OsStr) -> bool {
        name == STAGED_FILES_DIRECTORY_NAME
    }

    pub(super) fn staged_file_path(transaction_dir_path: &Path, staged_file: &Path) -> PathBuf {
        transaction_dir_path.join(staged_file)
    }

    pub(super) fn manifest_path(transaction_dir_path: &Path) -> PathBuf {
        transaction_dir_path.join(MANIFEST_FILE_NAME)
    }

    pub(super) fn commit_marker_path(transaction_dir_path: &Path) -> PathBuf {
        transaction_dir_path.join(COMMIT_MARKER_FILE_NAME)
    }

    pub(super) fn temporary_commit_marker_path(transaction_dir_path: &Path) -> PathBuf {
        transaction_dir_path.join(TEMPORARY_COMMIT_MARKER_FILE_NAME)
    }

    pub(super) fn transaction_lock_path(transactions_dir_path: &Path) -> PathBuf {
        transactions_dir_path.join(TRANSACTION_LOCK_FILE_NAME)
    }

    pub(super) fn revision_path(&self) -> PathBuf {
        self.storage_dir_path.join(REVISION_FILE_NAME)
    }

    pub(super) fn target_path(&self, relative_path: &Path) -> PathBuf {
        self.storage_dir_path.join(relative_path)
    }

    pub(super) fn cleanup_dir_path(&self, transaction_id: Uuid) -> PathBuf {
        self.transactions_dir_path().join(format!(
            "{CLEANUP_DIRECTORY_PREFIX}{}",
            transaction_id.hyphenated()
        ))
    }

    pub(super) fn cleanup_transaction_id(name: &str) -> Option<Uuid> {
        let uuid_text = name.strip_prefix(CLEANUP_DIRECTORY_PREFIX)?;
        let uuid = Uuid::parse_str(uuid_text).ok()?;
        (uuid.hyphenated().to_string() == uuid_text).then_some(uuid)
    }

    pub(super) fn live_temporary_path(
        parent_path: &Path,
        file_name: &OsStr,
        transaction_id: Uuid,
    ) -> PathBuf {
        parent_path.join(format!(
            ".{}.{}.tmp",
            file_name.to_string_lossy(),
            transaction_id.hyphenated()
        ))
    }
}

pub(super) fn validate_storage_relative_path(
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

pub(super) fn invalid_target_path_error(
    path: &Path,
    message: &'static str,
) -> StorageTransactionError {
    StorageTransactionError::new(
        StorageTransactionOperation::ValidateTargetPath,
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidInput, message),
    )
}
