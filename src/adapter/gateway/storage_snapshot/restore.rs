use super::create::finalize_publication;
use super::error::{SnapshotError, SnapshotOperation};
use super::io::{rename_no_replace, DirectoryTree, FileSystemSnapshotIo};
use super::layout::{staging_path, MANIFEST_FILE_NAME, PAYLOAD_DIRECTORY_NAME};
use super::verify::load_verified_snapshot;
use super::SnapshotSummary;
use crate::adapter::gateway::task_repository::TaskRepository;
use crate::application::interface::TaskRepositoryTrait;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn restore_snapshot(
    snapshot_directory: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<SnapshotSummary, SnapshotError> {
    let snapshot = snapshot_directory.as_ref();
    let destination = destination.as_ref();
    validate_destination(snapshot, destination)?;
    let verified = load_verified_snapshot(snapshot)?;
    let staging = staging_path(destination)?;
    let result = materialize_restore(&staging, destination, &verified.tree);
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result.map(|()| SnapshotSummary::new(verified.manifest.revision, verified.manifest.files.len()))
}

fn validate_destination(snapshot: &Path, destination: &Path) -> Result<(), SnapshotError> {
    if destination.exists() {
        return Err(invalid(destination, "restore destination must not exist"));
    }
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            invalid(
                destination,
                "restore destination must have a parent directory",
            )
        })?;
    let canonical_snapshot = fs::canonicalize(snapshot)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, snapshot, error))?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, parent, error))?;
    if canonical_parent.starts_with(&canonical_snapshot) {
        return Err(invalid(
            destination,
            "restore destination must be outside the snapshot",
        ));
    }
    Ok(())
}

fn materialize_restore(
    staging: &Path,
    destination: &Path,
    tree: &DirectoryTree,
) -> Result<(), SnapshotError> {
    fs::create_dir(staging)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Create, staging, error))?;
    for directory in tree
        .directories
        .iter()
        .filter(|entry| entry.path != Path::new(PAYLOAD_DIRECTORY_NAME))
    {
        let relative = payload_relative(&directory.path)?;
        let path = staging.join(relative);
        fs::create_dir(&path)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Create, &path, error))?;
    }
    for file in tree
        .files
        .iter()
        .filter(|entry| entry.path != Path::new(MANIFEST_FILE_NAME))
    {
        let relative = payload_relative(&file.path)?;
        let path = staging.join(relative);
        let mut output = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &path, error))?;
        output
            .write_all(&file.bytes)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &path, error))?;
        fs::set_permissions(&path, file.permissions.clone())
            .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &path, error))?;
        output
            .sync_all()
            .map_err(|error| SnapshotError::new(SnapshotOperation::Sync, &path, error))?;
    }
    for directory in tree
        .directories
        .iter()
        .filter(|entry| entry.path != Path::new(PAYLOAD_DIRECTORY_NAME))
        .rev()
    {
        let relative = payload_relative(&directory.path)?;
        let path = staging.join(relative);
        fs::set_permissions(&path, directory.permissions.clone())
            .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &path, error))?;
        sync_directory(&path)?;
    }
    strict_load(staging)?;
    sync_directory(staging)?;
    rename_no_replace(staging, destination)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Write, destination, error))?;
    finalize_publication(&FileSystemSnapshotIo, destination)
}

fn strict_load(storage: &Path) -> Result<(), SnapshotError> {
    let storage_text = storage
        .to_str()
        .ok_or_else(|| invalid(storage, "restore destination must be valid Unicode"))?;
    let mut repository = TaskRepository::new(storage_text);
    repository
        .load()
        .map_err(|error| SnapshotError::new(SnapshotOperation::RepositoryLoad, storage, error))
}

fn payload_relative(path: &Path) -> Result<&Path, SnapshotError> {
    path.strip_prefix(PAYLOAD_DIRECTORY_NAME)
        .map_err(|_| invalid(path, "snapshot entry is outside storage payload"))
}

fn sync_directory(path: &Path) -> Result<(), SnapshotError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| SnapshotError::new(SnapshotOperation::Sync, path, error))
}

fn invalid(path: impl Into<PathBuf>, message: &'static str) -> SnapshotError {
    SnapshotError::new(
        SnapshotOperation::Validate,
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    )
}
