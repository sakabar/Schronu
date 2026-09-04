use super::*;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Barrier, Mutex};

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
    ReadMetadata,
    Create,
    SetPermissions,
    Write,
    Sync,
}

struct FailingStagedFileIo {
    phase: FailingStagedFilePhase,
}

struct CommitOrderIo {
    storage_dir_path: PathBuf,
    transaction_dir_path: Mutex<Option<PathBuf>>,
    manifest_file_synced: Mutex<bool>,
    marker_file_synced: Mutex<bool>,
    marker_directory_synced: Mutex<bool>,
    first_target_path: PathBuf,
    second_target_path: PathBuf,
}

struct BlockingMarkerPublicationIo {
    marker_published: AtomicBool,
    marker_sync_started: Barrier,
    marker_sync_resume: Barrier,
}

impl StorageTransactionIo for BlockingMarkerPublicationIo {
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        FileSystemStorageTransactionIo.rename(from, to)?;
        if to.file_name().is_some_and(|name| name == "commit") {
            self.marker_published.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        if path.file_name().is_some_and(|name| name == ".active")
            && self.marker_published.load(Ordering::SeqCst)
        {
            self.marker_sync_started.wait();
            self.marker_sync_resume.wait();
        }
        FileSystemStorageTransactionIo.sync_directory(path)
    }
}

impl StorageTransactionIo for CommitOrderIo {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        if path.file_name().is_some_and(|name| name == "markdown") {
            assert!(
                *self.marker_directory_synced.lock().unwrap(),
                "live directory must not be created before the commit marker directory is synced"
            );
        }
        FileSystemStorageTransactionIo.create_dir_all(path)
    }

    fn create_new_file(&self, path: &Path) -> std::io::Result<()> {
        if path.file_name().is_some_and(|name| name == "commit.tmp") {
            assert!(
                *self.manifest_file_synced.lock().unwrap(),
                "immutable manifest must be synced before marker publication starts"
            );
        } else if path.parent() == Some(self.storage_dir_path.as_path())
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
        if path.file_name().is_some_and(|name| name == "manifest.json") {
            *self.manifest_file_synced.lock().unwrap() = true;
        } else if path.file_name().is_some_and(|name| name == "commit.tmp") {
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
    MarkerRename,
    MarkerDirectorySync,
    LiveWrite,
    LiveSync,
    LiveRename,
    TargetDirectory,
    LiveDirectorySync,
    RevisionWrite,
    RevisionSync,
    RevisionRename,
    CleanupRename,
    CleanupHandoffSync,
    CleanupDelete,
}

struct FailingCommitIo {
    phase: FailingCommitPhase,
    marker_published: AtomicBool,
    marker_dir_path: Mutex<Option<PathBuf>>,
    live_target_renamed: AtomicBool,
    cleanup_handoff: AtomicBool,
}

impl StorageTransactionIo for FailingCommitIo {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        if matches!(self.phase, FailingCommitPhase::TargetDirectory)
            && path.file_name().is_some_and(|name| name == "markdown")
        {
            return Err(std::io::Error::other("injected target directory failure"));
        }
        FileSystemStorageTransactionIo.create_dir_all(path)
    }

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
            || (matches!(self.phase, FailingCommitPhase::RevisionSync)
                && file_name.contains("revision"))
        {
            return Err(std::io::Error::other("injected commit sync failure"));
        }
        FileSystemStorageTransactionIo.sync_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        if to.file_name().is_some_and(|name| name == "commit")
            && matches!(self.phase, FailingCommitPhase::MarkerRename)
        {
            return Err(std::io::Error::other("injected marker rename failure"));
        }
        if matches!(self.phase, FailingCommitPhase::LiveRename)
            && to.file_name().is_some_and(|name| name == "project.yaml")
        {
            return Err(std::io::Error::other("injected live rename failure"));
        }
        if matches!(self.phase, FailingCommitPhase::RevisionRename)
            && to.file_name().is_some_and(|name| name == ".revision")
        {
            return Err(std::io::Error::other("injected revision rename failure"));
        }
        if to
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".cleanup-"))
            && matches!(self.phase, FailingCommitPhase::CleanupRename)
        {
            return Err(std::io::Error::other("injected cleanup rename failure"));
        }
        let result = FileSystemStorageTransactionIo.rename(from, to);
        if result.is_ok() {
            if to.file_name().is_some_and(|name| name == "commit") {
                self.marker_published.store(true, Ordering::SeqCst);
                *self.marker_dir_path.lock().unwrap() = to.parent().map(Path::to_path_buf);
            } else if to.file_name().is_some_and(|name| name == "project.yaml") {
                self.live_target_renamed.store(true, Ordering::SeqCst);
            } else if to
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".cleanup-"))
            {
                self.cleanup_handoff.store(true, Ordering::SeqCst);
            }
        }
        result
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        if matches!(self.phase, FailingCommitPhase::MarkerDirectorySync)
            && self
                .marker_dir_path
                .lock()
                .unwrap()
                .as_deref()
                .is_some_and(|marker_dir_path| marker_dir_path == path)
        {
            return Err(std::io::Error::other(
                "injected marker directory sync failure",
            ));
        }
        if matches!(self.phase, FailingCommitPhase::LiveDirectorySync)
            && self.marker_published.load(Ordering::SeqCst)
            && self.live_target_renamed.load(Ordering::SeqCst)
        {
            return Err(std::io::Error::other(
                "injected live directory sync failure",
            ));
        }
        if matches!(self.phase, FailingCommitPhase::CleanupHandoffSync)
            && self.cleanup_handoff.load(Ordering::SeqCst)
        {
            return Err(std::io::Error::other(
                "injected cleanup handoff sync failure",
            ));
        }
        FileSystemStorageTransactionIo.sync_directory(path)
    }

    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        if matches!(self.phase, FailingCommitPhase::CleanupDelete) {
            return Err(std::io::Error::other("injected cleanup failure"));
        }
        FileSystemStorageTransactionIo.remove_dir_all(path)
    }
}

