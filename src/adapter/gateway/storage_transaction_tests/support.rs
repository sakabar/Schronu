
fn file_system_io() -> Arc<dyn StorageTransactionIo> {
    Arc::new(FileSystemStorageTransactionIo)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordingOperation {
    CreateDirectory,
    ReadTargetMetadata,
    ReadFile,
    CreateFile,
    SetPermissions,
    WriteFile,
    SyncFile,
    Rename,
    RemoveDirectory,
    RemoveFile,
    SyncDirectory,
}

#[derive(Debug)]
enum PathMatcher {
    Any,
    Exact(PathBuf),
    FileName(&'static str),
    FileNamePrefix(&'static str),
    FileNameContains(&'static str),
}

struct FaultRule {
    operation: RecordingOperation,
    path_matcher: PathMatcher,
    occurrence: usize,
    error_kind: std::io::ErrorKind,
    error_message: &'static str,
}

#[derive(Clone, Debug)]
struct IoEvent {
    operation: RecordingOperation,
    path: PathBuf,
}

struct RecordingIo {
    faults: Vec<FaultRule>,
    events: Mutex<Vec<IoEvent>>,
}

impl RecordingIo {
    fn new(faults: Vec<FaultRule>) -> Self {
        Self {
            faults,
            events: Mutex::new(Vec::new()),
        }
    }

    fn record(&self, operation: RecordingOperation, path: &Path) -> std::io::Result<()> {
        let mut events = self.events.lock().unwrap();
        events.push(IoEvent {
            operation,
            path: path.to_path_buf(),
        });
        for fault in &self.faults {
            if fault.operation != operation || !fault.path_matcher.matches(path) {
                continue;
            }
            let occurrence = events
                .iter()
                .filter(|event| {
                    event.operation == fault.operation && fault.path_matcher.matches(&event.path)
                })
                .count();
            if occurrence == fault.occurrence {
                return Err(std::io::Error::new(
                    fault.error_kind,
                    fault.error_message,
                ));
            }
        }
        Ok(())
    }

    fn events(&self) -> Vec<IoEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl PathMatcher {
    fn matches(&self, path: &Path) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(expected) => path == expected,
            Self::FileName(expected) => path.file_name().is_some_and(|name| name == *expected),
            Self::FileNamePrefix(expected) => path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(expected)),
            Self::FileNameContains(expected) => path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains(expected)),
        }
    }
}

fn event_position(
    events: &[IoEvent],
    operation: RecordingOperation,
    path_matcher: &PathMatcher,
    occurrence: usize,
) -> usize {
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            event.operation == operation && path_matcher.matches(event.path.as_path())
        })
        .nth(occurrence - 1)
        .map(|(index, _)| index)
        .unwrap_or_else(|| {
            panic!(
                "missing event {operation:?} matching {path_matcher:?} occurrence {occurrence}"
            )
        })
}

fn last_event_position(
    events: &[IoEvent],
    operation: RecordingOperation,
    path_matcher: &PathMatcher,
) -> usize {
    events
        .iter()
        .rposition(|event| {
            event.operation == operation && path_matcher.matches(event.path.as_path())
        })
        .unwrap_or_else(|| panic!("missing event {operation:?} matching {path_matcher:?}"))
}

impl StorageTransactionIo for RecordingIo {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::CreateDirectory, path)?;
        FileSystemStorageTransactionIo.create_dir_all(path)
    }

    fn target_permissions(&self, path: &Path) -> std::io::Result<Option<fs::Permissions>> {
        self.record(RecordingOperation::ReadTargetMetadata, path)?;
        FileSystemStorageTransactionIo.target_permissions(path)
    }

    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.record(RecordingOperation::ReadFile, path)?;
        FileSystemStorageTransactionIo.read_file(path)
    }

    fn create_new_file(&self, path: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::CreateFile, path)?;
        FileSystemStorageTransactionIo.create_new_file(path)
    }

    fn set_permissions(&self, path: &Path, permissions: fs::Permissions) -> std::io::Result<()> {
        self.record(RecordingOperation::SetPermissions, path)?;
        FileSystemStorageTransactionIo.set_permissions(path, permissions)
    }

    fn write_file(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        self.record(RecordingOperation::WriteFile, path)?;
        FileSystemStorageTransactionIo.write_file(path, bytes)
    }

    fn sync_file(&self, path: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::SyncFile, path)?;
        FileSystemStorageTransactionIo.sync_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::Rename, to)?;
        FileSystemStorageTransactionIo.rename(from, to)
    }

    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::RemoveDirectory, path)?;
        FileSystemStorageTransactionIo.remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::RemoveFile, path)?;
        FileSystemStorageTransactionIo.remove_file(path)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::SyncDirectory, path)?;
        FileSystemStorageTransactionIo.sync_directory(path)
    }
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

struct TestStorageDir {
    path: PathBuf,
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
    let manifest = serde_json::from_slice(
        &fs::read(transaction_dir_path.join("manifest.json")).unwrap(),
    )
    .unwrap();
    let transaction_lock = acquire_transaction_lock(&transactions_dir_path).unwrap();
    prepared_from_manifest(
        io,
        TransactionPaths {
            storage_dir_path: storage_dir_path.to_path_buf(),
            transactions_dir_path,
            transaction_dir_path,
        },
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
