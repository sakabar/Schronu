use super::create::capture::scan_storage_entries;
use super::error::{SnapshotError, SnapshotOperation};
use super::layout::PAYLOAD_DIRECTORY_NAME;
use super::verify::load_verified_snapshot;
use super::{create_snapshot_with_lock, SnapshotSummary, DEFAULT_RESOURCE_LIMITS};
use crate::adapter::gateway::storage_lock::StorageLock;
use crate::adapter::gateway::storage_transaction::{
    prepare_with_directories_and_deletes, FileSystemStorageTransactionIo, WriteRequest,
};
use chrono::{DateTime, Local};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

pub fn restore_current_snapshot(
    current_storage: &Path,
    snapshot: &Path,
    pre_backup: &Path,
    storage_lock: &StorageLock,
) -> Result<SnapshotSummary, SnapshotError> {
    restore_current_snapshot_at(
        current_storage,
        snapshot,
        pre_backup,
        Local::now(),
        storage_lock,
    )
}

pub(in crate::adapter::gateway) fn restore_current_snapshot_at(
    current_storage: &Path,
    snapshot: &Path,
    pre_backup: &Path,
    created_at: DateTime<Local>,
    storage_lock: &StorageLock,
) -> Result<SnapshotSummary, SnapshotError> {
    restore_current_snapshot_impl(
        current_storage,
        snapshot,
        pre_backup,
        created_at,
        storage_lock,
        Arc::new(FileSystemStorageTransactionIo),
    )
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn restore_current_snapshot_at_with_transaction_io(
    current_storage: &Path,
    snapshot: &Path,
    pre_backup: &Path,
    created_at: DateTime<Local>,
    storage_lock: &StorageLock,
    transaction_io: Arc<dyn crate::adapter::gateway::storage_transaction::StorageTransactionIo>,
) -> Result<SnapshotSummary, SnapshotError> {
    restore_current_snapshot_impl(
        current_storage,
        snapshot,
        pre_backup,
        created_at,
        storage_lock,
        transaction_io,
    )
}

fn restore_current_snapshot_impl(
    current_storage: &Path,
    snapshot: &Path,
    pre_backup: &Path,
    created_at: DateTime<Local>,
    storage_lock: &StorageLock,
    transaction_io: Arc<dyn crate::adapter::gateway::storage_transaction::StorageTransactionIo>,
) -> Result<SnapshotSummary, SnapshotError> {
    let expected_lock_path = current_storage.join(".lock");
    if storage_lock.path() != expected_lock_path {
        return Err(invalid(
            expected_lock_path,
            "current restore requires the current storage lock",
        ));
    }

    let verified = load_verified_snapshot(snapshot)?;
    create_snapshot_with_lock(current_storage, pre_backup, created_at, storage_lock)?;
    let current = scan_storage_entries(
        current_storage,
        DEFAULT_RESOURCE_LIMITS,
        &super::io::FileSystemSnapshotIo,
    )?;

    let desired_files = verified
        .tree
        .files
        .iter()
        .filter_map(|file| {
            file.path
                .strip_prefix(PAYLOAD_DIRECTORY_NAME)
                .ok()
                .filter(|relative| *relative != Path::new(".revision"))
                .map(|relative| (relative.to_path_buf(), file.bytes.as_slice()))
        })
        .collect::<Vec<_>>();
    let desired_file_paths = desired_files
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<HashSet<_>>();
    let desired_directories = verified
        .tree
        .directories
        .iter()
        .filter_map(|directory| {
            directory
                .path
                .strip_prefix(PAYLOAD_DIRECTORY_NAME)
                .ok()
                .filter(|relative| !relative.as_os_str().is_empty())
                .map(Path::to_path_buf)
        })
        .collect::<Vec<_>>();
    let desired_directory_paths = desired_directories.iter().cloned().collect::<HashSet<_>>();

    let write_paths = desired_files
        .iter()
        .map(|(relative, _)| current_storage.join(relative))
        .collect::<Vec<_>>();
    let writes = write_paths
        .iter()
        .zip(desired_files.iter())
        .map(|(target_path, (_, bytes))| WriteRequest { target_path, bytes })
        .collect::<Vec<_>>();
    let directory_paths = desired_directories
        .iter()
        .map(|relative| current_storage.join(relative))
        .collect::<Vec<_>>();
    let deletes = current
        .files
        .iter()
        .filter(|file| {
            file.relative != Path::new(".revision") && !desired_file_paths.contains(&file.relative)
        })
        .map(|file| current_storage.join(&file.relative))
        .chain(
            current
                .directories
                .iter()
                .filter(|directory| !desired_directory_paths.contains(&directory.relative))
                .map(|directory| current_storage.join(&directory.relative)),
        )
        .collect::<Vec<PathBuf>>();
    let directory_refs = directory_paths
        .iter()
        .map(PathBuf::as_path)
        .collect::<Vec<_>>();
    let delete_refs = deletes.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    let revision = Uuid::new_v4();
    let prepared = prepare_with_directories_and_deletes(
        transaction_io,
        current_storage,
        revision,
        &writes,
        &directory_refs,
        &delete_refs,
    )
    .map_err(|error| SnapshotError::new(SnapshotOperation::Write, current_storage, error))?;
    prepared
        .commit()
        .map_err(|error| SnapshotError::new(SnapshotOperation::Write, current_storage, error))?;

    Ok(SnapshotSummary::new(
        Some(revision),
        verified.manifest.files.len(),
    ))
}

fn invalid(path: impl Into<PathBuf>, message: &'static str) -> SnapshotError {
    SnapshotError::new(
        SnapshotOperation::Validate,
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    )
}
