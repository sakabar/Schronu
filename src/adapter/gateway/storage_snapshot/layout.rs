use super::error::{SnapshotError, SnapshotOperation};
use std::path::{Component, Path, PathBuf};

pub(super) const MANIFEST_FILE_NAME: &str = "manifest.json";
pub(super) const PAYLOAD_DIRECTORY_NAME: &str = "storage";

pub(super) fn validate_relative_path(path: &Path) -> Result<PathBuf, SnapshotError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(invalid_path(
            path,
            "snapshot path must be a non-empty relative path",
        ));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => normalized.push(name),
            _ => {
                return Err(invalid_path(
                    path,
                    "snapshot path must contain only normalized components",
                ))
            }
        }
    }
    if is_reserved_path(&normalized) {
        return Err(invalid_path(
            path,
            "snapshot path uses a reserved storage name",
        ));
    }
    Ok(normalized)
}

pub(super) fn is_reserved_path(path: &Path) -> bool {
    let mut components = path.components();
    let Some(Component::Normal(first)) = components.next() else {
        return true;
    };
    first == ".lock"
        || first == ".schronu-transactions"
        || is_temporary_name(first)
        || components.any(|component| match component {
            Component::Normal(name) => is_temporary_name(name),
            _ => true,
        })
}

fn is_temporary_name(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    let Some(hidden) = name.strip_prefix('.') else {
        return false;
    };
    if let Some(stem) = hidden.strip_suffix(".tmp") {
        return stem
            .rsplit_once('.')
            .is_some_and(|(_, id)| is_canonical_uuid(id));
    }
    hidden
        .rsplit_once(".tmp-")
        .is_some_and(|(_, id)| is_canonical_uuid(id))
}

fn is_canonical_uuid(text: &str) -> bool {
    uuid::Uuid::parse_str(text).is_ok_and(|id| id.hyphenated().to_string() == text)
}

fn invalid_path(path: &Path, message: &'static str) -> SnapshotError {
    SnapshotError::new(
        SnapshotOperation::Validate,
        path,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    )
}
