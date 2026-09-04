use super::*;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

fn file_system_io() -> Arc<dyn StorageTransactionIo> {
    Arc::new(FileSystemStorageTransactionIo)
}

struct FailingPrepareIo {
    fail_write_call: Option<usize>,
    fail_file_sync_call: Option<usize>,
    fail_sync_call: Option<usize>,
    write_calls: AtomicUsize,
    file_sync_calls: AtomicUsize,
    sync_calls: AtomicUsize,
}

struct FailSecondCreateDirectoryIo {
    create_calls: AtomicUsize,
}

impl StorageTransactionIo for FailSecondCreateDirectoryIo {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        let call = self.create_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == 2 {
            FileSystemStorageTransactionIo
                .create_dir_all(path.parent().expect("staged files directory has a parent"))?;
            return Err(std::io::Error::other(
                "injected staged files directory failure",
            ));
        }
        FileSystemStorageTransactionIo.create_dir_all(path)
    }
}

impl FailingPrepareIo {
    fn new(
        fail_write_call: Option<usize>,
        fail_file_sync_call: Option<usize>,
        fail_sync_call: Option<usize>,
    ) -> Self {
        Self {
            fail_write_call,
            fail_file_sync_call,
            fail_sync_call,
            write_calls: AtomicUsize::new(0),
            file_sync_calls: AtomicUsize::new(0),
            sync_calls: AtomicUsize::new(0),
        }
    }
}

impl StorageTransactionIo for FailingPrepareIo {
    fn write_file(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let call = self.write_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_write_call == Some(call) {
            return Err(std::io::Error::other("injected write/sync failure"));
        }
        FileSystemStorageTransactionIo.write_file(path, bytes)
    }

    fn sync_file(&self, path: &Path) -> std::io::Result<()> {
        let call = self.file_sync_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_file_sync_call == Some(call) {
            return Err(std::io::Error::other("injected file sync failure"));
        }
        FileSystemStorageTransactionIo.sync_file(path)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        let call = self.sync_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_sync_call == Some(call) {
            return Err(std::io::Error::other("injected directory sync failure"));
        }
        FileSystemStorageTransactionIo.sync_directory(path)
    }
}

#[derive(Clone, Copy)]
enum FailingStagedFilePhase {
    Create,
    SetPermissions,
    Write,
}

struct FailingStagedFileIo {
    phase: FailingStagedFilePhase,
}

struct CommitOrderIo {
    storage_dir_path: PathBuf,
    transaction_dir_path: Mutex<Option<PathBuf>>,
    marker_file_synced: Mutex<bool>,
    marker_directory_synced: Mutex<bool>,
    first_target_path: PathBuf,
    second_target_path: PathBuf,
}

impl StorageTransactionIo for CommitOrderIo {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        FileSystemStorageTransactionIo.create_dir_all(path)
    }

    fn create_new_file(&self, path: &Path) -> std::io::Result<()> {
        if path.parent() == Some(self.storage_dir_path.as_path())
            || path.parent() == self.first_target_path.parent()
            || path.parent() == self.second_target_path.parent()
        {
            assert!(
                *self.marker_directory_synced.lock().unwrap(),
                "live target must not be prepared before the commit marker directory is synced"
            );
        }
        FileSystemStorageTransactionIo.create_new_file(path)
    }

    fn write_file(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if path.parent() == Some(self.storage_dir_path.as_path())
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("revision"))
        {
            assert_eq!(fs::read(&self.first_target_path).unwrap(), b"first-new");
            assert_eq!(fs::read(&self.second_target_path).unwrap(), b"second-new");
        }
        FileSystemStorageTransactionIo.write_file(path, bytes)
    }

    fn sync_file(&self, path: &Path) -> std::io::Result<()> {
        if path.file_name().is_some_and(|name| name == "commit.tmp") {
            *self.marker_file_synced.lock().unwrap() = true;
        }
        FileSystemStorageTransactionIo.sync_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        if to.file_name().is_some_and(|name| name == "commit") {
            assert!(
                *self.marker_file_synced.lock().unwrap(),
                "commit marker temporary file must be synced before rename"
            );
            *self.transaction_dir_path.lock().unwrap() = to.parent().map(Path::to_path_buf);
        }
        FileSystemStorageTransactionIo.rename(from, to)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        if self
            .transaction_dir_path
            .lock()
            .unwrap()
            .as_deref()
            .is_some_and(|transaction_dir_path| transaction_dir_path == path)
        {
            assert!(
                *self.marker_file_synced.lock().unwrap(),
                "commit marker file must be synced before its directory"
            );
            *self.marker_directory_synced.lock().unwrap() = true;
        }
        FileSystemStorageTransactionIo.sync_directory(path)
    }
}