impl StorageTransactionIo for FailingStagedFileIo {
    fn target_permissions(&self, path: &Path) -> std::io::Result<Option<fs::Permissions>> {
        if matches!(self.phase, FailingStagedFilePhase::ReadMetadata) {
            return Err(std::io::Error::other("injected metadata failure"));
        }
        FileSystemStorageTransactionIo.target_permissions(path)
    }

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

    fn sync_file(&self, path: &Path) -> std::io::Result<()> {
        if matches!(self.phase, FailingStagedFilePhase::Sync) {
            return Err(std::io::Error::other("injected sync failure"));
        }
        FileSystemStorageTransactionIo.sync_file(path)
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
fn test_recover_uncommitted_markerなしactive_transactionを破棄して再実行できる() {
    let storage_dir = TestStorageDir::new();
    let active_transaction_path = storage_dir
        .path
        .join(TRANSACTION_DIRECTORY_NAME)
        .join(ACTIVE_TRANSACTION_DIRECTORY_NAME);
    fs::create_dir_all(active_transaction_path.join("files")).unwrap();
    fs::write(active_transaction_path.join("files/0"), b"partial").unwrap();
    fs::write(active_transaction_path.join("commit.tmp"), b"").unwrap();

    recover_uncommitted(file_system_io(), &storage_dir.path).unwrap();
    recover_uncommitted(file_system_io(), &storage_dir.path).unwrap();

    assert!(!active_transaction_path.exists());
}

#[test]
fn test_recover_uncommitted_markerありactive_transactionを破棄しない() {
    let storage_dir = TestStorageDir::new();
    let active_transaction_path = storage_dir
        .path
        .join(TRANSACTION_DIRECTORY_NAME)
        .join(ACTIVE_TRANSACTION_DIRECTORY_NAME);
    fs::create_dir_all(&active_transaction_path).unwrap();
    fs::write(active_transaction_path.join("commit"), b"").unwrap();

    let actual = recover_uncommitted(file_system_io(), &storage_dir.path).unwrap_err();

    assert!(actual.to_string().contains("CommittedTransaction"));
    assert!(active_transaction_path.join("commit").is_file());
}

#[cfg(unix)]
#[test]
fn test_transaction_root_symlinkはprepareとrecoveryで拒否して外部を変更しない() {
    use std::os::unix::fs::symlink;

    let storage_dir = TestStorageDir::new();
    let external_dir = TestStorageDir::new();
    let external_active_path = external_dir.path.join(ACTIVE_TRANSACTION_DIRECTORY_NAME);
    fs::create_dir(&external_active_path).unwrap();
    let external_manifest_path = external_active_path.join("manifest.json");
    fs::write(&external_manifest_path, b"external").unwrap();
    let transactions_dir_path = storage_dir.path.join(TRANSACTION_DIRECTORY_NAME);
    symlink(&external_dir.path, &transactions_dir_path).unwrap();
    let target_path = storage_dir.path.join("project.yaml");

    let recover_error = recover_uncommitted(file_system_io(), &storage_dir.path).unwrap_err();
    let prepare_error = match prepare(
        file_system_io(),
        &storage_dir.path,
        Uuid::from_u128(0x2254),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"new",
        }],
    ) {
        Err(error) => error,
        Ok(_) => panic!("transaction root symlink must be rejected"),
    };

