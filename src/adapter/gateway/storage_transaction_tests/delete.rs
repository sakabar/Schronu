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
    let io = Arc::new(RecordingIo::new(vec![]));
    let prepared = prepare_delete_transaction(
        io.clone(),
        &storage_dir.path,
        "project/project.yaml",
        revision,
    );

    prepared
        .commit()
        .unwrap();

    let events = io.events();
    let marker_directory_path = storage_dir
        .path
        .join(TRANSACTION_DIRECTORY_NAME)
        .join(ACTIVE_TRANSACTION_DIRECTORY_NAME);
    let marker_sync = event_position(
        &events,
        RecordingOperation::SyncDirectory,
        &PathMatcher::Exact(marker_directory_path),
        1,
    );
    assert!(
        event_position(
            &events,
            RecordingOperation::SyncFile,
            &PathMatcher::FileName("commit.tmp"),
            1,
        ) < event_position(
            &events,
            RecordingOperation::Rename,
            &PathMatcher::FileName("commit"),
            1,
        )
    );
    assert!(
        event_position(
            &events,
            RecordingOperation::Rename,
            &PathMatcher::FileName("commit"),
            1,
        ) < marker_sync
    );
    assert!(
        marker_sync
            < event_position(
                &events,
                RecordingOperation::RemoveFile,
                &PathMatcher::Exact(target_path.clone()),
                1,
            )
    );
    assert!(!target_path.exists());
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
    let target_parent_path = target_path.parent().unwrap().to_path_buf();
    let io = Arc::new(RecordingIo::new(vec![]));

    recover(io.clone(), &storage_dir.path).unwrap();

    let events = io.events();
    assert!(
        event_position(
            &events,
            RecordingOperation::RemoveFile,
            &PathMatcher::Exact(target_path),
            1,
        ) < event_position(
            &events,
            RecordingOperation::SyncDirectory,
            &PathMatcher::Exact(target_parent_path),
            1,
        )
    );
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
    let io = Arc::new(RecordingIo::new(vec![FaultRule {
        operation: RecordingOperation::SyncDirectory,
        path_matcher: PathMatcher::Exact(target_path.parent().unwrap().to_path_buf()),
        occurrence: 1,
        error_kind: std::io::ErrorKind::Other,
        error_message: "injected delete directory sync failure",
    }]));

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