#[derive(Clone, Copy, Debug)]
enum FailingCommitPhase {
    MarkerCreate,
    MarkerSync,
    LiveWrite,
    LiveSync,
    LiveRename,
    RevisionWrite,
    CleanupDelete,
}

struct FailingCommitIo {
    phase: FailingCommitPhase,
}

impl StorageTransactionIo for FailingCommitIo {
    fn create_new_file(&self, path: &Path) -> std::io::Result<()> {
        if matches!(self.phase, FailingCommitPhase::MarkerCreate)
            && path.file_name().is_some_and(|name| name == "commit.tmp")
        {
            return Err(std::io::Error::other("injected marker create failure"));
        }
        FileSystemStorageTransactionIo.create_new_file(path)
    }

    fn write_file(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        if (matches!(self.phase, FailingCommitPhase::LiveWrite)
            && file_name.starts_with(".project.yaml."))
            || (matches!(self.phase, FailingCommitPhase::RevisionWrite)
                && file_name.contains("revision"))
        {
            return Err(std::io::Error::other("injected live write failure"));
        }
        FileSystemStorageTransactionIo.write_file(path, bytes)
    }

    fn sync_file(&self, path: &Path) -> std::io::Result<()> {
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        if (matches!(self.phase, FailingCommitPhase::MarkerSync) && file_name == "commit.tmp")
            || (matches!(self.phase, FailingCommitPhase::LiveSync)
                && file_name.starts_with(".project.yaml."))
        {
            return Err(std::io::Error::other("injected commit sync failure"));
        }
        FileSystemStorageTransactionIo.sync_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        if matches!(self.phase, FailingCommitPhase::LiveRename)
            && to.file_name().is_some_and(|name| name == "project.yaml")
        {
            return Err(std::io::Error::other("injected live rename failure"));
        }
        FileSystemStorageTransactionIo.rename(from, to)
    }

    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        if matches!(self.phase, FailingCommitPhase::CleanupDelete) {
            return Err(std::io::Error::other("injected cleanup failure"));
        }
        FileSystemStorageTransactionIo.remove_dir_all(path)
    }
}

impl StorageTransactionIo for FailingStagedFileIo {
    fn create_new_file(&self, path: &Path) -> std::io::Result<()> {
        if matches!(self.phase, FailingStagedFilePhase::Create) {
            return Err(std::io::Error::other("injected create failure"));
        }
        FileSystemStorageTransactionIo.create_new_file(path)
    }

    fn set_permissions(&self, path: &Path, permissions: fs::Permissions) -> std::io::Result<()> {
        if matches!(self.phase, FailingStagedFilePhase::SetPermissions) {
            return Err(std::io::Error::other("injected permission failure"));
        }
        FileSystemStorageTransactionIo.set_permissions(path, permissions)
    }

    fn write_file(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if matches!(self.phase, FailingStagedFilePhase::Write) {
            return Err(std::io::Error::other("injected write failure"));
        }
        FileSystemStorageTransactionIo.write_file(path, bytes)
    }
}

struct TestStorageDir {
    path: PathBuf,
}

impl TestStorageDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("schronu-transaction-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TestStorageDir {
    fn drop(&mut self) {
        if self.path.exists() {
            fs::remove_dir_all(&self.path).unwrap();
        }
    }
}

#[test]
fn test_prepare_staged_fileとimmutable_manifestを作成する() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project/project.yaml");
    let revision = Uuid::from_u128(0x2201);

    let prepared = prepare(
        file_system_io(),
        &storage_dir.path,
        revision,
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"project: {}\n",
        }],
    )
    .unwrap();

    let manifest_path = prepared.transaction_dir_path.join("manifest.json");
    let manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["version"], 1);
    assert_eq!(manifest["revision"], revision.to_string());
    assert_eq!(manifest["entries"][0]["target"], "project/project.yaml");
    assert_eq!(manifest["entries"][0]["staged_file"], "files/0");
    assert_eq!(
        fs::read(prepared.transaction_dir_path.join("files/0")).unwrap(),
        b"project: {}\n"
    );

    prepared.discard().unwrap();
    assert!(storage_dir.path.join(TRANSACTION_DIRECTORY_NAME).is_dir());
}

#[test]
fn test_prepare_staged_files_directory作成失敗時はuuid_directoryを残さない() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
    let io = Arc::new(FailSecondCreateDirectoryIo {
        create_calls: AtomicUsize::new(0),
    });

    let actual = prepare(
        io,
        &storage_dir.path,
        Uuid::from_u128(0x2205),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"project: {}\n",
        }],
    );

    assert!(actual.is_err());
    let transactions_dir_path = storage_dir.path.join(TRANSACTION_DIRECTORY_NAME);
    assert_eq!(fs::read_dir(transactions_dir_path).unwrap().count(), 0);
}

