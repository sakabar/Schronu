#[test]
fn test_save_prepare失敗時は複数projectとrevisionを旧snapshotに維持する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let revision_path = storage_dir.path.join(".revision");
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    let first_task = crate::test_support::new_task_handle("第一対象").unwrap();
    let first_task_id = first_task.get_id().unwrap();
    repository.start_new_project(first_task.clone()).unwrap();
    let second_task = crate::test_support::new_task_handle("第二対象").unwrap();
    let second_task_id = second_task.get_id().unwrap();
    repository.start_new_project(second_task.clone()).unwrap();
    repository.save().unwrap();
    let previous_revision = repository.storage_revision.get().unwrap();
    let first_project_yaml_path = storage_dir
        .project_dir_path("20260813", "第一対象", first_task_id)
        .join("project.yaml");
    let second_project_yaml_path = storage_dir
        .project_dir_path("20260813", "第二対象", second_task_id)
        .join("project.yaml");
    let first_old_bytes = fs::read(&first_project_yaml_path).unwrap();
    let second_old_bytes = fs::read(&second_project_yaml_path).unwrap();
    repository.storage_transaction_io = prepare_failure_io();
    first_task.set_estimated_work_seconds(30 * 60).unwrap();
    second_task.set_estimated_work_seconds(45 * 60).unwrap();

    let actual = repository.save();

    assert!(actual.is_err());
    let disk_revision =
        Uuid::parse_str(fs::read_to_string(&revision_path).unwrap().trim()).unwrap();
    assert_eq!(disk_revision, previous_revision);
    assert_eq!(fs::read(first_project_yaml_path).unwrap(), first_old_bytes);
    assert_eq!(
        fs::read(second_project_yaml_path).unwrap(),
        second_old_bytes
    );
    assert_eq!(repository.storage_revision.get(), Some(previous_revision));
}

#[test]
fn test_save_prepare失敗時は新規projectのlive_directoryを作成しない() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    let task = crate::test_support::new_task_handle("新規対象").unwrap();
    let task_id = task.get_id().unwrap();
    repository.start_new_project(task).unwrap();
    repository.storage_transaction_io = prepare_failure_io();
    let project_dir_path = storage_dir.project_dir_path("20260813", "新規対象", task_id);

    let actual = repository.save();

    assert!(actual.is_err());
    assert!(!project_dir_path.exists());
    assert!(!storage_dir.path.join(".revision").exists());
}

#[test]
fn test_save_post_marker失敗後はactive_transactionを保持して次saveを拒否する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let revision_path = storage_dir.path.join(".revision");
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    let task = crate::test_support::new_task_handle("commit失敗対象").unwrap();
    let task_id = task.get_id().unwrap();
    repository.start_new_project(task.clone()).unwrap();
    repository.save().unwrap();
    let project_yaml_path = storage_dir
        .project_dir_path("20260813", "commit失敗対象", task_id)
        .join("project.yaml");
    let old_project_bytes = fs::read(&project_yaml_path).unwrap();
    let old_revision_bytes = fs::read(&revision_path).unwrap();
    repository.storage_transaction_io = Arc::new(RecordingIo::new(vec![FaultRule {
        operation: RecordingOperation::WriteFile,
        path_matcher: PathMatcher::FileNamePrefix(".project.yaml."),
        occurrence: 1,
        error_kind: std::io::ErrorKind::Other,
        error_message: "injected committed project write failure",
    }]));
    task.set_estimated_work_seconds(30 * 60).unwrap();

    let first = repository.save();

    assert!(first.is_err());
    let transactions_dir_path = storage_dir
        .path
        .join(crate::adapter::gateway::storage_transaction::TRANSACTION_DIRECTORY_NAME);
    let active_transaction_path = transactions_dir_path.join(".active");
    assert!(active_transaction_path.join("commit").is_file());
    assert!(active_transaction_path.join("manifest.json").is_file());

    let second = repository.save().unwrap_err();

    let source = storage_transaction_error(&second);
    assert!(source.to_string().contains("ActiveTransaction"));
    assert!(source
        .to_string()
        .contains(&active_transaction_path.display().to_string()));
    assert_eq!(fs::read(project_yaml_path).unwrap(), old_project_bytes);
    assert_eq!(fs::read(revision_path).unwrap(), old_revision_bytes);
    assert!(active_transaction_path.join("commit").is_file());
    assert!(active_transaction_path.join("manifest.json").is_file());
}
