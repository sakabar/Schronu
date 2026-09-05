#[test]
fn test_load_markerなしtransactionをrevisionとproject読込前に破棄する() {
    for phase in [
        "staged-write",
        "staged-sync",
        "manifest-sync",
        "before-marker",
    ] {
        let storage_dir = TestStorageDir::new();
        let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
        let mut source_repository = TaskRepository::new(storage_dir.path_str());
        source_repository.sync_clock(now).unwrap();
        let task = crate::test_support::new_task_handle("old").unwrap();
        let task_id = task.get_id().unwrap();
        source_repository.start_new_project(task).unwrap();
        source_repository.save().unwrap();
        let project_yaml_path = storage_dir
            .project_dir_path("20260905", "old", task_id)
            .join("project.yaml");
        let old_project = fs::read(&project_yaml_path).unwrap();
        let revision = Uuid::parse_str(
            fs::read_to_string(storage_dir.path.join(".revision"))
                .unwrap()
                .trim(),
        )
        .unwrap();
        let old_revision = fs::read(storage_dir.path.join(".revision")).unwrap();
        let active_transaction_path = create_markerless_active_transaction(&storage_dir, phase);
        fs::write(
            active_transaction_path.join("files/project.yaml"),
            b"project: [",
        )
        .unwrap();
        let mut repository = TaskRepository::new(storage_dir.path_str());
        repository.sync_clock(now).unwrap();

        repository.load().unwrap();

        assert!(!active_transaction_path.exists(), "phase: {phase}");
        assert_eq!(fs::read(&project_yaml_path).unwrap(), old_project);
        assert_eq!(
            fs::read(storage_dir.path.join(".revision")).unwrap(),
            old_revision
        );
        assert_eq!(repository.storage_revision.get(), Some(revision));
    }
}

#[test]
fn test_reload_if_changed_markerなしtransactionをcache判定前に破棄する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    let task = crate::test_support::new_task_handle("old").unwrap();
    repository.start_new_project(task).unwrap();
    repository.save().unwrap();
    repository.load().unwrap();
    let revision = repository.storage_revision.get();
    let active_transaction_path =
        create_markerless_active_transaction(&storage_dir, "before-marker");

    let actual = repository.reload_if_changed(now).unwrap();

    assert_eq!(actual, RepositoryReloadOutcome::Cached);
    assert!(!active_transaction_path.exists());
    assert_eq!(repository.storage_revision.get(), revision);
}

#[test]
fn test_load_marker済みtransactionを各中断点からnew_snapshotへroll_forwardする() {
    for phase in [
        CommittedInterruption::BeforeFirstProjectRename,
        CommittedInterruption::AfterFirstProjectRename,
        CommittedInterruption::BeforeFinalProjectRename,
        CommittedInterruption::BeforeRevision,
        CommittedInterruption::AfterRevisionBeforeCleanup,
    ] {
        let storage_dir = TestStorageDir::new();
        let now = Local.with_ymd_and_hms(2026, 9, 5, 13, 0, 0).unwrap();
        let (first_id, second_id, committed_revision, active_transaction_path) =
            create_committed_transaction_interruption(&storage_dir, now, phase);
        if matches!(phase, CommittedInterruption::BeforeFirstProjectRename) {
            fs::write(
                storage_dir.path.join(".revision"),
                format!("{}\n", Uuid::from_u128(0x22ff)),
            )
            .unwrap();
        }
        let mut repository = TaskRepository::new(storage_dir.path_str());
        repository.sync_clock(now).unwrap();

        repository.load().unwrap();

        assert_recovered_projects(&repository, first_id, second_id);
        assert_eq!(repository.storage_revision.get(), Some(committed_revision));
        assert_eq!(
            fs::read_to_string(storage_dir.path.join(".revision")).unwrap(),
            format!("{committed_revision}\n")
        );
        assert!(!active_transaction_path.exists(), "phase: {phase:?}");
    }
}

