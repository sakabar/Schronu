use super::{invalid, permission_mode};
use crate::adapter::gateway::storage_snapshot::error::{
    SnapshotError, SnapshotLimitKind, SnapshotOperation,
};
use crate::adapter::gateway::storage_snapshot::io::{SnapshotFailurePoint, SnapshotIo};
use crate::adapter::gateway::storage_snapshot::layout::is_reserved_path;
use crate::adapter::gateway::storage_snapshot::SnapshotResourceLimits;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

pub(super) struct ScannedStorage {
    pub(super) directories: Vec<ScannedDirectory>,
    pub(super) files: Vec<ScannedFile>,
}

pub(super) struct ScannedDirectory {
    pub(super) relative: PathBuf,
    pub(super) metadata: fs::Metadata,
}

pub(super) struct ScannedFile {
    pub(super) path: PathBuf,
    pub(super) relative: PathBuf,
    pub(super) metadata: fs::Metadata,
    pub(super) bytes: Vec<u8>,
}

pub(super) fn scan_storage_entries(
    storage: &Path,
    limits: SnapshotResourceLimits,
    io: &dyn SnapshotIo,
) -> Result<ScannedStorage, SnapshotError> {
    use std::io::Read;

    let mut directories = Vec::new();
    let mut files = Vec::new();
    let mut file_count = 0_usize;
    let mut total_bytes = 0_u64;
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
        limits.check_path(entry.path(), &relative)?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| SnapshotError::new(SnapshotOperation::Read, entry.path(), error))?;
        if metadata.file_type().is_symlink() {
            return Err(invalid(
                entry.path(),
                "snapshot source must not contain symlinks",
            ));
        }
        if metadata.is_dir() {
            directories.push(ScannedDirectory { relative, metadata });
        } else if metadata.is_file() {
            let mut source = open_source_file(entry.path())?;
            let metadata = source.metadata().map_err(|error| {
                SnapshotError::new(SnapshotOperation::Read, entry.path(), error)
            })?;
            if !metadata.is_file() {
                return Err(invalid(
                    entry.path(),
                    "snapshot source entries must remain regular files while scanned",
                ));
            }
            file_count = file_count.checked_add(1).ok_or_else(|| {
                SnapshotError::limit(
                    entry.path(),
                    SnapshotLimitKind::FileCount,
                    limits.file_count as u64,
                    u64::MAX,
                    Some(relative.clone()),
                )
            })?;
            limits.check(
                entry.path(),
                Some(&relative),
                SnapshotLimitKind::FileCount,
                limits.file_count as u64,
                file_count as u64,
            )?;
            limits.check(
                entry.path(),
                Some(&relative),
                SnapshotLimitKind::FileBytes,
                limits.file_bytes,
                metadata.len(),
            )?;
            let metadata_total = total_bytes.checked_add(metadata.len()).ok_or_else(|| {
                SnapshotError::limit(
                    entry.path(),
                    SnapshotLimitKind::PayloadBytes,
                    limits.total_bytes,
                    u64::MAX,
                    Some(relative.clone()),
                )
            })?;
            limits.check(
                entry.path(),
                Some(&relative),
                SnapshotLimitKind::PayloadBytes,
                limits.total_bytes,
                metadata_total,
            )?;
            io.before(SnapshotFailurePoint::Read).map_err(|error| {
                SnapshotError::new(SnapshotOperation::Read, entry.path(), error)
            })?;
            let capacity = usize::try_from(metadata.len()).map_err(|error| {
                SnapshotError::new(SnapshotOperation::Read, entry.path(), error)
            })?;
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(capacity).map_err(|error| {
                SnapshotError::new(SnapshotOperation::Read, entry.path(), error)
            })?;
            source
                .by_ref()
                .take(
                    limits
                        .file_bytes
                        .min(limits.total_bytes - total_bytes)
                        .saturating_add(1),
                )
                .read_to_end(&mut bytes)
                .map_err(|error| {
                    SnapshotError::new(SnapshotOperation::Read, entry.path(), error)
                })?;
            limits.check(
                entry.path(),
                Some(&relative),
                SnapshotLimitKind::FileBytes,
                limits.file_bytes,
                bytes.len() as u64,
            )?;
            total_bytes = total_bytes.checked_add(bytes.len() as u64).ok_or_else(|| {
                SnapshotError::limit(
                    entry.path(),
                    SnapshotLimitKind::PayloadBytes,
                    limits.total_bytes,
                    u64::MAX,
                    Some(relative.clone()),
                )
            })?;
            limits.check(
                entry.path(),
                Some(&relative),
                SnapshotLimitKind::PayloadBytes,
                limits.total_bytes,
                total_bytes,
            )?;
            files.push(ScannedFile {
                path: entry.path().to_path_buf(),
                relative,
                metadata,
                bytes,
            });
        } else {
            return Err(invalid(
                entry.path(),
                "snapshot source entries must be regular files or directories",
            ));
        }
    }
    Ok(ScannedStorage { directories, files })
}

