use super::{invalid, permission_mode};
use crate::adapter::gateway::storage_snapshot::error::{
    SnapshotError, SnapshotLimitKind, SnapshotOperation,
};
use crate::adapter::gateway::storage_snapshot::io::{SnapshotFailurePoint, SnapshotIo};
use crate::adapter::gateway::storage_snapshot::layout::is_reserved_path;
use crate::adapter::gateway::storage_snapshot::manifest::encoded_directory_entry_len;
use crate::adapter::gateway::storage_snapshot::SnapshotResourceLimits;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::ffi::CString;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::fs::File;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::os::unix::ffi::OsStrExt;

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
    scan_storage_entries_secure(storage, limits, io)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn scan_storage_entries_secure(
    storage: &Path,
    limits: SnapshotResourceLimits,
    io: &dyn SnapshotIo,
) -> Result<ScannedStorage, SnapshotError> {
    use std::os::unix::fs::OpenOptionsExt;

    let root = File::options()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(storage)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Read, storage, error))?;
    let mut builder = CaptureBuilder {
        storage,
        limits,
        io,
        directories: Vec::new(),
        files: Vec::new(),
        file_count: 0,
        total_bytes: 0,
        directory_manifest_bytes: 0,
    };
    builder.read_directory(&root, Path::new(""))?;
    Ok(ScannedStorage {
        directories: builder.directories,
        files: builder.files,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn scan_storage_entries_secure(
    storage: &Path,
    _limits: SnapshotResourceLimits,
    _io: &dyn SnapshotIo,
) -> Result<ScannedStorage, SnapshotError> {
    Err(invalid(
        storage,
        "secure snapshot source capture is supported only on macOS and Linux",
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct CaptureBuilder<'a> {
    storage: &'a Path,
    limits: SnapshotResourceLimits,
    io: &'a dyn SnapshotIo,
    directories: Vec<ScannedDirectory>,
    files: Vec<ScannedFile>,
    file_count: usize,
    total_bytes: u64,
    directory_manifest_bytes: u64,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl CaptureBuilder<'_> {
    fn read_directory(&mut self, directory: &File, relative: &Path) -> Result<(), SnapshotError> {
        use std::os::fd::{AsRawFd, FromRawFd};

        for name in crate::adapter::gateway::storage_snapshot::io::read_directory_names(
            directory.as_raw_fd(),
        )
        .map_err(|error| {
            SnapshotError::new(SnapshotOperation::Read, self.storage.join(relative), error)
        })? {
            let name = name.map_err(|error| {
                SnapshotError::new(SnapshotOperation::Read, self.storage.join(relative), error)
            })?;
            let child_relative = relative.join(&name);
            if is_reserved_path(&child_relative) {
                continue;
            }
            let display_path = self.storage.join(&child_relative);
            self.limits.check_path(&display_path, &child_relative)?;
            let name = CString::new(name.as_bytes())
                .map_err(|_| invalid(&display_path, "snapshot source entry contains a NUL byte"))?;
            // SAFETY: name is a live CString and the returned descriptor is owned below.
            let descriptor = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                )
            };
            if descriptor < 0 {
                return Err(SnapshotError::new(
                    SnapshotOperation::Read,
                    &display_path,
                    std::io::Error::last_os_error(),
                ));
            }
            // SAFETY: openat returned a new descriptor transferred to File exactly once.
            let mut child = unsafe { File::from_raw_fd(descriptor) };
            let metadata = child.metadata().map_err(|error| {
                SnapshotError::new(SnapshotOperation::Read, &display_path, error)
            })?;
            if metadata.is_dir() {
                let entry_bytes = encoded_directory_entry_len(
                    &display_path,
                    &child_relative,
                    permission_mode(&metadata.permissions()),
                )?;
                let separator_bytes = u64::from(self.directory_manifest_bytes != 0);
                let observed = self
                    .directory_manifest_bytes
                    .checked_add(separator_bytes)
                    .and_then(|bytes| bytes.checked_add(entry_bytes))
                    .ok_or_else(|| {
                        SnapshotError::limit(
                            &display_path,
                            SnapshotLimitKind::ManifestBytes,
                            self.limits.manifest_bytes,
                            u64::MAX,
                            Some(child_relative.clone()),
                        )
                    })?;
                self.limits.check(
                    &display_path,
                    Some(&child_relative),
                    SnapshotLimitKind::ManifestBytes,
                    self.limits.manifest_bytes,
                    observed,
                )?;
                self.directory_manifest_bytes = observed;
                self.directories.push(ScannedDirectory {
                    relative: child_relative.clone(),
                    metadata,
                });
                self.read_directory(&child, &child_relative)?;
            } else if metadata.is_file() {
                self.capture_file(&display_path, child_relative, &mut child, metadata)?;
            } else {
                return Err(invalid(
                    &display_path,
                    "snapshot source entries must be regular files or directories",
                ));
            }
        }
        Ok(())
    }

    fn capture_file(
        &mut self,
        path: &Path,
        relative: PathBuf,
        source: &mut File,
        metadata: fs::Metadata,
    ) -> Result<(), SnapshotError> {
        use std::io::Read;

        self.file_count = self.file_count.checked_add(1).ok_or_else(|| {
            SnapshotError::limit(
                path,
                SnapshotLimitKind::FileCount,
                self.limits.file_count as u64,
                u64::MAX,
                Some(relative.clone()),
            )
        })?;
        self.limits.check(
            path,
            Some(&relative),
            SnapshotLimitKind::FileCount,
            self.limits.file_count as u64,
            self.file_count as u64,
        )?;
        self.limits.check(
            path,
            Some(&relative),
            SnapshotLimitKind::FileBytes,
            self.limits.file_bytes,
            metadata.len(),
        )?;
        let metadata_total = self
            .total_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| {
                SnapshotError::limit(
                    path,
                    SnapshotLimitKind::PayloadBytes,
                    self.limits.total_bytes,
                    u64::MAX,
                    Some(relative.clone()),
                )
            })?;
        self.limits.check(
            path,
            Some(&relative),
            SnapshotLimitKind::PayloadBytes,
            self.limits.total_bytes,
            metadata_total,
        )?;
        self.io
            .before(SnapshotFailurePoint::Read)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Read, path, error))?;
        let capacity = usize::try_from(metadata.len())
            .map_err(|error| SnapshotError::new(SnapshotOperation::Read, path, error))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Read, path, error))?;
        source
            .by_ref()
            .take(
                self.limits
                    .file_bytes
                    .min(self.limits.total_bytes - self.total_bytes)
                    .saturating_add(1),
            )
            .read_to_end(&mut bytes)
            .map_err(|error| SnapshotError::new(SnapshotOperation::Read, path, error))?;
        self.limits.check(
            path,
            Some(&relative),
            SnapshotLimitKind::FileBytes,
            self.limits.file_bytes,
            bytes.len() as u64,
        )?;
        self.total_bytes = self
            .total_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| {
                SnapshotError::limit(
                    path,
                    SnapshotLimitKind::PayloadBytes,
                    self.limits.total_bytes,
                    u64::MAX,
                    Some(relative.clone()),
                )
            })?;
        self.limits.check(
            path,
            Some(&relative),
            SnapshotLimitKind::PayloadBytes,
            self.limits.total_bytes,
            self.total_bytes,
        )?;
        self.files.push(ScannedFile {
            path: path.to_path_buf(),
            relative,
            metadata,
            bytes,
        });
        Ok(())
    }
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
