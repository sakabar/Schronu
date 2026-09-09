use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};

use super::cleanup::cleanup_committed_transaction;
use super::io::{sync_directory, validate_write_target, DirectoryPermissionError};
use super::layout::TransactionLayout;
use super::manifest::{content_matches, ValidatedEntry};
use super::{
    CommittedTransaction, PreparedTransaction, StorageTransactionError, StorageTransactionOperation,
};

enum PreflightEntry {
    AlreadyApplied,
    Write {
        target_path: PathBuf,
        relative_path: PathBuf,
        bytes: Vec<u8>,
        permissions: fs::Permissions,
    },
    Delete {
        target_path: PathBuf,
        relative_path: PathBuf,
    },
}

impl PreparedTransaction {
    #[cfg(test)]
    pub(super) fn transaction_dir_path(&self) -> &Path {
        &self.state.paths.transaction_dir_path
    }

    #[cfg(test)]
    pub(super) fn transaction_id(&self) -> uuid::Uuid {
        self.state.manifest.transaction_id
    }

    #[cfg(test)]
    pub(in crate::adapter::gateway) fn discard(self) -> Result<(), StorageTransactionError> {
        self.state
            .io
            .remove_dir_all(&self.state.paths.transaction_dir_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::Discard,
                    self.state.paths.transaction_dir_path,
                    error,
                )
            })
    }

    pub(in crate::adapter::gateway) fn commit(self) -> Result<(), StorageTransactionError> {
        let marker_temporary_path =
            TransactionLayout::temporary_commit_marker_path(&self.state.paths.transaction_dir_path);
        let marker_path =
            TransactionLayout::commit_marker_path(&self.state.paths.transaction_dir_path);
        self.state
            .io
            .create_new_file(&marker_temporary_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::CreateCommitMarker,
                    &marker_temporary_path,
                    error,
                )
            })?;
        self.state
            .io
            .sync_file(&marker_temporary_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::SyncCommitMarker,
                    &marker_temporary_path,
                    error,
                )
            })?;
        self.state
            .io
            .rename(&marker_temporary_path, &marker_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::RenameCommitMarker,
                    &marker_path,
                    error,
                )
            })?;
        sync_directory(
            self.state.io.as_ref(),
            &self.state.paths.transaction_dir_path,
        )
        .map_err(StorageTransactionError::with_commit_marker_established)?;

        CommittedTransaction { state: self.state }
            .roll_forward()
            .map_err(StorageTransactionError::with_commit_marker_established)
    }
}

impl CommittedTransaction {
    pub(super) fn roll_forward(self) -> Result<(), StorageTransactionError> {
        let mut preflight_entries = self.preflight_entries()?;
        let layout = TransactionLayout::new(&self.state.paths.storage_dir_path);
        let first_write = preflight_entries
            .iter()
            .position(|entry| !matches!(entry, PreflightEntry::Delete { .. }))
            .unwrap_or(preflight_entries.len());
        let writes = preflight_entries.split_off(first_write);
        for entry in preflight_entries {
            let PreflightEntry::Delete {
                target_path,
                relative_path,
            } = entry
            else {
                unreachable!("delete partition contains only deletes");
            };
            self.apply_delete(&target_path, &relative_path)?;
        }
        for directory in &self.state.manifest.directories {
            let directory_path = layout.target_path(directory);
            self.state
                .io
                .create_dir_all(&directory_path)
                .map_err(|error| {
                    StorageTransactionError::new(
                        StorageTransactionOperation::CreateTargetDirectory,
                        directory_path,
                        error,
                    )
                })?;
        }
        for entry in writes {
            match entry {
                PreflightEntry::AlreadyApplied => {}
                PreflightEntry::Write {
                    target_path,
                    relative_path,
                    bytes,
                    permissions,
                } => self.apply_bytes(&target_path, &relative_path, &bytes, Some(permissions))?,
                PreflightEntry::Delete { .. } => {
                    unreachable!("write partition does not contain deletes")
                }
            }
        }
        for (directory, mode) in self
            .state
            .manifest
            .directories
            .iter()
            .zip(&self.state.manifest.directory_modes)
            .rev()
        {
            let Some(mode) = mode else {
                continue;
            };
            let directory_path = layout.target_path(directory);
            self.state
                .io
                .set_and_sync_directory_permissions(&directory_path, permissions_from_mode(*mode))
                .map_err(|error| {
                    let (operation, source) = match error {
                        DirectoryPermissionError::Set(source) => {
                            (StorageTransactionOperation::SetLivePermissions, source)
                        }
                        DirectoryPermissionError::Sync(source) => {
                            (StorageTransactionOperation::SyncDirectory, source)
                        }
                    };
                    StorageTransactionError::new(operation, &directory_path, source)
                })?;
        }
        self.apply_revision(&layout.revision_path())?;
        cleanup_committed_transaction(&self)
    }

