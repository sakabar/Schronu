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
    assert_eq!(manifest["entries"][0]["content_length"], 12);
    assert_eq!(
        manifest["entries"][0]["content_checksum"],
        "fnv1a64:066228057cd3f0ee"
    );
    assert_eq!(
        fs::read(prepared.transaction_dir_path.join("files/0")).unwrap(),
        b"project: {}\n"
    );

    prepared.discard().unwrap();
    assert!(storage_dir.path.join(TRANSACTION_DIRECTORY_NAME).is_dir());
}

#[test]
fn test_prepare_staged_files_directory作成失敗時はuuid_directoryを残さない() {
    let storage_dir = TestStorageDir::new();
    let target_path = storage_dir.path.join("project.yaml");
    let io = Arc::new(FailSecondCreateDirectoryIo {
        create_calls: AtomicUsize::new(0),
    });

    let actual = prepare(
        io,
        &storage_dir.path,
        Uuid::from_u128(0x2205),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"project: {}\n",
        }],
    );

    assert!(actual.is_err());
    let transactions_dir_path = storage_dir.path.join(TRANSACTION_DIRECTORY_NAME);
    assert!(!transactions_dir_path
        .join(ACTIVE_TRANSACTION_DIRECTORY_NAME)
        .exists());
}

#[test]
fn test_prepare_cleanup_tombstoneはactive_transactionとして扱わない() {
    let storage_dir = TestStorageDir::new();
    let transactions_dir_path = storage_dir.path.join(TRANSACTION_DIRECTORY_NAME);
    fs::create_dir_all(transactions_dir_path.join(format!(".cleanup-{}", Uuid::from_u128(0x2211))))
        .unwrap();
    let target_path = storage_dir.path.join("project.yaml");

    let prepared = prepare(
        file_system_io(),
        &storage_dir.path,
        Uuid::from_u128(0x2212),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"new",
        }],
    )
    .unwrap();

    prepared.discard().unwrap();
}

#[test]
fn test_prepare_同時実行では一方だけがactive_transactionを取得する() {
    let storage_dir = TestStorageDir::new();
    let barrier = Arc::new(Barrier::new(3));
    let handles = (0..2)
        .map(|index| {
            let barrier = barrier.clone();
            let storage_dir_path = storage_dir.path.clone();
            std::thread::spawn(move || {
                let target_path = storage_dir_path.join(format!("project-{index}.yaml"));
                barrier.wait();
                prepare(
                    file_system_io(),
                    &storage_dir_path,
                    Uuid::from_u128(0x2213 + index),
                    &[WriteRequest {
                        target_path: &target_path,
                        bytes: b"new",
                    }],
                )
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();

    let mut prepared_transaction = None;
    let mut errors = Vec::new();
    for handle in handles {
        match handle.join().unwrap() {
            Ok(prepared) => {
                assert!(prepared_transaction.replace(prepared).is_none());
            }
            Err(error) => errors.push(error),
        }
    }

    assert_eq!(errors.len(), 1);
    assert!(errors[0].to_string().contains("ActiveTransaction"));
    assert!(storage_dir
        .path
        .join(TRANSACTION_DIRECTORY_NAME)
        .join(ACTIVE_TRANSACTION_DIRECTORY_NAME)
        .is_dir());
    prepared_transaction.unwrap().discard().unwrap();
}

#[test]
fn test_prepare_staged_file失敗はpathとphaseを保持する() {
    for (phase, expected_operation, reports_target_path) in [
        (
            FailingStagedFilePhase::ReadMetadata,
            StorageTransactionOperation::ReadTargetMetadata,
            true,
        ),
        (
            FailingStagedFilePhase::Create,
            StorageTransactionOperation::CreateStagedFile,
            false,
        ),
        (
            FailingStagedFilePhase::SetPermissions,
            StorageTransactionOperation::SetStagedPermissions,
            false,
        ),
        (
            FailingStagedFilePhase::Write,
            StorageTransactionOperation::WriteStagedFile,
            false,
        ),
        (
            FailingStagedFilePhase::Sync,
            StorageTransactionOperation::SyncStagedFile,
            false,
        ),
    ] {
        let storage_dir = TestStorageDir::new();
        let target_path = storage_dir.path.join("project.yaml");
        fs::write(&target_path, b"old").unwrap();
        let io = Arc::new(FailingStagedFileIo { phase });

        let actual = prepare(
            io,
            &storage_dir.path,
            Uuid::from_u128(0x2206),
            &[WriteRequest {
                target_path: &target_path,
                bytes: b"new",
            }],
        );

        let error = match actual {
            Err(error) => error,
            Ok(prepared) => {
                prepared.discard().unwrap();
                panic!("prepare must fail");
            }
        };
        assert_eq!(error.operation, expected_operation);
        if reports_target_path {
            assert_eq!(error.path, target_path);
        } else {
            assert_eq!(error.path.file_name().unwrap(), "0");
            assert_eq!(error.path.parent().unwrap().file_name().unwrap(), "files");
        }
        assert!(error.source().is_some());
    }
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
    for fail_sync_call in 1..=4 {
        let storage_dir = TestStorageDir::new();
        let target_path = storage_dir.path.join("project.yaml");
        fs::write(&target_path, b"old").unwrap();
        let io = Arc::new(FailingPrepareIo::new(None, None, Some(fail_sync_call)));

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
}
