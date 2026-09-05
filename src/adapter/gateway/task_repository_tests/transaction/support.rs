struct FailFirstCommittedProjectWriteIo {
    failed: AtomicBool,
}

struct FailUncommittedDiscardIo;

struct FailCleanupDeleteIo;

#[derive(Clone, Copy, Debug)]
enum CommittedCrashPhase {
    BeforeFirstProjectRename,
    AfterFirstProjectRename,
    BeforeFinalProjectRename,
    BeforeRevision,
    AfterRevisionBeforeCleanup,
}

struct CommittedCrashIo {
    phase: CommittedCrashPhase,
    project_temporary_creates: AtomicUsize,
    project_renames: AtomicUsize,
}

impl CommittedCrashIo {
    fn new(phase: CommittedCrashPhase) -> Self {
        Self {
            phase,
            project_temporary_creates: AtomicUsize::new(0),
            project_renames: AtomicUsize::new(0),
        }
    }

    fn is_project_temporary(path: &Path) -> bool {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".project.yaml."))
    }

    fn is_revision_temporary(path: &Path) -> bool {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("..revision."))
    }
}

impl StorageTransactionIo for CommittedCrashIo {
    fn create_new_file(&self, path: &Path) -> std::io::Result<()> {
        if Self::is_project_temporary(path) {
            let create_index = self
                .project_temporary_creates
                .fetch_add(1, Ordering::SeqCst);
            if matches!(self.phase, CommittedCrashPhase::AfterFirstProjectRename)
                && create_index == 1
            {
                return Err(std::io::Error::other(
                    "injected crash after first project rename",
                ));
            }
        }
        if matches!(self.phase, CommittedCrashPhase::BeforeRevision)
            && Self::is_revision_temporary(path)
        {
            return Err(std::io::Error::other("injected crash before revision"));
        }
        FileSystemStorageTransactionIo.create_new_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        if to.file_name().is_some_and(|name| name == "project.yaml") {
            let rename_index = self.project_renames.fetch_add(1, Ordering::SeqCst);
            if matches!(self.phase, CommittedCrashPhase::BeforeFirstProjectRename)
                && rename_index == 0
            {
                return Err(std::io::Error::other(
                    "injected crash before first project rename",
                ));
            }
            if matches!(self.phase, CommittedCrashPhase::BeforeFinalProjectRename)
                && rename_index == 1
            {
                return Err(std::io::Error::other(
                    "injected crash before final project rename",
                ));
            }
        }
        if matches!(self.phase, CommittedCrashPhase::AfterRevisionBeforeCleanup)
            && from.file_name().is_some_and(|name| name == ".active")
            && to
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".cleanup-"))
        {
            return Err(std::io::Error::other(
                "injected crash after revision before cleanup",
            ));
        }
        FileSystemStorageTransactionIo.rename(from, to)
    }
}

impl StorageTransactionIo for FailFirstCommittedProjectWriteIo {
    fn write_file(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".project.yaml."))
            && !self.failed.swap(true, Ordering::SeqCst)
        {
            return Err(std::io::Error::other(
                "injected committed project write failure",
            ));
        }
        FileSystemStorageTransactionIo.write_file(path, bytes)
    }
}

impl StorageTransactionIo for FailUncommittedDiscardIo {
    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        if path.file_name().is_some_and(|name| name == ".active") {
            return Err(std::io::Error::other(
                "injected uncommitted discard failure",
            ));
        }
        FileSystemStorageTransactionIo.remove_dir_all(path)
    }
}

impl StorageTransactionIo for FailCleanupDeleteIo {
    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".cleanup-"))
        {
            return Err(std::io::Error::other("injected transient cleanup failure"));
        }
        FileSystemStorageTransactionIo.remove_dir_all(path)
    }
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
    phase: CommittedCrashPhase,
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
    repository.storage_transaction_io = Arc::new(CommittedCrashIo::new(phase));

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
