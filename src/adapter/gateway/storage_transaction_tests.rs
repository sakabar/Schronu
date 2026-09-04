use super::*;
use serde_json::Value;

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
