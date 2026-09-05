use super::error::{SnapshotError, SnapshotOperation};
#[cfg(test)]
use super::io::{FailOnceSnapshotIo, SnapshotFailurePoint};
use super::io::{FileSystemSnapshotIo, SnapshotIo, StableDirectory, StableParent};
use super::layout::{staging_path, MANIFEST_FILE_NAME, PAYLOAD_DIRECTORY_NAME};
use super::manifest::{
    decode_manifest, encode_manifest_with_limits, DigestDescriptor, DirectoryEntry, FileEntry,
    SnapshotManifest, DIGEST_VERSION, FORMAT_VERSION,
};
use super::{SnapshotResourceLimits, SnapshotSummary, DEFAULT_RESOURCE_LIMITS};
use crate::adapter::gateway::storage_content_integrity::{content_digest, DIGEST_ALGORITHM};
use crate::adapter::gateway::storage_lock::{LockMode, StorageLock};
use crate::adapter::gateway::storage_transaction::{recover, FileSystemStorageTransactionIo};
use crate::adapter::gateway::task_repository::TaskRepository;
use chrono::{DateTime, Local};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

mod capture;

use capture::{scan_storage_entries, validate_capture_unchanged, ScannedStorage};

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
    create_snapshot_with_limits(
        storage_directory,
        destination,
        created_at,
        DEFAULT_RESOURCE_LIMITS,
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
        DEFAULT_RESOURCE_LIMITS,
        &FileSystemSnapshotIo,
        CreateHooks::new(after_parent_open, || {}, || {}, || {}),
    )
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn create_snapshot_after_capture(
    storage_directory: &Path,
    destination: &Path,
    created_at: DateTime<Local>,
    after_capture: impl FnOnce(),
) -> Result<SnapshotSummary, SnapshotError> {
    create_snapshot_impl(
        storage_directory,
        destination,
        created_at,
        DEFAULT_RESOURCE_LIMITS,
        &FileSystemSnapshotIo,
        CreateHooks::new(|| {}, after_capture, || {}, || {}),
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
        DEFAULT_RESOURCE_LIMITS,
        &FileSystemSnapshotIo,
        CreateHooks::new(|| {}, || {}, || {}, before_publish),
    )
}

#[cfg(test)]
pub(in crate::adapter::gateway) fn create_snapshot_before_strict_load(
    storage_directory: &Path,
    destination: &Path,
    created_at: DateTime<Local>,
    before_strict_load: impl FnOnce(),
) -> Result<SnapshotSummary, SnapshotError> {
    create_snapshot_impl(
        storage_directory,
        destination,
        created_at,
        DEFAULT_RESOURCE_LIMITS,
        &FileSystemSnapshotIo,
        CreateHooks::new(|| {}, || {}, before_strict_load, || {}),
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
        DEFAULT_RESOURCE_LIMITS,
        &io,
        CreateHooks::new(|| {}, || {}, || {}, || {}),
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
        DEFAULT_RESOURCE_LIMITS,
        &io,
        CreateHooks::new(|| {}, || {}, || {}, || {}),
    );
    (result, io.matching_calls())
}

pub(in crate::adapter::gateway) fn create_snapshot_with_limits(
    storage_directory: &Path,
    destination: &Path,
    created_at: DateTime<Local>,
    limits: SnapshotResourceLimits,
) -> Result<SnapshotSummary, SnapshotError> {
    create_snapshot_impl(
        storage_directory,
        destination,
        created_at,
        limits,
        &FileSystemSnapshotIo,
        CreateHooks::new(|| {}, || {}, || {}, || {}),
    )
}

fn create_snapshot_impl<AfterParent, AfterCapture, BeforeStrict, BeforePublish>(
    storage_directory: &Path,
    destination: &Path,
    created_at: DateTime<Local>,
    limits: SnapshotResourceLimits,
    io: &dyn SnapshotIo,
    hooks: CreateHooks<AfterParent, AfterCapture, BeforeStrict, BeforePublish>,
) -> Result<SnapshotSummary, SnapshotError>
where
    AfterParent: FnOnce(),
    AfterCapture: FnOnce(),
    BeforeStrict: FnOnce(),
    BeforePublish: FnOnce(),
{
    let publication = validate_endpoints(storage_directory, destination)?;
    ensure_parent_outside_storage(&publication, destination)?;
    let _lock = StorageLock::acquire(storage_directory, LockMode::Backup).map_err(|error| {
        let path = error.path().to_path_buf();
        SnapshotError::new(SnapshotOperation::AcquireLock, path, error)
    })?;

    recover_storage(storage_directory)?;
    let scanned = scan_storage_entries(storage_directory, limits, io)?;
    (hooks.after_capture)();
    validate_capture_unchanged(storage_directory, &scanned)?;
    let collected = collect_storage(scanned);
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
    let result = (|| {
        write_payload(&staging_publication, &collected, io)?;
        (hooks.before_strict_load)();
        strict_load_captured(&collected, storage_directory)?;
        (hooks.after_parent_open)();
        publish_manifest(
            &staging_publication,
            created_at,
            revision,
            &collected,
            limits,
            io,
            hooks.before_publish,
        )
    })();
    if let Err(primary) = result {
        return match publication
            .parent
            .remove_published_directory_if_present(&staging_name, &staging_directory)
        {
            Ok(()) => Err(primary),
            Err(cleanup) => Err(SnapshotError::followup_failure(primary, "cleanup", cleanup)),
        };
    }
    Ok(SnapshotSummary::new(revision, collected.files.len()))
}

