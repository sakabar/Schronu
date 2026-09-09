use super::layout::{self, validate_storage_relative_path, TransactionLayout};
use super::{StorageTransactionError, StorageTransactionOperation};
use fs2::FileExt;
use std::fs::{self, File, Metadata};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

#[cfg(test)]
pub(super) use super::layout::TRANSACTION_LOCK_FILE_NAME;

pub(crate) trait StorageTransactionIo: Send + Sync {
    fn write_storage_file_anchored(
        &self,
        _storage_dir_path: &Path,
        _relative_path: &Path,
        _transaction_id: Uuid,
        _bytes: &[u8],
        _permissions: Option<&fs::Permissions>,
    ) -> Result<bool, StorageTransactionError> {
        Ok(false)
    }

    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::create_dir_all(path)
    }

    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        fs::create_dir(path)
    }

    fn target_permissions(&self, path: &Path) -> std::io::Result<Option<fs::Permissions>> {
        match fs::metadata(path) {
            Ok(metadata) => Ok(Some(metadata.permissions())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn symlink_metadata(&self, path: &Path) -> std::io::Result<fs::Metadata> {
        fs::symlink_metadata(path)
    }

    fn read_directory_paths(&self, path: &Path) -> std::io::Result<Vec<PathBuf>> {
        Ok(fs::read_dir(path)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect())
    }

    fn create_new_file(&self, path: &Path) -> std::io::Result<()> {
        File::options()
            .write(true)
            .create_new(true)
            .open(path)
            .map(drop)
    }

    fn set_permissions(&self, path: &Path, permissions: fs::Permissions) -> std::io::Result<()> {
        fs::set_permissions(path, permissions)
    }

    fn set_and_sync_directory_permissions(
        &self,
        path: &Path,
        permissions: fs::Permissions,
    ) -> Result<(), DirectoryPermissionError> {
        self.set_permissions(path, permissions)
            .map_err(DirectoryPermissionError::Set)?;
        self.sync_directory(path)
            .map_err(DirectoryPermissionError::Sync)
    }

    fn write_file(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        File::options().write(true).open(path)?.write_all(bytes)
    }

    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn sync_file(&self, path: &Path) -> std::io::Result<()> {
        File::open(path)?.sync_all()
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        File::open(path)?.sync_all()
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_dir(path)
    }

    fn remove_storage_entry(
        &self,
        storage_dir_path: &Path,
        relative_path: &Path,
    ) -> std::io::Result<()> {
        let target_path = storage_dir_path.join(relative_path);
        let metadata = self.symlink_metadata(&target_path)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            self.remove_dir(&target_path)
        } else {
            self.remove_file(&target_path)
        }
    }

    fn remove_storage_directory_if_present(
        &self,
        storage_dir_path: &Path,
        relative_path: &Path,
    ) -> std::io::Result<()> {
        let target_path = storage_dir_path.join(relative_path);
        match self.symlink_metadata(&target_path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                self.remove_dir(&target_path)
            }
            Ok(_) => Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

pub(crate) enum DirectoryPermissionError {
    Set(std::io::Error),
    Sync(std::io::Error),
}

#[derive(Default)]
pub(in crate::adapter::gateway) struct FileSystemStorageTransactionIo;
impl StorageTransactionIo for FileSystemStorageTransactionIo {
    fn write_storage_file_anchored(
        &self,
        storage_dir_path: &Path,
        relative_path: &Path,
        transaction_id: Uuid,
        bytes: &[u8],
        permissions: Option<&fs::Permissions>,
    ) -> Result<bool, StorageTransactionError> {
        write_storage_file_anchored_secure(
            storage_dir_path,
            relative_path,
            transaction_id,
            bytes,
            permissions,
            || {},
        )?;
        Ok(true)
    }

    fn set_and_sync_directory_permissions(
        &self,
        path: &Path,
        permissions: fs::Permissions,
    ) -> Result<(), DirectoryPermissionError> {
        let directory = File::open(path).map_err(DirectoryPermissionError::Set)?;
        directory
            .set_permissions(permissions)
            .map_err(DirectoryPermissionError::Set)?;
        directory.sync_all().map_err(DirectoryPermissionError::Sync)
    }

    fn remove_storage_entry(
        &self,
        storage_dir_path: &Path,
        relative_path: &Path,
    ) -> std::io::Result<()> {
        remove_storage_entry_secure(storage_dir_path, relative_path)
    }

    fn remove_storage_directory_if_present(
        &self,
        storage_dir_path: &Path,
        relative_path: &Path,
    ) -> std::io::Result<()> {
        remove_storage_directory_if_present_secure(storage_dir_path, relative_path)
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn write_storage_file_anchored_secure(
    storage_dir_path: &Path,
    relative_path: &Path,
    transaction_id: Uuid,
    bytes: &[u8],
    permissions: Option<&fs::Permissions>,
    after_parent_open: impl FnOnce(),
) -> Result<(), StorageTransactionError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    let target_path = storage_dir_path.join(relative_path);
    validate_storage_relative_path(storage_dir_path, &target_path)?;
    let (parent, target_name) = open_storage_entry_parent(storage_dir_path, relative_path)
        .map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::ValidateTargetPath,
                &target_path,
                error,
            )
        })?;
    after_parent_open();
    let (current_parent, current_target_name) =
        open_storage_entry_parent(storage_dir_path, relative_path).map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::ValidateTargetPath,
                &target_path,
                error,
            )
        })?;
    let opened_metadata = parent.metadata().map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::ValidateTargetPath,
            &target_path,
            error,
        )
    })?;
    let current_metadata = current_parent.metadata().map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::ValidateTargetPath,
            &target_path,
            error,
        )
    })?;
    if opened_metadata.dev() != current_metadata.dev()
        || opened_metadata.ino() != current_metadata.ino()
        || target_name != current_target_name
    {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::ValidateTargetPath,
            &target_path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "write target parent changed after it was opened",
            ),
        ));
    }

    let temporary_name =
        anchored_temporary_name(&target_name, transaction_id).map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::CreateLiveTemporary,
                &target_path,
                error,
            )
        })?;
    let open_temporary = || unsafe {
        libc::openat(
            parent.as_raw_fd(),
            temporary_name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    let mut descriptor = open_temporary();
    if descriptor < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists
    {
        if unsafe { libc::unlinkat(parent.as_raw_fd(), temporary_name.as_ptr(), 0) } != 0 {
            return Err(StorageTransactionError::new(
                StorageTransactionOperation::RemoveLiveTemporary,
                &target_path,
                std::io::Error::last_os_error(),
            ));
        }
        descriptor = open_temporary();
    }
    if descriptor < 0 {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::CreateLiveTemporary,
            &target_path,
            std::io::Error::last_os_error(),
        ));
    }
    // SAFETY: openat returned a new descriptor whose ownership is transferred once.
    let mut temporary = unsafe { File::from_raw_fd(descriptor) };
    if let Some(permissions) = permissions {
        if unsafe { libc::fchmod(temporary.as_raw_fd(), permissions.mode() as libc::mode_t) } != 0 {
            return Err(StorageTransactionError::new(
                StorageTransactionOperation::SetLivePermissions,
                &target_path,
                std::io::Error::last_os_error(),
            ));
        }
    }
    temporary.write_all(bytes).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::WriteLiveTemporary,
            &target_path,
            error,
        )
    })?;
    temporary.sync_all().map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::SyncLiveTemporary,
            &target_path,
            error,
        )
    })?;
    if unsafe {
        libc::renameat(
            parent.as_raw_fd(),
            temporary_name.as_ptr(),
            parent.as_raw_fd(),
            target_name.as_ptr(),
        )
    } != 0
    {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::RenameLiveTarget,
            &target_path,
            std::io::Error::last_os_error(),
        ));
    }
    parent.sync_all().map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::SyncDirectory,
            target_path.parent().unwrap_or(storage_dir_path),
            error,
        )
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn write_storage_file_anchored_secure(
    storage_dir_path: &Path,
    _relative_path: &Path,
    _transaction_id: Uuid,
    _bytes: &[u8],
    _permissions: Option<&fs::Permissions>,
    _after_parent_open: impl FnOnce(),
) -> Result<(), StorageTransactionError> {
    Err(StorageTransactionError::new(
        StorageTransactionOperation::ValidateTargetPath,
        storage_dir_path,
        std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "anchored storage writes are supported only on macOS and Linux",
        ),
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn anchored_temporary_name(
    target_name: &std::ffi::CStr,
    transaction_id: Uuid,
) -> std::io::Result<std::ffi::CString> {
    let mut bytes = Vec::with_capacity(target_name.to_bytes().len() + 40);
    bytes.push(b'.');
    bytes.extend_from_slice(target_name.to_bytes());
    bytes.push(b'.');
    bytes.extend_from_slice(transaction_id.hyphenated().to_string().as_bytes());
    bytes.extend_from_slice(b".tmp");
    std::ffi::CString::new(bytes).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "write target contains a NUL byte",
        )
    })
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
pub(in crate::adapter::gateway) fn write_storage_file_anchored_after_parent_open(
    storage_dir_path: &Path,
    relative_path: &Path,
    transaction_id: Uuid,
    bytes: &[u8],
    permissions: Option<&fs::Permissions>,
    after_parent_open: impl FnOnce(),
) -> Result<(), StorageTransactionError> {
    write_storage_file_anchored_secure(
        storage_dir_path,
        relative_path,
        transaction_id,
        bytes,
        permissions,
        after_parent_open,
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn remove_storage_directory_if_present_secure(
    storage_dir_path: &Path,
    relative_path: &Path,
) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    let (directory, name) = match open_storage_entry_parent(storage_dir_path, relative_path) {
        Ok(parent) => parent,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let metadata = match storage_entry_metadata(&directory, &name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return Ok(());
    }
    // SAFETY: unlinkat is constrained to a directory below the retained parent fd.
    if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn remove_storage_directory_if_present_secure(
    storage_dir_path: &Path,
    relative_path: &Path,
) -> std::io::Result<()> {
    let target_path = storage_dir_path.join(relative_path);
    match fs::symlink_metadata(&target_path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir(target_path)
        }
        Ok(_) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn remove_storage_entry_secure(
    storage_dir_path: &Path,
    relative_path: &Path,
) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    let (directory, name) = open_storage_entry_parent(storage_dir_path, relative_path)?;
    let metadata = storage_entry_metadata(&directory, &name)?;
    let flags = if metadata.st_mode & libc::S_IFMT == libc::S_IFDIR {
        libc::AT_REMOVEDIR
    } else {
        0
    };
    // SAFETY: unlinkat operates below the retained directory fd on the validated final name.
    if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), flags) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn open_storage_entry_parent(
    storage_dir_path: &Path,
    relative_path: &Path,
) -> std::io::Result<(File, std::ffi::CString)> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    let mut directory = File::options()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(storage_dir_path)?;
    let mut components = relative_path.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "storage target must contain only normalized components",
            ));
        };
        let name = CString::new(name.as_bytes()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "storage target contains a NUL byte",
            )
        })?;
        if components.peek().is_none() {
            return Ok((directory, name));
        }
        // SAFETY: directory and name remain live for the call; a successful fd is owned below.
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY
                    | libc::O_DIRECTORY
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: openat returned a new owned descriptor.
        directory = unsafe { File::from_raw_fd(fd) };
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "storage target must not be empty",
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn storage_entry_metadata(directory: &File, name: &std::ffi::CStr) -> std::io::Result<libc::stat> {
    use std::os::fd::AsRawFd;

    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: metadata is writable and name and directory remain live for the call.
    if unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: fstatat initialized metadata on success.
    Ok(unsafe { metadata.assume_init() })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn remove_storage_entry_secure(
    storage_dir_path: &Path,
    relative_path: &Path,
) -> std::io::Result<()> {
    let target_path = storage_dir_path.join(relative_path);
    let metadata = fs::symlink_metadata(&target_path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir(target_path)
    } else {
        fs::remove_file(target_path)
    }
}

pub(super) fn validate_delete_target(
    io: &dyn StorageTransactionIo,
    storage_dir_path: &Path,
    target_path: &Path,
) -> Result<PathBuf, StorageTransactionError> {
    let target = validate_delete_target_path(storage_dir_path, target_path)?;
    validate_target_ancestors(
        io,
        storage_dir_path,
        &target,
        "delete target ancestors must be directories and must not be symbolic links",
    )?;
    Ok(target)
}

pub(super) fn validate_write_target(
    io: &dyn StorageTransactionIo,
    storage_dir_path: &Path,
    target_path: &Path,
    allow_non_directory_ancestor: bool,
) -> Result<PathBuf, StorageTransactionError> {
    let target = validate_storage_relative_path(storage_dir_path, target_path)?;
    validate_write_target_ancestors(io, storage_dir_path, &target, allow_non_directory_ancestor)?;
    Ok(target)
}

fn validate_write_target_ancestors(
    io: &dyn StorageTransactionIo,
    storage_dir_path: &Path,
    target: &Path,
    allow_non_directory_ancestor: bool,
) -> Result<(), StorageTransactionError> {
    let mut ancestor_path = storage_dir_path.to_path_buf();
    let Some(parent) = target.parent() else {
        return Ok(());
    };
    for component in parent.components() {
        let Component::Normal(name) = component else {
            unreachable!("validated transaction target must contain only normal components");
        };
        ancestor_path.push(name);
        match io.symlink_metadata(&ancestor_path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(metadata) if allow_non_directory_ancestor && !metadata.file_type().is_symlink() => {
                break;
            }
            Ok(_) => {
                return Err(layout::invalid_target_path_error(
                    &ancestor_path,
                    "write target ancestors must be directories and must not be symbolic links",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error)
                if allow_non_directory_ancestor
                    && error.kind() == std::io::ErrorKind::NotADirectory =>
            {
                break;
            }
            Err(error) => {
                return Err(StorageTransactionError::new(
                    StorageTransactionOperation::ValidateTargetPath,
                    &ancestor_path,
                    error,
                ));
            }
        }
    }
    Ok(())
}

fn validate_target_ancestors(
    io: &dyn StorageTransactionIo,
    storage_dir_path: &Path,
    target: &Path,
    invalid_message: &'static str,
) -> Result<(), StorageTransactionError> {
    let mut ancestor_path = storage_dir_path.to_path_buf();
    let Some(parent) = target.parent() else {
        return Ok(());
    };
    for component in parent.components() {
        let Component::Normal(name) = component else {
            unreachable!("validated transaction target must contain only normal components");
        };
        ancestor_path.push(name);
        match io.symlink_metadata(&ancestor_path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(layout::invalid_target_path_error(
                    &ancestor_path,
                    invalid_message,
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(StorageTransactionError::new(
                    StorageTransactionOperation::ValidateTargetPath,
                    &ancestor_path,
                    error,
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_delete_target_path(
    storage_dir_path: &Path,
    target_path: &Path,
) -> Result<PathBuf, StorageTransactionError> {
    let target = validate_storage_relative_path(storage_dir_path, target_path)?;
    if matches!(target.to_str(), Some(".lock" | ".revision")) {
        return Err(layout::invalid_target_path_error(
            target_path,
            "delete target must not use a reserved storage file",
        ));
    }
    Ok(target)
}

pub(super) fn resolve_transactions_directory(
    io: &dyn StorageTransactionIo,
    storage_dir_path: &Path,
    create: bool,
) -> Result<Option<PathBuf>, StorageTransactionError> {
    let transactions_dir_path = TransactionLayout::new(storage_dir_path).transactions_dir_path();
    let (metadata, created) = match io.symlink_metadata(&transactions_dir_path) {
        Ok(metadata) => (metadata, false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            io.create_dir_all(&transactions_dir_path).map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::CreateTransactionDirectory,
                    &transactions_dir_path,
                    error,
                )
            })?;
            (
                io.symlink_metadata(&transactions_dir_path)
                    .map_err(|error| {
                        StorageTransactionError::new(
                            StorageTransactionOperation::ValidateTransactionDirectory,
                            &transactions_dir_path,
                            error,
                        )
                    })?,
                true,
            )
        }
        Err(error) => {
            return Err(StorageTransactionError::new(
                StorageTransactionOperation::ValidateTransactionDirectory,
                &transactions_dir_path,
                error,
            ));
        }
    };
    validate_transactions_directory_metadata(&transactions_dir_path, &metadata)?;
    if create && !created {
        io.create_dir_all(&transactions_dir_path).map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::CreateTransactionDirectory,
                &transactions_dir_path,
                error,
            )
        })?;
        validate_transactions_directory(io, &transactions_dir_path)?;
    }
    Ok(Some(transactions_dir_path))
}

pub(super) fn validate_transactions_directory(
    io: &dyn StorageTransactionIo,
    path: &Path,
) -> Result<(), StorageTransactionError> {
    let metadata = io.symlink_metadata(path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::ValidateTransactionDirectory,
            path,
            error,
        )
    })?;
    validate_transactions_directory_metadata(path, &metadata)
}

fn validate_transactions_directory_metadata(
    path: &Path,
    metadata: &Metadata,
) -> Result<(), StorageTransactionError> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::ValidateTransactionDirectory,
            path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "transaction root must be a directory and must not be a symbolic link",
            ),
        ));
    }
    Ok(())
}

pub(super) struct TransactionLock {
    _file: File,
}

pub(super) fn acquire_transaction_lock(
    transactions_dir_path: &Path,
) -> Result<TransactionLock, StorageTransactionError> {
    let lock_path = TransactionLayout::transaction_lock_path(transactions_dir_path);
    let file = open_transaction_lock_file(&lock_path).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::AcquireTransactionLock,
            &lock_path,
            error,
        )
    })?;
    file.try_lock_exclusive().map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::AcquireTransactionLock,
            &lock_path,
            error,
        )
    })?;
    Ok(TransactionLock { _file: file })
}

#[cfg(unix)]
fn open_transaction_lock_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "transaction lock path must be a regular file",
        ));
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_transaction_lock_file(_path: &Path) -> std::io::Result<File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "transaction locking is supported only on Unix platforms",
    ))
}

pub(super) fn sync_directory(
    io: &dyn StorageTransactionIo,
    path: &Path,
) -> Result<(), StorageTransactionError> {
    io.sync_directory(path).map_err(|error| {
        StorageTransactionError::new(StorageTransactionOperation::SyncDirectory, path, error)
    })
}
