use chrono::{DateTime, Local};
use fs2::FileExt;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockMode {
    Cli,
    Mcp,
    Web,
}

impl fmt::Display for LockMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli => write!(formatter, "cli"),
            Self::Mcp => write!(formatter, "mcp"),
            Self::Web => write!(formatter, "web"),
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
        let mut file = open_lock_file(&path).map_err(|error| StorageLockError::io(&path, error))?;

        file.try_lock_exclusive()
            .map_err(|error| classify_lock_attempt_error(&path, error))?;

        write_metadata(&mut file, mode, Local::now()).map_err(|error| {
            let _ = FileExt::unlock(&file);
            StorageLockError::io(&path, error)
        })?;

        Ok(Self { _file: file, path })
    }

    pub fn acquire_with_timeout(
        storage_directory: &Path,
        mode: LockMode,
        timeout: Duration,
    ) -> Result<Self, StorageLockError> {
        retry_contended_with_timeout(timeout, || Self::acquire(storage_directory, mode))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn retry_contended_with_timeout<T>(
    timeout: Duration,
    mut operation: impl FnMut() -> Result<T, StorageLockError>,
) -> Result<T, StorageLockError> {
    let started_at = Instant::now();
    let mut last_contended_error = None;
    loop {
        if let Some(error) = last_contended_error.take() {
            if started_at.elapsed() >= timeout {
                return Err(error);
            }
        }
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) if error.kind() == StorageLockErrorKind::Contended => {
                let elapsed = started_at.elapsed();
                if elapsed >= timeout {
                    return Err(error);
                }
                let sleep_duration = LOCK_RETRY_INTERVAL.min(timeout - elapsed);
                last_contended_error = Some(error);
                std::thread::sleep(sleep_duration);
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn open_lock_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "storage lock path is not a regular file",
        ));
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_lock_file(_path: &Path) -> std::io::Result<File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "storage locking is supported only on Unix platforms",
    ))
}

