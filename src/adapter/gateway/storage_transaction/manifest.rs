use super::io::{validate_delete_target_ancestors, StorageTransactionIo};
use super::layout::{validate_storage_relative_path, TransactionLayout};
use super::{StorageTransactionError, StorageTransactionOperation};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
pub(super) struct RawTransactionManifest {
    pub(super) version: u32,
    pub(super) transaction_id: Uuid,
    pub(super) revision: Uuid,
    pub(super) directories: Vec<PathBuf>,
    pub(super) entries: Vec<RawManifestEntry>,
}

#[derive(Deserialize, Serialize)]
pub(super) struct RawManifestEntry {
    pub(super) target: PathBuf,
    #[serde(default, skip_serializing_if = "ManifestEntryOperation::is_write")]
    pub(super) operation: ManifestEntryOperation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) staged_file: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) content_length: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) content_checksum: Option<String>,
}

pub(super) struct ValidatedManifest {
    pub(super) transaction_id: Uuid,
    pub(super) revision: Uuid,
    pub(super) directories: Vec<PathBuf>,
    pub(super) entries: Vec<ValidatedEntry>,
}

pub(super) enum ValidatedEntry {
    Write {
        target: PathBuf,
        staged_file: PathBuf,
        integrity: ContentIntegrity,
    },
    Delete {
        target: PathBuf,
    },
}

pub(super) struct ContentIntegrity {
    pub(super) content_length: u64,
    pub(super) checksum: String,
}

impl From<&ValidatedManifest> for RawTransactionManifest {
    fn from(manifest: &ValidatedManifest) -> Self {
        Self {
            version: 1,
            transaction_id: manifest.transaction_id,
            revision: manifest.revision,
            directories: manifest.directories.clone(),
            entries: manifest
                .entries
                .iter()
                .map(|entry| match entry {
                    ValidatedEntry::Write {
                        target,
                        staged_file,
                        integrity,
                    } => RawManifestEntry {
                        target: target.clone(),
                        operation: ManifestEntryOperation::Write,
                        staged_file: Some(staged_file.clone()),
                        content_length: Some(integrity.content_length),
                        content_checksum: Some(integrity.checksum.clone()),
                    },
                    ValidatedEntry::Delete { target } => RawManifestEntry {
                        target: target.clone(),
                        operation: ManifestEntryOperation::Delete,
                        staged_file: None,
                        content_length: None,
                        content_checksum: None,
                    },
                })
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ManifestEntryOperation {
    #[default]
    Write,
    Delete,
}

impl ManifestEntryOperation {
    fn is_write(operation: &Self) -> bool {
        *operation == Self::Write
    }
}

pub(super) fn read_validated_manifest(
    io: &dyn StorageTransactionIo,
    storage_dir_path: &Path,
    transaction_dir_path: &Path,
) -> Result<ValidatedManifest, StorageTransactionError> {
    let manifest_path = TransactionLayout::manifest_path(transaction_dir_path);
    let manifest_metadata = io.symlink_metadata(&manifest_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::ReadManifest,
            &manifest_path,
            error,
        )
    })?;
    if !manifest_metadata.file_type().is_file() {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::ValidateManifest,
            &manifest_path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "transaction manifest must be a regular file",
            ),
        ));
    }
    let manifest_bytes = io.read_file(&manifest_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::ReadManifest,
            &manifest_path,
            error,
        )
    })?;
    let manifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::ParseManifest,
            &manifest_path,
            std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        )
    })?;
    validate_raw_manifest(io, storage_dir_path, transaction_dir_path, manifest)
}