#[test]
fn test_reload_if_changed_marker済みtransactionをcache判定前にroll_forwardする() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 13, 0, 0).unwrap();
    let mut cached = TaskRepository::new(storage_dir.path_str());
    cached.sync_clock(now).unwrap();
    let first = crate::test_support::new_task_handle("roll-forward-first").unwrap();
    let second = crate::test_support::new_task_handle("roll-forward-second").unwrap();
    let first_id = first.get_id().unwrap();
    let second_id = second.get_id().unwrap();
    cached.start_new_project(first.clone()).unwrap();
    cached.start_new_project(second.clone()).unwrap();
    cached.save().unwrap();
    cached.load().unwrap();
    cached
        .get_by_id(first_id)
        .unwrap()
        .unwrap()
        .set_estimated_work_seconds(30 * 60)
        .unwrap();
    cached
        .get_by_id(second_id)
        .unwrap()
        .unwrap()
        .set_estimated_work_seconds(45 * 60)
        .unwrap();
    cached.storage_transaction_io =
        committed_interruption_io(CommittedInterruption::AfterFirstProjectRename);
    cached.save().unwrap_err();
    cached.storage_transaction_io = Arc::new(FileSystemStorageTransactionIo);

    let outcome = cached.reload_if_changed(now).unwrap();

    assert_eq!(outcome, RepositoryReloadOutcome::Reloaded);
    assert_recovered_projects(&cached, first_id, second_id);
}

#[test]
fn test_load_roll_forward中断後も再実行して同じnew_snapshotへ到達する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 13, 0, 0).unwrap();
    let (first_id, second_id, committed_revision, active_transaction_path) =
        create_committed_transaction_interruption(
            &storage_dir,
            now,
            CommittedInterruption::BeforeFirstProjectRename,
        );
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository.storage_transaction_io =
        committed_interruption_io(CommittedInterruption::AfterFirstProjectRename);

    let interrupted = repository.load().unwrap_err();

    assert_eq!(
        interrupted.operation(),
        ApplicationRepositoryOperation::Load
    );
    assert!(active_transaction_path.join("commit").is_file());
    repository.storage_transaction_io = Arc::new(FileSystemStorageTransactionIo);
    repository.load().unwrap();
    repository.load().unwrap();
    assert_recovered_projects(&repository, first_id, second_id);
    assert_eq!(repository.storage_revision.get(), Some(committed_revision));
    assert!(!active_transaction_path.exists());
}

#[test]
fn test_load_marker済みtransactionの不正manifestはpathとphaseを保持する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 13, 0, 0).unwrap();
    let (_, _, _, active_transaction_path) = create_committed_transaction_interruption(
        &storage_dir,
        now,
        CommittedInterruption::BeforeRevision,
    );
    let manifest_path = active_transaction_path.join("manifest.json");
    fs::write(&manifest_path, b"not-json").unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let actual = repository.load().unwrap_err();

    let source = storage_transaction_error(&actual);
    assert!(source.to_string().contains("ParseManifest"));
    assert!(source
        .to_string()
        .contains(&manifest_path.display().to_string()));
    assert!(active_transaction_path.join("commit").is_file());
}

#[test]
fn test_load_marker済みtransactionのmanifest意味違反はlive_snapshotを変更しない() {
    for case in [
        "unsupported-version",
        "nil-transaction-id",
        "escaping-target",
        "reserved-target",
        "escaping-directory",
        "invalid-staged-file",
    ] {
        let storage_dir = TestStorageDir::new();
        let now = Local.with_ymd_and_hms(2026, 9, 5, 13, 0, 0).unwrap();
        let (first_id, second_id, _, active_transaction_path) =
            create_committed_transaction_interruption(
                &storage_dir,
                now,
                CommittedInterruption::BeforeFirstProjectRename,
            );
        let first_project_path = storage_dir
            .project_dir_path("20260905", "roll-forward-first", first_id)
            .join("project.yaml");
        let second_project_path = storage_dir
            .project_dir_path("20260905", "roll-forward-second", second_id)
            .join("project.yaml");
        let old_first = fs::read(&first_project_path).unwrap();
        let old_second = fs::read(&second_project_path).unwrap();
        let old_revision = fs::read(storage_dir.path.join(".revision")).unwrap();
        let manifest_path = active_transaction_path.join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        let (expected_phase, expected_path) = match case {
            "unsupported-version" => {
                manifest["version"] = serde_json::json!(2);
                ("ValidateManifest", manifest_path.clone())
            }
            "nil-transaction-id" => {
                manifest["transaction_id"] = serde_json::json!(Uuid::nil().to_string());
                ("ValidateManifest", manifest_path.clone())
            }
            "escaping-target" => {
                manifest["entries"][0]["target"] = serde_json::json!("../outside.yaml");
                (
                    "ValidateTargetPath",
                    storage_dir.path.join("../outside.yaml"),
                )
            }
            "reserved-target" => {
                manifest["entries"][0]["target"] =
                    serde_json::json!(".schronu-transactions/outside.yaml");
                (
                    "ValidateTargetPath",
                    storage_dir.path.join(".schronu-transactions/outside.yaml"),
                )
            }
            "escaping-directory" => {
                manifest["directories"] = serde_json::json!(["../outside"]);
                ("ValidateTargetPath", storage_dir.path.join("../outside"))
            }
            "invalid-staged-file" => {
                manifest["entries"][0]["staged_file"] = serde_json::json!("../material");
                (
                    "ValidateManifest",
                    active_transaction_path.join("../material"),
                )
            }
            _ => unreachable!(),
        };
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let mut repository = TaskRepository::new(storage_dir.path_str());

        let actual = repository.load().unwrap_err();

        let source = storage_transaction_error(&actual);
        assert!(source.to_string().contains(expected_phase), "case: {case}");
        assert!(
            source
                .to_string()
                .contains(&expected_path.display().to_string()),
            "case: {case}"
        );
        assert!(active_transaction_path.join("commit").is_file());
        assert_eq!(fs::read(&first_project_path).unwrap(), old_first);
        assert_eq!(fs::read(&second_project_path).unwrap(), old_second);
        assert_eq!(
            fs::read(storage_dir.path.join(".revision")).unwrap(),
            old_revision
        );
    }
}

