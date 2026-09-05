mod publication;
mod read;

use std::fs;
use std::path::PathBuf;

pub(in crate::adapter::gateway) use publication::SnapshotFailurePoint;
pub(super) use publication::{FileSystemSnapshotIo, SnapshotIo, StableDirectory, StableParent};
pub(super) use read::read_directory_tree;

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
fn read_directory_names(
    descriptor: std::os::fd::RawFd,
) -> std::io::Result<Vec<std::ffi::OsString>> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStringExt;

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
    let mut names = Vec::new();
    loop {
        clear_errno();
        // SAFETY: directory remains valid until closedir below.
        let entry = unsafe { libc::readdir(directory) };
        if entry.is_null() {
            let error = std::io::Error::last_os_error();
            // SAFETY: directory is a live DIR pointer and is closed exactly once.
            unsafe { libc::closedir(directory) };
            return if error.raw_os_error() == Some(0) {
                Ok(names)
            } else {
                Err(error)
            };
        }
        // SAFETY: readdir returns a NUL-terminated d_name valid until the next call.
        let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if bytes != b"." && bytes != b".." {
            names.push(std::ffi::OsString::from_vec(bytes.to_vec()));
        }
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
