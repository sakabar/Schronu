
use super::io::acquire_transaction_lock;

fn file_system_io() -> Arc<dyn StorageTransactionIo> {
    Arc::new(FileSystemStorageTransactionIo)
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