#[test]
fn test_load_marker済みtransactionの未適用staged_file欠落はpathとphaseを保持する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 13, 0, 0).unwrap();
    let (first_id, second_id, _, active_transaction_path) =
        create_committed_transaction_interruption(
            &storage_dir,
            now,
            CommittedInterruption::BeforeFirstProjectRename,
        );
    let first_project_path = storage_dir
        .project_dir_path("20260905", "roll-forward-first", first_id)
        .join("project.yaml");
    let second_project_path = storage_dir
        .project_dir_path("20260905", "roll-forward-second", second_id)
        .join("project.yaml");
    let old_first = fs::read(&first_project_path).unwrap();
    let old_second = fs::read(&second_project_path).unwrap();
    let old_revision = fs::read(storage_dir.path.join(".revision")).unwrap();
    let staged_file_path = active_transaction_path.join("files/1");
    fs::remove_file(&staged_file_path).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let actual = repository.load().unwrap_err();

    let source = storage_transaction_error(&actual);
    assert!(source.to_string().contains("ReadStagedFile"));
    assert!(source
        .to_string()
        .contains(&staged_file_path.display().to_string()));
    assert!(active_transaction_path.join("commit").is_file());
    assert_eq!(fs::read(first_project_path).unwrap(), old_first);
    assert_eq!(fs::read(second_project_path).unwrap(), old_second);
    assert_eq!(
        fs::read(storage_dir.path.join(".revision")).unwrap(),
        old_revision
    );
}

#[test]
fn test_load_marker済みtransactionの同一長staged破損を適用状態に関わらず拒否する() {
    for target_already_applied in [false, true] {
        let storage_dir = TestStorageDir::new();
        let now = Local.with_ymd_and_hms(2026, 9, 5, 13, 0, 0).unwrap();
        let (_, _, _, active_transaction_path) = create_committed_transaction_interruption(
            &storage_dir,
            now,
            CommittedInterruption::BeforeFirstProjectRename,
        );
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(active_transaction_path.join("manifest.json")).unwrap(),
        )
        .unwrap();
        let entries = manifest["entries"].as_array().unwrap();
        let corrupt_index = if target_already_applied { 0 } else { 1 };
        let mut expected_live = entries
            .iter()
            .map(|entry| {
                let target_path = storage_dir.path.join(entry["target"].as_str().unwrap());
                let bytes = fs::read(&target_path).unwrap();
                (target_path, bytes)
            })
            .collect::<Vec<_>>();
        let staged_file_path =
            active_transaction_path.join(entries[corrupt_index]["staged_file"].as_str().unwrap());
        let expected_bytes = fs::read(&staged_file_path).unwrap();
        if target_already_applied {
            fs::write(&expected_live[corrupt_index].0, &expected_bytes).unwrap();
            expected_live[corrupt_index].1 = expected_bytes.clone();
        }
        let mut corrupted = expected_bytes;
        corrupted[0] ^= 1;
        fs::write(&staged_file_path, &corrupted).unwrap();
        assert_eq!(
            fs::metadata(&staged_file_path).unwrap().len(),
            corrupted.len() as u64
        );
        let revision_path = storage_dir.path.join(".revision");
        let old_revision = fs::read(&revision_path).unwrap();
        let mut repository = TaskRepository::new(storage_dir.path_str());

        let actual = repository.load().unwrap_err();

        assert!(
            active_transaction_path.join("commit").is_file(),
            "target_already_applied: {target_already_applied}"
        );
        for (target_path, expected_bytes) in expected_live {
            assert_eq!(
                fs::read(target_path).unwrap(),
                expected_bytes,
                "target_already_applied: {target_already_applied}"
            );
        }
        assert_eq!(fs::read(revision_path).unwrap(), old_revision);
        let source = storage_transaction_error(&actual);
        assert!(source.to_string().contains("ValidateStagedContent"));
        assert!(source
            .to_string()
            .contains(&staged_file_path.display().to_string()));
    }
}

