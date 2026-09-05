use std::fs;
use std::path::{Path, PathBuf};

use super::cleanup::cleanup_committed_transaction;
use super::io::sync_directory;
use super::layout::TransactionLayout;
use super::manifest::{content_matches, ValidatedEntry};
use super::{
    CommittedTransaction, PreparedTransaction, StorageTransactionError, StorageTransactionOperation,
};

enum PreflightEntry {
    AlreadyApplied,
    Write {
        target_path: PathBuf,
        bytes: Vec<u8>,
        permissions: fs::Permissions,
    },
    Delete {
        target_path: PathBuf,
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
        let preflight_entries = self.preflight_entries()?;
        let layout = TransactionLayout::new(&self.state.paths.storage_dir_path);
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
        for entry in preflight_entries {
            match entry {
                PreflightEntry::AlreadyApplied => {}
                PreflightEntry::Write {
                    target_path,
                    bytes,
                    permissions,
                } => self.apply_bytes(&target_path, &bytes, Some(permissions))?,
                PreflightEntry::Delete { target_path } => self.apply_delete(&target_path)?,
            }
        }
        self.apply_revision(&layout.revision_path())?;
        cleanup_committed_transaction(&self)
    }

    fn preflight_entries(&self) -> Result<Vec<PreflightEntry>, StorageTransactionError> {
        let layout = TransactionLayout::new(&self.state.paths.storage_dir_path);
        self.state
            .manifest
            .entries
            .iter()
            .map(|entry| match entry {
                ValidatedEntry::Delete { target } => Ok(PreflightEntry::Delete {
                    target_path: layout.target_path(target),
                }),
                ValidatedEntry::Write {
                    target,
                    staged_file,
                    integrity,
                } => {
                    let target_path = layout.target_path(target);
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
                            ) {
                                return Ok(PreflightEntry::AlreadyApplied);
                            }
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(StorageTransactionError::new(
                                StorageTransactionOperation::ReadTargetMetadata,
                                &target_path,
                                error,
                            ));
                        }
                    }
                    let (bytes, permissions) = staged_material.ok_or_else(|| {
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
                        bytes,
                        permissions,
                    })
                }
            })
            .collect()
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
            format!("{}\n", self.state.manifest.revision).as_bytes(),
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
        match self.state.io.remove_file(target_path) {
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