    for error in [recover_error, prepare_error] {
        assert_eq!(
            error.operation,
            StorageTransactionOperation::ValidateTransactionDirectory
        );
        assert_eq!(error.path, transactions_dir_path);
    }
    assert_eq!(fs::read(external_manifest_path).unwrap(), b"external");
    assert!(!target_path.exists());
}

#[test]
fn test_recover_uncommitted_prepared_transaction_drop後にlockを再取得する() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
    let prepared = prepare(
        file_system_io(),
        &storage_dir.path,
        Uuid::from_u128(0x2253),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"new",
        }],
    )
    .unwrap();
    let active_transaction_path = prepared.transaction_dir_path.clone();

    drop(prepared);
    recover_uncommitted(file_system_io(), &storage_dir.path).unwrap();

    assert!(!active_transaction_path.exists());
}

#[test]
fn test_recover_uncommitted_marker公開中のlive_writerとは競合してactiveを削除しない() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
    let revision_path = storage_dir.path.join(".revision");
    let io = Arc::new(BlockingMarkerPublicationIo {
        marker_published: AtomicBool::new(false),
        marker_sync_started: Barrier::new(2),
        marker_sync_resume: Barrier::new(2),
    });
    let prepared = prepare(
        io.clone(),
        &storage_dir.path,
        Uuid::from_u128(0x2252),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"new",
        }],
    )
    .unwrap();
    let active_transaction_path = prepared.transaction_dir_path.clone();
    let commit_thread = std::thread::spawn(move || prepared.commit(&revision_path));
    io.marker_sync_started.wait();

    let actual = recover_uncommitted(file_system_io(), &storage_dir.path);
    let marker_was_preserved = active_transaction_path.join("commit").is_file();
    io.marker_sync_resume.wait();
    let commit_result = commit_thread.join();

    let actual = actual.unwrap_err();
    assert!(actual.to_string().contains("AcquireTransactionLock"));
    assert!(actual.source().is_some_and(|source| source
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| { error.kind() == std::io::ErrorKind::WouldBlock })));
    assert!(marker_was_preserved);
    commit_result.unwrap().unwrap();
    assert_eq!(fs::read(target_path).unwrap(), b"new");
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
    assert!(!transactions_dir_path
        .join(ACTIVE_TRANSACTION_DIRECTORY_NAME)
        .exists());
}

#[test]
fn test_prepare_cleanup_tombstoneはactive_transactionとして扱わない() {
    let storage_dir = TestStorageDir::new();
    let transactions_dir_path = storage_dir.path.join(TRANSACTION_DIRECTORY_NAME);
    fs::create_dir_all(transactions_dir_path.join(format!(".cleanup-{}", Uuid::from_u128(0x2211))))
        .unwrap();
    let target_path = storage_dir.path.join("project.yaml");

    let prepared = prepare(
        file_system_io(),
        &storage_dir.path,
        Uuid::from_u128(0x2212),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"new",
        }],
    )
    .unwrap();

    prepared.discard().unwrap();
}