#[test]
fn test_reload_if_changed_marker済みtransactionの適用済みtargetを検証してstaged欠落を許容する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 13, 0, 0).unwrap();
    let (first_id, second_id, committed_revision, active_transaction_path) =
        create_committed_transaction_interruption(
            &storage_dir,
            now,
            CommittedInterruption::BeforeFirstProjectRename,
        );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(active_transaction_path.join("manifest.json")).unwrap())
            .unwrap();
    let applied_entry = &manifest["entries"][0];
    let applied_target_path = storage_dir
        .path
        .join(applied_entry["target"].as_str().unwrap());
    let applied_staged_path =
        active_transaction_path.join(applied_entry["staged_file"].as_str().unwrap());
    fs::write(
        &applied_target_path,
        fs::read(&applied_staged_path).unwrap(),
    )
    .unwrap();
    fs::remove_file(&applied_staged_path).unwrap();
    assert!(active_transaction_path.join("files/1").is_file());
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let outcome = repository.reload_if_changed(now).unwrap();

    assert_eq!(outcome, RepositoryReloadOutcome::Reloaded);
    assert_recovered_projects(&repository, first_id, second_id);
    assert_eq!(repository.storage_revision.get(), Some(committed_revision));
    assert_eq!(
        fs::read_to_string(storage_dir.path.join(".revision")).unwrap(),
        format!("{committed_revision}\n")
    );
    assert!(!active_transaction_path.exists());
}

#[cfg(unix)]
#[test]
fn test_load_marker済みtransactionの適用済みtargetでもstaged_symlinkを拒否する() {
    use std::os::unix::fs::symlink;

    let storage_dir = TestStorageDir::new();
    let external_dir = TestStorageDir::new();
    fs::create_dir_all(&external_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 13, 0, 0).unwrap();
    let (first_id, second_id, _, active_transaction_path) =
        create_committed_transaction_interruption(
            &storage_dir,
            now,
            CommittedInterruption::BeforeFirstProjectRename,
        );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(active_transaction_path.join("manifest.json")).unwrap())
            .unwrap();
    let applied_entry = &manifest["entries"][0];
    let applied_target_path = storage_dir
        .path
        .join(applied_entry["target"].as_str().unwrap());
    let applied_staged_path =
        active_transaction_path.join(applied_entry["staged_file"].as_str().unwrap());
    let expected_bytes = fs::read(&applied_staged_path).unwrap();
    fs::write(&applied_target_path, &expected_bytes).unwrap();
    fs::remove_file(&applied_staged_path).unwrap();
    let external_path = external_dir.path.join("staged.external");
    fs::write(&external_path, &expected_bytes).unwrap();
    symlink(&external_path, &applied_staged_path).unwrap();
    let external_bytes = fs::read(&external_path).unwrap();
    let second_project_path = storage_dir
        .project_dir_path("20260905", "roll-forward-second", second_id)
        .join("project.yaml");
    let old_second = fs::read(&second_project_path).unwrap();
    let revision_path = storage_dir.path.join(".revision");
    let old_revision = fs::read(&revision_path).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let actual = repository.load().unwrap_err();

    let source = storage_transaction_error(&actual);
    assert!(source.to_string().contains("ValidateStagedFile"));
    assert!(source
        .to_string()
        .contains(&applied_staged_path.display().to_string()));
    assert!(active_transaction_path.join("commit").is_file());
    assert_eq!(fs::read(applied_target_path).unwrap(), expected_bytes);
    assert_eq!(fs::read(second_project_path).unwrap(), old_second);
    assert_eq!(fs::read(revision_path).unwrap(), old_revision);
    assert_eq!(fs::read(external_path).unwrap(), external_bytes);
    assert!(repository.get_by_id(first_id).unwrap().is_none());
}

