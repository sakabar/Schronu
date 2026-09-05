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
fn test_commit_target内容読込失敗はpathと専用phaseを保持する() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
    fs::write(&target_path, b"old").unwrap();
    let prepared = prepare(
        Arc::new(FailTargetContentReadIo {
            target_path: target_path.clone(),
        }),
        &storage_dir.path,
        Uuid::from_u128(0x2241),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"new",
        }],
    )
    .unwrap();
    let transaction_dir_path = prepared.transaction_dir_path.clone();

    let actual = prepared
        .commit(&storage_dir.path.join(".revision"))
        .unwrap_err();

    assert_eq!(
        actual.operation,
        StorageTransactionOperation::ReadTargetContent
    );
    assert_eq!(actual.path, target_path);
    assert_eq!(actual.source.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read(&actual.path).unwrap(), b"old");
    assert!(!storage_dir.path.join(".revision").exists());
    assert!(transaction_dir_path.join("commit").is_file());
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
