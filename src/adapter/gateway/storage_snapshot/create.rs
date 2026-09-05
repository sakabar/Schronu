use super::error::{SnapshotError, SnapshotOperation};
#[cfg(test)]
use super::io::FailOnceSnapshotIo;
use super::io::{
    FileSystemSnapshotIo, SnapshotFailurePoint, SnapshotIo, StableDirectory, StableParent,
};
use super::layout::{is_reserved_path, staging_path, MANIFEST_FILE_NAME, PAYLOAD_DIRECTORY_NAME};
use super::manifest::{
    decode_manifest, encode_manifest, DigestDescriptor, DirectoryEntry, FileEntry,
    SnapshotManifest, DIGEST_VERSION, FORMAT_VERSION,
};
use super::SnapshotSummary;
use crate::adapter::gateway::storage_content_integrity::{content_digest, DIGEST_ALGORITHM};
use crate::adapter::gateway::storage_lock::{LockMode, StorageLock};
use crate::adapter::gateway::task_repository::TaskRepository;
use crate::application::interface::TaskRepositoryTrait;
use chrono::{DateTime, Local};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use walkdir::{DirEntry, WalkDir};

pub fn create_snapshot(
    storage_directory: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<SnapshotSummary, SnapshotError> {
    create_snapshot_at(
        storage_directory.as_ref(),
        destination.as_ref(),
        Local::now(),
    )
}

pub(in crate::adapter::gateway) fn create_snapshot_at(
    storage_directory: &Path,
    destination: &Path,
    created_at: DateTime<Local>,
) -> Result<SnapshotSummary, SnapshotError> {
    create_snapshot_impl(
        storage_directory,
        destination,
        created_at,
        &FileSystemSnapshotIo,
        || {},
        || {},
    )
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn create_snapshot_after_parent_open(
    storage_directory: &Path,
    destination: &Path,
    created_at: DateTime<Local>,
    after_parent_open: impl FnOnce(),
) -> Result<SnapshotSummary, SnapshotError> {
    create_snapshot_impl(
        storage_directory,
        destination,
        created_at,
        &FileSystemSnapshotIo,
        after_parent_open,
        || {},
    )
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn create_snapshot_before_publish(
    storage_directory: &Path,
    destination: &Path,
    created_at: DateTime<Local>,
    before_publish: impl FnOnce(),
) -> Result<SnapshotSummary, SnapshotError> {
    create_snapshot_impl(
        storage_directory,
        destination,
        created_at,
        &FileSystemSnapshotIo,
        || {},
        before_publish,
    )
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn create_snapshot_with_failure(
    storage_directory: &Path,
    destination: &Path,
    created_at: DateTime<Local>,
    point: SnapshotFailurePoint,
) -> Result<SnapshotSummary, SnapshotError> {
    let io = FailOnceSnapshotIo::new(point);
    create_snapshot_impl(
        storage_directory,
        destination,
        created_at,
        &io,
        || {},
        || {},
    )
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn create_snapshot_with_failure_observation(
    storage_directory: &Path,
    destination: &Path,
    created_at: DateTime<Local>,
    point: SnapshotFailurePoint,
) -> (Result<SnapshotSummary, SnapshotError>, usize) {
    let io = FailOnceSnapshotIo::new(point);
    let result = create_snapshot_impl(
        storage_directory,
        destination,
        created_at,
        &io,
        || {},
        || {},
    );
    (result, io.matching_calls())
}

fn create_snapshot_impl(
    storage_directory: &Path,
    destination: &Path,
    created_at: DateTime<Local>,
    io: &dyn SnapshotIo,
    after_parent_open: impl FnOnce(),
    before_publish: impl FnOnce(),
) -> Result<SnapshotSummary, SnapshotError> {
    let publication = validate_endpoints(storage_directory, destination)?;
    after_parent_open();
    ensure_parent_outside_storage(&publication, destination)?;
    let _lock = StorageLock::acquire(storage_directory, LockMode::Backup).map_err(|error| {
        let path = error.path().to_path_buf();
        SnapshotError::new(SnapshotOperation::AcquireLock, path, error)
    })?;

    strict_load(storage_directory)?;
    let collected = collect_storage(storage_directory, io)?;
    let revision = read_revision(&collected.files)?;
    let staging = staging_path(destination)?;
    let staging_name = staging
        .file_name()
        .expect("staging path has a file name")
        .to_os_string();
    let staging_directory = publication
        .parent
        .create_directory(&staging_name)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Create, &staging, error))?;
    let staging_publication = StagingPublication {
        path: &staging,
        name: &staging_name,
        directory: &staging_directory,
        destination,
        target: &publication,
    };
    let result = publish_snapshot(
        &staging_publication,
        created_at,
        revision,
        &collected,
        io,
        before_publish,
    );
    if result.is_err() {
        let _ = publication
            .parent
            .remove_published_directory(&staging_name, &staging_directory);
    }
    result.map(|()| SnapshotSummary::new(revision, collected.files.len()))
}

fn strict_load(storage: &Path) -> Result<(), SnapshotError> {
    let storage_text = storage.to_str().ok_or_else(|| {
        invalid(
            storage,
            "snapshot source path must be valid Unicode for repository validation",
        )
    })?;
    let mut repository = TaskRepository::new(storage_text);
    repository
        .load()
        .map_err(|error| SnapshotError::new(SnapshotOperation::RepositoryLoad, storage, error))
}

struct CollectedStorage {
    directories: Vec<CollectedDirectory>,
    files: Vec<CollectedFile>,
}

struct CollectedDirectory {
    path: PathBuf,
    permissions: fs::Permissions,
}

struct CollectedFile {
    path: PathBuf,
    bytes: Vec<u8>,
    permissions: fs::Permissions,
}

struct PublicationDestination {
    parent: StableParent,
    destination_name: OsString,
    storage: PathBuf,
}

fn validate_endpoints(
    storage: &Path,
    destination: &Path,
) -> Result<PublicationDestination, SnapshotError> {
    let storage_metadata = fs::symlink_metadata(storage)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, storage, error))?;
    if storage_metadata.file_type().is_symlink() || !storage_metadata.is_dir() {
        return Err(invalid(
            storage,
            "snapshot source must be a non-symlink directory",
        ));
    }
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            invalid(
                destination,
                "snapshot destination must have a parent directory",
            )
        })?;
    let destination_name = destination
        .file_name()
        .ok_or_else(|| invalid(destination, "snapshot destination must have a file name"))?
        .to_os_string();
    let stable_parent = StableParent::open(parent)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, parent, error))?;
    if stable_parent
        .entry_exists(&destination_name)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, destination, error))?
    {
        return Err(invalid(destination, "snapshot destination must not exist"));
    }
    let canonical_storage = fs::canonicalize(storage)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, storage, error))?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, parent, error))?;
    if !stable_parent
        .matches_path(&canonical_parent)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, parent, error))?
    {
        return Err(invalid(
            destination,
            "snapshot destination parent changed during validation",
        ));
    }
    if canonical_parent.starts_with(&canonical_storage) {
        return Err(invalid(
            destination,
            "snapshot destination must be outside the source storage",
        ));
    }
    Ok(PublicationDestination {
        parent: stable_parent,
        destination_name,
        storage: canonical_storage,
    })
}

