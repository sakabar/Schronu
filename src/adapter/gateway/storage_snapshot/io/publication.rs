#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::read_directory_names;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::ffi::CString;
use std::fs::{self, File};
use std::path::{Component, Path};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::os::unix::ffi::OsStrExt;

#[cfg(test)]
pub(in crate::adapter::gateway) trait SnapshotIo: Send + Sync {
    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        File::open(path)?.sync_all()
    }

    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_dir_all(path)
    }
}

pub(in crate::adapter::gateway::storage_snapshot) struct StableParent {
    directory: File,
}

pub(in crate::adapter::gateway::storage_snapshot) struct StableDirectory {
    directory: File,
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
        StableDirectory::open_at(self.raw_fd(), &name)
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn rename_no_replace(
        &self,
        from: &std::ffi::OsStr,
        to: &std::ffi::OsStr,
        published: &StableDirectory,
    ) -> std::io::Result<()> {
        let from = c_name(from)?;
        let to = c_name(to)?;
        if !published.matches_entry(self.raw_fd(), &from)? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "staging directory was replaced before publication",
            ));
        }
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

    pub(in crate::adapter::gateway::storage_snapshot) fn remove_directory_tree(
        &self,
        name: &std::ffi::OsStr,
    ) -> std::io::Result<()> {
        let name = c_name(name)?;
        let directory = StableDirectory::open_at(self.raw_fd(), &name)?;
        directory.remove_contents()?;
        // SAFETY: name is a live CString and names an entry below the fixed parent handle.
        if unsafe { libc::unlinkat(self.raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
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

    pub(in crate::adapter::gateway::storage_snapshot) fn sync(&self) -> std::io::Result<()> {
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
    ) -> std::io::Result<()> {
        use std::io::Write;
        use std::os::fd::FromRawFd;

        let (parent, name) = self.parent_and_name(relative)?;
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
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: openat returned a new owned descriptor transferred exactly once.
        let mut file = unsafe { File::from_raw_fd(descriptor) };
        file.write_all(bytes)?;
        file.set_permissions(permissions)?;
        file.sync_all()
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
    ) -> std::io::Result<()> {
        self.open_relative_directory(relative)?.directory.sync_all()
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn sync(&self) -> std::io::Result<()> {
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

    fn remove_contents(&self) -> std::io::Result<()> {
        for name in read_directory_names(self.raw_fd())? {
            let name = c_name(&name)?;
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            // SAFETY: stat is writable and name is live for the call.
            if unsafe {
                libc::fstatat(
                    self.raw_fd(),
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
            if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
                let child = Self::open_at(self.raw_fd(), &name)?;
                child.remove_contents()?;
                // SAFETY: name is a child of the fixed directory handle.
                if unsafe { libc::unlinkat(self.raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
            } else {
                // SAFETY: name is a child of the fixed directory handle.
                if unsafe { libc::unlinkat(self.raw_fd(), name.as_ptr(), 0) } != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
        }
        Ok(())
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
    ) -> std::io::Result<()> {
        Err(unsupported_publication())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn remove_directory_tree(
        &self,
        _name: &std::ffi::OsStr,
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

    pub(in crate::adapter::gateway::storage_snapshot) fn sync(&self) -> std::io::Result<()> {
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
    ) -> std::io::Result<()> {
        Err(unsupported_publication())
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
    ) -> std::io::Result<()> {
        Err(unsupported_publication())
    }

    pub(in crate::adapter::gateway::storage_snapshot) fn sync(&self) -> std::io::Result<()> {
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

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
fn c_path(path: &Path) -> std::io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "filesystem path contains a NUL byte",
        )
    })
}

#[cfg(all(test, target_os = "macos"))]
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

#[cfg(all(test, target_os = "linux"))]
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

#[cfg(all(test, not(any(target_os = "macos", target_os = "linux"))))]
pub(in crate::adapter::gateway) fn rename_no_replace(
    _from: &Path,
    _to: &Path,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic no-replace rename is supported only on macOS and Linux",
    ))
}
