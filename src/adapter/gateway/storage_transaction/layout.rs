use super::{StorageTransactionError, StorageTransactionOperation};
use std::path::{Component, Path, PathBuf};

pub(in crate::adapter::gateway) const TRANSACTION_DIRECTORY_NAME: &str = ".schronu-transactions";
pub(super) const ACTIVE_TRANSACTION_DIRECTORY_NAME: &str = ".active";

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
