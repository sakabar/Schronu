use chrono::{DateTime, Local};
use fs2::FileExt;
use std::error::Error;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockMode {
    Cli,
    Mcp,
}

impl fmt::Display for LockMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli => write!(formatter, "cli"),
            Self::Mcp => write!(formatter, "mcp"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageLockErrorKind {
    Contended,
    Io,
}

#[derive(Debug)]
pub struct StorageLockError {
    kind: StorageLockErrorKind,
    path: PathBuf,
    holder_metadata: Option<String>,
    source: std::io::Error,
}

impl StorageLockError {
    fn io(path: &Path, source: std::io::Error) -> Self {
        Self {
            kind: StorageLockErrorKind::Io,
            path: path.to_path_buf(),
            holder_metadata: None,
            source,
        }
    }

    fn contended(path: &Path, source: std::io::Error) -> Self {
        Self {
            kind: StorageLockErrorKind::Contended,
            path: path.to_path_buf(),
            holder_metadata: std::fs::read_to_string(path).ok(),
            source,
        }
    }

    pub fn kind(&self) -> StorageLockErrorKind {
        self.kind
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn holder_metadata(&self) -> Option<&str> {
        self.holder_metadata.as_deref()
    }
}

impl fmt::Display for StorageLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.kind, &self.holder_metadata) {
            (StorageLockErrorKind::Contended, Some(metadata)) => write!(
                formatter,
                "storage lock is already held at {} ({})",
                self.path.display(),
                metadata.trim().replace('\n', ", ")
            ),
            (StorageLockErrorKind::Contended, None) => write!(
                formatter,
                "storage lock is already held at {}",
                self.path.display()
            ),
            (StorageLockErrorKind::Io, _) => write!(
                formatter,
                "storage lock operation failed at {}: {}",
                self.path.display(),
                self.source
            ),
        }
    }
}

impl Error for StorageLockError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug)]
pub struct StorageLock {
    _file: File,
    path: PathBuf,
}

impl StorageLock {
    pub fn acquire(storage_directory: &Path, mode: LockMode) -> Result<Self, StorageLockError> {
        let path = storage_directory.join(".lock");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| StorageLockError::io(&path, error))?;

        file.try_lock_exclusive()
            .map_err(|error| StorageLockError::contended(&path, error))?;

        write_metadata(&mut file, mode, Local::now()).map_err(|error| {
            let _ = FileExt::unlock(&file);
            StorageLockError::io(&path, error)
        })?;

        Ok(Self { _file: file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn write_metadata(
    file: &mut File,
    mode: LockMode,
    started_at: DateTime<Local>,
) -> std::io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    write!(
        file,
        "pid={}\nstarted_at={}\nmode={}\n",
        std::process::id(),
        started_at.to_rfc3339(),
        mode
    )?;
    file.sync_data()
}

#[cfg(test)]
mod tests {
    use super::{LockMode, StorageLock, StorageLockErrorKind};
    use std::fs;
    use std::io::ErrorKind;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "schronu-storage-lock-test-{}",
                Uuid::new_v4().hyphenated()
            ));
            fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn storage_lock_最初の取得が成功しmetadataを記録する() {
        let directory = TestDir::new();

        let _guard = StorageLock::acquire(directory.path(), LockMode::Cli).unwrap();

        let metadata = fs::read_to_string(directory.path().join(".lock")).unwrap();
        assert!(metadata.contains(&format!("pid={}", std::process::id())));
        assert!(metadata.contains("mode=cli"));
        assert!(metadata.contains("started_at="));
    }

    #[test]
    fn storage_lock_同じ保存先の二重取得を拒否する() {
        let directory = TestDir::new();
        let _first = StorageLock::acquire(directory.path(), LockMode::Cli).unwrap();

        let error = StorageLock::acquire(directory.path(), LockMode::Mcp).unwrap_err();

        assert_eq!(error.kind(), StorageLockErrorKind::Contended);
        assert!(error.holder_metadata().unwrap().contains("mode=cli"));
    }

    #[test]
    fn storage_lock_guard_drop後に再取得できる() {
        let directory = TestDir::new();
        let first = StorageLock::acquire(directory.path(), LockMode::Cli).unwrap();
        drop(first);

        let second = StorageLock::acquire(directory.path(), LockMode::Mcp);

        assert!(second.is_ok());
    }

    #[test]
    fn storage_lock_競合時に保存先の実データを変更しない() {
        let directory = TestDir::new();
        let project_file = directory.path().join("project.yaml");
        fs::write(&project_file, "task: unchanged\n").unwrap();
        let _first = StorageLock::acquire(directory.path(), LockMode::Cli).unwrap();

        let _error = StorageLock::acquire(directory.path(), LockMode::Mcp).unwrap_err();

        assert_eq!(
            fs::read_to_string(project_file).unwrap(),
            "task: unchanged\n"
        );
    }

    #[test]
    fn storage_lock_would_blockだけを競合errorに分類する() {
        let path = Path::new("tasks/.lock");

        let contended =
            super::classify_lock_attempt_error(path, std::io::Error::from(ErrorKind::WouldBlock));
        let io = super::classify_lock_attempt_error(
            path,
            std::io::Error::from(ErrorKind::PermissionDenied),
        );

        assert_eq!(contended.kind(), StorageLockErrorKind::Contended);
        assert_eq!(contended.path(), path);
        assert_eq!(source_kind(&contended), ErrorKind::WouldBlock);
        assert_eq!(io.kind(), StorageLockErrorKind::Io);
        assert_eq!(io.path(), path);
        assert_eq!(source_kind(&io), ErrorKind::PermissionDenied);
    }

    fn source_kind(error: &super::StorageLockError) -> ErrorKind {
        std::error::Error::source(error)
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap()
            .kind()
    }
}
