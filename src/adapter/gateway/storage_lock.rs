#[cfg(test)]
mod tests {
    use super::{LockMode, StorageLock, StorageLockErrorKind};
    use std::fs;
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
}