#[test]
fn test_prepare_同時実行では一方だけがactive_transactionを取得する() {
    let storage_dir = TestStorageDir::new();
    let barrier = Arc::new(Barrier::new(3));
    let handles = (0..2)
        .map(|index| {
            let barrier = barrier.clone();
            let storage_dir_path = storage_dir.path.clone();
            std::thread::spawn(move || {
                let target_path = storage_dir_path.join(format!("project-{index}.yaml"));
                barrier.wait();
                prepare(
                    file_system_io(),
                    &storage_dir_path,
                    Uuid::from_u128(0x2213 + index),
                    &[WriteRequest {
                        target_path: &target_path,
                        bytes: b"new",
                    }],
                )
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();

    let mut prepared_transaction = None;
    let mut errors = Vec::new();
    for handle in handles {
        match handle.join().unwrap() {
            Ok(prepared) => {
                assert!(prepared_transaction.replace(prepared).is_none());
            }
            Err(error) => errors.push(error),
        }
    }

    assert_eq!(errors.len(), 1);
    assert!(errors[0].to_string().contains("ActiveTransaction"));
    assert!(storage_dir
        .path
        .join(TRANSACTION_DIRECTORY_NAME)
        .join(ACTIVE_TRANSACTION_DIRECTORY_NAME)
        .is_dir());
    prepared_transaction.unwrap().discard().unwrap();
}

#[test]
fn test_prepare_file_targetのpath_escapeと予約namespaceを拒否する() {
    let storage_dir = TestStorageDir::new();
    let target_paths = [
        storage_dir.path.clone(),
        storage_dir.path.join("../escaped.yaml"),
        storage_dir.path.join(".schronu-transactions/live.yaml"),
    ];
    for target_path in target_paths {
        let actual = prepare(
            file_system_io(),
            &storage_dir.path,
            Uuid::from_u128(0x2214),
            &[WriteRequest {
                target_path: &target_path,
                bytes: b"new",
            }],
        );

        let error = match actual {
            Err(error) => error,
            Ok(prepared) => {
                prepared.discard().unwrap();
                panic!("invalid file target must fail: {}", target_path.display());
            }
        };
        assert_eq!(
            error.operation,
            StorageTransactionOperation::ValidateTargetPath
        );
        assert_eq!(error.path, target_path);
        assert!(!storage_dir
            .path
            .join(TRANSACTION_DIRECTORY_NAME)
            .join(ACTIVE_TRANSACTION_DIRECTORY_NAME)
            .exists());
    }
}

#[test]
fn test_prepare_directory_targetの空path_escapeと予約namespaceを拒否する() {
    let storage_dir = TestStorageDir::new();
    let directory_paths = [
        storage_dir.path.clone(),
        storage_dir.path.join("../escaped"),
        storage_dir.path.join(".schronu-transactions/live"),
    ];

    for directory_path in &directory_paths {
        let actual = prepare_with_directories(
            file_system_io(),
            &storage_dir.path,
            Uuid::from_u128(0x2215),
            &[],
            &[directory_path],
        );

        let error = match actual {
            Err(error) => error,
            Ok(prepared) => {
                prepared.discard().unwrap();
                panic!(
                    "invalid directory target must fail: {}",
                    directory_path.display()
                );
            }
        };
        assert_eq!(
            error.operation,
            StorageTransactionOperation::ValidateTargetPath
        );
        assert_eq!(error.path, *directory_path);
        assert!(!storage_dir
            .path
            .join(TRANSACTION_DIRECTORY_NAME)
            .join(ACTIVE_TRANSACTION_DIRECTORY_NAME)
            .exists());
    }
}

#[test]
fn test_prepare_staged_file失敗はpathとphaseを保持する() {
    for (phase, expected_operation, reports_target_path) in [
        (
            FailingStagedFilePhase::ReadMetadata,
            StorageTransactionOperation::ReadTargetMetadata,
            true,
        ),
        (
            FailingStagedFilePhase::Create,
            StorageTransactionOperation::CreateStagedFile,
            false,
        ),
        (
            FailingStagedFilePhase::SetPermissions,
            StorageTransactionOperation::SetStagedPermissions,
            false,
        ),
        (
            FailingStagedFilePhase::Write,
            StorageTransactionOperation::WriteStagedFile,
            false,
        ),
        (
            FailingStagedFilePhase::Sync,
            StorageTransactionOperation::SyncStagedFile,
            false,
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
        if reports_target_path {
            assert_eq!(error.path, target_path);
        } else {
            assert_eq!(error.path.file_name().unwrap(), "0");
            assert_eq!(error.path.parent().unwrap().file_name().unwrap(), "files");
        }
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
        manifest_file_synced: Mutex::new(false),
        marker_file_synced: Mutex::new(false),
        marker_directory_synced: Mutex::new(false),
        first_target_path: first_target_path.clone(),
        second_target_path: second_target_path.clone(),
    });
    let markdown_dir_path = storage_dir.path.join("third/markdown");
    let prepared = prepare_with_directories(
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
        &[&markdown_dir_path],
    )
    .unwrap();
    let manifest: Value = serde_json::from_slice(
        &fs::read(prepared.transaction_dir_path.join("manifest.json")).unwrap(),
    )
    .unwrap();
    let transaction_id = Uuid::parse_str(manifest["transaction_id"].as_str().unwrap()).unwrap();
    assert_eq!(manifest["revision"], revision.to_string());
    assert_eq!(manifest["directories"][0], "third/markdown");

    prepared
        .commit(&storage_dir.path.join(".revision"))
        .unwrap();

    assert_ne!(transaction_id, Uuid::nil());
    assert_eq!(fs::read(first_target_path).unwrap(), b"first-new");
    assert_eq!(fs::read(second_target_path).unwrap(), b"second-new");
    assert!(markdown_dir_path.is_dir());
    assert_eq!(
        fs::read_to_string(storage_dir.path.join(".revision")).unwrap(),
        format!("{revision}\n")
    );
    let transaction_entries = fs::read_dir(storage_dir.path.join(TRANSACTION_DIRECTORY_NAME))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(transaction_entries.len(), 1);
    assert_eq!(
        transaction_entries[0].file_name(),
        TRANSACTION_LOCK_FILE_NAME
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
        FailingCommitPhase::MarkerRename,
        FailingCommitPhase::MarkerDirectorySync,
        FailingCommitPhase::LiveWrite,
        FailingCommitPhase::LiveSync,
        FailingCommitPhase::LiveRename,
        FailingCommitPhase::TargetDirectory,
        FailingCommitPhase::LiveDirectorySync,
        FailingCommitPhase::RevisionWrite,
        FailingCommitPhase::RevisionSync,
        FailingCommitPhase::RevisionRename,
        FailingCommitPhase::CleanupRename,
    ] {
        let storage_dir = TestStorageDir::new();
        let target_path = storage_dir.path.join("project.yaml");
        fs::write(&target_path, b"old").unwrap();
        let markdown_dir_path = storage_dir.path.join("markdown");
        let prepared = prepare_with_directories(
            Arc::new(FailingCommitIo {
                phase,
                marker_published: AtomicBool::new(false),
                marker_dir_path: Mutex::new(None),
                live_target_renamed: AtomicBool::new(false),
                cleanup_handoff: AtomicBool::new(false),
            }),
            &storage_dir.path,
            Uuid::from_u128(0x2209),
            &[WriteRequest {
                target_path: &target_path,
                bytes: b"new",
            }],
            &[&markdown_dir_path],
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
                FailingCommitPhase::MarkerCreate
                    | FailingCommitPhase::MarkerSync
                    | FailingCommitPhase::MarkerRename
            )
        );
    }
}

#[test]
fn test_commit_cleanup失敗はtombstoneへ回復情報を保持して成功する() {
    for phase in [
        FailingCommitPhase::CleanupHandoffSync,
        FailingCommitPhase::CleanupDelete,
    ] {
        let storage_dir = TestStorageDir::new();
        let target_path = storage_dir.path.join("project.yaml");
        fs::write(&target_path, b"old").unwrap();
        let prepared = prepare(
            Arc::new(FailingCommitIo {
                phase,
                marker_published: AtomicBool::new(false),
                marker_dir_path: Mutex::new(None),
                live_target_renamed: AtomicBool::new(false),
                cleanup_handoff: AtomicBool::new(false),
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
        assert!(cleanup_dir_path.join("commit").is_file(), "{phase:?}");
        assert!(
            cleanup_dir_path.join("manifest.json").is_file(),
            "{phase:?}"
        );
        assert!(cleanup_dir_path.join("files/0").is_file(), "{phase:?}");
        assert_eq!(fs::read(target_path).unwrap(), b"new");
    }
}
