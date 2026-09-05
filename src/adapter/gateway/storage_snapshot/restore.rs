use super::error::{SnapshotError, SnapshotOperation};
use super::io::{DirectoryTree, StableDirectory, StableParent};
use super::layout::{staging_path, MANIFEST_FILE_NAME, PAYLOAD_DIRECTORY_NAME};
use super::verify::load_verified_snapshot;
use super::SnapshotSummary;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

pub fn restore_snapshot(
    snapshot_directory: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<SnapshotSummary, SnapshotError> {
    restore_snapshot_impl(snapshot_directory.as_ref(), destination.as_ref(), || {})
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn restore_snapshot_after_parent_open(
    snapshot: &Path,
    destination: &Path,
    after_parent_open: impl FnOnce(),
) -> Result<SnapshotSummary, SnapshotError> {
    restore_snapshot_impl(snapshot, destination, after_parent_open)
}

fn restore_snapshot_impl(
    snapshot: &Path,
    destination: &Path,
    after_parent_open: impl FnOnce(),
) -> Result<SnapshotSummary, SnapshotError> {
    let publication = validate_destination(snapshot, destination)?;
    after_parent_open();
    let verified = load_verified_snapshot(snapshot)?;
    let staging = staging_path(destination)?;
    let staging_name = staging
        .file_name()
        .expect("staging path has a file name")
        .to_os_string();
    let staging_directory = publication
        .parent
        .create_directory(&staging_name)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Create, &staging, error))?;
    let result = materialize_restore(
        &staging,
        &staging_name,
        &staging_directory,
        destination,
        &publication,
        &verified.tree,
    );
    if result.is_err()
        && publication
            .parent
            .entry_exists(&staging_name)
            .unwrap_or(false)
    {
        let _ = publication.parent.remove_directory_tree(&staging_name);
    }
    result.map(|()| SnapshotSummary::new(verified.manifest.revision, verified.manifest.files.len()))
}

struct PublicationDestination {
    parent: StableParent,
    destination_name: OsString,
}

fn validate_destination(
    snapshot: &Path,
    destination: &Path,
) -> Result<PublicationDestination, SnapshotError> {
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            invalid(
                destination,
                "restore destination must have a parent directory",
            )
        })?;
    let destination_name = destination
        .file_name()
        .ok_or_else(|| invalid(destination, "restore destination must have a file name"))?
        .to_os_string();
    let stable_parent = StableParent::open(parent)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, parent, error))?;
    if stable_parent
        .entry_exists(&destination_name)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, destination, error))?
    {
        return Err(invalid(destination, "restore destination must not exist"));
    }
    let canonical_snapshot = fs::canonicalize(snapshot)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, snapshot, error))?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, parent, error))?;
    if !stable_parent
        .matches_path(&canonical_parent)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, parent, error))?
    {
        return Err(invalid(
            destination,
            "restore destination parent changed during validation",
        ));
    }
    if canonical_parent.starts_with(&canonical_snapshot) {
        return Err(invalid(
            destination,
            "restore destination must be outside the snapshot",
        ));
    }
    Ok(PublicationDestination {
        parent: stable_parent,
        destination_name,
    })
}

fn materialize_restore(
    staging: &Path,
    staging_name: &OsStr,
    staging_directory: &StableDirectory,
    destination: &Path,
    publication: &PublicationDestination,
    tree: &DirectoryTree,
) -> Result<(), SnapshotError> {
    for directory in tree
        .directories
        .iter()
        .filter(|entry| entry.path != Path::new(PAYLOAD_DIRECTORY_NAME))
    {
        let relative = payload_relative(&directory.path)?;
        let path = staging.join(relative);
        staging_directory
            .create_directory(relative)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Create, &path, error))?;
    }
    for file in tree
        .files
        .iter()
        .filter(|entry| entry.path != Path::new(MANIFEST_FILE_NAME))
    {
        let relative = payload_relative(&file.path)?;
        let path = staging.join(relative);
        staging_directory
            .write_file(relative, &file.bytes, file.permissions.clone())
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
        staging_directory
            .set_directory_permissions(relative, directory.permissions.clone())
            .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &path, error))?;
        staging_directory
            .sync_directory(relative)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Sync, &path, error))?;
    }
    staging_directory
        .sync()
        .map_err(|error| SnapshotError::new(SnapshotOperation::Sync, staging, error))?;
    publication
        .parent
        .rename_no_replace(staging_name, &publication.destination_name)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Write, destination, error))?;
    if let Err(sync_error) = publication.parent.sync() {
        publication
            .parent
            .remove_directory_tree(&publication.destination_name)
            .map_err(|cleanup_error| {
                SnapshotError::new(SnapshotOperation::Write, destination, cleanup_error)
            })?;
        let _ = publication.parent.sync();
        return Err(SnapshotError::new(
            SnapshotOperation::Sync,
            destination.parent().expect("destination has a parent"),
            sync_error,
        ));
    }
    Ok(())
}

fn payload_relative(path: &Path) -> Result<&Path, SnapshotError> {
    path.strip_prefix(PAYLOAD_DIRECTORY_NAME)
        .map_err(|_| invalid(path, "snapshot entry is outside storage payload"))
}

fn invalid(path: impl Into<PathBuf>, message: &'static str) -> SnapshotError {
    SnapshotError::new(
        SnapshotOperation::Validate,
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    )
}
