use super::{StorageTransactionError, StorageTransactionOperation};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
pub(super) struct TransactionManifest {
    pub(super) version: u32,
    pub(super) transaction_id: Uuid,
    pub(super) revision: Uuid,
    pub(super) directories: Vec<PathBuf>,
    pub(super) entries: Vec<ManifestEntry>,
}

#[derive(Deserialize, Serialize)]
pub(super) struct ManifestEntry {
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
        [Component::Normal(directory), Component::Normal(_)] if *directory == "files"
    ) {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::ValidateManifest,
            transaction_dir_path.join(staged_file),
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "staged file must be a direct child of the transaction files directory",
            ),
        ));
    }
    Ok(())
}
