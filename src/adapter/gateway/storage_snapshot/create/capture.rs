use super::invalid;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::adapter::gateway::storage_snapshot::error::SnapshotLimitKind;
use crate::adapter::gateway::storage_snapshot::error::{SnapshotError, SnapshotOperation};
#[cfg(test)]
use crate::adapter::gateway::storage_snapshot::io::FileSystemSnapshotIo;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::adapter::gateway::storage_snapshot::io::SnapshotFailurePoint;
use crate::adapter::gateway::storage_snapshot::io::SnapshotIo;
use crate::adapter::gateway::storage_snapshot::layout::is_reserved_path;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::adapter::gateway::storage_snapshot::manifest::accumulate_directory_manifest_bytes;
use crate::adapter::gateway::storage_snapshot::{permission_mode, SnapshotResourceLimits};
use std::fs;
use std::path::{Path, PathBuf};

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
                self.directory_manifest_bytes = accumulate_directory_manifest_bytes(
                    &display_path,
                    &child_relative,
                    permission_mode(&metadata.permissions()),
                    self.directory_manifest_bytes,
                    self.limits,
                )?;
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

pub(super) fn validate_capture_unchanged(
    storage: &Path,
    scanned: &ScannedStorage,
    limits: SnapshotResourceLimits,
) -> Result<(), SnapshotError> {
    validate_capture_unchanged_secure(storage, scanned, limits)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn validate_capture_unchanged_secure(
    storage: &Path,
    scanned: &ScannedStorage,
    limits: SnapshotResourceLimits,
) -> Result<(), SnapshotError> {
    use std::os::unix::fs::OpenOptionsExt;

    let root = File::options()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(storage)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Read, storage, error))?;
    let directories = scanned
        .directories
        .iter()
        .map(|entry| (entry.relative.as_path(), entry))
        .collect::<std::collections::HashMap<_, _>>();
    let files = scanned
        .files
        .iter()
        .map(|entry| (entry.relative.as_path(), entry))
        .collect::<std::collections::HashMap<_, _>>();
    let mut validator = CaptureValidator {
        storage,
        limits,
        directories,
        files,
        file_count: 0,
        total_bytes: 0,
        directory_manifest_bytes: 0,
    };
    validator.read_directory(&root, Path::new(""))?;
    if let Some(expected) = validator.directories.values().next() {
        return Err(capture_changed(storage.join(&expected.relative)));
    }
    if let Some(expected) = validator.files.values().next() {
        return Err(capture_changed(&expected.path));
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn validate_capture_unchanged_secure(
    storage: &Path,
    _scanned: &ScannedStorage,
    _limits: SnapshotResourceLimits,
) -> Result<(), SnapshotError> {
    Err(invalid(
        storage,
        "secure snapshot source validation is supported only on macOS and Linux",
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct CaptureValidator<'a> {
    storage: &'a Path,
    limits: SnapshotResourceLimits,
    directories: std::collections::HashMap<&'a Path, &'a ScannedDirectory>,
    files: std::collections::HashMap<&'a Path, &'a ScannedFile>,
    file_count: usize,
    total_bytes: u64,
    directory_manifest_bytes: u64,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl CaptureValidator<'_> {
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
                self.validate_directory(&display_path, &child_relative, &metadata)?;
                self.read_directory(&child, &child_relative)?;
            } else if metadata.is_file() {
                self.validate_file(&display_path, &child_relative, &mut child, &metadata)?;
            } else {
                return Err(capture_changed(display_path));
            }
        }
        Ok(())
    }

    fn validate_directory(
        &mut self,
        path: &Path,
        relative: &Path,
        metadata: &fs::Metadata,
    ) -> Result<(), SnapshotError> {
        self.directory_manifest_bytes = accumulate_directory_manifest_bytes(
            path,
            relative,
            permission_mode(&metadata.permissions()),
            self.directory_manifest_bytes,
            self.limits,
        )?;
        let Some(expected) = self.directories.remove(relative) else {
            return Err(capture_changed(path));
        };
        if permission_mode(&metadata.permissions())
            != permission_mode(&expected.metadata.permissions())
        {
            return Err(capture_changed(path));
        }
        Ok(())
    }

    fn validate_file(
        &mut self,
        path: &Path,
        relative: &Path,
        source: &mut File,
        metadata: &fs::Metadata,
    ) -> Result<(), SnapshotError> {
        use std::io::Read;

        self.file_count = self.file_count.checked_add(1).ok_or_else(|| {
            SnapshotError::limit(
                path,
                SnapshotLimitKind::FileCount,
                self.limits.file_count as u64,
                u64::MAX,
                Some(relative.to_path_buf()),
            )
        })?;
        self.limits.check(
            path,
            Some(relative),
            SnapshotLimitKind::FileCount,
            self.limits.file_count as u64,
            self.file_count as u64,
        )?;
        self.limits.check(
            path,
            Some(relative),
            SnapshotLimitKind::FileBytes,
            self.limits.file_bytes,
            metadata.len(),
        )?;
        self.total_bytes = self
            .total_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| {
                SnapshotError::limit(
                    path,
                    SnapshotLimitKind::PayloadBytes,
                    self.limits.total_bytes,
                    u64::MAX,
                    Some(relative.to_path_buf()),
                )
            })?;
        self.limits.check(
            path,
            Some(relative),
            SnapshotLimitKind::PayloadBytes,
            self.limits.total_bytes,
            self.total_bytes,
        )?;
        let Some(expected) = self.files.remove(relative) else {
            return Err(capture_changed(path));
        };
        if metadata.len() != expected.bytes.len() as u64
            || permission_mode(&metadata.permissions())
                != permission_mode(&expected.metadata.permissions())
        {
            return Err(capture_changed(path));
        }
        let prior_total = self
            .total_bytes
            .checked_sub(metadata.len())
            .ok_or_else(|| {
                SnapshotError::limit(
                    path,
                    SnapshotLimitKind::PayloadBytes,
                    self.limits.total_bytes,
                    u64::MAX,
                    Some(relative.to_path_buf()),
                )
            })?;
        let mut offset = 0_usize;
        let mut buffer = [0_u8; 8 * 1024];
        let read_limit = u64::try_from(expected.bytes.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let mut bounded = source.take(read_limit);
        loop {
            let read = bounded
                .read(&mut buffer)
                .map_err(|error| SnapshotError::new(SnapshotOperation::Read, path, error))?;
            if read == 0 {
                break;
            }
            let end = offset.checked_add(read).ok_or_else(|| {
                SnapshotError::limit(
                    path,
                    SnapshotLimitKind::FileBytes,
                    self.limits.file_bytes,
                    u64::MAX,
                    Some(relative.to_path_buf()),
                )
            })?;
            let observed_file = u64::try_from(end).unwrap_or(u64::MAX);
            self.limits.check(
                path,
                Some(relative),
                SnapshotLimitKind::FileBytes,
                self.limits.file_bytes,
                observed_file,
            )?;
            let observed_total = prior_total.checked_add(observed_file).ok_or_else(|| {
                SnapshotError::limit(
                    path,
                    SnapshotLimitKind::PayloadBytes,
                    self.limits.total_bytes,
                    u64::MAX,
                    Some(relative.to_path_buf()),
                )
            })?;
            self.limits.check(
                path,
                Some(relative),
                SnapshotLimitKind::PayloadBytes,
                self.limits.total_bytes,
                observed_total,
            )?;
            if expected.bytes.get(offset..end) != Some(&buffer[..read]) {
                return Err(capture_changed(path));
            }
            offset = end;
        }
        if offset != expected.bytes.len() {
            return Err(capture_changed(path));
        }
        Ok(())
    }
}