fn open_source_file(path: &Path) -> Result<fs::File, SnapshotError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    options
        .open(path)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Read, path, error))
}

pub(super) fn validate_capture_unchanged(
    storage: &Path,
    scanned: &ScannedStorage,
) -> Result<(), SnapshotError> {
    use std::io::Read;

    let mut directories = scanned
        .directories
        .iter()
        .map(|entry| (entry.relative.as_path(), entry))
        .collect::<HashMap<_, _>>();
    let mut files = scanned
        .files
        .iter()
        .map(|entry| (entry.relative.as_path(), entry))
        .collect::<HashMap<_, _>>();
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
            .expect("WalkDir entries remain below the storage root");
        if is_reserved_path(relative) {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| SnapshotError::new(SnapshotOperation::Read, entry.path(), error))?;
        if metadata.is_dir() {
            let expected = directories
                .remove(relative)
                .ok_or_else(|| capture_changed(entry.path()))?;
            if permission_mode(&metadata.permissions())
                != permission_mode(&expected.metadata.permissions())
            {
                return Err(capture_changed(entry.path()));
            }
        } else if metadata.is_file() {
            let expected = files
                .remove(relative)
                .ok_or_else(|| capture_changed(entry.path()))?;
            let mut source = open_source_file(entry.path())?;
            let opened = source.metadata().map_err(|error| {
                SnapshotError::new(SnapshotOperation::Read, entry.path(), error)
            })?;
            if !opened.is_file()
                || opened.len() != expected.bytes.len() as u64
                || permission_mode(&opened.permissions())
                    != permission_mode(&expected.metadata.permissions())
            {
                return Err(capture_changed(entry.path()));
            }
            let mut offset = 0_usize;
            let mut buffer = [0_u8; 8 * 1024];
            loop {
                let read = source.read(&mut buffer).map_err(|error| {
                    SnapshotError::new(SnapshotOperation::Read, entry.path(), error)
                })?;
                if read == 0 {
                    break;
                }
                let end = offset
                    .checked_add(read)
                    .ok_or_else(|| capture_changed(entry.path()))?;
                if expected.bytes.get(offset..end) != Some(&buffer[..read]) {
                    return Err(capture_changed(entry.path()));
                }
                offset = end;
            }
            if offset != expected.bytes.len() {
                return Err(capture_changed(entry.path()));
            }
        } else {
            return Err(capture_changed(entry.path()));
        }
    }
    if let Some(entry) = directories.values().next() {
        return Err(capture_changed(storage.join(&entry.relative)));
    }
    if let Some(entry) = files.values().next() {
        return Err(capture_changed(&entry.path));
    }
    Ok(())
}

fn capture_changed(path: impl Into<PathBuf>) -> SnapshotError {
    invalid(
        path,
        "snapshot source changed after resource capture and strict validation",
    )
}

fn should_descend(storage: &Path, entry: &DirEntry) -> bool {
    entry.depth() == 0
        || entry
            .path()
            .strip_prefix(storage)
            .map(|path| !is_reserved_path(path))
            .unwrap_or(false)
}