    fn preflight_entries(&self) -> Result<Vec<PreflightEntry>, StorageTransactionError> {
        let layout = TransactionLayout::new(&self.state.paths.storage_dir_path);
        let mut entries = self
            .state
            .manifest
            .entries
            .iter()
            .map(|entry| match entry {
                ValidatedEntry::Delete { target } => Ok(PreflightEntry::Delete {
                    target_path: layout.target_path(target),
                    relative_path: target.clone(),
                }),
                ValidatedEntry::Write {
                    target,
                    staged_file,
                    mode,
                    integrity,
                } => {
                    let target_path = layout.target_path(target);
                    validate_write_target(
                        self.state.io.as_ref(),
                        &self.state.paths.storage_dir_path,
                        &target_path,
                        self.state.manifest.replace_target_directories,
                    )?;
                    let staged_file_path = TransactionLayout::staged_file_path(
                        &self.state.paths.transaction_dir_path,
                        staged_file,
                    );
                    let staged_material = match self.state.io.symlink_metadata(&staged_file_path) {
                        Ok(metadata) if metadata.file_type().is_file() => {
                            let bytes = self.state.io.read_file(&staged_file_path).map_err(|error| {
                                StorageTransactionError::new(
                                    StorageTransactionOperation::ReadStagedFile,
                                    &staged_file_path,
                                    error,
                                )
                            })?;
                            if !content_matches(
                                &bytes,
                                integrity.content_length,
                                &integrity.checksum,
                            ) {
                                return Err(StorageTransactionError::new(
                                    StorageTransactionOperation::ValidateStagedContent,
                                    &staged_file_path,
                                    std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        "staged transaction material does not match manifest content",
                                    ),
                                ));
                            }
                            Some((bytes, metadata.permissions()))
                        }
                        Ok(_) => {
                            return Err(StorageTransactionError::new(
                                StorageTransactionOperation::ValidateStagedFile,
                                &staged_file_path,
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "staged transaction material must be a regular file",
                                ),
                            ));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                        Err(error) => {
                            return Err(StorageTransactionError::new(
                                StorageTransactionOperation::ReadStagedFile,
                                &staged_file_path,
                                error,
                            ));
                        }
                    };
                    match self.state.io.symlink_metadata(&target_path) {
                        Ok(metadata) if metadata.file_type().is_file() => {
                            let target_bytes = self.state.io.read_file(&target_path).map_err(|error| {
                                StorageTransactionError::new(
                                    StorageTransactionOperation::ReadTargetContent,
                                    &target_path,
                                    error,
                                )
                            })?;
                            if content_matches(
                                &target_bytes,
                                integrity.content_length,
                                &integrity.checksum,
                            ) && permission_matches(&metadata.permissions(), *mode)
                            {
                                return Ok(PreflightEntry::AlreadyApplied);
                            }
                        }
                        Ok(_) => {}
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::NotFound
                                    | std::io::ErrorKind::NotADirectory
                            ) => {}
                        Err(error) => {
                            return Err(StorageTransactionError::new(
                                StorageTransactionOperation::ReadTargetMetadata,
                                &target_path,
                                error,
                            ));
                        }
                    }
                    let (bytes, staged_permissions) = staged_material.ok_or_else(|| {
                        StorageTransactionError::new(
                            StorageTransactionOperation::ReadStagedFile,
                            &staged_file_path,
                            std::io::Error::new(
                                std::io::ErrorKind::NotFound,
                                "staged transaction material does not exist",
                            ),
                        )
                    })?;
                    Ok(PreflightEntry::Write {
                        target_path,
                        relative_path: target.clone(),
                        bytes,
                        permissions: mode
                            .map(permissions_from_mode)
                            .unwrap_or(staged_permissions),
                    })
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| match entry {
            PreflightEntry::Delete { target_path, .. } => {
                (0, Reverse(target_path.components().count()))
            }
            PreflightEntry::AlreadyApplied | PreflightEntry::Write { .. } => (1, Reverse(0)),
        });
        Ok(entries)
    }

