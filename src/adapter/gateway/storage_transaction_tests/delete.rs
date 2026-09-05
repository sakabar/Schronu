#[test]
fn test_delete_entry_markerなしではtargetを維持しtransactionを破棄する() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project/project.yaml");
    fs::create_dir_all(target_path.parent().unwrap()).unwrap();
    fs::write(&target_path, b"old").unwrap();
    let transaction_dir_path = create_delete_transaction(
        &storage_dir.path,
        "project/project.yaml",
        Uuid::from_u128(0x2231),
        false,
    );

    recover(file_system_io(), &storage_dir.path).unwrap();

    assert_eq!(fs::read(target_path).unwrap(), b"old");
    assert!(!transaction_dir_path.exists());
}

#[test]
fn test_delete_entry_markerありではtargetを削除しrevisionを更新する() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project/project.yaml");
    fs::create_dir_all(target_path.parent().unwrap()).unwrap();
    fs::write(&target_path, b"old").unwrap();
    let revision = Uuid::from_u128(0x2232);
    create_delete_transaction(&storage_dir.path, "project/project.yaml", revision, true);

    recover(file_system_io(), &storage_dir.path).unwrap();

    assert!(!target_path.exists());
    assert_eq!(
        fs::read_to_string(storage_dir.path.join(".revision")).unwrap(),
        format!("{revision}\n")
    );
}

#[test]
fn test_delete_entryは通常commitでmarker公開後にtargetを削除する() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project/project.yaml");
    fs::create_dir_all(target_path.parent().unwrap()).unwrap();
    fs::write(&target_path, b"old").unwrap();
    let revision = Uuid::from_u128(0x2238);
    let io = Arc::new(DeleteCommitOrderIo {
        target_path: target_path.clone(),
        marker_file_synced: AtomicBool::new(false),
        marker_directory_path: Mutex::new(None),
        marker_directory_synced: AtomicBool::new(false),
    });
    let prepared = prepare_delete_transaction(
        io.clone(),
        &storage_dir.path,
        "project/project.yaml",
        revision,
    );

    prepared
        .commit(&storage_dir.path.join(".revision"))
        .unwrap();

    assert!(!target_path.exists());
    assert!(io.marker_file_synced.load(Ordering::SeqCst));
    assert!(io.marker_directory_synced.load(Ordering::SeqCst));
    assert_eq!(
        fs::read_to_string(storage_dir.path.join(".revision")).unwrap(),
        format!("{revision}\n")
    );
}

#[test]
fn test_delete_entry_targetがなくても再実行可能である() {
    let storage_dir = TestStorageDir::new();
    let revision = Uuid::from_u128(0x2233);
    create_delete_transaction(&storage_dir.path, "project/project.yaml", revision, true);

    recover(file_system_io(), &storage_dir.path).unwrap();
    recover(file_system_io(), &storage_dir.path).unwrap();

    assert_eq!(
        fs::read_to_string(storage_dir.path.join(".revision")).unwrap(),
        format!("{revision}\n")
    );
}

#[test]
fn test_delete_entry後にparent_directoryをsyncする() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project/project.yaml");
    fs::create_dir_all(target_path.parent().unwrap()).unwrap();
    fs::write(&target_path, b"old").unwrap();
    create_delete_transaction(
        &storage_dir.path,
        "project/project.yaml",
        Uuid::from_u128(0x2234),
        true,
    );
    let io = Arc::new(DeleteSyncIo {
        target_path,
        target_removed: AtomicBool::new(false),
        parent_synced_after_remove: AtomicBool::new(false),
        fail_first_target_parent_sync: AtomicBool::new(false),
    });

    recover(io.clone(), &storage_dir.path).unwrap();

    assert!(io.target_removed.load(Ordering::SeqCst));
    assert!(io.parent_synced_after_remove.load(Ordering::SeqCst));
}

#[test]
fn test_delete_entry_sync中断後の回復再試行でnew_snapshotへ到達する() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project/project.yaml");
    fs::create_dir_all(target_path.parent().unwrap()).unwrap();
    fs::write(&target_path, b"old").unwrap();
    let revision = Uuid::from_u128(0x2235);
    let transaction_dir_path =
        create_delete_transaction(&storage_dir.path, "project/project.yaml", revision, true);
    let io = Arc::new(DeleteSyncIo {
        target_path: target_path.clone(),
        target_removed: AtomicBool::new(false),
        parent_synced_after_remove: AtomicBool::new(false),
        fail_first_target_parent_sync: AtomicBool::new(true),
    });

    let first = recover(io, &storage_dir.path);
    assert!(first.is_err());
    assert!(!target_path.exists());
    assert!(transaction_dir_path.join("commit").is_file());

    recover(file_system_io(), &storage_dir.path).unwrap();

    assert!(!target_path.exists());
    assert!(!transaction_dir_path.exists());
    assert_eq!(
        fs::read_to_string(storage_dir.path.join(".revision")).unwrap(),
        format!("{revision}\n")
    );
}
