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

    recover(file_system_io(), &storage_dir.path).unwrap();
    recover(file_system_io(), &storage_dir.path).unwrap();

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

    let actual = recover(file_system_io(), &storage_dir.path).unwrap_err();

    assert!(actual.to_string().contains("ReadManifest"));
    assert!(active_transaction_path.join("commit").is_file());
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
    let active_transaction_path = prepared.transaction_dir_path().to_path_buf();

    drop(prepared);
    recover(file_system_io(), &storage_dir.path).unwrap();

    assert!(!active_transaction_path.exists());
}

#[test]
fn test_recover_uncommitted_marker公開中のlive_writerとは競合してactiveを削除しない() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
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
    let active_transaction_path = prepared.transaction_dir_path().to_path_buf();
    let commit_thread = std::thread::spawn(move || prepared.commit());
    io.marker_sync_started.wait();

    let actual = recover(file_system_io(), &storage_dir.path);
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