#[test]
fn test_prepare_staged_file失敗はpathとphaseを保持する() {
    for (phase, expected_operation) in [
        (
            FailingStagedFilePhase::Create,
            StorageTransactionOperation::CreateStagedFile,
        ),
        (
            FailingStagedFilePhase::SetPermissions,
            StorageTransactionOperation::SetStagedPermissions,
        ),
        (
            FailingStagedFilePhase::Write,
            StorageTransactionOperation::WriteStagedFile,
        ),
    ] {
        let storage_dir = TestStorageDir::new();
        let target_path = storage_dir.path.join("project.yaml");
        fs::write(&target_path, b"old").unwrap();
        let io = Arc::new(FailingStagedFileIo { phase });

        let actual = prepare(
            io,
            &storage_dir.path,
            Uuid::from_u128(0x2206),
            &[WriteRequest {
                target_path: &target_path,
                bytes: b"new",
            }],
        );

        let error = match actual {
            Err(error) => error,
            Ok(prepared) => {
                prepared.discard().unwrap();
                panic!("prepare must fail");
            }
        };
        assert_eq!(error.operation, expected_operation);
        assert_eq!(error.path.file_name().unwrap(), "0");
        assert_eq!(error.path.parent().unwrap().file_name().unwrap(), "files");
        assert!(error.source().is_some());
    }
}

#[cfg(unix)]
#[test]
fn test_prepare_既存targetのpermissionをstaged_fileへ引き継ぐ() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
    fs::write(&target_path, b"old").unwrap();
    fs::set_permissions(&target_path, fs::Permissions::from_mode(0o600)).unwrap();

    let prepared = prepare(
        file_system_io(),
        &storage_dir.path,
        Uuid::from_u128(0x2202),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"new",
        }],
    )
    .unwrap();

    assert_eq!(
        fs::metadata(prepared.transaction_dir_path.join("files/0"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn test_prepare_途中のwriteとsync失敗ではlive_targetを変更しない() {
    for (fail_write_call, fail_file_sync_call) in [
        (Some(2), None),
        (None, Some(2)),
        (Some(3), None),
        (None, Some(3)),
    ] {
        let storage_dir = TestStorageDir::new();
        let first_target_path = storage_dir.path.join("first.yaml");
        let second_target_path = storage_dir.path.join("second.yaml");
        fs::write(&first_target_path, b"first-old").unwrap();
        fs::write(&second_target_path, b"second-old").unwrap();
        let io = Arc::new(FailingPrepareIo::new(
            fail_write_call,
            fail_file_sync_call,
            None,
        ));

        let actual = prepare(
            io,
            &storage_dir.path,
            Uuid::from_u128(0x2203),
            &[
                WriteRequest {
                    target_path: &first_target_path,
                    bytes: b"first-new",
                },
                WriteRequest {
                    target_path: &second_target_path,
                    bytes: b"second-new",
                },
            ],
        );

        assert!(actual.is_err());
        assert_eq!(fs::read(first_target_path).unwrap(), b"first-old");
        assert_eq!(fs::read(second_target_path).unwrap(), b"second-old");
    }
}

#[test]
fn test_prepare_directory_sync失敗ではlive_targetを変更しない() {
    for fail_sync_call in 1..=4 {
        let storage_dir = TestStorageDir::new();
        let target_path = storage_dir.path.join("project.yaml");
        fs::write(&target_path, b"old").unwrap();
        let io = Arc::new(FailingPrepareIo::new(None, None, Some(fail_sync_call)));

        let actual = prepare(
            io,
            &storage_dir.path,
            Uuid::from_u128(0x2204),
            &[WriteRequest {
                target_path: &target_path,
                bytes: b"new",
            }],
        );

        assert!(actual.is_err());
        assert_eq!(fs::read(target_path).unwrap(), b"old");
    }
}

#[test]
fn test_commit_markerをsyncしてからprojectを適用しrevisionを最後に更新する() {
    let storage_dir = TestStorageDir::new();
    let first_target_path = storage_dir.path.join("first/project.yaml");
    let second_target_path = storage_dir.path.join("second/project.yaml");
    fs::create_dir_all(first_target_path.parent().unwrap()).unwrap();
    fs::create_dir_all(second_target_path.parent().unwrap()).unwrap();
    fs::write(&first_target_path, b"first-old").unwrap();
    fs::write(&second_target_path, b"second-old").unwrap();
    let revision = Uuid::from_u128(0x2207);
    let io = Arc::new(CommitOrderIo {
        storage_dir_path: storage_dir.path.clone(),
        transaction_dir_path: Mutex::new(None),
        marker_file_synced: Mutex::new(false),
        marker_directory_synced: Mutex::new(false),
        first_target_path: first_target_path.clone(),
        second_target_path: second_target_path.clone(),
    });
    let prepared = prepare(
        io,
        &storage_dir.path,
        revision,
        &[
            WriteRequest {
                target_path: &first_target_path,
                bytes: b"first-new",
            },
            WriteRequest {
                target_path: &second_target_path,
                bytes: b"second-new",
            },
        ],
    )
    .unwrap();
    let manifest: Value = serde_json::from_slice(
        &fs::read(prepared.transaction_dir_path.join("manifest.json")).unwrap(),
    )
    .unwrap();
    let transaction_id = Uuid::parse_str(manifest["transaction_id"].as_str().unwrap()).unwrap();
    assert_eq!(manifest["revision"], revision.to_string());

    prepared
        .commit(&storage_dir.path.join(".revision"))
        .unwrap();

    assert_ne!(transaction_id, Uuid::nil());
    assert_eq!(fs::read(first_target_path).unwrap(), b"first-new");
    assert_eq!(fs::read(second_target_path).unwrap(), b"second-new");
    assert_eq!(
        fs::read_to_string(storage_dir.path.join(".revision")).unwrap(),
        format!("{revision}\n")
    );
    assert_eq!(
        fs::read_dir(storage_dir.path.join(TRANSACTION_DIRECTORY_NAME))
            .unwrap()
            .count(),
        0
    );
}

#[cfg(unix)]
#[test]
fn test_commit_既存targetのpermissionを維持する() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
    fs::write(&target_path, b"old").unwrap();
    fs::set_permissions(&target_path, fs::Permissions::from_mode(0o600)).unwrap();
    let prepared = prepare(
        file_system_io(),
        &storage_dir.path,
        Uuid::from_u128(0x2208),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"new",
        }],
    )
    .unwrap();

    prepared
        .commit(&storage_dir.path.join(".revision"))
        .unwrap();

    assert_eq!(fs::metadata(target_path).unwrap().mode() & 0o777, 0o600);
}