fn capture_changed(path: impl Into<PathBuf>) -> SnapshotError {
    invalid(
        path,
        "snapshot source changed after resource capture and strict validation",
    )
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;

    fn validate_growth_after_metadata(
        storage: &Path,
        limits: SnapshotResourceLimits,
    ) -> SnapshotError {
        use std::io::Write;

        fs::write(storage.join("file"), b"12345").unwrap();
        let scanned = scan_storage_entries(storage, limits, &FileSystemSnapshotIo).unwrap();
        let expected = &scanned.files[0];
        let mut source = File::open(&expected.path).unwrap();
        let metadata = source.metadata().unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(&expected.path)
            .unwrap()
            .write_all(b"6")
            .unwrap();
        let mut validator = CaptureValidator {
            storage,
            limits,
            directories: std::collections::HashMap::new(),
            files: std::collections::HashMap::from([(expected.relative.as_path(), expected)]),
            file_count: 0,
            total_bytes: 0,
            directory_manifest_bytes: 0,
        };
        validator
            .validate_file(&expected.path, &expected.relative, &mut source, &metadata)
            .unwrap_err()
    }

    #[test]
    fn capture不変性再検査も同じfile件数上限で停止する() {
        let storage = std::env::temp_dir().join(format!(
            "schronu-capture-validation-limit-{}",
            uuid::Uuid::new_v4().hyphenated()
        ));
        fs::create_dir(&storage).unwrap();
        fs::write(storage.join("first"), b"first").unwrap();
        let limits = SnapshotResourceLimits::new(u64::MAX, 1, u64::MAX, u64::MAX, usize::MAX, 64);
        let scanned = scan_storage_entries(&storage, limits, &FileSystemSnapshotIo).unwrap();
        fs::write(storage.join("second"), b"second").unwrap();

        let error = validate_capture_unchanged(&storage, &scanned, limits).unwrap_err();

        assert_eq!(error.limit_kind(), Some(SnapshotLimitKind::FileCount));
        assert_eq!(error.limit_value(), Some(1));
        assert_eq!(error.observed_value(), Some(2));
        fs::remove_dir_all(storage).unwrap();
    }

    #[test]
    fn capture不変性streamはmetadata後増大をtyped上限で拒否する() {
        let storage = std::env::temp_dir().join(format!(
            "schronu-capture-validation-growth-{}",
            uuid::Uuid::new_v4().hyphenated()
        ));
        fs::create_dir(&storage).unwrap();
        let file_limits = SnapshotResourceLimits::new(u64::MAX, 1, 5, u64::MAX, usize::MAX, 64);

        let error = validate_growth_after_metadata(&storage, file_limits);

        assert_eq!(error.limit_kind(), Some(SnapshotLimitKind::FileBytes));
        assert_eq!(error.limit_value(), Some(5));
        assert_eq!(error.observed_value(), Some(6));
        assert_eq!(error.limit_path(), Some(Path::new("file")));

        let payload_limits = SnapshotResourceLimits::new(u64::MAX, 1, 6, 5, usize::MAX, 64);
        let error = validate_growth_after_metadata(&storage, payload_limits);

        assert_eq!(error.limit_kind(), Some(SnapshotLimitKind::PayloadBytes));
        assert_eq!(error.limit_value(), Some(5));
        assert_eq!(error.observed_value(), Some(6));
        assert_eq!(error.limit_path(), Some(Path::new("file")));
        fs::remove_dir_all(storage).unwrap();
    }
}
