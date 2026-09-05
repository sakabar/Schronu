use super::error::{SnapshotError, SnapshotOperation};
use super::layout::{MANIFEST_FILE_NAME, PAYLOAD_DIRECTORY_NAME};
use super::manifest::{decode_manifest, SnapshotManifest};
use super::SnapshotSummary;
use crate::adapter::gateway::storage_content_integrity::content_matches;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn verify_snapshot(
    snapshot_directory: impl AsRef<Path>,
) -> Result<SnapshotSummary, SnapshotError> {
    let snapshot = snapshot_directory.as_ref();
    require_directory(snapshot)?;
    validate_snapshot_root(snapshot)?;
    let manifest_path = snapshot.join(MANIFEST_FILE_NAME);
    let (bytes, _) = read_regular_file(&manifest_path)?;
    let manifest = decode_manifest(&manifest_path, &bytes)?;
    let payload = snapshot.join(PAYLOAD_DIRECTORY_NAME);
    require_directory(&payload)?;
    let revision_bytes = verify_payload(&payload, &manifest)?;
    verify_revision(&payload, &manifest, revision_bytes.as_deref())?;
    Ok(SnapshotSummary::new(
        manifest.revision,
        manifest.files.len(),
    ))
}

fn verify_revision(
    payload: &Path,
    manifest: &SnapshotManifest,
    revision_bytes: Option<&[u8]>,
) -> Result<(), SnapshotError> {
    let revision_path = payload.join(".revision");
    match manifest.revision {
        None if !manifest
            .files
            .iter()
            .any(|file| file.path == Path::new(".revision")) =>
        {
            Ok(())
        }
        Some(expected) => {
            let bytes = revision_bytes
                .ok_or_else(|| invalid(&revision_path, "snapshot payload is missing .revision"))?;
            let text = std::str::from_utf8(bytes).map_err(|error| {
                SnapshotError::new(SnapshotOperation::Validate, &revision_path, error)
            })?;
            let actual = uuid::Uuid::parse_str(text.trim()).map_err(|error| {
                SnapshotError::new(SnapshotOperation::Validate, &revision_path, error)
            })?;
            if actual != expected {
                return Err(invalid(
                    revision_path,
                    "snapshot manifest revision does not match payload",
                ));
            }
            Ok(())
        }
        None => Err(invalid(
            revision_path,
            "snapshot without a revision must not contain .revision",
        )),
    }
}

fn validate_snapshot_root(snapshot: &Path) -> Result<(), SnapshotError> {
    let entries = fs::read_dir(snapshot)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Read, snapshot, error))?;
    let mut names = HashSet::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| SnapshotError::new(SnapshotOperation::Read, snapshot, error))?;
        names.insert(entry.file_name());
    }
    let expected = [MANIFEST_FILE_NAME, PAYLOAD_DIRECTORY_NAME]
        .into_iter()
        .map(std::ffi::OsString::from)
        .collect::<HashSet<_>>();
    if names != expected {
        return Err(invalid(
            snapshot,
            "snapshot root must contain only manifest.json and storage",
        ));
    }
    Ok(())
}

fn verify_payload(
    payload: &Path,
    manifest: &SnapshotManifest,
) -> Result<Option<Vec<u8>>, SnapshotError> {
    let expected_directories = manifest
        .directories
        .iter()
        .map(|entry| (entry.path.clone(), entry.mode))
        .collect::<HashMap<_, _>>();
    let expected_files = manifest
        .files
        .iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect::<HashMap<_, _>>();
    let mut actual_directories = HashSet::new();
    let mut actual_files = HashSet::new();
    let mut revision_bytes = None;

    for entry in WalkDir::new(payload)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.map_err(|error| {
            let path = error.path().unwrap_or(payload).to_path_buf();
            SnapshotError::new(SnapshotOperation::Read, path, std::io::Error::other(error))
        })?;
        if entry.depth() == 0 {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(payload)
            .expect("WalkDir entries remain below payload root")
            .to_path_buf();
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| SnapshotError::new(SnapshotOperation::Read, entry.path(), error))?;
        if metadata.file_type().is_symlink() {
            return Err(invalid(
                entry.path(),
                "snapshot payload must not contain symlinks",
            ));
        }
        if metadata.is_dir() {
            let Some(expected_mode) = expected_directories.get(&relative) else {
                return Err(invalid(
                    entry.path(),
                    "snapshot payload contains an extra directory",
                ));
            };
            verify_mode(entry.path(), &metadata.permissions(), *expected_mode)?;
            actual_directories.insert(relative);
        } else if metadata.is_file() {
            let Some(expected) = expected_files.get(&relative) else {
                return Err(invalid(
                    entry.path(),
                    "snapshot payload contains an extra file",
                ));
            };
            let (bytes, permissions) = read_regular_file(entry.path())?;
            if !content_matches(&bytes, expected.content_length, &expected.content_digest) {
                return Err(invalid(
                    entry.path(),
                    "snapshot file content does not match manifest",
                ));
            }
            verify_mode(entry.path(), &permissions, expected.mode)?;
            if relative == Path::new(".revision") {
                revision_bytes = Some(bytes);
            }
            actual_files.insert(relative);
        } else {
            return Err(invalid(
                entry.path(),
                "snapshot payload entries must be regular files or directories",
            ));
        }
    }

    if actual_directories.len() != expected_directories.len() {
        return Err(invalid(payload, "snapshot payload is missing a directory"));
    }
    if actual_files.len() != expected_files.len() {
        return Err(invalid(payload, "snapshot payload is missing a file"));
    }
    Ok(revision_bytes)
}

fn require_directory(path: &Path) -> Result<(), SnapshotError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid(
            path,
            "snapshot path must be a non-symlink directory",
        ));
    }
    Ok(())
}

fn read_regular_file(path: &Path) -> Result<(Vec<u8>, fs::Permissions), SnapshotError> {
    let mut file = open_no_follow(path)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Read, path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| SnapshotError::new(SnapshotOperation::Read, path, error))?;
    if !metadata.is_file() {
        return Err(invalid(path, "snapshot path must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Read, path, error))?;
    Ok((bytes, metadata.permissions()))
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> std::io::Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "snapshot file must not be a symbolic link",
        ));
    }
    File::open(path)
}

#[cfg(unix)]
fn verify_mode(
    path: &Path,
    permissions: &fs::Permissions,
    expected: Option<u32>,
) -> Result<(), SnapshotError> {
    use std::os::unix::fs::PermissionsExt;
    if expected.is_some_and(|mode| permissions.mode() & 0o7777 != mode) {
        return Err(invalid(
            path,
            "snapshot entry permission does not match manifest",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_mode(
    _path: &Path,
    _permissions: &fs::Permissions,
    _expected: Option<u32>,
) -> Result<(), SnapshotError> {
    Ok(())
}

fn invalid(path: impl Into<PathBuf>, message: &'static str) -> SnapshotError {
    SnapshotError::new(
        SnapshotOperation::Validate,
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    )
}