#[test]
fn test_commit_failure時は回復用manifestとstaged_fileを維持する() {
    for phase in [
        FailingCommitPhase::MarkerCreate,
        FailingCommitPhase::MarkerSync,
        FailingCommitPhase::LiveWrite,
        FailingCommitPhase::LiveSync,
        FailingCommitPhase::LiveRename,
        FailingCommitPhase::RevisionWrite,
    ] {
        let storage_dir = TestStorageDir::new();
        let target_path = storage_dir.path.join("project.yaml");
        fs::write(&target_path, b"old").unwrap();
        let prepared = prepare(
            Arc::new(FailingCommitIo { phase }),
            &storage_dir.path,
            Uuid::from_u128(0x2209),
            &[WriteRequest {
                target_path: &target_path,
                bytes: b"new",
            }],
        )
        .unwrap();
        let transaction_dir_path = prepared.transaction_dir_path.clone();

        let actual = prepared.commit(&storage_dir.path.join(".revision"));

        assert!(actual.is_err(), "{phase:?} must fail");
        assert!(transaction_dir_path.join("manifest.json").is_file());
        assert!(transaction_dir_path.join("files/0").is_file());
        assert_eq!(
            transaction_dir_path.join("commit").is_file(),
            !matches!(
                phase,
                FailingCommitPhase::MarkerCreate | FailingCommitPhase::MarkerSync
            )
        );
    }
}

#[test]
fn test_commit_cleanup削除失敗はtombstoneへ回復情報を保持して成功する() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
    fs::write(&target_path, b"old").unwrap();
    let prepared = prepare(
        Arc::new(FailingCommitIo {
            phase: FailingCommitPhase::CleanupDelete,
        }),
        &storage_dir.path,
        Uuid::from_u128(0x2210),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"new",
        }],
    )
    .unwrap();
    let transaction_id = prepared.transaction_id;

    prepared
        .commit(&storage_dir.path.join(".revision"))
        .unwrap();

    let cleanup_dir_path = storage_dir
        .path
        .join(TRANSACTION_DIRECTORY_NAME)
        .join(format!(".cleanup-{}", transaction_id.hyphenated()));
    assert!(cleanup_dir_path.join("commit").is_file());
    assert!(cleanup_dir_path.join("manifest.json").is_file());
    assert!(cleanup_dir_path.join("files/0").is_file());
    assert_eq!(fs::read(target_path).unwrap(), b"new");
}
