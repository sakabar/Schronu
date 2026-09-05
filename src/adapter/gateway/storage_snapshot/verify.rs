use super::error::{SnapshotError, SnapshotOperation};
use super::io::{read_directory_tree, DirectoryTree, TreeDirectory, TreeFile};
use super::layout::{MANIFEST_FILE_NAME, PAYLOAD_DIRECTORY_NAME};
use super::manifest::{decode_manifest, SnapshotManifest};
use super::SnapshotSummary;
use crate::adapter::gateway::storage_content_integrity::content_matches;
use crate::adapter::gateway::task_repository::TaskRepository;
use crate::application::interface::TaskRepositoryTrait;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub fn verify_snapshot(
    snapshot_directory: impl AsRef<Path>,
) -> Result<SnapshotSummary, SnapshotError> {
    let snapshot = snapshot_directory.as_ref();
    let tree = read_directory_tree(snapshot)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Read, snapshot, error))?;
    validate_snapshot_root(snapshot, &tree)?;
    let manifest_file = tree
        .files
        .iter()
        .find(|file| file.path == Path::new(MANIFEST_FILE_NAME))
        .expect("validated snapshot root contains manifest.json");
    let manifest_path = snapshot.join(MANIFEST_FILE_NAME);
    let manifest = decode_manifest(&manifest_path, &manifest_file.bytes)?;
    let revision_bytes = verify_payload(snapshot, &tree, &manifest)?;
    verify_revision(snapshot, &manifest, revision_bytes)?;
    strict_validate_payload(snapshot, &tree)?;
    Ok(SnapshotSummary::new(
        manifest.revision,
        manifest.files.len(),
    ))
}

fn strict_validate_payload(snapshot: &Path, tree: &DirectoryTree) -> Result<(), SnapshotError> {
    let validation_root = std::env::temp_dir().join(format!(
        ".schronu-snapshot-verify-{}",
        uuid::Uuid::new_v4().hyphenated()
    ));
    fs::create_dir(&validation_root)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &validation_root, error))?;
    let result = materialize_for_validation(&validation_root, tree).and_then(|()| {
        let root_text = validation_root.to_str().ok_or_else(|| {
            invalid(
                &validation_root,
                "snapshot validation path must be valid Unicode",
            )
        })?;
        let mut repository = TaskRepository::new(root_text);
        repository.load().map_err(|error| {
            SnapshotError::new(
                SnapshotOperation::RepositoryLoad,
                snapshot.join(PAYLOAD_DIRECTORY_NAME),
                error,
            )
        })
    });
    let cleanup = fs::remove_dir_all(&validation_root)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &validation_root, error));
    result.and(cleanup)
}

fn materialize_for_validation(root: &Path, tree: &DirectoryTree) -> Result<(), SnapshotError> {
    for directory in tree
        .directories
        .iter()
        .filter(|entry| entry.path != Path::new(PAYLOAD_DIRECTORY_NAME))
    {
        let relative = directory
            .path
            .strip_prefix(PAYLOAD_DIRECTORY_NAME)
            .expect("verified directory belongs to payload");
        let path = root.join(relative);
        fs::create_dir(&path)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &path, error))?;
    }
    for file in tree
        .files
        .iter()
        .filter(|entry| entry.path != Path::new(MANIFEST_FILE_NAME))
    {
        let relative = file
            .path
            .strip_prefix(PAYLOAD_DIRECTORY_NAME)
            .expect("verified file belongs to payload");
        let path = root.join(relative);
        fs::write(&path, &file.bytes)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Write, &path, error))?;
    }
    Ok(())
}

