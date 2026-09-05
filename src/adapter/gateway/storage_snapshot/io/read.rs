use super::DirectoryTree;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::{TreeDirectory, TreeFile};
use crate::adapter::gateway::storage_snapshot::error::{
    SnapshotError, SnapshotLimitKind, SnapshotOperation,
};
use crate::adapter::gateway::storage_snapshot::layout::MANIFEST_FILE_NAME;
use crate::adapter::gateway::storage_snapshot::SnapshotResourceLimits;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::fs::File;
use std::path::Path;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::ffi::CString;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::os::unix::ffi::OsStrExt;

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(in crate::adapter::gateway::storage_snapshot) fn read_directory_tree_with_limits(
    path: &Path,
    limits: SnapshotResourceLimits,
) -> Result<DirectoryTree, SnapshotError> {
    use std::os::unix::fs::OpenOptionsExt;

    let root = File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| read_error(path, error))?;
    if !root
        .metadata()
        .map_err(|error| read_error(path, error))?
        .is_dir()
    {
        return Err(read_error(
            path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "snapshot root must be a directory",
            ),
        ));
    }
    let mut tree = DirectoryTree {
        directories: Vec::new(),
        files: Vec::new(),
    };
    let mut usage = TraversalUsage::default();
    read_directory_handle(&root, path, Path::new(""), &mut tree, &mut usage, limits)?;
    Ok(tree)
}

#[derive(Default)]
struct TraversalUsage {
    file_count: usize,
    total_bytes: u64,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read_directory_handle(
    directory: &File,
    root: &Path,
    relative: &Path,
    tree: &mut DirectoryTree,
    usage: &mut TraversalUsage,
    limits: SnapshotResourceLimits,
) -> Result<(), SnapshotError> {
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd};

    for name in super::read_directory_names(directory.as_raw_fd())
        .map_err(|error| read_error(root.join(relative), error))?
    {
        let name = name.map_err(|error| read_error(root.join(relative), error))?;
        let child_path = relative.join(&name);
        let display_path = root.join(&child_path);
        let logical_path = child_path
            .strip_prefix("storage")
            .ok()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(&child_path);
        if child_path != Path::new(MANIFEST_FILE_NAME) && child_path != Path::new("storage") {
            limits.check_path(&display_path, logical_path)?;
        }
        let name = CString::new(name.as_bytes()).map_err(|_| {
            read_error(
                &display_path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "filesystem entry contains a NUL byte",
                ),
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
            return Err(read_error(
                &display_path,
                std::io::Error::new(
                    error.kind(),
                    format!("openat {} failed: {error}", child_path.display()),
                ),
            ));
        }
        // SAFETY: openat returned a new owned descriptor which is transferred to File exactly once.
        let mut child = unsafe { File::from_raw_fd(descriptor) };
        let metadata = child
            .metadata()
            .map_err(|error| read_error(&display_path, error))?;
        if metadata.is_dir() {
            tree.directories.push(TreeDirectory {
                path: child_path.clone(),
                permissions: metadata.permissions(),
            });
            read_directory_handle(&child, root, &child_path, tree, usage, limits)?;
        } else if metadata.is_file() {
            let is_manifest = child_path == Path::new(MANIFEST_FILE_NAME);
            let byte_limit = if is_manifest {
                limits.manifest_bytes
            } else {
                let observed_count = usage.file_count.checked_add(1).ok_or_else(|| {
                    SnapshotError::limit(
                        &display_path,
                        SnapshotLimitKind::FileCount,
                        limits.file_count as u64,
                        u64::MAX,
                        Some(logical_path.to_path_buf()),
                    )
                })?;
                limits.check(
                    &display_path,
                    Some(logical_path),
                    SnapshotLimitKind::FileCount,
                    limits.file_count as u64,
                    observed_count as u64,
                )?;
                limits.check(
                    &display_path,
                    Some(logical_path),
                    SnapshotLimitKind::FileBytes,
                    limits.file_bytes,
                    metadata.len(),
                )?;
                let observed_total =
                    usage
                        .total_bytes
                        .checked_add(metadata.len())
                        .ok_or_else(|| {
                            SnapshotError::limit(
                                &display_path,
                                SnapshotLimitKind::PayloadBytes,
                                limits.total_bytes,
                                u64::MAX,
                                Some(logical_path.to_path_buf()),
                            )
                        })?;
                limits.check(
                    &display_path,
                    Some(logical_path),
                    SnapshotLimitKind::PayloadBytes,
                    limits.total_bytes,
                    observed_total,
                )?;
                limits
                    .file_bytes
                    .min(limits.total_bytes - usage.total_bytes)
            };
            let limit_kind = if is_manifest {
                SnapshotLimitKind::ManifestBytes
            } else {
                SnapshotLimitKind::FileBytes
            };
            limits.check(
                &display_path,
                (!is_manifest).then_some(logical_path),
                limit_kind,
                byte_limit,
                metadata.len(),
            )?;
            let mut bytes = Vec::new();
            let capacity = usize::try_from(metadata.len()).map_err(|error| {
                SnapshotError::new(SnapshotOperation::Read, &display_path, error)
            })?;
            bytes.try_reserve_exact(capacity).map_err(|error| {
                SnapshotError::new(SnapshotOperation::Read, &display_path, error)
            })?;
            child
                .by_ref()
                .take(byte_limit.saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(|error| read_error(&display_path, error))?;
            limits.check(
                &display_path,
                (!is_manifest).then_some(logical_path),
                limit_kind,
                byte_limit,
                bytes.len() as u64,
            )?;
            if !is_manifest {
                usage.file_count += 1;
                usage.total_bytes = usage
                    .total_bytes
                    .checked_add(bytes.len() as u64)
                    .ok_or_else(|| {
                        SnapshotError::limit(
                            &display_path,
                            SnapshotLimitKind::PayloadBytes,
                            limits.total_bytes,
                            u64::MAX,
                            Some(logical_path.to_path_buf()),
                        )
                    })?;
                limits.check(
                    &display_path,
                    Some(logical_path),
                    SnapshotLimitKind::PayloadBytes,
                    limits.total_bytes,
                    usage.total_bytes,
                )?;
            }
            tree.files.push(TreeFile {
                path: child_path,
                bytes,
                permissions: metadata.permissions(),
            });
        } else {
            return Err(read_error(
                &display_path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "directory tree contains a non-regular entry",
                ),
            ));
        }
    }
    Ok(())
}

fn read_error(
    path: impl Into<std::path::PathBuf>,
    error: impl std::error::Error + Send + Sync + 'static,
) -> SnapshotError {
    SnapshotError::new(SnapshotOperation::Read, path, error)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(in crate::adapter::gateway::storage_snapshot) fn read_directory_tree_with_limits(
    path: &Path,
    _limits: SnapshotResourceLimits,
) -> Result<DirectoryTree, SnapshotError> {
    Err(read_error(
        path,
        std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "secure snapshot traversal is supported only on Unix platforms",
        ),
    ))
}
