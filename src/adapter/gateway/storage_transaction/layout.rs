use super::{StorageTransactionError, StorageTransactionIo, StorageTransactionOperation};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(in crate::adapter::gateway) const TRANSACTION_DIRECTORY_NAME: &str = ".schronu-transactions";
pub(super) const ACTIVE_TRANSACTION_DIRECTORY_NAME: &str = ".active";

pub(super) fn validate_delete_target_ancestors(
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

pub(super) fn resolve_transactions_directory(
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

pub(super) fn validate_transactions_directory(path: &Path) -> Result<(), StorageTransactionError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::ValidateTransactionDirectory,
            path,
            error,
        )
    })?;
    validate_transactions_directory_metadata(path, &metadata)
}

pub(super) fn validate_transactions_directory_metadata(
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