fn validate_snapshot_root(snapshot: &Path, tree: &DirectoryTree) -> Result<(), SnapshotError> {
    let root_directories = tree
        .directories
        .iter()
        .filter(|entry| entry.path.components().count() == 1)
        .map(|entry| entry.path.as_path())
        .collect::<Vec<_>>();
    let root_files = tree
        .files
        .iter()
        .filter(|entry| entry.path.components().count() == 1)
        .map(|entry| entry.path.as_path())
        .collect::<Vec<_>>();
    if root_directories != [Path::new(PAYLOAD_DIRECTORY_NAME)]
        || root_files != [Path::new(MANIFEST_FILE_NAME)]
    {
        return Err(invalid(
            snapshot,
            "snapshot root must contain only manifest.json and storage",
        ));
    }
    Ok(())
}

fn verify_payload<'a>(
    snapshot: &Path,
    tree: &'a DirectoryTree,
    manifest: &SnapshotManifest,
) -> Result<Option<&'a [u8]>, SnapshotError> {
    let payload = snapshot.join(PAYLOAD_DIRECTORY_NAME);
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
    let actual_directories = payload_directories(tree)?;
    let actual_files = payload_files(tree)?;

    if actual_directories.len() != expected_directories.len() {
        return Err(invalid(
            &payload,
            "snapshot payload directory set differs from manifest",
        ));
    }
    for (relative, directory) in actual_directories {
        let Some(expected_mode) = expected_directories.get(&relative) else {
            return Err(invalid(
                payload.join(relative),
                "snapshot payload contains an extra directory",
            ));
        };
        verify_mode(
            &payload.join(relative),
            &directory.permissions,
            *expected_mode,
        )?;
    }

    if actual_files.len() != expected_files.len() {
        return Err(invalid(
            &payload,
            "snapshot payload file set differs from manifest",
        ));
    }
    let mut revision_bytes = None;
    for (relative, file) in actual_files {
        let Some(expected) = expected_files.get(&relative) else {
            return Err(invalid(
                payload.join(relative),
                "snapshot payload contains an extra file",
            ));
        };
        if !content_matches(
            &file.bytes,
            expected.content_length,
            &expected.content_digest,
        ) {
            return Err(invalid(
                payload.join(&relative),
                "snapshot file content does not match manifest",
            ));
        }
        verify_mode(&payload.join(&relative), &file.permissions, expected.mode)?;
        if relative == Path::new(".revision") {
            revision_bytes = Some(file.bytes.as_slice());
        }
    }
    Ok(revision_bytes)
}

fn payload_directories(
    tree: &DirectoryTree,
) -> Result<HashMap<PathBuf, &TreeDirectory>, SnapshotError> {
    tree.directories
        .iter()
        .filter(|entry| entry.path != Path::new(PAYLOAD_DIRECTORY_NAME))
        .map(|entry| {
            entry
                .path
                .strip_prefix(PAYLOAD_DIRECTORY_NAME)
                .map(|path| (path.to_path_buf(), entry))
                .map_err(|_| invalid(&entry.path, "snapshot entry is outside storage payload"))
        })
        .collect()
}

fn payload_files(tree: &DirectoryTree) -> Result<HashMap<PathBuf, &TreeFile>, SnapshotError> {
    tree.files
        .iter()
        .filter(|entry| entry.path != Path::new(MANIFEST_FILE_NAME))
        .map(|entry| {
            entry
                .path
                .strip_prefix(PAYLOAD_DIRECTORY_NAME)
                .map(|path| (path.to_path_buf(), entry))
                .map_err(|_| invalid(&entry.path, "snapshot entry is outside storage payload"))
        })
        .collect()
}

fn verify_revision(
    snapshot: &Path,
    manifest: &SnapshotManifest,
    revision_bytes: Option<&[u8]>,
) -> Result<(), SnapshotError> {
    let revision_path = snapshot.join(PAYLOAD_DIRECTORY_NAME).join(".revision");
    match (manifest.revision, revision_bytes) {
        (None, None) => Ok(()),
        (Some(expected), Some(bytes)) => {
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
        (Some(_), None) => Err(invalid(
            revision_path,
            "snapshot payload is missing .revision",
        )),
        (None, Some(_)) => Err(invalid(
            revision_path,
            "snapshot without a revision must not contain .revision",
        )),
    }
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
