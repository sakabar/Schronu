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

pub(in crate::adapter::gateway) fn encode_manifest_with_limits(
    manifest_path: &Path,
    manifest: &SnapshotManifest,
    limits: SnapshotResourceLimits,
) -> Result<Vec<u8>, SnapshotError> {
    let mut manifest = manifest.clone();
    manifest
        .directories
        .sort_by(|left, right| left.path.cmp(&right.path));
    manifest
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    let mut writer = BoundedManifestWriter::new(limits.manifest_bytes);
    if let Err(error) = serde_json::to_writer(&mut writer, &manifest) {
        if let Some(observed) = writer.limit_observed {
            return Err(SnapshotError::limit(
                manifest_path,
                super::error::SnapshotLimitKind::ManifestBytes,
                limits.manifest_bytes,
                observed,
                None,
            ));
        }
        return Err(SnapshotError::new(
            SnapshotOperation::Encode,
            manifest_path,
            error,
        ));
    }
    Ok(writer.bytes)
}

struct BoundedManifestWriter {
    bytes: Vec<u8>,
    limit: u64,
    limit_observed: Option<u64>,
}

impl BoundedManifestWriter {
    fn new(limit: u64) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            limit_observed: None,
        }
    }
}

impl std::io::Write for BoundedManifestWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let current = u64::try_from(self.bytes.len()).unwrap_or(u64::MAX);
        let observed = current.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if observed > self.limit {
            self.limit_observed = Some(self.limit.saturating_add(1));
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "snapshot manifest exceeds its resource limit",
            ));
        }
        self.bytes.try_reserve(bytes.len()).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::OutOfMemory, error.to_string())
        })?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn encoded_directory_entry_len(
    operation_path: &Path,
    path: &Path,
    mode: Option<u32>,
) -> Result<u64, SnapshotError> {
    let bytes = serde_json::to_vec(&DirectoryEntry {
        path: path.to_path_buf(),
        mode,
    })
    .map_err(|error| SnapshotError::new(SnapshotOperation::Encode, operation_path, error))?;
    Ok(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
}

pub(in crate::adapter::gateway) fn decode_manifest(
    manifest_path: &Path,
    bytes: &[u8],
) -> Result<SnapshotManifest, SnapshotError> {
    decode_manifest_with_limits(manifest_path, bytes, DEFAULT_RESOURCE_LIMITS)
}

pub(in crate::adapter::gateway) fn decode_manifest_with_limits(
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

    for directory in &mut manifest.directories {
        directory.path = validate_relative_path(&directory.path)?;
    }
    for file in &mut manifest.files {
        file.path = validate_relative_path(&file.path)?;
    }

    if manifest.files.len() > limits.file_count {
        let relative_path = manifest.files[limits.file_count].path.clone();
        return Err(SnapshotError::limit(
            manifest_path,
            super::error::SnapshotLimitKind::FileCount,
            limits.file_count as u64,
            u64::try_from(manifest.files.len()).unwrap_or(u64::MAX),
            Some(relative_path),
        ));
    }
    let mut paths = HashSet::new();
    for directory in &mut manifest.directories {
        limits.check_path(manifest_path, &directory.path)?;
        if !paths.insert(directory.path.clone()) {
            return Err(invalid_manifest(manifest_path, "duplicate snapshot path"));
        }
    }
    let mut total_bytes = 0_u64;
    for file in &mut manifest.files {
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
