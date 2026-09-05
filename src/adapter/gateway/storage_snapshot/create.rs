use super::error::{SnapshotError, SnapshotOperation};
use super::io::{rename_no_replace, FileSystemSnapshotIo, SnapshotIo};
use super::layout::{is_reserved_path, MANIFEST_FILE_NAME, PAYLOAD_DIRECTORY_NAME};
use super::manifest::{
    decode_manifest, encode_manifest, DigestDescriptor, DirectoryEntry, FileEntry,
    SnapshotManifest, DIGEST_VERSION, FORMAT_VERSION,
};
use super::SnapshotSummary;
use crate::adapter::gateway::storage_content_integrity::{content_digest, DIGEST_ALGORITHM};
use crate::adapter::gateway::storage_lock::{LockMode, StorageLock};
use chrono::{DateTime, Local};
use std::fs::{self, File};
use std::io::Write;
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
    validate_endpoints(storage_directory, destination)?;
    let _lock = StorageLock::acquire(storage_directory, LockMode::Backup).map_err(|error| {
        let path = error.path().to_path_buf();
        SnapshotError::new(SnapshotOperation::AcquireLock, path, error)
    })?;

    let collected = collect_storage(storage_directory)?;
    let revision = read_revision(&collected.files)?;
    let staging = staging_path(destination)?;
    let result = publish_snapshot(&staging, destination, created_at, revision, &collected);
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result.map(|()| SnapshotSummary::new(revision, collected.files.len()))
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

fn validate_endpoints(storage: &Path, destination: &Path) -> Result<(), SnapshotError> {
    let storage_metadata = fs::symlink_metadata(storage)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, storage, error))?;
    if storage_metadata.file_type().is_symlink() || !storage_metadata.is_dir() {
        return Err(invalid(
            storage,
            "snapshot source must be a non-symlink directory",
        ));
    }
    if destination.exists() {
        return Err(invalid(destination, "snapshot destination must not exist"));
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
    let canonical_storage = fs::canonicalize(storage)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, storage, error))?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, parent, error))?;
    if canonical_parent.starts_with(&canonical_storage) {
        return Err(invalid(
            destination,
            "snapshot destination must be outside the source storage",
        ));
    }
    Ok(())
}

fn collect_storage(storage: &Path) -> Result<CollectedStorage, SnapshotError> {
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

fn staging_path(destination: &Path) -> Result<PathBuf, SnapshotError> {
    let parent = destination
        .parent()
        .expect("validated destination has a parent directory");
    let name = destination
        .file_name()
        .ok_or_else(|| invalid(destination, "snapshot destination must have a file name"))?;
    Ok(parent.join(format!(
        ".{}.tmp-{}",
        name.to_string_lossy(),
        Uuid::new_v4().hyphenated()
    )))
}

fn publish_snapshot(
    staging: &Path,
    destination: &Path,
    created_at: DateTime<Local>,
    revision: Option<Uuid>,
    collected: &CollectedStorage,
) -> Result<(), SnapshotError> {
    fs::create_dir(staging)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Create, staging, error))?;
    let payload = staging.join(PAYLOAD_DIRECTORY_NAME);
    fs::create_dir(&payload)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Create, &payload, error))?;

    for directory in &collected.directories {
        let path = payload.join(&directory.path);
        fs::create_dir(&path)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Create, &path, error))?;
    }
    for file in &collected.files {
        let path = payload.join(&file.path);
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
    for directory in collected.directories.iter().rev() {
        let path = payload.join(&directory.path);
        fs::set_permissions(&path, directory.permissions.clone())
            .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &path, error))?;
        sync_directory(&path)?;
    }
    sync_directory(&payload)?;

    let manifest = build_manifest(created_at, revision, collected);
    let manifest_bytes = encode_manifest(&manifest)?;
    let manifest_path = staging.join(MANIFEST_FILE_NAME);
    decode_manifest(&manifest_path, &manifest_bytes)?;
    let mut manifest_file = File::options()
        .write(true)
        .create_new(true)
        .open(&manifest_path)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &manifest_path, error))?;
    manifest_file
        .write_all(&manifest_bytes)
        .and_then(|()| manifest_file.sync_all())
        .map_err(|error| SnapshotError::new(SnapshotOperation::Sync, &manifest_path, error))?;
    sync_directory(staging)?;
    rename_no_replace(staging, destination)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Write, destination, error))?;
    finalize_publication(&FileSystemSnapshotIo, destination)
}

pub(in crate::adapter::gateway) fn finalize_publication(
    io: &dyn SnapshotIo,
    destination: &Path,
) -> Result<(), SnapshotError> {
    let parent = destination.parent().expect("destination has a parent");
    if let Err(sync_error) = io.sync_directory(parent) {
        io.remove_dir_all(destination).map_err(|cleanup_error| {
            SnapshotError::new(SnapshotOperation::Write, destination, cleanup_error)
        })?;
        let _ = io.sync_directory(parent);
        return Err(SnapshotError::new(
            SnapshotOperation::Sync,
            parent,
            sync_error,
        ));
    }
    Ok(())
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
