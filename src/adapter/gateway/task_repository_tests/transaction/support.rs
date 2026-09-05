#[derive(Clone, Copy, Debug)]
enum CommittedInterruption {
    BeforeFirstProjectRename,
    AfterFirstProjectRename,
    BeforeFinalProjectRename,
    BeforeRevision,
    AfterRevisionBeforeCleanup,
}

impl CommittedInterruption {
    fn fault_rule(self) -> FaultRule {
        let (operation, path_matcher, occurrence, error_message) = match self {
            Self::BeforeFirstProjectRename => (
                RecordingOperation::Rename,
                PathMatcher::FileName("project.yaml"),
                1,
                "injected crash before first project rename",
            ),
            Self::AfterFirstProjectRename => (
                RecordingOperation::CreateFile,
                PathMatcher::FileNamePrefix(".project.yaml."),
                2,
                "injected crash after first project rename",
            ),
            Self::BeforeFinalProjectRename => (
                RecordingOperation::Rename,
                PathMatcher::FileName("project.yaml"),
                2,
                "injected crash before final project rename",
            ),
            Self::BeforeRevision => (
                RecordingOperation::CreateFile,
                PathMatcher::FileNamePrefix("..revision."),
                1,
                "injected crash before revision",
            ),
            Self::AfterRevisionBeforeCleanup => (
                RecordingOperation::Rename,
                PathMatcher::FileNamePrefix(".cleanup-"),
                1,
                "injected crash after revision before cleanup",
            ),
        };
        FaultRule {
            operation,
            path_matcher,
            occurrence,
            error_kind: std::io::ErrorKind::Other,
            error_message,
        }
    }
}

fn committed_interruption_io(
    interruption: CommittedInterruption,
) -> Arc<dyn StorageTransactionIo> {
    Arc::new(RecordingIo::new(vec![interruption.fault_rule()]))
}

fn create_markerless_active_transaction(storage_dir: &TestStorageDir, phase: &str) -> PathBuf {
    let active_transaction_path = storage_dir
        .path
        .join(crate::adapter::gateway::storage_transaction::TRANSACTION_DIRECTORY_NAME)
        .join(".active");
    fs::create_dir_all(active_transaction_path.join("files")).unwrap();
    match phase {
        "staged-write" => fs::write(active_transaction_path.join("files/0"), b"partial").unwrap(),
        "staged-sync" => fs::write(active_transaction_path.join("files/0"), b"staged").unwrap(),
        "manifest-sync" => {
            fs::write(active_transaction_path.join("files/0"), b"staged").unwrap();
            fs::write(active_transaction_path.join("manifest.json"), b"{}").unwrap();
        }
        "before-marker" => {
            fs::write(active_transaction_path.join("files/0"), b"staged").unwrap();
            fs::write(active_transaction_path.join("manifest.json"), b"{}").unwrap();
            fs::write(active_transaction_path.join("commit.tmp"), b"").unwrap();
        }
        _ => panic!("unknown interruption phase: {phase}"),
    }
    active_transaction_path
}

fn create_committed_transaction_interruption(
    storage_dir: &TestStorageDir,
    now: DateTime<Local>,
    interruption: CommittedInterruption,
) -> (Uuid, Uuid, Uuid, PathBuf) {
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    let first = crate::test_support::new_task_handle("roll-forward-first").unwrap();
    let second = crate::test_support::new_task_handle("roll-forward-second").unwrap();
    let first_id = first.get_id().unwrap();
    let second_id = second.get_id().unwrap();
    repository.start_new_project(first.clone()).unwrap();
    repository.start_new_project(second.clone()).unwrap();
    repository.save().unwrap();
    first.set_estimated_work_seconds(30 * 60).unwrap();
    second.set_estimated_work_seconds(45 * 60).unwrap();
    repository.storage_transaction_io = committed_interruption_io(interruption);

    let error = repository.save().unwrap_err();
    assert_eq!(error.operation(), ApplicationRepositoryOperation::Save);

    let active_transaction_path = storage_dir
        .path
        .join(crate::adapter::gateway::storage_transaction::TRANSACTION_DIRECTORY_NAME)
        .join(".active");
    assert!(active_transaction_path.join("commit").is_file());
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(active_transaction_path.join("manifest.json")).unwrap())
            .unwrap();
    let revision = Uuid::parse_str(manifest["revision"].as_str().unwrap()).unwrap();
    (first_id, second_id, revision, active_transaction_path)
}

fn assert_recovered_projects(repository: &TaskRepository, first_id: Uuid, second_id: Uuid) {
    assert_eq!(
        repository
            .get_by_id(first_id)
            .unwrap()
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        30 * 60
    );
    assert_eq!(
        repository
            .get_by_id(second_id)
            .unwrap()
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        45 * 60
    );
}
