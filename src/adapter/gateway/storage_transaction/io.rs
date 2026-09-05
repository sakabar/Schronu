use super::layout::TransactionLayout;
use super::{StorageTransactionError, StorageTransactionOperation};
use fs2::FileExt;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(test)]
pub(super) use super::layout::TRANSACTION_LOCK_FILE_NAME;

pub(in crate::adapter::gateway) trait StorageTransactionIo:
    Send + Sync
{
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
}

#[derive(Default)]
pub(in crate::adapter::gateway) struct FileSystemStorageTransactionIo;
impl StorageTransactionIo for FileSystemStorageTransactionIo {}

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
