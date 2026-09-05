
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

struct FailTargetContentReadIo {
    target_path: PathBuf,
}

impl StorageTransactionIo for FailTargetContentReadIo {
    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        if path == self.target_path {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected target content read failure",
            ));
        }
        FileSystemStorageTransactionIo.read_file(path)
    }
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

struct DeleteCommitOrderIo {
    target_path: PathBuf,
    marker_file_synced: AtomicBool,
    marker_directory_path: Mutex<Option<PathBuf>>,
    marker_directory_synced: AtomicBool,
}

impl StorageTransactionIo for DeleteCommitOrderIo {
    fn sync_file(&self, path: &Path) -> std::io::Result<()> {
        if path.file_name().is_some_and(|name| name == "commit.tmp") {
            self.marker_file_synced.store(true, Ordering::SeqCst);
        }
        FileSystemStorageTransactionIo.sync_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        if to.file_name().is_some_and(|name| name == "commit") {
            assert!(self.marker_file_synced.load(Ordering::SeqCst));
            *self.marker_directory_path.lock().unwrap() = to.parent().map(Path::to_path_buf);
        }
        FileSystemStorageTransactionIo.rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        if path == self.target_path {
            assert!(
                self.marker_directory_synced.load(Ordering::SeqCst),
                "delete target must remain unchanged until the commit marker directory is synced"
            );
        }
        FileSystemStorageTransactionIo.remove_file(path)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        if self
            .marker_directory_path
            .lock()
            .unwrap()
            .as_deref()
            .is_some_and(|marker_directory_path| marker_directory_path == path)
        {
            self.marker_directory_synced.store(true, Ordering::SeqCst);
        }
        FileSystemStorageTransactionIo.sync_directory(path)
    }
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

struct DeleteSyncIo {
    target_path: PathBuf,
    target_removed: AtomicBool,
    parent_synced_after_remove: AtomicBool,
    fail_first_target_parent_sync: AtomicBool,
}

impl StorageTransactionIo for DeleteSyncIo {
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        let result = FileSystemStorageTransactionIo.remove_file(path);
        if result.is_ok() && path == self.target_path {
            self.target_removed.store(true, Ordering::SeqCst);
        }
        result
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        if self.target_removed.load(Ordering::SeqCst) && Some(path) == self.target_path.parent() {
            if self
                .fail_first_target_parent_sync
                .swap(false, Ordering::SeqCst)
            {
                return Err(std::io::Error::other(
                    "injected delete directory sync failure",
                ));
            }
            self.parent_synced_after_remove
                .store(true, Ordering::SeqCst);
        }
        FileSystemStorageTransactionIo.sync_directory(path)
    }
}

fn create_delete_transaction(
    storage_dir_path: &Path,
    target: &str,
    revision: Uuid,
    committed: bool,
) -> PathBuf {
    let transaction_dir_path = storage_dir_path
        .join(TRANSACTION_DIRECTORY_NAME)
        .join(ACTIVE_TRANSACTION_DIRECTORY_NAME);
    fs::create_dir_all(transaction_dir_path.join("files")).unwrap();
    let manifest = serde_json::json!({
        "version": 1,
        "transaction_id": Uuid::from_u128(0x2230),
        "revision": revision,
        "directories": [],
        "entries": [{
            "target": target,
            "operation": "delete"
        }]
    });
    fs::write(
        transaction_dir_path.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    if committed {
        fs::write(transaction_dir_path.join("commit"), b"").unwrap();
    }
    transaction_dir_path
}

fn prepare_delete_transaction(
    io: Arc<dyn StorageTransactionIo>,
    storage_dir_path: &Path,
    target: &str,
    revision: Uuid,
) -> PreparedTransaction {
    let transaction_dir_path = create_delete_transaction(storage_dir_path, target, revision, false);
    let transactions_dir_path = transaction_dir_path.parent().unwrap().to_path_buf();
    let manifest_path = transaction_dir_path.join("manifest.json");
    let manifest = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let transaction_lock = acquire_transaction_lock(&transactions_dir_path).unwrap();
    prepared_from_manifest(
        io,
        storage_dir_path,
        transactions_dir_path,
        transaction_dir_path,
        manifest_path,
        manifest,
        transaction_lock,
    )
    .unwrap()
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