    fn apply_revision(&self, revision_path: &Path) -> Result<(), StorageTransactionError> {
        let permissions = self
            .state
            .io
            .target_permissions(revision_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::ReadTargetMetadata,
                    revision_path,
                    error,
                )
            })?;
        self.apply_bytes(
            revision_path,
            Path::new(".revision"),
            format!("{}\n", self.state.manifest.revision).as_bytes(),
            permissions,
        )
    }

    fn apply_delete(
        &self,
        target_path: &Path,
        relative_path: &Path,
    ) -> Result<(), StorageTransactionError> {
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
        let removal = self
            .state
            .io
            .remove_storage_entry(&self.state.paths.storage_dir_path, relative_path);
        match removal {
            Ok(()) => sync_directory(self.state.io.as_ref(), parent_path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match self.state.io.sync_directory(parent_path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(StorageTransactionError::new(
                        StorageTransactionOperation::SyncDirectory,
                        parent_path,
                        error,
                    )),
                }
            }
            Err(error)
                if self.state.manifest.replace_target_directories
                    && error.kind() == std::io::ErrorKind::NotADirectory
                    && self.delete_is_shadowed_by_desired_file(relative_path) =>
            {
                Ok(())
            }
            Err(error)
                if self.state.manifest.replace_target_directories
                    && error.kind() == std::io::ErrorKind::DirectoryNotEmpty
                    && self
                        .state
                        .manifest
                        .directories
                        .iter()
                        .any(|directory| directory == relative_path) =>
            {
                Ok(())
            }
            Err(error) => Err(StorageTransactionError::new(
                StorageTransactionOperation::RemoveLiveTarget,
                target_path,
                error,
            )),
        }
    }

    fn delete_is_shadowed_by_desired_file(&self, relative_path: &Path) -> bool {
        self.state.manifest.entries.iter().any(|entry| {
            matches!(
                entry,
                ValidatedEntry::Write { target, .. }
                    if target != relative_path && relative_path.starts_with(target)
            )
        })
    }

    fn apply_bytes(
        &self,
        target_path: &Path,
        relative_path: &Path,
        bytes: &[u8],
        permissions: Option<fs::Permissions>,
    ) -> Result<(), StorageTransactionError> {
        if !self.state.manifest.replace_target_directories
            && self.state.io.write_storage_file_anchored(
                &self.state.paths.storage_dir_path,
                relative_path,
                self.state.manifest.transaction_id,
                bytes,
                permissions.as_ref(),
            )?
        {
            return Ok(());
        }
        validate_write_target(
            self.state.io.as_ref(),
            &self.state.paths.storage_dir_path,
            target_path,
            self.state.manifest.replace_target_directories,
        )?;
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
        self.state.io.create_dir_all(parent_path).map_err(|error| {
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
        let temporary_path = TransactionLayout::live_temporary_path(
            parent_path,
            file_name,
            self.state.manifest.transaction_id,
        );
        if let Err(error) = self.state.io.create_new_file(&temporary_path) {
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(StorageTransactionError::new(
                    StorageTransactionOperation::CreateLiveTemporary,
                    &temporary_path,
                    error,
                ));
            }
            self.state
                .io
                .remove_file(&temporary_path)
                .map_err(|error| {
                    StorageTransactionError::new(
                        StorageTransactionOperation::RemoveLiveTemporary,
                        &temporary_path,
                        error,
                    )
                })?;
            self.state
                .io
                .create_new_file(&temporary_path)
                .map_err(|error| {
                    StorageTransactionError::new(
                        StorageTransactionOperation::CreateLiveTemporary,
                        &temporary_path,
                        error,
                    )
                })?;
        }
        if let Some(permissions) = permissions {
            self.state
                .io
                .set_permissions(&temporary_path, permissions)
                .map_err(|error| {
                    StorageTransactionError::new(
                        StorageTransactionOperation::SetLivePermissions,
                        &temporary_path,
                        error,
                    )
                })?;
        }
        self.state
            .io
            .write_file(&temporary_path, bytes)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::WriteLiveTemporary,
                    &temporary_path,
                    error,
                )
            })?;
        self.state.io.sync_file(&temporary_path).map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::SyncLiveTemporary,
                &temporary_path,
                error,
            )
        })?;
        validate_write_target(
            self.state.io.as_ref(),
            &self.state.paths.storage_dir_path,
            target_path,
            self.state.manifest.replace_target_directories,
        )?;
        if self.state.manifest.replace_target_directories {
            self.state
                .io
                .remove_storage_directory_if_present(
                    &self.state.paths.storage_dir_path,
                    relative_path,
                )
                .map_err(|error| {
                    StorageTransactionError::new(
                        StorageTransactionOperation::RemoveLiveTarget,
                        target_path,
                        error,
                    )
                })?;
        }
        self.state
            .io
            .rename(&temporary_path, target_path)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::RenameLiveTarget,
                    target_path,
                    error,
                )
            })?;
        sync_directory(self.state.io.as_ref(), parent_path)
    }
}

#[cfg(unix)]
fn permissions_from_mode(mode: u32) -> fs::Permissions {
    use std::os::unix::fs::PermissionsExt;

    fs::Permissions::from_mode(mode)
}

#[cfg(not(unix))]
fn permissions_from_mode(_mode: u32) -> fs::Permissions {
    unreachable!("transaction modes are not produced on non-Unix platforms")
}

#[cfg(unix)]
fn permission_matches(permissions: &fs::Permissions, expected_mode: Option<u32>) -> bool {
    use std::os::unix::fs::PermissionsExt;

    expected_mode.is_none_or(|mode| permissions.mode() & 0o7777 == mode)
}

#[cfg(not(unix))]
fn permission_matches(_permissions: &fs::Permissions, _expected_mode: Option<u32>) -> bool {
    true
}