pub(super) fn validate_raw_manifest(
    io: &dyn StorageTransactionIo,
    storage_dir_path: &Path,
    transaction_dir_path: &Path,
    manifest: RawTransactionManifest,
) -> Result<ValidatedManifest, StorageTransactionError> {
    let manifest_path = TransactionLayout::manifest_path(transaction_dir_path);
    let RawTransactionManifest {
        version,
        transaction_id,
        revision,
        directories,
        entries,
    } = manifest;
    let layout = TransactionLayout::new(storage_dir_path);
    if version != 1 || transaction_id.is_nil() {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::ValidateManifest,
            manifest_path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "transaction manifest must use version 1 and a non-nil transaction id",
            ),
        ));
    }
    let directories = directories
        .into_iter()
        .map(|directory| {
            validate_storage_relative_path(storage_dir_path, &layout.target_path(&directory))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let entries = entries
        .into_iter()
        .map(|entry| {
            let target = validate_storage_relative_path(
                storage_dir_path,
                &layout.target_path(&entry.target),
            )?;
            if entry.operation == ManifestEntryOperation::Delete {
                validate_delete_target_ancestors(io, storage_dir_path, &target)?;
            }
            match (
                entry.operation,
                entry.staged_file,
                entry.content_length,
                entry.content_checksum,
            ) {
                (
                    ManifestEntryOperation::Write,
                    Some(staged_file),
                    Some(content_length),
                    Some(content_checksum),
                ) => {
                    validate_staged_file_path(transaction_dir_path, &staged_file)?;
                    validate_content_integrity(&manifest_path, content_length, &content_checksum)?;
                    Ok(ValidatedEntry::Write {
                        target,
                        staged_file,
                        integrity: ContentIntegrity {
                            content_length,
                            checksum: content_checksum,
                        },
                    })
                }
                (ManifestEntryOperation::Delete, None, None, None) => {
                    Ok(ValidatedEntry::Delete { target })
                }
                (ManifestEntryOperation::Write, None, _, _) => Err(invalid_manifest_entry_error(
                    &manifest_path,
                    "write entry must contain a staged file",
                )),
                (ManifestEntryOperation::Write, Some(_), _, _) => {
                    Err(invalid_manifest_entry_error(
                        &manifest_path,
                        "write entry must contain content length and checksum",
                    ))
                }
                (ManifestEntryOperation::Delete, Some(_), _, _) => {
                    Err(invalid_manifest_entry_error(
                        &manifest_path,
                        "delete entry must not contain a staged file",
                    ))
                }
                (ManifestEntryOperation::Delete, None, _, _) => Err(invalid_manifest_entry_error(
                    &manifest_path,
                    "delete entry must not contain content integrity information",
                )),
            }
        })
        .collect::<Result<Vec<_>, StorageTransactionError>>()?;
    Ok(ValidatedManifest {
        transaction_id,
        revision,
        directories,
        entries,
    })
}

pub(super) fn invalid_manifest_entry_error(
    path: &Path,
    message: &'static str,
) -> StorageTransactionError {
    StorageTransactionError::new(
        StorageTransactionOperation::ValidateManifest,
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    )
}

pub(super) fn validate_content_integrity(
    manifest_path: &Path,
    content_length: u64,
    content_checksum: &str,
) -> Result<(), StorageTransactionError> {
    let checksum = content_checksum.strip_prefix("fnv1a64:").ok_or_else(|| {
        invalid_manifest_entry_error(
            manifest_path,
            "write checksum must use the fnv1a64 algorithm",
        )
    })?;
    if content_length > isize::MAX as u64 {
        return Err(invalid_manifest_entry_error(
            manifest_path,
            "write content length exceeds the supported file size",
        ));
    }
    if checksum.len() != 16
        || !checksum
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid_manifest_entry_error(
            manifest_path,
            "write checksum must contain 16 lowercase hexadecimal digits",
        ));
    }
    Ok(())
}

pub(super) fn content_checksum(bytes: &[u8]) -> String {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001b3;

    let checksum = bytes.iter().fold(FNV_OFFSET_BASIS, |checksum, byte| {
        (checksum ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    });
    format!("fnv1a64:{checksum:016x}")
}

pub(super) fn content_matches(bytes: &[u8], expected_length: u64, expected_checksum: &str) -> bool {
    bytes.len() as u64 == expected_length && content_checksum(bytes) == expected_checksum
}

pub(super) fn validate_staged_file_path(
    transaction_dir_path: &Path,
    staged_file: &Path,
) -> Result<(), StorageTransactionError> {
    let components = staged_file.components().collect::<Vec<_>>();
    if !matches!(
        components.as_slice(),
        [Component::Normal(directory), Component::Normal(_)]
            if TransactionLayout::is_staged_files_directory_name(directory)
    ) {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::ValidateManifest,
            TransactionLayout::staged_file_path(transaction_dir_path, staged_file),
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "staged file must be a direct child of the transaction files directory",
            ),
        ));
    }
    Ok(())
}
