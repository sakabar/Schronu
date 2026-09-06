use super::error::{SnapshotError, SnapshotOperation};
#[cfg(test)]
use super::io::FailOnceSnapshotIo;
use super::io::{
    finalize_publication, DirectoryTree, FileSystemSnapshotIo, SnapshotFailurePoint, SnapshotIo,
    StableDirectory, StableParent,
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
        None,
        || {},
        || {},
    )
}

pub(crate) fn restore_snapshot_to_alternate(
    snapshot_directory: &Path,
    destination: &Path,
    current_storage_directory: &Path,
) -> Result<SnapshotSummary, SnapshotError> {
    let current_destination = CurrentDestination::from_path(current_storage_directory)?;
    restore_snapshot_impl(
        snapshot_directory,
        destination,
        &FileSystemSnapshotIo,
        Some(&current_destination),
        || {},
        || {},
    )
}

struct CurrentDestination {
    parent: PathBuf,
    name: OsString,
}

impl CurrentDestination {
    fn from_path(path: &Path) -> Result<Self, SnapshotError> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        let parent = parent.unwrap_or_else(|| Path::new("."));
        let name = path
            .file_name()
            .ok_or_else(|| invalid(path, "restore path must have a file name"))?
            .to_os_string();
        let parent = fs::canonicalize(parent)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, parent, error))?;
        Ok(Self { parent, name })
    }
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
        None,
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
        None,
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
    restore_snapshot_impl(snapshot, destination, &io, None, || {}, || {})
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn restore_snapshot_with_failure_observation(
    snapshot: &Path,
    destination: &Path,
    point: SnapshotFailurePoint,
) -> (Result<SnapshotSummary, SnapshotError>, usize) {
    let io = FailOnceSnapshotIo::new(point);
    let result = restore_snapshot_impl(snapshot, destination, &io, None, || {}, || {});
    (result, io.matching_calls())
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn restore_snapshot_to_alternate_after_parent_open(
    snapshot: &Path,
    destination: &Path,
    current_storage_directory: &Path,
    after_parent_open: impl FnOnce(),
) -> Result<SnapshotSummary, SnapshotError> {
    let current_destination = CurrentDestination::from_path(current_storage_directory)?;
    restore_snapshot_impl(
        snapshot,
        destination,
        &FileSystemSnapshotIo,
        Some(&current_destination),
        after_parent_open,
        || {},
    )
}

fn restore_snapshot_impl(
    snapshot: &Path,
    destination: &Path,
    io: &dyn SnapshotIo,
    current_destination: Option<&CurrentDestination>,
    after_parent_open: impl FnOnce(),
    before_publish: impl FnOnce(),
) -> Result<SnapshotSummary, SnapshotError> {
    ensure_path_not_current(destination, current_destination)?;
    let publication = validate_destination(snapshot, destination)?;
    after_parent_open();
    ensure_not_current(&publication, destination, current_destination)?;
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
    let result = materialize_restore(
        &staging_publication,
        &verified.tree,
        io,
        current_destination,
        before_publish,
    );
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

fn ensure_path_not_current(
    destination: &Path,
    current_destination: Option<&CurrentDestination>,
) -> Result<(), SnapshotError> {
    let Some(current_destination) = current_destination else {
        return Ok(());
    };
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
        .ok_or_else(|| invalid(destination, "restore destination must have a file name"))?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, parent, error))?;
    if canonical_parent == current_destination.parent
        && destination_name == current_destination.name.as_os_str()
    {
        Err(invalid(
            destination,
            "ordinary restore destination must differ from current storage",
        ))
    } else {
        Ok(())
    }
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

fn ensure_not_current(
    publication: &PublicationDestination,
    destination: &Path,
    current_destination: Option<&CurrentDestination>,
) -> Result<(), SnapshotError> {
    let Some(current_destination) = current_destination else {
        return Ok(());
    };
    let same_parent = publication
        .parent
        .matches_path(&current_destination.parent)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, destination, error))?;
    if same_parent && publication.destination_name == current_destination.name {
        Err(invalid(
            destination,
            "ordinary restore destination must differ from current storage",
        ))
    } else {
        Ok(())
    }
}

fn materialize_restore(
    staging: &RestoreStaging<'_>,
    tree: &DirectoryTree,
    io: &dyn SnapshotIo,
    current_destination: Option<&CurrentDestination>,
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
    ensure_not_current(staging.target, staging.destination, current_destination)?;
    ensure_parent_outside_snapshot(staging.target, staging.destination)?;
    finalize_publication(
        &staging.target.parent,
        staging.name,
        &staging.target.destination_name,
        staging.directory,
        staging.destination,
        io,
    )
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
