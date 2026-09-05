#[test]
fn test_transaction_root_symlinkはprepareとrecoveryで拒否して外部を変更しない() {
    use std::os::unix::fs::symlink;

    let storage_dir = TestStorageDir::new();
    let external_dir = TestStorageDir::new();
    let external_active_path = external_dir.path.join(ACTIVE_TRANSACTION_DIRECTORY_NAME);
    fs::create_dir(&external_active_path).unwrap();
    let external_manifest_path = external_active_path.join("manifest.json");
    fs::write(&external_manifest_path, b"external").unwrap();
    let transactions_dir_path = storage_dir.path.join(TRANSACTION_DIRECTORY_NAME);
    symlink(&external_dir.path, &transactions_dir_path).unwrap();
    let target_path = storage_dir.path.join("project.yaml");

    let recover_error = recover(file_system_io(), &storage_dir.path).unwrap_err();
    let prepare_error = match prepare(
        file_system_io(),
        &storage_dir.path,
        Uuid::from_u128(0x2254),
        &[WriteRequest {
            target_path: &target_path,
            bytes: b"new",
        }],
    ) {
        Err(error) => error,
        Ok(_) => panic!("transaction root symlink must be rejected"),
    };

    for error in [recover_error, prepare_error] {
        assert_eq!(
            error.operation,
            StorageTransactionOperation::ValidateTransactionDirectory
        );
        assert_eq!(error.path, transactions_dir_path);
    }
    assert_eq!(fs::read(external_manifest_path).unwrap(), b"external");
    assert!(!target_path.exists());
}

#[test]
fn test_prepare_file_targetのpath_escapeと予約namespaceを拒否する() {
    let storage_dir = TestStorageDir::new();
    let target_paths = [
        storage_dir.path.clone(),
        storage_dir.path.join("../escaped.yaml"),
        storage_dir.path.join(".schronu-transactions/live.yaml"),
    ];
    for target_path in target_paths {
        let actual = prepare(
            file_system_io(),
            &storage_dir.path,
            Uuid::from_u128(0x2214),
            &[WriteRequest {
                target_path: &target_path,
                bytes: b"new",
            }],
        );

        let error = match actual {
            Err(error) => error,
            Ok(prepared) => {
                prepared.discard().unwrap();
                panic!("invalid file target must fail: {}", target_path.display());
            }
        };
        assert_eq!(
            error.operation,
            StorageTransactionOperation::ValidateTargetPath
        );
        assert_eq!(error.path, target_path);
        assert!(!storage_dir
            .path
            .join(TRANSACTION_DIRECTORY_NAME)
            .join(ACTIVE_TRANSACTION_DIRECTORY_NAME)
            .exists());
    }
}

#[test]
fn test_prepare_directory_targetの空path_escapeと予約namespaceを拒否する() {
    let storage_dir = TestStorageDir::new();
    let directory_paths = [
        storage_dir.path.clone(),
        storage_dir.path.join("../escaped"),
        storage_dir.path.join(".schronu-transactions/live"),
    ];

    for directory_path in &directory_paths {
        let actual = prepare_with_directories(
            file_system_io(),
            &storage_dir.path,
            Uuid::from_u128(0x2215),
            &[],
            &[directory_path],
        );

        let error = match actual {
            Err(error) => error,
            Ok(prepared) => {
                prepared.discard().unwrap();
                panic!(
                    "invalid directory target must fail: {}",
                    directory_path.display()
                );
            }
        };
        assert_eq!(
            error.operation,
            StorageTransactionOperation::ValidateTargetPath
        );
        assert_eq!(error.path, *directory_path);
        assert!(!storage_dir
            .path
            .join(TRANSACTION_DIRECTORY_NAME)
            .join(ACTIVE_TRANSACTION_DIRECTORY_NAME)
            .exists());
    }
}

#[cfg(unix)]
#[test]
fn test_delete_entry_symlinkは参照先を変更せずlinkだけを削除する() {
    use std::os::unix::fs::symlink;

    let storage_dir = TestStorageDir::new();
    let external_dir = TestStorageDir::new();
    let external_path = external_dir.path.join("external.yaml");
    fs::write(&external_path, b"external").unwrap();
    let target_path = storage_dir.path.join("project.yaml");
    symlink(&external_path, &target_path).unwrap();
    create_delete_transaction(
        &storage_dir.path,
        "project.yaml",
        Uuid::from_u128(0x2236),
        true,
    );

    recover(file_system_io(), &storage_dir.path).unwrap();

    assert!(!target_path.exists());
    assert_eq!(fs::read(external_path).unwrap(), b"external");
}

#[cfg(unix)]
#[test]
fn test_delete_entry_symlink_parentを拒否し参照先を変更しない() {
    use std::os::unix::fs::symlink;

    let storage_dir = TestStorageDir::new();
    let external_dir = TestStorageDir::new();
    let external_path = external_dir.path.join("project.yaml");
    fs::write(&external_path, b"external").unwrap();
    symlink(&external_dir.path, storage_dir.path.join("linked-project")).unwrap();
    create_delete_transaction(
        &storage_dir.path,
        "linked-project/project.yaml",
        Uuid::from_u128(0x2239),
        true,
    );

    let actual = recover(file_system_io(), &storage_dir.path).unwrap_err();

    assert_eq!(
        actual.operation,
        StorageTransactionOperation::ValidateTargetPath
    );
    assert_eq!(fs::read(external_path).unwrap(), b"external");
}

#[test]
fn test_delete_entryはstorage外へのpath_escapeを拒否する() {
    let storage_dir = TestStorageDir::new();
    let external_file_name = format!("schronu-delete-external-{}.yaml", Uuid::new_v4());
    let external_path = storage_dir.path.parent().unwrap().join(&external_file_name);
    let manifest_target = format!("../{external_file_name}");
    fs::write(&external_path, b"external").unwrap();
    create_delete_transaction(
        &storage_dir.path,
        &manifest_target,
        Uuid::from_u128(0x2237),
        true,
    );

    let actual = recover(file_system_io(), &storage_dir.path).unwrap_err();

    assert_eq!(
        actual.operation,
        StorageTransactionOperation::ValidateTargetPath
    );
    assert_eq!(fs::read(&external_path).unwrap(), b"external");
    fs::remove_file(external_path).unwrap();
}
