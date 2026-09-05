use super::error::{SnapshotError, SnapshotOperation};
#[cfg(test)]
use super::io::FailOnceSnapshotIo;
use super::io::{
    DirectoryTree, FileSystemSnapshotIo, SnapshotFailurePoint, SnapshotIo, StableDirectory,
    StableParent,
};
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
    restore_snapshot_impl(
        snapshot_directory.as_ref(),
        destination.as_ref(),
        &FileSystemSnapshotIo,
        || {},
        || {},
    )
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn restore_snapshot_after_parent_open(
    snapshot: &Path,
    destination: &Path,
    after_parent_open: impl FnOnce(),
) -> Result<SnapshotSummary, SnapshotError> {
    restore_snapshot_impl(
        snapshot,
        destination,
        &FileSystemSnapshotIo,
        after_parent_open,
        || {},
    )
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn restore_snapshot_before_publish(
    snapshot: &Path,
    destination: &Path,
    before_publish: impl FnOnce(),
) -> Result<SnapshotSummary, SnapshotError> {
    restore_snapshot_impl(
        snapshot,
        destination,
        &FileSystemSnapshotIo,
        || {},
        before_publish,
    )
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn restore_snapshot_with_failure(
    snapshot: &Path,
    destination: &Path,
    point: SnapshotFailurePoint,
) -> Result<SnapshotSummary, SnapshotError> {
    let io = FailOnceSnapshotIo::new(point);
    restore_snapshot_impl(snapshot, destination, &io, || {}, || {})
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn restore_snapshot_with_failure_observation(
    snapshot: &Path,
    destination: &Path,
    point: SnapshotFailurePoint,
) -> (Result<SnapshotSummary, SnapshotError>, usize) {
    let io = FailOnceSnapshotIo::new(point);
    let result = restore_snapshot_impl(snapshot, destination, &io, || {}, || {});
    (result, io.matching_calls())
}

fn restore_snapshot_impl(
    snapshot: &Path,
    destination: &Path,
    io: &dyn SnapshotIo,
    after_parent_open: impl FnOnce(),
    before_publish: impl FnOnce(),
) -> Result<SnapshotSummary, SnapshotError> {
    let publication = validate_destination(snapshot, destination)?;
    after_parent_open();
    ensure_parent_outside_snapshot(&publication, destination)?;
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
    let staging_publication = RestoreStaging {
        path: &staging,
        name: &staging_name,
        directory: &staging_directory,
        destination,
        target: &publication,
    };
    let result = materialize_restore(&staging_publication, &verified.tree, io, before_publish);
    if let Err(primary) = result {
        return match publication
            .parent
            .remove_published_directory_if_present(&staging_name, &staging_directory)
        {
            Ok(()) => Err(primary),
            Err(cleanup) => Err(SnapshotError::followup_failure(primary, "cleanup", cleanup)),
        };
    }
    Ok(SnapshotSummary::new(
        verified.manifest.revision,
        verified.manifest.files.len(),
    ))
}

struct PublicationDestination {
    parent: StableParent,
    destination_name: OsString,
    snapshot_root: PathBuf,
}

struct RestoreStaging<'a> {
    path: &'a Path,
    name: &'a OsStr,
    directory: &'a StableDirectory,
    destination: &'a Path,
    target: &'a PublicationDestination,
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
    let publication = PublicationDestination {
        parent: stable_parent,
        destination_name,
        snapshot_root: canonical_snapshot,
    };
    ensure_parent_outside_snapshot(&publication, destination)?;
    Ok(publication)
}

fn ensure_parent_outside_snapshot(
    publication: &PublicationDestination,
    destination: &Path,
) -> Result<(), SnapshotError> {
    if publication
        .parent
        .is_within(&publication.snapshot_root)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, destination, error))?
    {
        Err(invalid(
            destination,
            "restore destination must be outside the snapshot",
        ))
    } else {
        Ok(())
    }
}

fn materialize_restore(
    staging: &RestoreStaging<'_>,
    tree: &DirectoryTree,
    io: &dyn SnapshotIo,
    before_publish: impl FnOnce(),
) -> Result<(), SnapshotError> {
    for directory in tree
        .directories
        .iter()
        .filter(|entry| entry.path != Path::new(PAYLOAD_DIRECTORY_NAME))
    {
        let relative = payload_relative(&directory.path)?;
        let path = staging.path.join(relative);
        staging
            .directory
            .create_directory(relative)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Create, &path, error))?;
    }
    for file in tree
        .files
        .iter()
        .filter(|entry| entry.path != Path::new(MANIFEST_FILE_NAME))
    {
        let relative = payload_relative(&file.path)?;
        let path = staging.path.join(relative);
        io.before(SnapshotFailurePoint::Copy)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &path, error))?;
        staging
            .directory
            .write_file(relative, &file.bytes, file.permissions.clone(), io)
            .map_err(|error| SnapshotError::file_write(&path, error))?;
    }
    for directory in tree
        .directories
        .iter()
        .filter(|entry| entry.path != Path::new(PAYLOAD_DIRECTORY_NAME))
        .rev()
    {
        let relative = payload_relative(&directory.path)?;
        let path = staging.path.join(relative);
        staging
            .directory
            .set_directory_permissions(relative, directory.permissions.clone())
            .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &path, error))?;
        staging
            .directory
            .sync_directory(relative, io)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Sync, &path, error))?;
    }
    staging
        .directory
        .sync(io)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Sync, staging.path, error))?;
    before_publish();
    ensure_parent_outside_snapshot(staging.target, staging.destination)?;
    staging
        .target
        .parent
        .rename_no_replace(
            staging.name,
            &staging.target.destination_name,
            staging.directory,
            io,
        )
        .map_err(|error| {
            SnapshotError::new(SnapshotOperation::Write, staging.destination, error)
        })?;
    if let Err(sync_error) = staging.target.parent.sync(io) {
        let primary = SnapshotError::new(
            SnapshotOperation::Sync,
            staging
                .destination
                .parent()
                .expect("destination has a parent"),
            sync_error,
        );
        if let Err(cleanup_error) = staging
            .target
            .parent
            .remove_published_directory(&staging.target.destination_name, staging.directory)
        {
            return Err(SnapshotError::followup_failure(
                primary,
                "cleanup",
                cleanup_error,
            ));
        }
        if let Err(sync_error) = staging.target.parent.sync(io) {
            return Err(SnapshotError::followup_failure(
                primary,
                "rollback parent sync",
                sync_error,
            ));
        }
        return Err(primary);
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