fn classify_lock_attempt_error(path: &Path, source: std::io::Error) -> StorageLockError {
    if source.kind() == std::io::ErrorKind::WouldBlock {
        StorageLockError::contended(path, source)
    } else {
        StorageLockError::io(path, source)
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
    use std::time::Duration;
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

    #[cfg(unix)]
    #[test]
    fn storage_lock_最初の取得が成功しmetadataを記録する() {
        let directory = TestDir::new();

        let _guard = StorageLock::acquire(directory.path(), LockMode::Cli).unwrap();

        let metadata = fs::read_to_string(directory.path().join(".lock")).unwrap();
        assert!(metadata.contains(&format!("pid={}", std::process::id())));
        assert!(metadata.contains("mode=cli"));
        assert!(metadata.contains("started_at="));
    }

    #[cfg(unix)]
    #[test]
    fn storage_lock_web取得時にmetadataへmodeを記録する() {
        let directory = TestDir::new();

        let _guard = StorageLock::acquire(directory.path(), LockMode::Web).unwrap();

        let metadata = fs::read_to_string(directory.path().join(".lock")).unwrap();
        assert!(metadata.contains("mode=web"));
    }

    #[cfg(unix)]
    #[test]
    fn storage_lock_webはcliとmcpと同じlockで競合する() {
        let directory = TestDir::new();
        let _web_lock = StorageLock::acquire(directory.path(), LockMode::Web).unwrap();

        for mode in [LockMode::Cli, LockMode::Mcp] {
            let error = StorageLock::acquire(directory.path(), mode).unwrap_err();

            assert_eq!(error.kind(), StorageLockErrorKind::Contended);
            assert!(error.holder_metadata().unwrap().contains("mode=web"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn storage_lock_同じ保存先の二重取得を拒否する() {
        let directory = TestDir::new();
        let _first = StorageLock::acquire(directory.path(), LockMode::Cli).unwrap();

        let error = StorageLock::acquire(directory.path(), LockMode::Mcp).unwrap_err();

        assert_eq!(error.kind(), StorageLockErrorKind::Contended);
        assert!(error.holder_metadata().unwrap().contains("mode=cli"));
    }

    #[cfg(unix)]
    #[test]
    fn storage_lock_guardのdrop後に再取得できる() {
        let directory = TestDir::new();
        let first = StorageLock::acquire(directory.path(), LockMode::Cli).unwrap();
        drop(first);

        let second = StorageLock::acquire(directory.path(), LockMode::Mcp);

        assert!(second.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn storage_lock_timeout内に競合が解消すれば取得できる() {
        let directory = TestDir::new();
        let first = StorageLock::acquire(directory.path(), LockMode::Cli).unwrap();
        let release_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            drop(first);
        });

        let second = StorageLock::acquire_with_timeout(
            directory.path(),
            LockMode::Mcp,
            Duration::from_millis(500),
        );

        release_thread.join().unwrap();
        assert!(second.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn storage_lock_timeoutまで競合が続けばcontendedを返す() {
        let directory = TestDir::new();
        let _first = StorageLock::acquire(directory.path(), LockMode::Cli).unwrap();

        let error = StorageLock::acquire_with_timeout(
            directory.path(),
            LockMode::Mcp,
            Duration::from_millis(30),
        )
        .unwrap_err();

        assert_eq!(error.kind(), StorageLockErrorKind::Contended);
        assert_eq!(source_kind(&error), ErrorKind::WouldBlock);
        assert!(error.holder_metadata().unwrap().contains("mode=cli"));
    }

    #[cfg(unix)]
    #[test]
    fn storage_lock_timeout付き取得は競合以外のio_errorをそのまま返す() {
        use std::os::unix::fs::symlink;

        let directory = TestDir::new();
        let sentinel = directory.path().join("project.yaml");
        let lock_path = directory.path().join(".lock");
        fs::write(&sentinel, "unchanged").unwrap();
        symlink(&sentinel, &lock_path).unwrap();

        let error = StorageLock::acquire_with_timeout(
            directory.path(),
            LockMode::Mcp,
            Duration::from_secs(1),
        )
        .unwrap_err();

        assert_eq!(error.kind(), StorageLockErrorKind::Io);
        assert_eq!(error.path(), lock_path);
        assert_ne!(source_kind(&error), ErrorKind::WouldBlock);
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "unchanged");
    }

    #[test]
    fn storage_lock_timeout到達後は再取得しない() {
        let path = Path::new("tasks/.lock");
        let mut attempt_count = 0;

        let result: Result<(), _> =
            super::retry_contended_with_timeout(Duration::from_millis(1), || {
                attempt_count += 1;
                if attempt_count == 1 {
                    Err(super::classify_lock_attempt_error(
                        path,
                        std::io::Error::from(ErrorKind::WouldBlock),
                    ))
                } else {
                    Err(super::classify_lock_attempt_error(
                        path,
                        std::io::Error::from(ErrorKind::PermissionDenied),
                    ))
                }
            });
        let error = result.unwrap_err();

        assert_eq!(attempt_count, 1);
        assert_eq!(error.kind(), StorageLockErrorKind::Contended);
    }

    #[cfg(unix)]
    #[test]
    fn storage_lockはsymlinkをio_errorで拒否し参照先を変更しない() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let directory = TestDir::new();
        let sentinel = directory.path().join("project.yaml");
        let lock_path = directory.path().join(".lock");
        let sentinel_content = b"task: unchanged\n";
        fs::write(&sentinel, sentinel_content).unwrap();
        fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o640)).unwrap();
        let original_metadata = fs::metadata(&sentinel).unwrap();
        symlink(&sentinel, &lock_path).unwrap();

        let result = StorageLock::acquire(directory.path(), LockMode::Mcp);

        assert!(result.is_err(), ".lock fileがsymlinkの場合は拒否されるべき");
        let error = result.unwrap_err();
        assert_eq!(error.kind(), StorageLockErrorKind::Io);
        assert_eq!(error.path(), lock_path);
        assert!(std::error::Error::source(&error).is_some());
        assert_eq!(fs::read(&sentinel).unwrap(), sentinel_content);
        let current_metadata = fs::metadata(&sentinel).unwrap();
        assert_eq!(current_metadata.len(), original_metadata.len());
        assert_eq!(
            current_metadata.permissions().mode(),
            original_metadata.permissions().mode()
        );
        assert!(fs::symlink_metadata(&lock_path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_link(lock_path).unwrap(), sentinel);
    }

    #[cfg(not(unix))]
    #[test]
    fn storage_lockは非unix環境でunsupportedのio_errorを返す() {
        let directory = TestDir::new();
        let lock_path = directory.path().join(".lock");

        let error = StorageLock::acquire(directory.path(), LockMode::Cli).unwrap_err();

        assert_eq!(error.kind(), StorageLockErrorKind::Io);
        assert_eq!(error.path(), lock_path);
        assert_eq!(source_kind(&error), ErrorKind::Unsupported);
    }

    #[cfg(unix)]
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