#[cfg(unix)]
#[test]
fn test_load_marker済みtransactionのcontrol_file_symlinkを拒否して外部とliveを変更しない() {
    use std::os::unix::fs::symlink;

    for (case, expected_phase) in [
        ("marker", "ValidateCommitMarker"),
        ("manifest", "ValidateManifest"),
        ("later-staged", "ValidateStagedFile"),
    ] {
        let storage_dir = TestStorageDir::new();
        let external_dir = TestStorageDir::new();
        fs::create_dir_all(&external_dir.path).unwrap();
        let external_path = external_dir.path.join(format!("{case}.external"));
        let now = Local.with_ymd_and_hms(2026, 9, 5, 13, 0, 0).unwrap();
        let (first_id, second_id, _, active_transaction_path) =
            create_committed_transaction_interruption(
                &storage_dir,
                now,
                CommittedInterruption::BeforeFirstProjectRename,
            );
        let first_project_path = storage_dir
            .project_dir_path("20260905", "roll-forward-first", first_id)
            .join("project.yaml");
        let second_project_path = storage_dir
            .project_dir_path("20260905", "roll-forward-second", second_id)
            .join("project.yaml");
        let old_first = fs::read(&first_project_path).unwrap();
        let old_second = fs::read(&second_project_path).unwrap();
        let old_revision = fs::read(storage_dir.path.join(".revision")).unwrap();
        let replaced_path = match case {
            "marker" => {
                fs::write(&external_path, b"external-marker").unwrap();
                active_transaction_path.join("commit")
            }
            "manifest" => {
                fs::write(&external_path, b"external-manifest").unwrap();
                active_transaction_path.join("manifest.json")
            }
            "later-staged" => {
                let staged_path = active_transaction_path.join("files/1");
                fs::write(&external_path, fs::read(&staged_path).unwrap()).unwrap();
                staged_path
            }
            _ => unreachable!(),
        };
        fs::remove_file(&replaced_path).unwrap();
        symlink(&external_path, &replaced_path).unwrap();
        let external_bytes = fs::read(&external_path).unwrap();
        let mut repository = TaskRepository::new(storage_dir.path_str());

        let actual = repository.load().unwrap_err();

        let source = storage_transaction_error(&actual);
        assert!(
            source.to_string().contains(expected_phase),
            "case: {case}, error: {source}"
        );
        assert!(
            source
                .to_string()
                .contains(&replaced_path.display().to_string()),
            "case: {case}"
        );
        assert!(active_transaction_path.exists(), "case: {case}");
        assert_eq!(fs::read(first_project_path).unwrap(), old_first);
        assert_eq!(fs::read(second_project_path).unwrap(), old_second);
        assert_eq!(
            fs::read(storage_dir.path.join(".revision")).unwrap(),
            old_revision
        );
        assert_eq!(fs::read(external_path).unwrap(), external_bytes);
    }
}

