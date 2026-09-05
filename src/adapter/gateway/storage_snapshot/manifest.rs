use super::error::{SnapshotError, SnapshotOperation};
use super::layout::validate_relative_path;
use super::{SnapshotResourceLimits, DEFAULT_RESOURCE_LIMITS};
use crate::adapter::gateway::storage_content_integrity::DIGEST_ALGORITHM;
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(super) const FORMAT_VERSION: u32 = 1;
pub(super) const DIGEST_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(in crate::adapter::gateway) struct SnapshotManifest {
    pub(in crate::adapter::gateway) format_version: u32,
    pub(in crate::adapter::gateway) tool_version: String,
    pub(in crate::adapter::gateway) created_at: DateTime<FixedOffset>,
    pub(in crate::adapter::gateway) revision: Option<Uuid>,
    pub(in crate::adapter::gateway) digest: DigestDescriptor,
    pub(in crate::adapter::gateway) directories: Vec<DirectoryEntry>,
    pub(in crate::adapter::gateway) files: Vec<FileEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(in crate::adapter::gateway) struct DigestDescriptor {
    pub(in crate::adapter::gateway) algorithm: String,
    pub(in crate::adapter::gateway) version: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(in crate::adapter::gateway) struct DirectoryEntry {
    pub(in crate::adapter::gateway) path: PathBuf,
    pub(in crate::adapter::gateway) mode: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(in crate::adapter::gateway) struct FileEntry {
    pub(in crate::adapter::gateway) path: PathBuf,
    pub(in crate::adapter::gateway) mode: Option<u32>,
    pub(in crate::adapter::gateway) content_length: u64,
    pub(in crate::adapter::gateway) content_digest: String,
}

pub(in crate::adapter::gateway) fn encode_manifest(
    manifest: &SnapshotManifest,
) -> Result<Vec<u8>, SnapshotError> {
    let mut manifest = manifest.clone();
    manifest
        .directories
        .sort_by(|left, right| left.path.cmp(&right.path));
    manifest
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    serde_json::to_vec(&manifest)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Encode, "manifest.json", error))
}

pub(in crate::adapter::gateway) fn decode_manifest(
    manifest_path: &Path,
    bytes: &[u8],
) -> Result<SnapshotManifest, SnapshotError> {
    decode_manifest_with_limits(manifest_path, bytes, DEFAULT_RESOURCE_LIMITS)
}

pub(super) fn decode_manifest_with_limits(
    manifest_path: &Path,
    bytes: &[u8],
    limits: SnapshotResourceLimits,
) -> Result<SnapshotManifest, SnapshotError> {
    limits.check(
        manifest_path,
        None,
        super::error::SnapshotLimitKind::ManifestBytes,
        limits.manifest_bytes,
        bytes.len() as u64,
    )?;
    let mut manifest: SnapshotManifest = serde_json::from_slice(bytes)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Decode, manifest_path, error))?;
    validate_manifest(manifest_path, &mut manifest, limits)?;
    Ok(manifest)
}

fn validate_manifest(
    manifest_path: &Path,
    manifest: &mut SnapshotManifest,
    limits: SnapshotResourceLimits,
) -> Result<(), SnapshotError> {
    if manifest.format_version != FORMAT_VERSION
        || manifest.digest.algorithm != DIGEST_ALGORITHM
        || manifest.digest.version != DIGEST_VERSION
    {
        return Err(invalid_manifest(
            manifest_path,
            "unsupported snapshot format or digest algorithm version",
        ));
    }

    limits.check(
        manifest_path,
        None,
        super::error::SnapshotLimitKind::FileCount,
        limits.file_count as u64,
        u64::try_from(manifest.files.len()).unwrap_or(u64::MAX),
    )?;
    let mut paths = HashSet::new();
    for directory in &mut manifest.directories {
        directory.path = validate_relative_path(&directory.path)?;
        limits.check_path(manifest_path, &directory.path)?;
        if !paths.insert(directory.path.clone()) {
            return Err(invalid_manifest(manifest_path, "duplicate snapshot path"));
        }
    }
    let mut total_bytes = 0_u64;
    for file in &mut manifest.files {
        file.path = validate_relative_path(&file.path)?;
        limits.check_path(manifest_path, &file.path)?;
        limits.check(
            manifest_path,
            Some(&file.path),
            super::error::SnapshotLimitKind::FileBytes,
            limits.file_bytes,
            file.content_length,
        )?;
        total_bytes = total_bytes
            .checked_add(file.content_length)
            .ok_or_else(|| {
                SnapshotError::limit(
                    &file.path,
                    super::error::SnapshotLimitKind::PayloadBytes,
                    limits.total_bytes,
                    u64::MAX,
                    Some(file.path.clone()),
                )
            })?;
        limits.check(
            manifest_path,
            Some(&file.path),
            super::error::SnapshotLimitKind::PayloadBytes,
            limits.total_bytes,
            total_bytes,
        )?;
        if !paths.insert(file.path.clone()) {
            return Err(invalid_manifest(manifest_path, "duplicate snapshot path"));
        }
        validate_digest(manifest_path, &file.content_digest)?;
    }
    let file_paths = manifest
        .files
        .iter()
        .map(|file| file.path.as_path())
        .collect::<HashSet<_>>();
    let has_file_ancestor = manifest.directories.iter().any(|directory| {
        directory
            .path
            .ancestors()
            .filter(|ancestor| !ancestor.as_os_str().is_empty())
            .any(|ancestor| file_paths.contains(ancestor))
    }) || manifest.files.iter().any(|file| {
        file.path
            .ancestors()
            .skip(1)
            .filter(|ancestor| !ancestor.as_os_str().is_empty())
            .any(|ancestor| file_paths.contains(ancestor))
    });
    if has_file_ancestor {
        return Err(invalid_manifest(
            manifest_path,
            "snapshot file path must not be an ancestor of another entry",
        ));
    }
    Ok(())
}

fn validate_digest(manifest_path: &Path, digest: &str) -> Result<(), SnapshotError> {
    let Some(hex) = digest.strip_prefix("fnv1a64:") else {
        return Err(invalid_manifest(
            manifest_path,
            "file digest must use fnv1a64",
        ));
    };
    if hex.len() != 16
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid_manifest(
            manifest_path,
            "file digest must contain 16 lowercase hexadecimal digits",
        ));
    }
    Ok(())
}

fn invalid_manifest(path: &Path, message: &'static str) -> SnapshotError {
    SnapshotError::new(
        SnapshotOperation::Validate,
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    )
}
