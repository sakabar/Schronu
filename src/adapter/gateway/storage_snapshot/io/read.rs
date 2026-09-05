use super::DirectoryTree;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::{TreeDirectory, TreeFile};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::fs::File;
use std::path::Path;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::ffi::CString;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::os::unix::ffi::OsStrExt;

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(in crate::adapter::gateway::storage_snapshot) fn read_directory_tree(
    path: &Path,
) -> std::io::Result<DirectoryTree> {
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read_directory_handle(
    directory: &File,
    relative: &Path,
    tree: &mut DirectoryTree,
) -> std::io::Result<()> {
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd};

    for name in super::read_directory_names(directory.as_raw_fd())? {
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

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(in crate::adapter::gateway::storage_snapshot) fn read_directory_tree(
    _path: &Path,
) -> std::io::Result<DirectoryTree> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "secure snapshot traversal is supported only on Unix platforms",
    ))
}
