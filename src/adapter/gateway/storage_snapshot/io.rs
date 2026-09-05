mod publication;
mod read;

use std::fmt;
use std::fs;
use std::path::PathBuf;

#[cfg(test)]
pub(in crate::adapter::gateway) use publication::FailOnceSnapshotIo;
pub(in crate::adapter::gateway) use publication::SnapshotFailurePoint;
pub(super) use publication::{FileSystemSnapshotIo, SnapshotIo, StableDirectory, StableParent};
pub(super) use read::read_directory_tree_with_limits;

#[derive(Clone, Copy, Debug)]
pub(super) enum FileWriteStage {
    Write,
    Sync,
}

#[derive(Debug)]
pub(super) struct FileWriteError {
    stage: FileWriteStage,
    source: std::io::Error,
}

impl FileWriteError {
    pub(super) fn write(source: std::io::Error) -> Self {
        Self {
            stage: FileWriteStage::Write,
            source,
        }
    }

    pub(super) fn sync(source: std::io::Error) -> Self {
        Self {
            stage: FileWriteStage::Sync,
            source,
        }
    }

    pub(super) fn stage(&self) -> FileWriteStage {
        self.stage
    }
}

impl fmt::Display for FileWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for FileWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub(super) struct TreeDirectory {
    pub(super) path: PathBuf,
    pub(super) permissions: fs::Permissions,
}

pub(super) struct TreeFile {
    pub(super) path: PathBuf,
    pub(super) bytes: Vec<u8>,
    pub(super) permissions: fs::Permissions,
}

pub(super) struct DirectoryTree {
    pub(super) directories: Vec<TreeDirectory>,
    pub(super) files: Vec<TreeFile>,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(in crate::adapter::gateway::storage_snapshot) fn read_directory_names(
    descriptor: std::os::fd::RawFd,
) -> std::io::Result<DirectoryNames> {
    // SAFETY: dup creates an independently owned descriptor for fdopendir.
    let duplicate = unsafe { libc::dup(descriptor) };
    if duplicate < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: duplicate is valid and ownership transfers to DIR on success.
    let directory = unsafe { libc::fdopendir(duplicate) };
    if directory.is_null() {
        // SAFETY: fdopendir did not take ownership on failure.
        unsafe { libc::close(duplicate) };
        return Err(std::io::Error::last_os_error());
    }
    Ok(DirectoryNames {
        directory,
        finished: false,
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(in crate::adapter::gateway::storage_snapshot) struct DirectoryNames {
    directory: *mut libc::DIR,
    finished: bool,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Iterator for DirectoryNames {
    type Item = std::io::Result<std::ffi::OsString>;

    fn next(&mut self) -> Option<Self::Item> {
        use std::ffi::CStr;
        use std::os::unix::ffi::OsStringExt;

        if self.finished {
            return None;
        }
        loop {
            clear_errno();
            // SAFETY: directory remains valid until Drop closes it.
            let entry = unsafe { libc::readdir(self.directory) };
            if entry.is_null() {
                let error = std::io::Error::last_os_error();
                self.finished = true;
                return if error.raw_os_error() == Some(0) {
                    None
                } else {
                    Some(Err(error))
                };
            }
            // SAFETY: readdir returns a NUL-terminated d_name valid until the next call.
            let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if bytes != b"." && bytes != b".." {
                return Some(Ok(std::ffi::OsString::from_vec(bytes.to_vec())));
            }
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Drop for DirectoryNames {
    fn drop(&mut self) {
        // SAFETY: fdopendir transferred ownership to this live DIR pointer.
        unsafe { libc::closedir(self.directory) };
    }
}

#[cfg(target_os = "macos")]
fn clear_errno() {
    // SAFETY: __error returns the calling thread's errno location.
    unsafe { *libc::__error() = 0 };
}

#[cfg(target_os = "linux")]
fn clear_errno() {
    // SAFETY: __errno_location returns the calling thread's errno location.
    unsafe { *libc::__errno_location() = 0 };
}