fn ensure_parent_outside_storage(
    publication: &PublicationDestination,
    destination: &Path,
) -> Result<(), SnapshotError> {
    if publication
        .parent
        .is_within(&publication.storage)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, destination, error))?
    {
        Err(invalid(
            destination,
            "snapshot destination parent moved inside the source storage",
        ))
    } else {
        Ok(())
    }
}

fn collect_storage(storage: &Path, io: &dyn SnapshotIo) -> Result<CollectedStorage, SnapshotError> {
    let mut directories = Vec::new();
    let mut files = Vec::new();
    let walker = WalkDir::new(storage)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| should_descend(storage, entry));
    for entry in walker {
        let entry = entry.map_err(|error| {
            let path = error.path().unwrap_or(storage).to_path_buf();
            SnapshotError::new(SnapshotOperation::Read, path, std::io::Error::other(error))
        })?;
        if entry.depth() == 0 {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(storage)
            .expect("WalkDir entries remain below the storage root")
            .to_path_buf();
        if is_reserved_path(&relative) {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| SnapshotError::new(SnapshotOperation::Read, entry.path(), error))?;
        if metadata.file_type().is_symlink() {
            return Err(invalid(
                entry.path(),
                "snapshot source must not contain symlinks",
            ));
        }
        if metadata.is_dir() {
            directories.push(CollectedDirectory {
                path: relative,
                permissions: metadata.permissions(),
            });
        } else if metadata.is_file() {
            io.before(SnapshotFailurePoint::Read).map_err(|error| {
                SnapshotError::new(SnapshotOperation::Read, entry.path(), error)
            })?;
            let bytes = fs::read(entry.path()).map_err(|error| {
                SnapshotError::new(SnapshotOperation::Read, entry.path(), error)
            })?;
            files.push(CollectedFile {
                path: relative,
                bytes,
                permissions: metadata.permissions(),
            });
        } else {
            return Err(invalid(
                entry.path(),
                "snapshot source entries must be regular files or directories",
            ));
        }
    }
    Ok(CollectedStorage { directories, files })
}

fn should_descend(storage: &Path, entry: &DirEntry) -> bool {
    entry.depth() == 0
        || entry
            .path()
            .strip_prefix(storage)
            .map(|path| !is_reserved_path(path))
            .unwrap_or(false)
}

