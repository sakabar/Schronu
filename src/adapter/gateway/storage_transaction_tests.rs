use super::*;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    fn write_new_file(
        &self,
        path: &Path,
        bytes: &[u8],
        permissions: Option<fs::Permissions>,
    ) -> std::io::Result<()> {
        let call = self.write_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_write_call == Some(call) {
            return Err(std::io::Error::other("injected write/sync failure"));
        }
        FileSystemStorageTransactionIo.write_new_file(path, bytes, permissions)
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
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
    fs::write(&target_path, b"old").unwrap();
    let io = Arc::new(FailingPrepareIo::new(None, None, Some(1)));

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
