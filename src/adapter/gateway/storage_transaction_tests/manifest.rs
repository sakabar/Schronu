#[test]
fn test_manifest_entryは旧write形式とdelete_operationを区別する() {
    let old_write: ManifestEntry = serde_json::from_value(serde_json::json!({
        "target": "project.yaml",
        "staged_file": "files/0"
    }))
    .unwrap();
    let delete: ManifestEntry = serde_json::from_value(serde_json::json!({
        "target": "project.yaml",
        "operation": "delete"
    }))
    .unwrap();

    assert_eq!(old_write.operation, ManifestEntryOperation::Write);
    assert_eq!(old_write.staged_file, Some(PathBuf::from("files/0")));
    assert_eq!(delete.operation, ManifestEntryOperation::Delete);
    assert_eq!(delete.staged_file, None);
    let serialized_delete = serde_json::to_value(&delete).unwrap();
    assert!(serialized_delete.get("content_length").is_none());
    assert!(serialized_delete.get("content_checksum").is_none());
    assert_eq!(
        serde_json::to_value(&old_write).unwrap(),
        serde_json::json!({
            "target": "project.yaml",
            "staged_file": "files/0"
        })
    );
}

#[test]
fn test_manifest_v1の保存bytesはfield順と省略規則を維持する() {
    let manifest = TransactionManifest {
        version: 1,
        transaction_id: Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap(),
        revision: Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap(),
        directories: vec![PathBuf::from("project"), PathBuf::from("archive")],
        entries: vec![
            ManifestEntry {
                target: PathBuf::from("project/project.yaml"),
                operation: ManifestEntryOperation::Write,
                staged_file: Some(PathBuf::from("files/0")),
                content_length: Some(12),
                content_checksum: Some("fnv1a64:0123456789abcdef".to_string()),
            },
            ManifestEntry {
                target: PathBuf::from("archive/old.yaml"),
                operation: ManifestEntryOperation::Delete,
                staged_file: None,
                content_length: None,
                content_checksum: None,
            },
        ],
    };

    assert_eq!(
        serde_json::to_vec(&manifest).unwrap(),
        br#"{"version":1,"transaction_id":"11111111-2222-3333-4444-555555555555","revision":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","directories":["project","archive"],"entries":[{"target":"project/project.yaml","staged_file":"files/0","content_length":12,"content_checksum":"fnv1a64:0123456789abcdef"},{"target":"archive/old.yaml","operation":"delete"}]}"#
    );
}

#[test]
fn test_committed_write_manifestは内容検証情報の欠落を拒否する() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
    let prepared = prepare(
        file_system_io(),
        &storage_dir.path,
        Uuid::from_u128(0x2240),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"new",
        }],
    )
    .unwrap();
    let transaction_dir_path = prepared.transaction_dir_path().to_path_buf();
    drop(prepared);
    let manifest_path = transaction_dir_path.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["entries"][0]
        .as_object_mut()
        .unwrap()
        .remove("content_length");
    manifest["entries"][0]
        .as_object_mut()
        .unwrap()
        .remove("content_checksum");
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::write(transaction_dir_path.join("commit"), b"").unwrap();

    let actual = recover(file_system_io(), &storage_dir.path).unwrap_err();

    assert_eq!(
        actual.operation,
        StorageTransactionOperation::ValidateManifest
    );
    assert_eq!(
        fs::read(&target_path).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
}

#[test]
fn test_committed_write_manifestは不正な内容検証情報を拒否する() {
    for (content_length, content_checksum) in [
        (serde_json::json!(3), serde_json::json!("sha256:abc")),
        (serde_json::json!(3), serde_json::json!("fnv1a64:1234")),
        (
            serde_json::json!(u64::MAX),
            serde_json::json!("fnv1a64:0123456789abcdef"),
        ),
    ] {
        let storage_dir = TestStorageDir::new();
        let target_path = storage_dir.path.join("project.yaml");
        let prepared = prepare(
            file_system_io(),
            &storage_dir.path,
            Uuid::from_u128(0x2241),
            &[WriteRequest {
                target_path: &target_path,
                bytes: b"new",
            }],
        )
        .unwrap();
        let transaction_dir_path = prepared.transaction_dir_path().to_path_buf();
        drop(prepared);
        let manifest_path = transaction_dir_path.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["entries"][0]["content_length"] = content_length;
        manifest["entries"][0]["content_checksum"] = content_checksum;
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        fs::write(transaction_dir_path.join("commit"), b"").unwrap();

        let actual = recover(file_system_io(), &storage_dir.path).unwrap_err();

        assert_eq!(
            actual.operation,
            StorageTransactionOperation::ValidateManifest
        );
        assert_eq!(
            fs::read(&target_path).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }
}
