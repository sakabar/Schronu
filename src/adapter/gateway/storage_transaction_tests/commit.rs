#[cfg(any(target_os = "macos", target_os = "linux"))]
struct SwapParentAfterAnchoredReadIo {
    original_parent: PathBuf,
    detached_parent: PathBuf,
    external_parent: PathBuf,
    swapped: AtomicBool,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl StorageTransactionIo for SwapParentAfterAnchoredReadIo {
    fn read_storage_file_anchored(
        &self,
        storage_dir_path: &Path,
        relative_path: &Path,
    ) -> Result<super::super::io::AnchoredReadOutcome, StorageTransactionError> {
        read_storage_file_anchored_after_read(storage_dir_path, relative_path, || {
            if !self.swapped.swap(true, Ordering::SeqCst) {
                fs::rename(&self.original_parent, &self.detached_parent).unwrap();
                std::os::unix::fs::symlink(&self.external_parent, &self.original_parent).unwrap();
            }
        })
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn test_適用済み判定後に親directoryが差し替わった場合revisionを更新しない() {
    let storage_dir = TestStorageDir::new();
    let external_dir = TestStorageDir::new();
    let original_parent = storage_dir.path.join("discarded_sessions");
    let detached_parent = storage_dir.path.join("discarded_sessions-original");
    fs::create_dir_all(&original_parent).unwrap();
    fs::write(original_parent.join("2026-09.yaml"), b"same-content").unwrap();
    fs::write(storage_dir.path.join(".revision"), b"old-revision\n").unwrap();
    fs::write(external_dir.path.join("2026-09.yaml"), b"same-content").unwrap();
    let io = Arc::new(SwapParentAfterAnchoredReadIo {
        original_parent,
        detached_parent,
        external_parent: external_dir.path.clone(),
        swapped: AtomicBool::new(false),
    });
    let target_path = storage_dir.path.join("discarded_sessions/2026-09.yaml");
    let prepared = prepare(
        io,
        &storage_dir.path,
        Uuid::from_u128(0x2260),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"same-content",
        }],
    )
    .unwrap();

    let error = prepared.commit().unwrap_err();

    assert_eq!(
        error.operation,
        StorageTransactionOperation::ValidateTargetPath
    );
    assert_eq!(
        fs::read(storage_dir.path.join(".revision")).unwrap(),
        b"old-revision\n"
    );
    assert_eq!(
        fs::read(external_dir.path.join("2026-09.yaml")).unwrap(),
        b"same-content"
    );
}

#[test]
fn test_anchored_io非対応実装は従来pathで適用済みを判定する() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
    fs::write(&target_path, b"already-applied").unwrap();
    let revision = Uuid::from_u128(0x2261);
    let prepared = prepare(
        Arc::new(RecordingIo::new(vec![])),
        &storage_dir.path,
        revision,
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"already-applied",
        }],
    )
    .unwrap();
    let manifest: Value = serde_json::from_slice(
        &fs::read(prepared.transaction_dir_path().join("manifest.json")).unwrap(),
    )
    .unwrap();
    let staged_file = manifest["entries"][0]["staged_file"].as_str().unwrap();
    fs::remove_file(prepared.transaction_dir_path().join(staged_file)).unwrap();

    prepared.commit().unwrap();

    assert_eq!(fs::read(&target_path).unwrap(), b"already-applied");
    assert_eq!(
        fs::read_to_string(storage_dir.path.join(".revision")).unwrap(),
        format!("{revision}\n")
    );
}

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
    assert!(
        marker_directory_sync
            < event_position(
                &events,
                RecordingOperation::CreateFile,
                &PathMatcher::FileNameContains("revision"),
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
        last_event_position(
            &events,
            RecordingOperation::Rename,
            &PathMatcher::Exact(first_target_path.clone()),
        ) < revision_write
    );
    assert!(
        last_event_position(
            &events,
            RecordingOperation::Rename,
            &PathMatcher::Exact(second_target_path.clone()),
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

#[cfg(unix)]
#[test]
fn test_commit_directory_permissionのsetとsync失敗を区別する() {
    use std::os::unix::fs::PermissionsExt;

    for (operation, message, expected_operation) in [
        (
            RecordingOperation::SetPermissions,
            "injected directory permission failure",
            StorageTransactionOperation::SetLivePermissions,
        ),
        (
            RecordingOperation::SyncDirectory,
            "injected directory sync failure",
            StorageTransactionOperation::SyncDirectory,
        ),
    ] {
        let storage_dir = TestStorageDir::new();
        let directory_path = storage_dir.path.join("markdown");
        let permissions = [fs::Permissions::from_mode(0o750)];
        let prepared = prepare_replacing_with_directories_and_deletes(
            Arc::new(RecordingIo::new(vec![FaultRule {
                operation,
                path_matcher: PathMatcher::Exact(directory_path.clone()),
                occurrence: 1,
                error_kind: std::io::ErrorKind::Other,
                error_message: message,
            }])),
            &storage_dir.path,
            Uuid::from_u128(0x2242),
            ReplacementRequest {
                writes: &[],
                file_permissions: &[],
                directories: &[&directory_path],
                directory_permissions: &permissions,
                deletes: &[],
            },
        )
        .unwrap();

        let error = prepared.commit().unwrap_err();

        assert_eq!(error.operation, expected_operation);
        assert_eq!(error.path, directory_path);
        assert_eq!(error.source.to_string(), message);
        assert_eq!(
            error.commit_state(),
            StorageTransactionCommitState::CommitMarkerEstablished
        );
    }
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

        let error = prepared.commit().unwrap_err();

        assert_eq!(
            error.commit_state(),
            if marker_exists {
                StorageTransactionCommitState::CommitMarkerEstablished
            } else {
                StorageTransactionCommitState::NotCommitted
            },
            "unexpected commit state for {name}"
        );
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
