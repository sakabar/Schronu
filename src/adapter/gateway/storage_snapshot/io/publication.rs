use super::FileWriteError;
use crate::adapter::gateway::storage_snapshot::error::{SnapshotError, SnapshotOperation};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::ffi::CString;
use std::fs::{self, File};
use std::path::{Component, Path};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::os::unix::ffi::OsStrExt;

#[path = "cleanup.rs"]
mod cleanup;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::adapter::gateway) enum SnapshotFailurePoint {
    Read,
    Copy,
    Write,
    Permission,
    FileSync,
    DirectorySync,
    Rename,
    ParentSync,
}

pub(in crate::adapter::gateway::storage_snapshot) trait SnapshotIo:
    Send + Sync
{
    fn before(&self, _point: SnapshotFailurePoint) -> std::io::Result<()> {
        Ok(())
    }
}

pub(in crate::adapter::gateway::storage_snapshot) struct FileSystemSnapshotIo;
impl SnapshotIo for FileSystemSnapshotIo {}

#[cfg(test)]
pub(in crate::adapter::gateway) struct FailOnceSnapshotIo {
    point: SnapshotFailurePoint,
    failed: std::sync::atomic::AtomicBool,
    matching_calls: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl FailOnceSnapshotIo {
    pub(in crate::adapter::gateway) fn new(point: SnapshotFailurePoint) -> Self {
        Self {
            point,
            failed: std::sync::atomic::AtomicBool::new(false),
            matching_calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub(in crate::adapter::gateway) fn matching_calls(&self) -> usize {
        self.matching_calls
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
impl SnapshotIo for FailOnceSnapshotIo {
    fn before(&self, point: SnapshotFailurePoint) -> std::io::Result<()> {
        use std::sync::atomic::Ordering;

        if point != self.point {
            return Ok(());
        }
        self.matching_calls.fetch_add(1, Ordering::SeqCst);
        if !self.failed.swap(true, Ordering::SeqCst) {
            Err(std::io::Error::other(format!("injected {point:?} failure")))
        } else {
            Ok(())
        }
    }
}

pub(in crate::adapter::gateway::storage_snapshot) struct StableParent {
    directory: File,
}

pub(in crate::adapter::gateway::storage_snapshot) struct StableDirectory {
    directory: File,
}

pub(in crate::adapter::gateway::storage_snapshot) fn finalize_publication(
    parent: &StableParent,
    staging_name: &std::ffi::OsStr,
    destination_name: &std::ffi::OsStr,
    published: &StableDirectory,
    destination: &Path,
    io: &dyn SnapshotIo,
) -> Result<(), SnapshotError> {
    parent
        .rename_no_replace(staging_name, destination_name, published, io)
        .map_err(|error| SnapshotError::new(SnapshotOperation::Write, destination, error))?;
    if let Err(sync_error) = parent.sync(io) {
        let primary = SnapshotError::new(
            SnapshotOperation::Sync,
            destination.parent().expect("destination has a parent"),
            sync_error,
        );
        if let Err(cleanup_error) = parent.remove_published_directory(destination_name, published) {
            return Err(SnapshotError::followup_failure(
                primary,
                "cleanup",
                cleanup_error,
            ));
        }
        if let Err(sync_error) = parent.sync(io) {
            return Err(SnapshotError::followup_failure(
                primary,
                "rollback parent sync",
                sync_error,
            ));
        }
        return Err(primary);
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl StableParent {
    pub(in crate::adapter::gateway::storage_snapshot) fn open(
        path: &Path,
    ) -> std::io::Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;

        let directory = File::options()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        Ok(Self { directory })
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn matches_path(
        &self,
        path: &Path,
    ) -> std::io::Result<bool> {
        use std::os::unix::fs::MetadataExt;

        let handle = self.directory.metadata()?;
        let path = fs::metadata(path)?;
        Ok(handle.dev() == path.dev() && handle.ino() == path.ino())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn is_within(
        &self,
        ancestor: &Path,
    ) -> std::io::Result<bool> {
        use std::os::unix::fs::MetadataExt;

        let ancestor = fs::metadata(ancestor)?;
        let mut current = self.directory.try_clone()?;
        loop {
            let metadata = current.metadata()?;
            if metadata.dev() == ancestor.dev() && metadata.ino() == ancestor.ino() {
                return Ok(true);
            }
            let parent = open_parent_directory(&current)?;
            let parent_metadata = parent.metadata()?;
            if parent_metadata.dev() == metadata.dev() && parent_metadata.ino() == metadata.ino() {
                return Ok(false);
            }
            current = parent;
        }
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn entry_exists(
        &self,
        name: &std::ffi::OsStr,
    ) -> std::io::Result<bool> {
        let name = c_name(name)?;
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: stat points to writable storage and name is a live CString.
        let result = unsafe {
            libc::fstatat(
                self.raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            Ok(true)
        } else {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ENOENT) {
                Ok(false)
            } else {
                Err(error)
            }
        }
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn create_directory(
        &self,
        name: &std::ffi::OsStr,
    ) -> std::io::Result<StableDirectory> {
        let name = c_name(name)?;
        // SAFETY: name is a live CString and raw_fd remains open for the call.
        if unsafe { libc::mkdirat(self.raw_fd(), name.as_ptr(), 0o700) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        match StableDirectory::open_at(self.raw_fd(), &name) {
            Ok(directory) => Ok(directory),
            Err(open_error) => {
                // SAFETY: this call removes only the empty directory just created below this fd.
                if unsafe { libc::unlinkat(self.raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } == 0
                {
                    Err(open_error)
                } else {
                    Err(std::io::Error::other(format!(
                        "opening the created staging directory failed ({open_error}); cleanup failed ({})",
                        std::io::Error::last_os_error()
                    )))
                }
            }
        }
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn rename_no_replace(
        &self,
        from: &std::ffi::OsStr,
        to: &std::ffi::OsStr,
        published: &StableDirectory,
        io: &dyn SnapshotIo,
    ) -> std::io::Result<()> {
        let from = c_name(from)?;
        let to = c_name(to)?;
        if !published.matches_entry(self.raw_fd(), &from)? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "staging directory was replaced before publication",
            ));
        }
        io.before(SnapshotFailurePoint::Rename)?;
        rename_at_no_replace(self.raw_fd(), &from, &to)?;
        if published.matches_entry(self.raw_fd(), &to)? {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "published destination does not match the staged directory",
            ))
        }
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn remove_published_directory(
        &self,
        name: &std::ffi::OsStr,
        published: &StableDirectory,
    ) -> std::io::Result<()> {
        let name = c_name(name)?;
        if !published.matches_entry(self.raw_fd(), &name)? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "published destination was replaced before rollback",
            ));
        }
        published.remove_contents()?;
        if !published.matches_entry(self.raw_fd(), &name)? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "published destination was replaced during rollback",
            ));
        }
        // SAFETY: the entry identity was checked against the retained published handle.
        if unsafe { libc::unlinkat(self.raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn remove_published_directory_if_present(
        &self,
        name: &std::ffi::OsStr,
        published: &StableDirectory,
    ) -> std::io::Result<()> {
        if self.entry_exists(name)? {
            self.remove_published_directory(name, published)
        } else {
            Ok(())
        }
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn sync(
        &self,
        io: &dyn SnapshotIo,
    ) -> std::io::Result<()> {
        io.before(SnapshotFailurePoint::ParentSync)?;
        self.directory.sync_all()
    }

    fn raw_fd(&self) -> std::os::fd::RawFd {
        use std::os::fd::AsRawFd;
        self.directory.as_raw_fd()
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl StableDirectory {
    fn open_at(parent: std::os::fd::RawFd, name: &CString) -> std::io::Result<Self> {
        use std::os::fd::FromRawFd;

        // SAFETY: name is live, and a successful descriptor is uniquely owned below.
        let descriptor = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: openat returned a new owned descriptor transferred exactly once.
        let directory = unsafe { File::from_raw_fd(descriptor) };
        Ok(Self { directory })
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn create_directory(
        &self,
        relative: &Path,
    ) -> std::io::Result<()> {
        let (parent, name) = self.parent_and_name(relative)?;
        // SAFETY: name is live and the parent descriptor remains open for the call.
        if unsafe { libc::mkdirat(parent.raw_fd(), name.as_ptr(), 0o700) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn write_file(
        &self,
        relative: &Path,
        bytes: &[u8],
        permissions: fs::Permissions,
        io: &dyn SnapshotIo,
    ) -> Result<(), FileWriteError> {
        use std::io::Write;
        use std::os::fd::FromRawFd;

        let (parent, name) = self
            .parent_and_name(relative)
            .map_err(FileWriteError::write)?;
        // SAFETY: name is live, and a successful descriptor is uniquely owned below.
        let descriptor = unsafe {
            libc::openat(
                parent.raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if descriptor < 0 {
            return Err(FileWriteError::write(std::io::Error::last_os_error()));
        }
        // SAFETY: openat returned a new owned descriptor transferred exactly once.
        let mut file = unsafe { File::from_raw_fd(descriptor) };
        io.before(SnapshotFailurePoint::Write)
            .map_err(FileWriteError::write)?;
        file.write_all(bytes).map_err(FileWriteError::write)?;
        io.before(SnapshotFailurePoint::Permission)
            .map_err(FileWriteError::write)?;
        file.set_permissions(permissions)
            .map_err(FileWriteError::write)?;
        io.before(SnapshotFailurePoint::FileSync)
            .map_err(FileWriteError::sync)?;
        file.sync_all().map_err(FileWriteError::sync)
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn set_directory_permissions(
        &self,
        relative: &Path,
        permissions: fs::Permissions,
    ) -> std::io::Result<()> {
        let directory = self.open_relative_directory(relative)?;
        directory.directory.set_permissions(permissions)
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn sync_directory(
        &self,
        relative: &Path,
        io: &dyn SnapshotIo,
    ) -> std::io::Result<()> {
        io.before(SnapshotFailurePoint::DirectorySync)?;
        self.open_relative_directory(relative)?.directory.sync_all()
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn sync(
        &self,
        io: &dyn SnapshotIo,
    ) -> std::io::Result<()> {
        io.before(SnapshotFailurePoint::DirectorySync)?;
        self.directory.sync_all()
    }

    fn parent_and_name(&self, relative: &Path) -> std::io::Result<(Self, CString)> {
        let name = relative
            .file_name()
            .ok_or_else(|| invalid_relative(relative))?;
        let parent = relative.parent().unwrap_or_else(|| Path::new(""));
        Ok((self.open_relative_directory(parent)?, c_name(name)?))
    }

    fn open_relative_directory(&self, relative: &Path) -> std::io::Result<Self> {
        let mut current = Self {
            directory: self.directory.try_clone()?,
        };
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(invalid_relative(relative));
            };
            current = Self::open_at(current.raw_fd(), &c_name(name)?)?;
        }
        Ok(current)
    }

    fn matches_entry(&self, parent: std::os::fd::RawFd, name: &CString) -> std::io::Result<bool> {
        use std::os::unix::fs::MetadataExt;

        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: stat is writable and name is live for the call.
        if unsafe {
            libc::fstatat(
                parent,
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: fstatat initialized stat on success.
        let stat = unsafe { stat.assume_init() };
        let metadata = self.directory.metadata()?;
        Ok(metadata.dev() == stat.st_dev as u64 && metadata.ino() == stat.st_ino)
    }

    fn raw_fd(&self) -> std::os::fd::RawFd {
        use std::os::fd::AsRawFd;
        self.directory.as_raw_fd()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
impl StableParent {
    pub(in crate::adapter::gateway::storage_snapshot) fn open(
        _path: &Path,
    ) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "stable snapshot publication is supported only on Unix platforms",
        ))
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn matches_path(
        &self,
        _path: &Path,
    ) -> std::io::Result<bool> {
        Err(unsupported_publication())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn is_within(
        &self,
        _ancestor: &Path,
    ) -> std::io::Result<bool> {
        Err(unsupported_publication())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn entry_exists(
        &self,
        _name: &std::ffi::OsStr,
    ) -> std::io::Result<bool> {
        Err(unsupported_publication())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn create_directory(
        &self,
        _name: &std::ffi::OsStr,
    ) -> std::io::Result<StableDirectory> {
        Err(unsupported_publication())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn rename_no_replace(
        &self,
        _from: &std::ffi::OsStr,
        _to: &std::ffi::OsStr,
        _published: &StableDirectory,
        _io: &dyn SnapshotIo,
    ) -> std::io::Result<()> {
        Err(unsupported_publication())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn remove_published_directory(
        &self,
        _name: &std::ffi::OsStr,
        _published: &StableDirectory,
    ) -> std::io::Result<()> {
        Err(unsupported_publication())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn remove_published_directory_if_present(
        &self,
        _name: &std::ffi::OsStr,
        _published: &StableDirectory,
    ) -> std::io::Result<()> {
        Err(unsupported_publication())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn sync(
        &self,
        _io: &dyn SnapshotIo,
    ) -> std::io::Result<()> {
        Err(unsupported_publication())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
impl StableDirectory {
    pub(in crate::adapter::gateway::storage_snapshot) fn create_directory(
        &self,
        _relative: &Path,
    ) -> std::io::Result<()> {
        Err(unsupported_publication())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn write_file(
        &self,
        _relative: &Path,
        _bytes: &[u8],
        _permissions: fs::Permissions,
        _io: &dyn SnapshotIo,
    ) -> Result<(), FileWriteError> {
        Err(FileWriteError::write(unsupported_publication()))
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn set_directory_permissions(
        &self,
        _relative: &Path,
        _permissions: fs::Permissions,
    ) -> std::io::Result<()> {
        Err(unsupported_publication())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn sync_directory(
        &self,
        _relative: &Path,
        _io: &dyn SnapshotIo,
    ) -> std::io::Result<()> {
        Err(unsupported_publication())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn sync(
        &self,
        _io: &dyn SnapshotIo,
    ) -> std::io::Result<()> {
        Err(unsupported_publication())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn unsupported_publication() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "stable snapshot publication is supported only on Unix platforms",
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn c_name(name: &std::ffi::OsStr) -> std::io::Result<CString> {
    if Path::new(name).components().count() != 1
        || !matches!(
            Path::new(name).components().next(),
            Some(Component::Normal(_))
        )
    {
        return Err(invalid_relative(Path::new(name)));
    }
    CString::new(name.as_bytes()).map_err(|_| invalid_relative(Path::new(name)))
}

fn invalid_relative(path: &Path) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "path must contain only normal relative components: {}",
            path.display()
        ),
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn open_parent_directory(directory: &File) -> std::io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};

    // SAFETY: the static name is NUL-terminated, and a successful descriptor is owned below.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c"..".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        // SAFETY: openat returned a new owned descriptor transferred exactly once.
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }
}

#[cfg(target_os = "macos")]
fn rename_at_no_replace(
    parent: std::os::fd::RawFd,
    from: &CString,
    to: &CString,
) -> std::io::Result<()> {
    // SAFETY: names are live CStrings and parent remains open for the call.
    let result = unsafe {
        libc::renameatx_np(
            parent,
            from.as_ptr(),
            parent,
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn rename_at_no_replace(
    parent: std::os::fd::RawFd,
    from: &CString,
    to: &CString,
) -> std::io::Result<()> {
    // SAFETY: names are live CStrings and parent remains open for the call.
    let result = unsafe {
        libc::renameat2(
            parent,
            from.as_ptr(),
            parent,
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