#[cfg(unix)]
#[test]
fn test_load_前回失敗したcleanup_tombstoneだけを再清掃してsnapshotを維持する() {
    use std::os::unix::fs::symlink;

    let storage_dir = TestStorageDir::new();
    let external_dir = TestStorageDir::new();
    fs::create_dir_all(&external_dir.path).unwrap();
    let external_sentinel = external_dir.path.join("sentinel");
    fs::write(&external_sentinel, b"external").unwrap();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 13, 0, 0).unwrap();
    let mut source = TaskRepository::new(storage_dir.path_str());
    source.sync_clock(now).unwrap();
    let task = crate::test_support::new_task_handle("cleanup-retry").unwrap();
    let task_id = task.get_id().unwrap();
    source.start_new_project(task).unwrap();
    source.storage_transaction_io = Arc::new(RecordingIo::new(vec![FaultRule {
        operation: RecordingOperation::RemoveDirectory,
        path_matcher: PathMatcher::FileNamePrefix(".cleanup-"),
        occurrence: 1,
        error_kind: std::io::ErrorKind::Other,
        error_message: "injected transient cleanup failure",
    }]));

    source.save().unwrap();

    let project_path = storage_dir
        .project_dir_path("20260905", "cleanup-retry", task_id)
        .join("project.yaml");
    let project_bytes = fs::read(&project_path).unwrap();
    let revision_bytes = fs::read(storage_dir.path.join(".revision")).unwrap();
    let transactions_dir_path = storage_dir
        .path
        .join(crate::adapter::gateway::storage_transaction::TRANSACTION_DIRECTORY_NAME);
    let cleanup_path = fs::read_dir(&transactions_dir_path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".cleanup-"))
        })
        .unwrap();
    assert!(cleanup_path.join("manifest.json").is_file());
    assert!(cleanup_path.join("files/0").is_file());
    let arbitrary_path = transactions_dir_path.join(".cleanup-not-a-uuid");
    fs::create_dir(&arbitrary_path).unwrap();
    fs::write(arbitrary_path.join("sentinel"), b"arbitrary").unwrap();
    let symlink_path =
        transactions_dir_path.join(format!(".cleanup-{}", Uuid::from_u128(0x22fe).hyphenated()));
    symlink(&external_dir.path, &symlink_path).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();

    repository.load().unwrap();

    assert!(!cleanup_path.exists());
    assert!(arbitrary_path.join("sentinel").is_file());
    assert!(symlink_path.is_symlink());
    assert_eq!(fs::read(external_sentinel).unwrap(), b"external");
    assert_eq!(fs::read(project_path).unwrap(), project_bytes);
    assert_eq!(
        fs::read(storage_dir.path.join(".revision")).unwrap(),
        revision_bytes
    );
    assert!(repository.get_by_id(task_id).unwrap().is_some());
}

#[test]
fn test_load_markerなしtransaction破棄失敗はpathとphaseを保持してmemoryを変更しない() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let active_transaction_path =
        create_markerless_active_transaction(&storage_dir, "before-marker");
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    let memory_task = crate::test_support::new_task_handle("memory").unwrap();
    let memory_task_id = memory_task.get_id().unwrap();
    repository.start_new_project(memory_task).unwrap();
    repository.storage_transaction_io = Arc::new(RecordingIo::new(vec![FaultRule {
        operation: RecordingOperation::RemoveDirectory,
        path_matcher: PathMatcher::FileName(".active"),
        occurrence: 1,
        error_kind: std::io::ErrorKind::Other,
        error_message: "injected uncommitted discard failure",
    }]));

    let actual = repository.load().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    let source = storage_transaction_error(&actual);
    assert!(source.to_string().contains("DiscardUncommitted"));
    assert!(source
        .to_string()
        .contains(&active_transaction_path.display().to_string()));
    assert!(source
        .source()
        .unwrap()
        .to_string()
        .contains("injected uncommitted discard failure"));
    assert_eq!(repository.get_all_projects().len(), 1);
    assert!(repository.get_by_id(memory_task_id).unwrap().is_some());
    assert!(!repository.has_loaded);
    assert_eq!(repository.storage_revision.get(), None);
}

#[cfg(unix)]
#[test]
fn test_load_transaction_root_symlinkを拒否して外部activeを変更しない() {
    use std::os::unix::fs::symlink;

    let storage_dir = TestStorageDir::new();
    fs::create_dir(&storage_dir.path).unwrap();
    let external_dir = TestStorageDir::new();
    fs::create_dir(&external_dir.path).unwrap();
    let external_active_path = external_dir.path.join(".active");
    fs::create_dir(&external_active_path).unwrap();
    let external_manifest_path = external_active_path.join("manifest.json");
    fs::write(&external_manifest_path, b"external").unwrap();
    let transactions_dir_path = storage_dir
        .path
        .join(crate::adapter::gateway::storage_transaction::TRANSACTION_DIRECTORY_NAME);
    symlink(&external_dir.path, &transactions_dir_path).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let actual = repository.load().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    let source = storage_transaction_error(&actual);
    assert!(source.to_string().contains("ValidateTransactionDirectory"));
    assert!(source
        .to_string()
        .contains(&transactions_dir_path.display().to_string()));
    assert_eq!(fs::read(external_manifest_path).unwrap(), b"external");
}
