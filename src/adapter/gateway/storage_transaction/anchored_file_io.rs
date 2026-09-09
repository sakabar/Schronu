use std::ffi::{CStr, CString};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::io::{
    open_storage_entry_parent, AnchoredReadOutcome, AnchoredTargetFile, AnchoredTargetGuard,
};
use super::layout::validate_storage_relative_path;
use super::{StorageTransactionError, StorageTransactionOperation};

struct StableTargetParent {
    directory: File,
    target_name: CString,
    storage_dir_path: PathBuf,
    relative_path: PathBuf,
}

impl StableTargetParent {
    fn open(
        storage_dir_path: &Path,
        relative_path: &Path,
    ) -> Result<Self, StorageTransactionError> {
        let target_path = storage_dir_path.join(relative_path);
        validate_storage_relative_path(storage_dir_path, &target_path)?;
        let (directory, target_name) = open_storage_entry_parent(storage_dir_path, relative_path)
            .map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::ValidateTargetPath,
                &target_path,
                error,
            )
        })?;
        Ok(Self {
            directory,
            target_name,
            storage_dir_path: storage_dir_path.to_path_buf(),
            relative_path: relative_path.to_path_buf(),
        })
    }

    fn target_path(&self) -> PathBuf {
        self.storage_dir_path.join(&self.relative_path)
    }

    fn validate_current_parent(&self) -> Result<(), StorageTransactionError> {
        let target_path = self.target_path();
        let (current_parent, current_target_name) =
            open_storage_entry_parent(&self.storage_dir_path, &self.relative_path).map_err(
                |error| {
                    StorageTransactionError::new(
                        StorageTransactionOperation::ValidateTargetPath,
                        &target_path,
                        error,
                    )
                },
            )?;
        let opened_metadata = self.directory.metadata().map_err(|error| {
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
            || self.target_name != current_target_name
        {
            return Err(StorageTransactionError::new(
                StorageTransactionOperation::ValidateTargetPath,
                &target_path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "storage target parent changed after it was opened",
                ),
            ));
        }
        Ok(())
    }
}

struct StableTargetFile {
    parent: StableTargetParent,
    inspected_target: File,
    bytes: Vec<u8>,
    mode: u32,
}

impl AnchoredTargetGuard for StableTargetFile {
    fn validate_current(&self) -> Result<(), StorageTransactionError> {
        self.parent.validate_current_parent()?;
        let target_path = self.parent.target_path();
        let descriptor = unsafe {
            libc::openat(
                self.parent.directory.as_raw_fd(),
                self.parent.target_name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if descriptor < 0 {
            return Err(StorageTransactionError::new(
                StorageTransactionOperation::ValidateTargetPath,
                &target_path,
                std::io::Error::last_os_error(),
            ));
        }
        // SAFETY: openat returned a new descriptor whose ownership is transferred once.
        let mut current_target = unsafe { File::from_raw_fd(descriptor) };
        let inspected_metadata = self.inspected_target.metadata().map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::ValidateTargetPath,
                &target_path,
                error,
            )
        })?;
        let current_metadata = current_target.metadata().map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::ValidateTargetPath,
                &target_path,
                error,
            )
        })?;
        let mut current_bytes = Vec::new();
        current_target
            .read_to_end(&mut current_bytes)
            .map_err(|error| {
                StorageTransactionError::new(
                    StorageTransactionOperation::ReadTargetContent,
                    &target_path,
                    error,
                )
            })?;
        if inspected_metadata.dev() != current_metadata.dev()
            || inspected_metadata.ino() != current_metadata.ino()
            || current_metadata.mode() != self.mode
            || current_bytes != self.bytes
        {
            return Err(StorageTransactionError::new(
                StorageTransactionOperation::ValidateTargetPath,
                &target_path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "storage target changed after it was inspected",
                ),
            ));
        }
        Ok(())
    }
}

pub(super) fn read_storage_file_anchored(
    storage_dir_path: &Path,
    relative_path: &Path,
) -> Result<AnchoredReadOutcome, StorageTransactionError> {
    read_storage_file_anchored_with_hook(storage_dir_path, relative_path, || {})
}

