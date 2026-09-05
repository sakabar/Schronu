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
    let io = Arc::new(RecordingIo::new(vec![]));
    let markdown_dir_path = storage_dir.path.join("third/markdown");
    let prepared = prepare_with_directories(
        io.clone(),
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
        &fs::read(prepared.transaction_dir_path().join("manifest.json")).unwrap(),
    )
    .unwrap();
    let transaction_id = Uuid::parse_str(manifest["transaction_id"].as_str().unwrap()).unwrap();
    assert_eq!(manifest["revision"], revision.to_string());
    assert_eq!(manifest["directories"][0], "third/markdown");

    prepared
        .commit()
        .unwrap();

    let events = io.events();
    let marker_directory_path = storage_dir
        .path
        .join(TRANSACTION_DIRECTORY_NAME)
        .join(ACTIVE_TRANSACTION_DIRECTORY_NAME);
    let marker_directory_sync = event_position(
        &events,
        RecordingOperation::SyncDirectory,
        &PathMatcher::Exact(marker_directory_path),
        2,
    );
    assert!(
        event_position(
            &events,
            RecordingOperation::SyncFile,
            &PathMatcher::FileName("manifest.json"),
            1,
        ) < event_position(
            &events,
            RecordingOperation::CreateFile,
            &PathMatcher::FileName("commit.tmp"),
            1,
        )
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
        ) < marker_directory_sync
    );
    assert!(
        marker_directory_sync
            < event_position(
                &events,
                RecordingOperation::CreateDirectory,
                &PathMatcher::Exact(markdown_dir_path.clone()),
                1,
            )
    );
    assert!(
        marker_directory_sync
            < event_position(
                &events,
                RecordingOperation::CreateFile,
                &PathMatcher::FileNamePrefix(".project.yaml."),
                1,
            )
    );
    let revision_write = event_position(
        &events,
        RecordingOperation::WriteFile,
        &PathMatcher::FileNameContains("revision"),
        1,
    );
    assert!(
        event_position(
            &events,
            RecordingOperation::Rename,
            &PathMatcher::Exact(first_target_path.clone()),
            1,
        ) < revision_write
    );
    assert!(
        event_position(
            &events,
            RecordingOperation::Rename,
            &PathMatcher::Exact(second_target_path.clone()),
            1,
        ) < revision_write
    );
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
        .commit()
        .unwrap();

    assert_eq!(fs::metadata(target_path).unwrap().mode() & 0o777, 0o600);
}

