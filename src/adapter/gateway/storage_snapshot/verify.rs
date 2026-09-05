use super::error::{SnapshotError, SnapshotOperation};
use super::layout::{MANIFEST_FILE_NAME, PAYLOAD_DIRECTORY_NAME};
use super::manifest::{decode_manifest, SnapshotManifest};
use super::SnapshotSummary;
use crate::adapter::gateway::storage_content_integrity::content_matches;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn verify_snapshot(
    snapshot_directory: impl AsRef<Path>,
) -> Result<SnapshotSummary, SnapshotError> {
    let snapshot = snapshot_directory.as_ref();
    require_directory(snapshot)?;
    validate_snapshot_root(snapshot)?;
    let manifest_path = snapshot.join(MANIFEST_FILE_NAME);
    require_regular_file(&manifest_path)?;
    let bytes = fs::read(&manifest_path)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Read, &manifest_path, error))?;
    let manifest = decode_manifest(&manifest_path, &bytes)?;
    let payload = snapshot.join(PAYLOAD_DIRECTORY_NAME);
    require_directory(&payload)?;
    verify_payload(&payload, &manifest)?;
    Ok(SnapshotSummary::new(
        manifest.revision,
        manifest.files.len(),
    ))
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

fn verify_payload(payload: &Path, manifest: &SnapshotManifest) -> Result<(), SnapshotError> {
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
            let bytes = fs::read(entry.path()).map_err(|error| {
                SnapshotError::new(SnapshotOperation::Read, entry.path(), error)
            })?;
            if !content_matches(&bytes, expected.content_length, &expected.content_digest) {
                return Err(invalid(
                    entry.path(),
                    "snapshot file content does not match manifest",
                ));
            }
            verify_mode(entry.path(), &metadata.permissions(), expected.mode)?;
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
    Ok(())
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

fn require_regular_file(path: &Path) -> Result<(), SnapshotError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Validate, path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid(
            path,
            "snapshot path must be a non-symlink regular file",
        ));
    }
    Ok(())
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