fn read_storage_file_anchored_with_hook(
    storage_dir_path: &Path,
    relative_path: &Path,
    after_read: impl FnOnce(),
) -> Result<AnchoredReadOutcome, StorageTransactionError> {
    let parent = match StableTargetParent::open(storage_dir_path, relative_path) {
        Ok(parent) => parent,
        Err(error)
            if matches!(
                error.source.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(AnchoredReadOutcome::MissingOrNotFile);
        }
        Err(error) => return Err(error),
    };
    let target_path = parent.target_path();
    let descriptor = unsafe {
        libc::openat(
            parent.directory.as_raw_fd(),
            parent.target_name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if descriptor < 0 {
        let error = std::io::Error::last_os_error();
        if matches!(
            error.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
        ) || error.raw_os_error() == Some(libc::ELOOP)
        {
            return Ok(AnchoredReadOutcome::MissingOrNotFile);
        }
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::ReadTargetMetadata,
            &target_path,
            error,
        ));
    }
    // SAFETY: openat returned a new descriptor whose ownership is transferred once.
    let mut target = unsafe { File::from_raw_fd(descriptor) };
    let metadata = target.metadata().map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::ReadTargetMetadata,
            &target_path,
            error,
        )
    })?;
    if !metadata.file_type().is_file() {
        return Ok(AnchoredReadOutcome::MissingOrNotFile);
    }
    let mut bytes = Vec::new();
    target.read_to_end(&mut bytes).map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::ReadTargetContent,
            &target_path,
            error,
        )
    })?;
    let permissions = metadata.permissions();
    let mode = metadata.mode();
    after_read();
    Ok(AnchoredReadOutcome::File(AnchoredTargetFile {
        bytes: bytes.clone(),
        permissions,
        guard: Box::new(StableTargetFile {
            parent,
            inspected_target: target,
            bytes,
            mode,
        }),
    }))
}

pub(super) fn write_storage_file_anchored_secure(
    storage_dir_path: &Path,
    relative_path: &Path,
    transaction_id: Uuid,
    bytes: &[u8],
    permissions: Option<&fs::Permissions>,
    after_parent_open: impl FnOnce(),
) -> Result<(), StorageTransactionError> {
    let parent = StableTargetParent::open(storage_dir_path, relative_path)?;
    let target_path = parent.target_path();
    after_parent_open();
    parent.validate_current_parent()?;

    let temporary_name =
        anchored_temporary_name(&parent.target_name, transaction_id).map_err(|error| {
            StorageTransactionError::new(
                StorageTransactionOperation::CreateLiveTemporary,
                &target_path,
                error,
            )
        })?;
    let open_temporary = || unsafe {
        libc::openat(
            parent.directory.as_raw_fd(),
            temporary_name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    let mut descriptor = open_temporary();
    if descriptor < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists
    {
        if unsafe { libc::unlinkat(parent.directory.as_raw_fd(), temporary_name.as_ptr(), 0) } != 0
        {
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
            parent.directory.as_raw_fd(),
            temporary_name.as_ptr(),
            parent.directory.as_raw_fd(),
            parent.target_name.as_ptr(),
        )
    } != 0
    {
        return Err(StorageTransactionError::new(
            StorageTransactionOperation::RenameLiveTarget,
            &target_path,
            std::io::Error::last_os_error(),
        ));
    }
    parent.directory.sync_all().map_err(|error| {
        StorageTransactionError::new(
            StorageTransactionOperation::SyncDirectory,
            target_path.parent().unwrap_or(storage_dir_path),
            error,
        )
    })
}

fn anchored_temporary_name(target_name: &CStr, transaction_id: Uuid) -> std::io::Result<CString> {
    let mut bytes = Vec::with_capacity(target_name.to_bytes().len() + 40);
    bytes.push(b'.');
    bytes.extend_from_slice(target_name.to_bytes());
    bytes.push(b'.');
    bytes.extend_from_slice(transaction_id.hyphenated().to_string().as_bytes());
    bytes.extend_from_slice(b".tmp");
    CString::new(bytes).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "write target contains a NUL byte",
        )
    })
}

#[cfg(test)]
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

#[cfg(test)]
pub(in crate::adapter::gateway) fn read_storage_file_anchored_after_read(
    storage_dir_path: &Path,
    relative_path: &Path,
    after_read: impl FnOnce(),
) -> Result<AnchoredReadOutcome, StorageTransactionError> {
    read_storage_file_anchored_with_hook(storage_dir_path, relative_path, after_read)
}
