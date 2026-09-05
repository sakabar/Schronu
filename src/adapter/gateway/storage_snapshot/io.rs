use std::ffi::CString;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

pub(in crate::adapter::gateway) trait SnapshotIo: Send + Sync {
    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        File::open(path)?.sync_all()
    }

    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_dir_all(path)
    }
}

pub(in crate::adapter::gateway) struct FileSystemSnapshotIo;
impl SnapshotIo for FileSystemSnapshotIo {}

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

#[cfg(unix)]
pub(super) fn read_directory_tree(path: &Path) -> std::io::Result<DirectoryTree> {
    use std::os::unix::fs::OpenOptionsExt;

    let root = File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if !root.metadata()?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "snapshot root must be a directory",
        ));
    }
    let mut tree = DirectoryTree {
        directories: Vec::new(),
        files: Vec::new(),
    };
    read_directory_handle(&root, Path::new(""), &mut tree)?;
    Ok(tree)
}

#[cfg(unix)]
fn read_directory_handle(
    directory: &File,
    relative: &Path,
    tree: &mut DirectoryTree,
) -> std::io::Result<()> {
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd};

    for name in read_directory_names(directory.as_raw_fd())? {
        let child_path = relative.join(&name);
        let name = CString::new(name.as_bytes()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "filesystem entry contains a NUL byte",
            )
        })?;
        // SAFETY: name is a live CString, and a successful descriptor is uniquely owned below.
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if descriptor < 0 {
            let error = std::io::Error::last_os_error();
            return Err(std::io::Error::new(
                error.kind(),
                format!("openat {} failed: {error}", child_path.display()),
            ));
        }
        // SAFETY: openat returned a new owned descriptor which is transferred to File exactly once.
        let mut child = unsafe { File::from_raw_fd(descriptor) };
        let metadata = child.metadata()?;
        if metadata.is_dir() {
            tree.directories.push(TreeDirectory {
                path: child_path.clone(),
                permissions: metadata.permissions(),
            });
            read_directory_handle(&child, &child_path, tree)?;
        } else if metadata.is_file() {
            let mut bytes = Vec::new();
            child.read_to_end(&mut bytes)?;
            tree.files.push(TreeFile {
                path: child_path,
                bytes,
                permissions: metadata.permissions(),
            });
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "directory tree contains a non-regular entry",
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
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

#[cfg(not(unix))]
pub(super) fn read_directory_tree(_path: &Path) -> std::io::Result<DirectoryTree> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "secure snapshot traversal is supported only on Unix platforms",
    ))
}

#[cfg(unix)]
fn c_path(path: &Path) -> std::io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "filesystem path contains a NUL byte",
        )
    })
}

#[cfg(target_os = "macos")]
pub(in crate::adapter::gateway) fn rename_no_replace(
    from: &Path,
    to: &Path,
) -> std::io::Result<()> {
    let from = c_path(from)?;
    let to = c_path(to)?;
    // SAFETY: both pointers are backed by live CStrings and renamex_np does not retain them.
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
pub(in crate::adapter::gateway) fn rename_no_replace(
    from: &Path,
    to: &Path,
) -> std::io::Result<()> {
    let from = c_path(from)?;
    let to = c_path(to)?;
    // SAFETY: both pointers are backed by live CStrings and renameat2 does not retain them.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(in crate::adapter::gateway) fn rename_no_replace(
    _from: &Path,
    _to: &Path,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic no-replace rename is supported only on macOS and Linux",
    ))
}