fn read_revision(files: &[CollectedFile]) -> Result<Option<Uuid>, SnapshotError> {
    let Some(file) = files
        .iter()
        .find(|file| file.path == Path::new(".revision"))
    else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&file.bytes)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, &file.path, error))?;
    Uuid::parse_str(text.trim())
        .map(Some)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, &file.path, error))
}

struct StagingPublication<'a> {
    path: &'a Path,
    name: &'a OsStr,
    directory: &'a StableDirectory,
    destination: &'a Path,
    target: &'a PublicationDestination,
}

fn publish_snapshot(
    staging: &StagingPublication<'_>,
    created_at: DateTime<Local>,
    revision: Option<Uuid>,
    collected: &CollectedStorage,
    io: &dyn SnapshotIo,
    before_publish: impl FnOnce(),
) -> Result<(), SnapshotError> {
    let payload = staging.path.join(PAYLOAD_DIRECTORY_NAME);
    staging
        .directory
        .create_directory(Path::new(PAYLOAD_DIRECTORY_NAME))
        .map_err(|error| SnapshotError::new(SnapshotOperation::Create, &payload, error))?;

    for directory in &collected.directories {
        let path = payload.join(&directory.path);
        staging
            .directory
            .create_directory(&Path::new(PAYLOAD_DIRECTORY_NAME).join(&directory.path))
            .map_err(|error| SnapshotError::new(SnapshotOperation::Create, &path, error))?;
    }
    for file in &collected.files {
        let path = payload.join(&file.path);
        staging
            .directory
            .write_file(
                &Path::new(PAYLOAD_DIRECTORY_NAME).join(&file.path),
                &file.bytes,
                file.permissions.clone(),
                io,
            )
            .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &path, error))?;
    }
    for directory in collected.directories.iter().rev() {
        let path = payload.join(&directory.path);
        let relative = Path::new(PAYLOAD_DIRECTORY_NAME).join(&directory.path);
        staging
            .directory
            .set_directory_permissions(&relative, directory.permissions.clone())
            .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &path, error))?;
        staging
            .directory
            .sync_directory(&relative, io)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Sync, &path, error))?;
    }
    staging
        .directory
        .sync_directory(Path::new(PAYLOAD_DIRECTORY_NAME), io)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Sync, &payload, error))?;

    let manifest = build_manifest(created_at, revision, collected);
    let manifest_bytes = encode_manifest(&manifest)?;
    let manifest_path = staging.path.join(MANIFEST_FILE_NAME);
    decode_manifest(&manifest_path, &manifest_bytes)?;
    staging
        .directory
        .write_file(
            Path::new(MANIFEST_FILE_NAME),
            &manifest_bytes,
            manifest_permissions(),
            io,
        )
        .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &manifest_path, error))?;
    staging
        .directory
        .sync(io)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Sync, staging.path, error))?;
    before_publish();
    ensure_parent_outside_storage(staging.target, staging.destination)?;
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
        staging
            .target
            .parent
            .remove_published_directory(&staging.target.destination_name, staging.directory)
            .map_err(|cleanup_error| {
                SnapshotError::new(SnapshotOperation::Write, staging.destination, cleanup_error)
            })?;
        let _ = staging.target.parent.sync(io);
        return Err(SnapshotError::new(
            SnapshotOperation::Sync,
            staging
                .destination
                .parent()
                .expect("destination has a parent"),
            sync_error,
        ));
    }
    Ok(())
}

fn manifest_permissions() -> fs::Permissions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::Permissions::from_mode(0o600)
    }
    #[cfg(not(unix))]
    {
        fs::metadata(".")
            .expect("current directory metadata is available")
            .permissions()
    }
}

fn build_manifest(
    created_at: DateTime<Local>,
    revision: Option<Uuid>,
    collected: &CollectedStorage,
) -> SnapshotManifest {
    SnapshotManifest {
        format_version: FORMAT_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: created_at.fixed_offset(),
        revision,
        digest: DigestDescriptor {
            algorithm: DIGEST_ALGORITHM.to_string(),
            version: DIGEST_VERSION,
        },
        directories: collected
            .directories
            .iter()
            .map(|directory| DirectoryEntry {
                path: directory.path.clone(),
                mode: permission_mode(&directory.permissions),
            })
            .collect(),
        files: collected
            .files
            .iter()
            .map(|file| FileEntry {
                path: file.path.clone(),
                mode: permission_mode(&file.permissions),
                content_length: file.bytes.len() as u64,
                content_digest: content_digest(&file.bytes),
            })
            .collect(),
    }
}

#[cfg(unix)]
fn permission_mode(permissions: &fs::Permissions) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(permissions.mode() & 0o7777)
}

#[cfg(not(unix))]
fn permission_mode(_permissions: &fs::Permissions) -> Option<u32> {
    None
}

fn invalid(path: impl Into<PathBuf>, message: &'static str) -> SnapshotError {
    SnapshotError::new(
        SnapshotOperation::Validate,
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    )
}