struct CreateHooks<AfterParent, AfterCapture, BeforeStrict, BeforePublish> {
    after_parent_open: AfterParent,
    after_capture: AfterCapture,
    before_strict_load: BeforeStrict,
    before_publish: BeforePublish,
}

impl<AfterParent, AfterCapture, BeforeStrict, BeforePublish>
    CreateHooks<AfterParent, AfterCapture, BeforeStrict, BeforePublish>
{
    fn new(
        after_parent_open: AfterParent,
        after_capture: AfterCapture,
        before_strict_load: BeforeStrict,
        before_publish: BeforePublish,
    ) -> Self {
        Self {
            after_parent_open,
            after_capture,
            before_strict_load,
            before_publish,
        }
    }
}

fn strict_load_captured(
    collected: &CollectedStorage,
    source_storage: &Path,
) -> Result<(), SnapshotError> {
    let revision_file = collected
        .files
        .iter()
        .find(|file| file.path == Path::new(".revision"));
    let revision_path = revision_file.map(|file| source_storage.join(&file.path));
    let revision = revision_file
        .zip(revision_path.as_deref())
        .map(|(file, path)| (path, file.bytes.as_slice()));
    let project_files = collected
        .files
        .iter()
        .filter(|file| file.path.file_name() == Some(OsStr::new("project.yaml")))
        .map(|file| (source_storage.join(&file.path), file.bytes.as_slice()))
        .collect::<Vec<_>>();
    let mut repository = TaskRepository::new("");
    repository
        .load_captured(
            revision,
            project_files
                .iter()
                .map(|(path, bytes)| (path.as_path(), *bytes)),
        )
        .map_err(|error| {
            SnapshotError::new(SnapshotOperation::RepositoryLoad, source_storage, error)
        })
}

fn recover_storage(storage: &Path) -> Result<(), SnapshotError> {
    recover(Arc::new(FileSystemStorageTransactionIo), storage)
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

fn collect_storage(scanned: ScannedStorage) -> CollectedStorage {
    let directories = scanned
        .directories
        .iter()
        .map(|entry| CollectedDirectory {
            path: entry.relative.clone(),
            permissions: entry.metadata.permissions(),
        })
        .collect();
    let files = scanned
        .files
        .into_iter()
        .map(|entry| CollectedFile {
            path: entry.relative,
            bytes: entry.bytes,
            permissions: entry.metadata.permissions(),
        })
        .collect();
    CollectedStorage { directories, files }
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

fn write_payload(
    staging: &StagingPublication<'_>,
    collected: &CollectedStorage,
    io: &dyn SnapshotIo,
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
            .map_err(|error| SnapshotError::file_write(&path, error))?;
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
        .map_err(|error| SnapshotError::new(SnapshotOperation::Sync, &payload, error))
}

fn publish_manifest(
    staging: &StagingPublication<'_>,
    created_at: DateTime<Local>,
    revision: Option<Uuid>,
    collected: &CollectedStorage,
    limits: SnapshotResourceLimits,
    io: &dyn SnapshotIo,
    before_publish: impl FnOnce(),
) -> Result<(), SnapshotError> {
    let manifest = build_manifest(created_at, revision, collected);
    let manifest_path = staging.path.join(MANIFEST_FILE_NAME);
    let manifest_bytes = encode_manifest_with_limits(&manifest_path, &manifest, limits)?;
    decode_manifest(&manifest_path, &manifest_bytes)?;
    staging
        .directory
        .write_file(
            Path::new(MANIFEST_FILE_NAME),
            &manifest_bytes,
            manifest_permissions(),
            io,
        )
        .map_err(|error| SnapshotError::file_write(&manifest_path, error))?;
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