#[test]
fn test_commit_failure時は回復用manifestとstaged_fileを維持する() {
    for (name, operation, path_matcher, occurrence, error_message, marker_exists) in [
        (
            "marker create",
            RecordingOperation::CreateFile,
            PathMatcher::FileName("commit.tmp"),
            1,
            "injected marker create failure",
            false,
        ),
        (
            "marker sync",
            RecordingOperation::SyncFile,
            PathMatcher::FileName("commit.tmp"),
            1,
            "injected commit sync failure",
            false,
        ),
        (
            "marker rename",
            RecordingOperation::Rename,
            PathMatcher::FileName("commit"),
            1,
            "injected marker rename failure",
            false,
        ),
        (
            "marker directory sync",
            RecordingOperation::SyncDirectory,
            PathMatcher::FileName(ACTIVE_TRANSACTION_DIRECTORY_NAME),
            2,
            "injected marker directory sync failure",
            true,
        ),
        (
            "live write",
            RecordingOperation::WriteFile,
            PathMatcher::FileNamePrefix(".project.yaml."),
            1,
            "injected live write failure",
            true,
        ),
        (
            "live sync",
            RecordingOperation::SyncFile,
            PathMatcher::FileNamePrefix(".project.yaml."),
            1,
            "injected commit sync failure",
            true,
        ),
        (
            "live rename",
            RecordingOperation::Rename,
            PathMatcher::FileName("project.yaml"),
            1,
            "injected live rename failure",
            true,
        ),
        (
            "target directory",
            RecordingOperation::CreateDirectory,
            PathMatcher::FileName("markdown"),
            1,
            "injected target directory failure",
            true,
        ),
        (
            "live directory sync",
            RecordingOperation::SyncDirectory,
            PathMatcher::Any,
            6,
            "injected live directory sync failure",
            true,
        ),
        (
            "revision write",
            RecordingOperation::WriteFile,
            PathMatcher::FileNameContains("revision"),
            1,
            "injected live write failure",
            true,
        ),
        (
            "revision sync",
            RecordingOperation::SyncFile,
            PathMatcher::FileNameContains("revision"),
            1,
            "injected commit sync failure",
            true,
        ),
        (
            "revision rename",
            RecordingOperation::Rename,
            PathMatcher::FileName(".revision"),
            1,
            "injected revision rename failure",
            true,
        ),
        (
            "cleanup rename",
            RecordingOperation::Rename,
            PathMatcher::FileNamePrefix(".cleanup-"),
            1,
            "injected cleanup rename failure",
            true,
        ),
    ] {
        let storage_dir = TestStorageDir::new();
        let target_path = storage_dir.path.join("project.yaml");
        fs::write(&target_path, b"old").unwrap();
        let markdown_dir_path = storage_dir.path.join("markdown");
        let prepared = prepare_with_directories(
            Arc::new(RecordingIo::new(vec![FaultRule {
                operation,
                path_matcher,
                occurrence,
                error_kind: std::io::ErrorKind::Other,
                error_message,
            }])),
            &storage_dir.path,
            Uuid::from_u128(0x2209),
            &[WriteRequest {
                target_path: &target_path,
                bytes: b"new",
            }],
            &[&markdown_dir_path],
        )
        .unwrap();
        let transaction_dir_path = prepared.transaction_dir_path().to_path_buf();

        let actual = prepared.commit();

        assert!(actual.is_err(), "{name} must fail");
        assert!(transaction_dir_path.join("manifest.json").is_file());
        assert!(transaction_dir_path.join("files/0").is_file());
        assert_eq!(
            transaction_dir_path.join("commit").is_file(),
            marker_exists
        );
    }
}
#[test]
fn test_commit_target内容読込失敗はpathと専用phaseを保持する() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
    fs::write(&target_path, b"old").unwrap();
    let prepared = prepare(
        Arc::new(RecordingIo::new(vec![FaultRule {
            operation: RecordingOperation::ReadFile,
            path_matcher: PathMatcher::Exact(target_path.clone()),
            occurrence: 1,
            error_kind: std::io::ErrorKind::PermissionDenied,
            error_message: "injected target content read failure",
        }])),
        &storage_dir.path,
        Uuid::from_u128(0x2241),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"new",
        }],
    )
    .unwrap();
    let transaction_dir_path = prepared.transaction_dir_path().to_path_buf();

    let actual = prepared
        .commit()
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
    for (name, operation, path_matcher, occurrence, error_message) in [
        (
            "cleanup handoff sync",
            RecordingOperation::SyncDirectory,
            PathMatcher::FileName(TRANSACTION_DIRECTORY_NAME),
            2,
            "injected cleanup handoff sync failure",
        ),
        (
            "cleanup delete",
            RecordingOperation::RemoveDirectory,
            PathMatcher::FileNamePrefix(".cleanup-"),
            1,
            "injected cleanup failure",
        ),
    ] {
        let storage_dir = TestStorageDir::new();
        let target_path = storage_dir.path.join("project.yaml");
        fs::write(&target_path, b"old").unwrap();
        let prepared = prepare(
            Arc::new(RecordingIo::new(vec![FaultRule {
                operation,
                path_matcher,
                occurrence,
                error_kind: std::io::ErrorKind::Other,
                error_message,
            }])),
            &storage_dir.path,
            Uuid::from_u128(0x2210),
            &[WriteRequest {
                target_path: &target_path,
                bytes: b"new",
            }],
        )
        .unwrap();
        let transaction_id = prepared.transaction_id();

        prepared
            .commit()
            .unwrap();

        let cleanup_dir_path = storage_dir
            .path
            .join(TRANSACTION_DIRECTORY_NAME)
            .join(format!(".cleanup-{}", transaction_id.hyphenated()));
        assert!(cleanup_dir_path.join("commit").is_file(), "{name}");
        assert!(
            cleanup_dir_path.join("manifest.json").is_file(),
            "{name}"
        );
        assert!(cleanup_dir_path.join("files/0").is_file(), "{name}");
        assert_eq!(fs::read(target_path).unwrap(), b"new");
    }
}
