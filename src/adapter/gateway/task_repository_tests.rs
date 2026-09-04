use super::*;
use crate::adapter::gateway::yaml::YamlConversionError;
use crate::application::interface::ProjectRegistrationError;
use chrono::{Duration, TimeZone};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct FailingStorageTransactionIo;

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

impl StorageTransactionIo for FailingStorageTransactionIo {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        if path.ends_with(crate::adapter::gateway::storage_transaction::TRANSACTION_DIRECTORY_NAME)
        {
            return Err(std::io::Error::other("injected prepare failure"));
        }
        fs::create_dir_all(path)
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

struct TestStorageDir {
    path: PathBuf,
}

impl TestStorageDir {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!("schronu-test-{}", Uuid::new_v4())),
        }
    }

    fn path_str(&self) -> &str {
        self.path.to_str().expect("test path must be valid UTF-8")
    }

    fn project_dir_path(&self, date: &str, project_name: &str, project_id: Uuid) -> PathBuf {
        self.path
            .join(format!("{date}-{project_name}-{project_id}"))
    }
}

impl Drop for TestStorageDir {
    fn drop(&mut self) {
        if self.path.is_dir() {
            fs::remove_dir_all(&self.path).expect("failed to remove test storage directory");
        } else if self.path.exists() {
            fs::remove_file(&self.path).expect("failed to remove test storage file");
        }
    }
}

fn write_project_yaml(
    storage_dir: &TestStorageDir,
    directory_name: &str,
    contents: &str,
) -> PathBuf {
    let project_dir_path = storage_dir.path.join(directory_name);
    fs::create_dir_all(&project_dir_path).unwrap();
    let project_yaml_file_path = project_dir_path.join("project.yaml");
    fs::write(&project_yaml_file_path, contents).unwrap();
    project_yaml_file_path
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

fn file_repository_error(error: &TaskRepositoryError) -> &FileRepositoryError {
    error
        .source()
        .and_then(|source| source.downcast_ref::<FileRepositoryError>())
        .expect("repository error source must be FileRepositoryError")
}

fn storage_transaction_error(
    error: &TaskRepositoryError,
) -> &crate::adapter::gateway::storage_transaction::StorageTransactionError {
    error
        .source()
        .and_then(|source| {
            source.downcast_ref::<
                crate::adapter::gateway::storage_transaction::StorageTransactionError,
            >()
        })
        .expect("repository error source must be StorageTransactionError")
}

fn duplicate_task_id_error(error: &TaskRepositoryError) -> &DuplicateTaskIdError {
    error
        .source()
        .and_then(|source| source.downcast_ref::<DuplicateTaskIdError>())
        .expect("repository error source must be DuplicateTaskIdError")
}

fn task_with_start_time(name: &str, start_time: DateTime<Local>) -> TaskHandle {
    let task = crate::test_support::new_task_handle(name).unwrap();
    task.set_start_time(start_time).unwrap();
    task.set_priority(5).unwrap();
    task
}

fn project_root_with_identity(name: &str, id: Uuid, now: DateTime<Local>) -> TaskHandle {
    let task = TaskHandle::with_identity(name, id, now).unwrap();
    task.set_priority(5).unwrap();
    task
}

fn assert_colliding_project_names_survive_save_load(
    first_name: &str,
    second_name: &str,
    first_id: Uuid,
    second_id: Uuid,
) {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository
        .start_new_project(project_root_with_identity(first_name, first_id, now))
        .unwrap();
    repository
        .start_new_project(project_root_with_identity(second_name, second_id, now))
        .unwrap();

    repository.save().unwrap();

    let expected_directories = [
        storage_dir
            .path
            .join(format!("20260904-{first_name}-{first_id}")),
        storage_dir
            .path
            .join(format!("20260904-{second_name}-{second_id}")),
    ];
    for directory in expected_directories {
        assert!(directory.join("project.yaml").is_file());
    }

    let mut loaded_repository = TaskRepository::new(storage_dir.path_str());
    loaded_repository.sync_clock(now).unwrap();
    loaded_repository.load().unwrap();

    assert_eq!(loaded_repository.get_all_projects().len(), 2);
    assert!(loaded_repository.get_by_id(first_id).unwrap().is_some());
    assert!(loaded_repository.get_by_id(second_id).unwrap().is_some());
}

fn pending_task_with_until(name: &str, pending_until: DateTime<Local>) -> TaskHandle {
    let task = crate::test_support::new_task_handle(name).unwrap();
    task.set_start_time(DateTime::<Local>::MIN_UTC.into())
        .unwrap();
    task.set_pending_until(pending_until).unwrap();
    task.set_orig_status(Status::Pending).unwrap();
    task.set_priority(5).unwrap();
    task
}

fn add_project(task_repository: &mut TaskRepository, root_task: TaskHandle) {
    task_repository
        .projects
        .push(Project::new(root_task, "".to_string(), "".to_string(), 5));
}

#[test]
fn test_start_new_project_taskをmemoryに登録する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let mut task_repository = TaskRepository::new(storage_dir.path_str());
    task_repository.sync_clock(now).unwrap();
    let root_task = crate::test_support::new_task_handle("メモリ登録対象").unwrap();
    let root_task_id = root_task.get_id().unwrap();

    task_repository.start_new_project(root_task).unwrap();

    assert_eq!(
        task_repository
            .get_by_id(root_task_id)
            .unwrap()
            .unwrap()
            .get_name()
            .unwrap(),
        "メモリ登録対象"
    );
}

#[test]
fn test_start_new_project_既存taskと同じuuidを拒否して状態を変更しない() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let duplicate_id = Uuid::from_u128(0x2101);
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository
        .start_new_project(project_root_with_identity("既存project", duplicate_id, now))
        .unwrap();

    let actual = repository
        .start_new_project(project_root_with_identity("重複project", duplicate_id, now))
        .unwrap_err();

    assert_eq!(
        actual,
        ProjectRegistrationError::DuplicateTaskId(duplicate_id)
    );
    assert_eq!(repository.get_all_projects().len(), 1);
    assert_eq!(
        repository
            .get_by_id(duplicate_id)
            .unwrap()
            .unwrap()
            .get_name()
            .unwrap(),
        "既存project"
    );
    assert!(!storage_dir.path.exists());
}

#[test]
fn test_start_new_project_既存projectと同じ保存先を拒否して状態を変更しない() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let existing_id = Uuid::from_u128(0x2111);
    let candidate_id = Uuid::from_u128(0x2112);
    let colliding_directory = storage_dir.path.join(project_directory_name(
        "20260904",
        "同じ保存先",
        candidate_id,
    ));
    let existing_root = project_root_with_identity("既存project", existing_id, now);
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository
        .cache_task_and_descendants(&existing_root)
        .unwrap();
    repository.projects.push(Project::new(
        existing_root,
        &colliding_directory,
        colliding_directory.join("project.yaml"),
        5,
    ));

    let actual = repository
        .start_new_project(project_root_with_identity("同じ保存先", candidate_id, now))
        .unwrap_err();

    assert_eq!(
        actual,
        ProjectRegistrationError::DuplicateStoragePath(colliding_directory)
    );
    assert_eq!(repository.get_all_projects().len(), 1);
    assert!(repository.get_by_id(existing_id).unwrap().is_some());
    assert!(repository.get_by_id(candidate_id).unwrap().is_none());
    assert!(!storage_dir.path.exists());
}

#[test]
fn sync_clockは全projectのrootと全descendantへ同じ時刻を伝搬する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let pending_until = now - Duration::hours(1);
    let root_task = pending_task_with_until("root", pending_until);
    let child_task = root_task.create_as_last_child(crate::test_support::new_task_attr("child"));
    child_task
        .set_start_time(DateTime::<Local>::MIN_UTC.into())
        .unwrap();
    child_task.set_orig_status(Status::Pending).unwrap();
    child_task.set_pending_until(pending_until).unwrap();
    let grandchild_task =
        child_task.create_as_last_child(crate::test_support::new_task_attr("grandchild"));
    grandchild_task
        .set_start_time(DateTime::<Local>::MIN_UTC.into())
        .unwrap();
    grandchild_task.set_orig_status(Status::Pending).unwrap();
    grandchild_task.set_pending_until(pending_until).unwrap();
    let second_root_task = pending_task_with_until("second root", pending_until);
    let mut task_repository = TaskRepository::new(storage_dir.path_str());
    add_project(&mut task_repository, root_task.clone());
    add_project(&mut task_repository, second_root_task.clone());

    task_repository.sync_clock(now).unwrap();

    for task in [root_task, child_task, grandchild_task, second_root_task] {
        assert_eq!(task.get_last_synced_time().unwrap(), now);
        assert_eq!(task.get_status().unwrap(), Status::Todo);
    }
    assert_eq!(task_repository.get_last_synced_time(), now);
}

#[test]
fn test_start_new_project_filesystemを変更しない() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let mut task_repository = TaskRepository::new(storage_dir.path_str());
    task_repository.sync_clock(now).unwrap();

    task_repository
        .start_new_project(crate::test_support::new_task_handle("filesystem非変更対象").unwrap())
        .unwrap();

    assert!(!storage_dir.path.exists());
}

#[test]
fn test_save_新規projectのdirectoryとyamlを作る() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let mut task_repository = TaskRepository::new(storage_dir.path_str());
    task_repository.sync_clock(now).unwrap();
    let root_task = crate::test_support::new_task_handle("保存対象").unwrap();
    let root_task_id = root_task.get_id().unwrap();
    task_repository.start_new_project(root_task).unwrap();
    let project_dir_path = storage_dir.project_dir_path("20260811", "保存対象", root_task_id);
    let markdown_dir_path = project_dir_path.join("markdown");
    let project_yaml_file_path = project_dir_path.join("project.yaml");

    assert!(!storage_dir.path.exists());
    task_repository.save().unwrap();

    assert!(project_dir_path.is_dir());
    assert!(markdown_dir_path.is_dir());
    assert!(project_yaml_file_path.is_file());

    let mut loaded_repository = TaskRepository::new(storage_dir.path_str());
    loaded_repository.sync_clock(now).unwrap();
    loaded_repository.load().unwrap();
    let loaded_task = loaded_repository.get_by_id(root_task_id).unwrap().unwrap();
    assert_eq!(loaded_task.get_name().unwrap(), "保存対象");
}

#[test]
fn test_save_load_同日同名projectを別directoryへ保存する() {
    assert_colliding_project_names_survive_save_load(
        "同名project",
        "同名project",
        Uuid::from_u128(0x2001),
        Uuid::from_u128(0x2002),
    );
}

#[test]
fn test_save_load_slash置換後に同名となるprojectを別directoryへ保存する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let first_id = Uuid::from_u128(0x2011);
    let second_id = Uuid::from_u128(0x2012);
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository
        .start_new_project(project_root_with_identity("a/b", first_id, now))
        .unwrap();
    repository
        .start_new_project(project_root_with_identity("a-b", second_id, now))
        .unwrap();

    repository.save().unwrap();

    for id in [first_id, second_id] {
        assert!(storage_dir
            .path
            .join(format!("20260904-a-b-{id}"))
            .join("project.yaml")
            .is_file());
    }
    let mut loaded_repository = TaskRepository::new(storage_dir.path_str());
    loaded_repository.sync_clock(now).unwrap();
    loaded_repository.load().unwrap();
    assert_eq!(loaded_repository.get_all_projects().len(), 2);
    assert!(loaded_repository.get_by_id(first_id).unwrap().is_some());
    assert!(loaded_repository.get_by_id(second_id).unwrap().is_some());
}

#[test]
fn test_save_load_url除去後に同名となるprojectを別directoryへ保存する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let first_id = Uuid::from_u128(0x2021);
    let second_id = Uuid::from_u128(0x2022);
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository
        .start_new_project(project_root_with_identity(
            "project http://example.com/one",
            first_id,
            now,
        ))
        .unwrap();
    repository
        .start_new_project(project_root_with_identity(
            "project https://example.com/two",
            second_id,
            now,
        ))
        .unwrap();

    repository.save().unwrap();

    for id in [first_id, second_id] {
        assert!(storage_dir
            .path
            .join(format!("20260904-project -{id}"))
            .join("project.yaml")
            .is_file());
    }
    let mut loaded_repository = TaskRepository::new(storage_dir.path_str());
    loaded_repository.sync_clock(now).unwrap();
    loaded_repository.load().unwrap();
    assert_eq!(loaded_repository.get_all_projects().len(), 2);
    assert!(loaded_repository.get_by_id(first_id).unwrap().is_some());
    assert!(loaded_repository.get_by_id(second_id).unwrap().is_some());
}

#[test]
fn test_save_uuid追加後も長いproject名をutf8境界で短縮して保存する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let task_id = Uuid::from_u128(0x2023);
    let project_name = "あ".repeat(80);
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository
        .start_new_project(project_root_with_identity(&project_name, task_id, now))
        .unwrap();

    repository.save().unwrap();

    let project_directory = fs::read_dir(&storage_dir.path)
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.path().join("project.yaml").is_file())
        .unwrap();
    let directory_name = project_directory.file_name();
    let directory_name = directory_name.to_str().unwrap();
    assert!(directory_name.len() <= 255);
    assert!(directory_name.ends_with(&format!("-{task_id}")));

    let mut loaded_repository = TaskRepository::new(storage_dir.path_str());
    loaded_repository.sync_clock(now).unwrap();
    loaded_repository.load().unwrap();
    assert!(loaded_repository.get_by_id(task_id).unwrap().is_some());
}

#[test]
fn test_load_旧形式directoryをrenameせず読み込む() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let task_id = Uuid::from_u128(0x2031);
    let root_task = project_root_with_identity("旧形式", task_id, now);
    let project = Project::new(root_task, "", "", 5);
    let bytes = TaskRepository::serialize_project(&project).unwrap();
    let legacy_directory = storage_dir.path.join("20260904-旧形式");
    fs::create_dir_all(&legacy_directory).unwrap();
    fs::write(legacy_directory.join("project.yaml"), bytes).unwrap();

    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository.load().unwrap();
    repository.save().unwrap();

    assert_eq!(repository.get_all_projects().len(), 1);
    assert!(repository.get_by_id(task_id).unwrap().is_some());
    assert!(legacy_directory.join("project.yaml").is_file());
    assert!(!storage_dir
        .path
        .join(format!("20260904-旧形式-{task_id}"))
        .exists());
}

fn project_with_all_persisted_yaml_fields() -> Project {
    let now = Local.with_ymd_and_hms(2026, 8, 28, 12, 0, 0).unwrap();
    let create_time = Local.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
    let start_time = Local.with_ymd_and_hms(2025, 2, 3, 4, 5, 6).unwrap();
    let end_time = Local.with_ymd_and_hms(2025, 3, 4, 5, 6, 7).unwrap();
    let pending_until = Local.with_ymd_and_hms(2037, 12, 31, 23, 59, 59).unwrap();
    let deadline_time = Local.with_ymd_and_hms(2040, 4, 5, 6, 7, 8).unwrap();
    let root_task = TaskHandle::with_identity(
        "root task",
        uuid::uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8"),
        now,
    )
    .unwrap();
    root_task.set_orig_status(Status::Pending).unwrap();
    root_task.set_is_on_other_side(true).unwrap();
    root_task.set_atomic(true).unwrap();
    root_task.set_pending_until(pending_until).unwrap();
    root_task.set_priority(8).unwrap();
    root_task
        .set_project_category_opt(Some(crate::entity::task::ProjectCategory::Investment))
        .unwrap();
    root_task.set_create_time(create_time).unwrap();
    root_task.set_start_time(start_time).unwrap();
    root_task.set_end_time_opt(Some(end_time)).unwrap();
    root_task
        .set_deadline_time_opt(Some(deadline_time))
        .unwrap();
    root_task.set_estimated_work_seconds(3600).unwrap();
    root_task.set_actual_work_seconds(120).unwrap();
    root_task.set_repetition_interval_days_opt(Some(7)).unwrap();
    root_task
        .set_repetition_anchor(crate::entity::task::RepetitionAnchor::Completion)
        .unwrap();
    root_task.set_days_in_advance(3).unwrap();

    let mut child_attr = crate::entity::task::TaskAttr::with_identity(
        "child task",
        uuid::uuid!("0aaee735-3e22-4216-8b59-d56d5caf29ee"),
        now,
    );
    child_attr.set_orig_status(Status::Pending);
    child_attr.set_is_on_other_side(true);
    child_attr.set_atomic(true);
    child_attr.set_pending_until(pending_until);
    child_attr.set_priority(99);
    child_attr.set_project_category_opt(Some(crate::entity::task::ProjectCategory::Consumption));
    child_attr.set_create_time(create_time);
    child_attr.set_start_time(start_time);
    child_attr.set_end_time_opt(Some(end_time));
    child_attr.set_deadline_time_opt(Some(deadline_time));
    child_attr.set_estimated_work_seconds(1800);
    child_attr.set_actual_work_seconds(60);
    child_attr.set_repetition_interval_days_opt(Some(2));
    child_attr.set_repetition_anchor(crate::entity::task::RepetitionAnchor::Completion);
    child_attr.set_days_in_advance(1);
    root_task.create_as_last_child(child_attr);
    let default_child_attr = crate::entity::task::TaskAttr::with_identity(
        "default child",
        uuid::uuid!("7ffcba2f-80e0-4a44-aee9-d68e0d2d1256"),
        now,
    );
    root_task.create_as_last_child(default_child_attr);

    Project::new(root_task, "", "project.yaml", 8)
}

#[test]
fn test_project_yaml保存bytesはkey順_既定値省略_root限定field_children_末尾改行を保つ() {
    let project = project_with_all_persisted_yaml_fields();

    let actual = TaskRepository::serialize_project(&project).unwrap();

    let expected = b"---\nproject:\n  name: root task\n  id: 67e55044-10b1-426f-9247-bb680e5fe0c8\n  status: pending\n  is_on_other_side: true\n  atomic: true\n  pending_until: \"2037/12/31 23:59:59\"\n  priority: 8\n  category: investment\n  create_time: \"2024/01/02 03:04:05\"\n  start_time: \"2025/02/03 04:05:06\"\n  end_time: \"2025/03/04 05:06:07\"\n  deadline_time: \"2040/04/05 06:07:08\"\n  estimated_work_seconds: 3600\n  actual_work_seconds: 120\n  repetition_interval_days: 7\n  repetition_anchor: completion\n  days_in_advance: 3\n  children:\n    - name: child task\n      id: 0aaee735-3e22-4216-8b59-d56d5caf29ee\n      status: pending\n      is_on_other_side: true\n      atomic: true\n      pending_until: \"2037/12/31 23:59:59\"\n      create_time: \"2024/01/02 03:04:05\"\n      start_time: \"2025/02/03 04:05:06\"\n      end_time: \"2025/03/04 05:06:07\"\n      deadline_time: \"2040/04/05 06:07:08\"\n      estimated_work_seconds: 1800\n      actual_work_seconds: 60\n      repetition_interval_days: 2\n      repetition_anchor: completion\n      days_in_advance: 1\n    - name: default child\n      id: 7ffcba2f-80e0-4a44-aee9-d68e0d2d1256\n      create_time: \"2026/08/28 12:00:00\"\n      start_time: \"2026/08/28 12:00:00\"\n";
    assert_eq!(actual, expected);
    assert_eq!(actual.last(), Some(&b'\n'));
}

fn assert_yaml_persisted_task_fields(
    actual: &TaskHandle,
    expected: &TaskHandle,
) -> Result<(), TaskTreeError> {
    let actual = actual.get_attr()?;
    let expected = expected.get_attr()?;
    assert_eq!(actual.get_id(), expected.get_id());
    assert_eq!(actual.get_name(), expected.get_name());
    assert_eq!(actual.get_orig_status(), expected.get_orig_status());
    assert_eq!(
        actual.get_is_on_other_side(),
        expected.get_is_on_other_side()
    );
    assert_eq!(actual.get_atomic(), expected.get_atomic());
    assert_eq!(actual.get_pending_until(), expected.get_pending_until());
    assert_eq!(actual.get_create_time(), expected.get_create_time());
    assert_eq!(actual.get_start_time(), expected.get_start_time());
    assert_eq!(actual.get_end_time_opt(), expected.get_end_time_opt());
    assert_eq!(
        actual.get_deadline_time_opt(),
        expected.get_deadline_time_opt()
    );
    assert_eq!(
        actual.get_estimated_work_seconds(),
        expected.get_estimated_work_seconds()
    );
    assert_eq!(
        actual.get_actual_work_seconds(),
        expected.get_actual_work_seconds()
    );
    assert_eq!(
        actual.get_repetition_interval_days_opt(),
        expected.get_repetition_interval_days_opt()
    );
    assert_eq!(
        actual.get_repetition_anchor(),
        expected.get_repetition_anchor()
    );
    assert_eq!(actual.get_days_in_advance(), expected.get_days_in_advance());
    Ok(())
}

#[test]
fn test_project_yaml_strict_round_tripは子のpriority_category以外の全永続fieldとuuidを保つ() {
    let project = project_with_all_persisted_yaml_fields();
    let bytes = TaskRepository::serialize_project(&project).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    let docs = YamlLoader::load_from_str(text).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 28, 12, 0, 0).unwrap();

    let actual = yaml_to_task(&docs[0]["project"], now).unwrap();

    assert_yaml_persisted_task_fields(&actual, &project.root_task).unwrap();
    assert_eq!(actual.get_priority().unwrap(), 8);
    assert_eq!(
        actual.get_project_category_opt().unwrap(),
        Some(crate::entity::task::ProjectCategory::Investment)
    );
    let actual_children = actual.get_children().unwrap();
    let expected_children = project.root_task.get_children().unwrap();
    assert_eq!(actual_children.len(), 2);
    let actual_child = &actual_children[0];
    let expected_child = &expected_children[0];
    assert_yaml_persisted_task_fields(actual_child, expected_child).unwrap();
    let actual_child_attr = actual_child.get_attr().unwrap();
    assert_eq!(actual_child_attr.get_priority(), 0);
    assert_eq!(actual_child_attr.get_project_category_opt(), None);
    assert_eq!(expected_child.get_attr().unwrap().get_priority(), 99);
    assert_eq!(
        expected_child
            .get_attr()
            .unwrap()
            .get_project_category_opt(),
        Some(crate::entity::task::ProjectCategory::Consumption)
    );
    assert_yaml_persisted_task_fields(&actual_children[1], &expected_children[1]).unwrap();
}

#[test]
fn test_save_directory作成失敗を型付きerrorで返す() {
    let storage_dir = TestStorageDir::new();
    fs::write(&storage_dir.path, b"not a directory").unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let mut task_repository = TaskRepository::new(storage_dir.path_str());
    task_repository.sync_clock(now).unwrap();
    let task = crate::test_support::new_task_handle("保存失敗対象").unwrap();
    task_repository.start_new_project(task).unwrap();

    let actual = task_repository.save().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Save);
    let source = file_repository_error(&actual);
    assert_eq!(source.operation, FileRepositoryOperation::CreateDirectory);
    assert_eq!(source.path, storage_dir.path);
    assert!(source.source.raw_os_error().is_some());
}

#[cfg(unix)]
#[test]
fn test_save_project_yaml_read失敗でもatomic_writeを試す() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let mut task_repository = TaskRepository::new(storage_dir.path_str());
    task_repository.sync_clock(now).unwrap();
    let task = crate::test_support::new_task_handle("read失敗対象").unwrap();
    let task_id = task.get_id().unwrap();
    task_repository.start_new_project(task).unwrap();
    let project_yaml_path = storage_dir
        .project_dir_path("20260811", "read失敗対象", task_id)
        .join("project.yaml");
    fs::create_dir_all(&project_yaml_path).unwrap();

    let actual = task_repository.save().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Save);
    let source = storage_transaction_error(&actual);
    assert!(source.to_string().contains("RenameLiveTarget"));
    assert!(source
        .to_string()
        .contains(&project_yaml_path.display().to_string()));
    assert!(project_yaml_path.is_dir());
}

#[test]
fn test_load_存在しない保存先はtraverse_errorを返す() {
    let storage_dir = TestStorageDir::new();
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let actual = repository.load().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    let source = file_repository_error(&actual);
    assert_eq!(source.operation, FileRepositoryOperation::TraverseDirectory);
    assert_eq!(source.path, storage_dir.path);
}

#[test]
fn test_load_壊れたyamlと不完全なdocumentをparse_errorにする() {
    for (directory_name, contents) in [
        ("broken", "project: ["),
        ("empty", ""),
        ("missing-project", "other: {}"),
    ] {
        let storage_dir = TestStorageDir::new();
        let project_yaml_file_path = write_project_yaml(&storage_dir, directory_name, contents);
        let mut repository = TaskRepository::new(storage_dir.path_str());

        let actual = repository.load().unwrap_err();

        assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
        let source = file_repository_error(&actual);
        assert_eq!(source.operation, FileRepositoryOperation::ParseProject);
        assert_eq!(source.path, project_yaml_file_path);
        assert_eq!(source.source.kind(), std::io::ErrorKind::InvalidData);
    }
}

#[test]
fn test_load_yaml変換errorをsource_chainに保持する() {
    let storage_dir = TestStorageDir::new();
    let project_yaml_file_path = write_project_yaml(
        &storage_dir,
        "invalid-children",
        "project:\n  name: broken\n  children: not-an-array\n",
    );
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let actual = repository.load().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    let file_error = file_repository_error(&actual);
    assert_eq!(file_error.operation, FileRepositoryOperation::ParseProject);
    assert_eq!(file_error.path, project_yaml_file_path);
    let conversion_error = file_error
        .source
        .get_ref()
        .and_then(|source| source.downcast_ref::<YamlConversionError>())
        .expect("io error source must be YamlConversionError");
    assert_eq!(
        conversion_error.to_string(),
        "cannot convert project YAML to task: project.children: must be an array or null"
    );
}

#[cfg(unix)]
#[test]
fn test_load_read失敗を型付きerrorにする() {
    let storage_dir = TestStorageDir::new();
    let project_yaml_file_path = storage_dir.path.join("unreadable/project.yaml");
    fs::create_dir_all(&project_yaml_file_path).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let actual = repository.load().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    let source = file_repository_error(&actual);
    assert_eq!(source.operation, FileRepositoryOperation::ReadFile);
    assert_eq!(source.path, project_yaml_file_path);
}

#[cfg(unix)]
#[test]
fn test_load_open失敗を型付きerrorにする() {
    use std::os::unix::fs::symlink;

    let storage_dir = TestStorageDir::new();
    let project_dir_path = storage_dir.path.join("unopenable");
    fs::create_dir_all(&project_dir_path).unwrap();
    let project_yaml_file_path = project_dir_path.join("project.yaml");
    symlink(
        project_dir_path.join("missing.yaml"),
        &project_yaml_file_path,
    )
    .unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let actual = repository.load().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    let source = file_repository_error(&actual);
    assert_eq!(source.operation, FileRepositoryOperation::OpenFile);
    assert_eq!(source.path, project_yaml_file_path);
}

#[cfg(unix)]
#[test]
fn test_open_project_fileはcanonical_identityとopened_fileを同じtargetへ固定する() {
    use std::os::unix::fs::symlink;

    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    let first_target = storage_dir.path.join("first.yaml");
    let second_target = storage_dir.path.join("second.yaml");
    fs::write(&first_target, "first").unwrap();
    fs::write(&second_target, "second").unwrap();
    let project_yaml_path = storage_dir.path.join("project.yaml");
    symlink(&first_target, &project_yaml_path).unwrap();

    let (mut opened_file, canonical_path) = open_project_file(&project_yaml_path).unwrap();
    fs::remove_file(&project_yaml_path).unwrap();
    symlink(&second_target, &project_yaml_path).unwrap();
    let mut contents = String::new();
    opened_file.read_to_string(&mut contents).unwrap();

    assert_eq!(canonical_path, fs::canonicalize(first_target).unwrap());
    assert_eq!(contents, "first");
}

#[cfg(unix)]
#[test]
fn test_load_canonical_pathが重複するprojectを拒否してmemoryを変更しない() {
    use std::os::unix::fs::symlink;

    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let stored_task = project_root_with_identity("保存済み", Uuid::from_u128(0x2201), now);
    let stored_project = Project::new(stored_task, "", "", 5);
    let shared_yaml_path = storage_dir.path.join("shared.yaml");
    fs::write(
        &shared_yaml_path,
        TaskRepository::serialize_project(&stored_project).unwrap(),
    )
    .unwrap();
    let first_project_yaml_path = storage_dir.path.join("alias-a/project.yaml");
    let second_project_yaml_path = storage_dir.path.join("alias-b/project.yaml");
    fs::create_dir_all(first_project_yaml_path.parent().unwrap()).unwrap();
    fs::create_dir_all(second_project_yaml_path.parent().unwrap()).unwrap();
    symlink(&shared_yaml_path, &first_project_yaml_path).unwrap();
    symlink(&shared_yaml_path, &second_project_yaml_path).unwrap();

    let memory_task_id = Uuid::from_u128(0x2202);
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository
        .start_new_project(project_root_with_identity(
            "memory project",
            memory_task_id,
            now,
        ))
        .unwrap();
    let original_revision = Uuid::from_u128(0x2203);
    repository.storage_revision.set(Some(original_revision));

    let actual = repository.load().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    let source = file_repository_error(&actual);
    assert_eq!(source.operation, FileRepositoryOperation::ParseProject);
    assert_eq!(source.path, second_project_yaml_path);
    assert_eq!(source.source.kind(), std::io::ErrorKind::InvalidData);
    let message = source.source.to_string();
    assert!(message.contains(first_project_yaml_path.to_str().unwrap()));
    assert!(message.contains(second_project_yaml_path.to_str().unwrap()));
    assert_eq!(repository.get_all_projects().len(), 1);
    assert!(repository.get_by_id(memory_task_id).unwrap().is_some());
    assert_eq!(repository.storage_revision.get(), Some(original_revision));
}

#[test]
fn test_load_途中失敗ではmemoryを部分更新しない() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let mut source_repository = TaskRepository::new(storage_dir.path_str());
    source_repository.sync_clock(now).unwrap();
    let stored_task = crate::test_support::new_task_handle("保存済みtask").unwrap();
    let stored_task_id = stored_task.get_id().unwrap();
    source_repository.start_new_project(stored_task).unwrap();
    source_repository.save().unwrap();
    write_project_yaml(&storage_dir, "zz-broken", "project: [");

    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    let memory_task = crate::test_support::new_task_handle("memory task").unwrap();
    let memory_task_id = memory_task.get_id().unwrap();
    repository.start_new_project(memory_task).unwrap();

    assert!(repository.load().is_err());
    assert!(repository.get_by_id(memory_task_id).unwrap().is_some());
    assert!(repository.get_by_id(stored_task_id).unwrap().is_none());
    assert_eq!(repository.get_all_projects().len(), 1);
}

#[test]
fn test_load_同一tree内の重複uuidを両方のpath付きerrorで拒否する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let duplicate_id = Uuid::from_u128(0x2211);
    let root_task = project_root_with_identity("root", duplicate_id, now);
    root_task.create_as_last_child(crate::entity::task::TaskAttr::with_identity(
        "duplicate child",
        duplicate_id,
        now,
    ));
    let project = Project::new(root_task, "", "", 5);
    let project_yaml_file_path = write_project_yaml(
        &storage_dir,
        "duplicate-tree",
        std::str::from_utf8(&TaskRepository::serialize_project(&project).unwrap()).unwrap(),
    );
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let actual = repository.load().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    let source = duplicate_task_id_error(&actual);
    assert_eq!(source.task_id(), duplicate_id);
    assert_eq!(
        source.first_project_yaml_file_path(),
        project_yaml_file_path
    );
    assert_eq!(source.first_task_path(), "project");
    assert_eq!(
        source.duplicate_project_yaml_file_path(),
        source.first_project_yaml_file_path()
    );
    assert_eq!(source.duplicate_task_path(), "project.children[0]");
}

#[test]
fn test_reload_if_changed初回loadの重複uuid失敗時は未読込状態を維持する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let duplicate_id = Uuid::from_u128(0x2212);
    for (directory_name, task_name) in [("first", "first"), ("second", "second")] {
        let project = Project::new(
            project_root_with_identity(task_name, duplicate_id, now),
            "",
            "",
            5,
        );
        write_project_yaml(
            &storage_dir,
            directory_name,
            std::str::from_utf8(&TaskRepository::serialize_project(&project).unwrap()).unwrap(),
        );
    }
    let changed_revision = Uuid::from_u128(0x2213);
    fs::write(
        storage_dir.path.join(".revision"),
        changed_revision.to_string(),
    )
    .unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());
    let original_clock = repository.get_last_synced_time();

    let actual = repository.reload_if_changed(now).unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    assert_eq!(duplicate_task_id_error(&actual).task_id(), duplicate_id);
    assert!(!repository.has_loaded);
    assert!(repository.projects.is_empty());
    assert!(repository.id_to_task_map.borrow().is_empty());
    assert_eq!(repository.storage_revision.get(), None);
    assert_eq!(repository.get_last_synced_time(), original_clock);
}

#[test]
fn test_reload_if_changed_project間の重複uuidを拒否して全memory状態を維持する() {
    let storage_dir = TestStorageDir::new();
    let before = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let after = before + Duration::hours(1);
    let memory_task_id = Uuid::from_u128(0x2221);
    let duplicate_id = Uuid::from_u128(0x2222);
    let mut source_repository = TaskRepository::new(storage_dir.path_str());
    source_repository.sync_clock(before).unwrap();
    let memory_root = project_root_with_identity("memory project", memory_task_id, before);
    let memory_child =
        memory_root.create_as_last_child(crate::entity::task::TaskAttr::with_identity(
            "memory child",
            Uuid::from_u128(0x2224),
            before,
        ));
    source_repository.start_new_project(memory_root).unwrap();
    source_repository.save().unwrap();

    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.reload_if_changed(before).unwrap();
    let memory_task = repository.get_by_id(memory_task_id).unwrap().unwrap();
    memory_task.set_actual_work_seconds(17).unwrap();
    let original_project_count = repository.projects.len();
    let original_cache_ids = repository
        .id_to_task_map
        .borrow()
        .keys()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let original_storage_revision = repository.storage_revision.get();
    let original_has_loaded = repository.has_loaded;
    let original_repository_clock = repository.get_last_synced_time();
    let original_task_clock = memory_task.get_last_synced_time().unwrap();
    let memory_child_id = memory_child.get_id().unwrap();
    let loaded_memory_child = repository.get_by_id(memory_child_id).unwrap().unwrap();
    let original_child_clock = loaded_memory_child.get_last_synced_time().unwrap();

    let first_path = write_project_yaml(
        &storage_dir,
        "zz-first-duplicate",
        std::str::from_utf8(
            &TaskRepository::serialize_project(&Project::new(
                project_root_with_identity("first", duplicate_id, after),
                "",
                "",
                5,
            ))
            .unwrap(),
        )
        .unwrap(),
    );
    let second_path = write_project_yaml(
        &storage_dir,
        "zz-second-duplicate",
        std::str::from_utf8(
            &TaskRepository::serialize_project(&Project::new(
                project_root_with_identity("second", duplicate_id, after),
                "",
                "",
                5,
            ))
            .unwrap(),
        )
        .unwrap(),
    );
    let changed_revision = Uuid::from_u128(0x2223);
    fs::write(
        storage_dir.path.join(".revision"),
        changed_revision.to_string(),
    )
    .unwrap();

    let actual = repository.reload_if_changed(after).unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    let source = duplicate_task_id_error(&actual);
    assert_eq!(source.task_id(), duplicate_id);
    assert_eq!(source.first_project_yaml_file_path(), first_path);
    assert_eq!(source.first_task_path(), "project");
    assert_eq!(source.duplicate_project_yaml_file_path(), second_path);
    assert_eq!(source.duplicate_task_path(), "project");
    assert_eq!(repository.projects.len(), original_project_count);
    assert_eq!(
        repository
            .id_to_task_map
            .borrow()
            .keys()
            .copied()
            .collect::<std::collections::HashSet<_>>(),
        original_cache_ids
    );
    assert_eq!(repository.storage_revision.get(), original_storage_revision);
    assert_eq!(repository.has_loaded, original_has_loaded);
    assert_eq!(repository.get_last_synced_time(), original_repository_clock);
    assert_eq!(
        memory_task.get_last_synced_time().unwrap(),
        original_task_clock
    );
    assert_eq!(
        loaded_memory_child.get_last_synced_time().unwrap(),
        original_child_clock
    );
    assert!(repository.get_by_id(memory_task_id).unwrap().is_some());
    assert_eq!(
        repository
            .get_by_id(memory_task_id)
            .unwrap()
            .unwrap()
            .get_actual_work_seconds()
            .unwrap(),
        17
    );
    assert_eq!(
        repository.get_all_projects()[0]
            .get_actual_work_seconds()
            .unwrap(),
        17
    );
    assert!(repository.get_by_id(duplicate_id).unwrap().is_none());
}

#[cfg(unix)]
#[test]
fn test_save_変更したprojectだけを置換する() {
    use std::os::unix::fs::MetadataExt;

    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    let changed_task = crate::test_support::new_task_handle("変更対象").unwrap();
    let unchanged_task = crate::test_support::new_task_handle("未変更対象").unwrap();
    let changed_task_id = changed_task.get_id().unwrap();
    let unchanged_task_id = unchanged_task.get_id().unwrap();
    repository.start_new_project(changed_task.clone()).unwrap();
    repository.start_new_project(unchanged_task).unwrap();
    repository.save().unwrap();

    let changed_yaml_path = storage_dir
        .project_dir_path("20260811", "変更対象", changed_task_id)
        .join("project.yaml");
    let unchanged_yaml_path = storage_dir
        .project_dir_path("20260811", "未変更対象", unchanged_task_id)
        .join("project.yaml");
    let changed_inode = fs::metadata(&changed_yaml_path).unwrap().ino();
    let unchanged_inode = fs::metadata(&unchanged_yaml_path).unwrap().ino();

    changed_task.set_estimated_work_seconds(30 * 60).unwrap();
    repository.save().unwrap();

    assert_ne!(
        fs::metadata(&changed_yaml_path).unwrap().ino(),
        changed_inode
    );
    assert_eq!(
        fs::metadata(&unchanged_yaml_path).unwrap().ino(),
        unchanged_inode
    );
    let mut loaded_repository = TaskRepository::new(storage_dir.path_str());
    loaded_repository.sync_clock(now).unwrap();
    loaded_repository.load().unwrap();
    assert_eq!(
        loaded_repository
            .get_by_id(changed_task.get_id().unwrap())
            .unwrap()
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        30 * 60
    );
}

#[test]
fn test_save_未変更projectはserialize比較対象にしない() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    let changed_task = crate::test_support::new_task_handle("変更対象").unwrap();
    let unchanged_task = crate::test_support::new_task_handle("未変更対象").unwrap();
    let unchanged_task_id = unchanged_task.get_id().unwrap();
    repository.start_new_project(changed_task.clone()).unwrap();
    repository.start_new_project(unchanged_task).unwrap();
    repository.save().unwrap();
    let unchanged_dir = storage_dir.project_dir_path("20260813", "未変更対象", unchanged_task_id);
    fs::remove_dir_all(&unchanged_dir).unwrap();

    changed_task.set_estimated_work_seconds(30 * 60).unwrap();
    repository.save().unwrap();

    assert!(!unchanged_dir.exists());
}

#[test]
fn test_save_load直後のprojectはcleanで新規projectだけを保存する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let mut source = TaskRepository::new(storage_dir.path_str());
    source.sync_clock(now).unwrap();
    let loaded_task = crate::test_support::new_task_handle("読込済み").unwrap();
    let loaded_task_id = loaded_task.get_id().unwrap();
    source.start_new_project(loaded_task).unwrap();
    source.save().unwrap();

    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository.load().unwrap();
    let loaded_dir = storage_dir.project_dir_path("20260813", "読込済み", loaded_task_id);
    fs::remove_dir_all(&loaded_dir).unwrap();
    let new_task = crate::test_support::new_task_handle("新規").unwrap();
    let new_task_id = new_task.get_id().unwrap();
    repository.start_new_project(new_task).unwrap();

    repository.save().unwrap();

    assert!(!loaded_dir.exists());
    assert!(storage_dir
        .project_dir_path("20260813", "新規", new_task_id)
        .join("project.yaml")
        .is_file());
}

#[test]
fn test_save_失敗後もdirtyを維持して再試行する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    let task = crate::test_support::new_task_handle("再試行対象").unwrap();
    let task_id = task.get_id().unwrap();
    repository.start_new_project(task.clone()).unwrap();
    repository.save().unwrap();
    task.set_estimated_work_seconds(30 * 60).unwrap();
    repository.storage_transaction_io = Arc::new(FailingStorageTransactionIo);

    assert!(repository.save().is_err());
    repository.storage_transaction_io = Arc::new(FileSystemStorageTransactionIo);
    repository.save().unwrap();

    let mut reloaded = TaskRepository::new(storage_dir.path_str());
    reloaded.sync_clock(now).unwrap();
    reloaded.load().unwrap();
    assert_eq!(
        reloaded
            .get_by_id(task_id)
            .unwrap()
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        30 * 60
    );
}

#[test]
fn test_load_revisionなしの既存storageを読める() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let mut source = TaskRepository::new(storage_dir.path_str());
    source.sync_clock(now).unwrap();
    source
        .start_new_project(crate::test_support::new_task_handle("既存project").unwrap())
        .unwrap();
    source.save().unwrap();
    let revision_path = storage_dir.path.join(".revision");
    fs::remove_file(&revision_path).unwrap();

    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository.load().unwrap();

    assert_eq!(repository.get_all_projects().len(), 1);
    assert_eq!(repository.storage_revision.get(), None);
}

#[test]
fn test_save_actual_writeだけがrevisionを更新する() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let revision_path = storage_dir.path.join(".revision");
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository
        .start_new_project(crate::test_support::new_task_handle("保存対象").unwrap())
        .unwrap();

    repository.save().unwrap();

    let first_text = fs::read_to_string(&revision_path).unwrap();
    let first_revision = Uuid::parse_str(first_text.trim()).unwrap();
    assert_eq!(repository.storage_revision.get(), Some(first_revision));

    repository.save().unwrap();

    assert_eq!(fs::read_to_string(&revision_path).unwrap(), first_text);
    assert_eq!(repository.storage_revision.get(), Some(first_revision));
}

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
    repository.storage_transaction_io = Arc::new(FailingStorageTransactionIo);
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
    repository.storage_transaction_io = Arc::new(FailingStorageTransactionIo);
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
    repository.storage_transaction_io = Arc::new(FailFirstCommittedProjectWriteIo {
        failed: AtomicBool::new(false),
    });
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
        CommittedCrashPhase::BeforeFirstProjectRename,
        CommittedCrashPhase::AfterFirstProjectRename,
        CommittedCrashPhase::BeforeFinalProjectRename,
        CommittedCrashPhase::BeforeRevision,
        CommittedCrashPhase::AfterRevisionBeforeCleanup,
    ] {
        let storage_dir = TestStorageDir::new();
        let now = Local.with_ymd_and_hms(2026, 9, 5, 13, 0, 0).unwrap();
        let (first_id, second_id, committed_revision, active_transaction_path) =
            create_committed_transaction_interruption(&storage_dir, now, phase);
        if matches!(phase, CommittedCrashPhase::BeforeFirstProjectRename) {
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
    cached.storage_transaction_io = Arc::new(CommittedCrashIo::new(
        CommittedCrashPhase::AfterFirstProjectRename,
    ));
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
            CommittedCrashPhase::BeforeFirstProjectRename,
        );
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository.storage_transaction_io = Arc::new(CommittedCrashIo::new(
        CommittedCrashPhase::AfterFirstProjectRename,
    ));

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
        CommittedCrashPhase::BeforeRevision,
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
                CommittedCrashPhase::BeforeFirstProjectRename,
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
            CommittedCrashPhase::BeforeFirstProjectRename,
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
            CommittedCrashPhase::BeforeFirstProjectRename,
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
            CommittedCrashPhase::BeforeFirstProjectRename,
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
            CommittedCrashPhase::BeforeFirstProjectRename,
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
                CommittedCrashPhase::BeforeFirstProjectRename,
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
    source.storage_transaction_io = Arc::new(FailCleanupDeleteIo);

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
    repository.storage_transaction_io = Arc::new(FailUncommittedDiscardIo);

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

#[test]
fn test_load_malformed_revisionをphase付きerrorにする() {
    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    fs::write(storage_dir.path.join(".revision"), "not-a-uuid\n").unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let actual = repository.load().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    let source = file_repository_error(&actual);
    assert_eq!(source.operation, FileRepositoryOperation::ParseRevision);
    assert_eq!(source.path, storage_dir.path.join(".revision"));
    assert!(actual.to_string().contains("invalid character"));
    assert!(source
        .source
        .get_ref()
        .and_then(|error| error.downcast_ref::<uuid::Error>())
        .is_some());
}

#[test]
fn test_load_revision読込io_errorにpathとsourceを保持する() {
    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(storage_dir.path.join(".revision")).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let actual = repository.load().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    let source = file_repository_error(&actual);
    assert_eq!(source.operation, FileRepositoryOperation::ReadFile);
    assert_eq!(source.path, storage_dir.path.join(".revision"));
    assert!(source.source.raw_os_error().is_some());
}

#[cfg(unix)]
#[test]
fn test_load_revision_symlinkを拒否して参照先を変更しない() {
    use std::os::unix::fs::symlink;

    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    let target_path = storage_dir.path.join("outside-revision");
    let target_content = format!("{}\n", Uuid::new_v4());
    fs::write(&target_path, &target_content).unwrap();
    let revision_path = storage_dir.path.join(".revision");
    symlink(&target_path, &revision_path).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let actual = repository.load().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Load);
    let source = file_repository_error(&actual);
    assert_eq!(source.operation, FileRepositoryOperation::ReadMetadata);
    assert_eq!(source.path, revision_path);
    assert_eq!(fs::read_to_string(target_path).unwrap(), target_content);
}

#[cfg(unix)]
#[test]
fn test_save_revision_symlinkを拒否して参照先を変更しない() {
    use std::os::unix::fs::symlink;

    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    let target_path = storage_dir.path.join("outside-revision");
    let target_content = format!("{}\n", Uuid::new_v4());
    fs::write(&target_path, &target_content).unwrap();
    let revision_path = storage_dir.path.join(".revision");
    symlink(&target_path, &revision_path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    let task = crate::test_support::new_task_handle("保存対象").unwrap();
    let task_id = task.get_id().unwrap();
    repository.start_new_project(task).unwrap();

    let actual = repository.save().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Save);
    let source = file_repository_error(&actual);
    assert_eq!(source.operation, FileRepositoryOperation::ReadMetadata);
    assert_eq!(source.path, revision_path);
    assert_eq!(fs::read_to_string(target_path).unwrap(), target_content);
    assert!(!storage_dir
        .project_dir_path("20260813", "保存対象", task_id)
        .join("project.yaml")
        .exists());
}

#[test]
fn test_reload_if_changed初回はrevisionなしでも必ずloadする() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let mut source = TaskRepository::new(storage_dir.path_str());
    source.sync_clock(now).unwrap();
    let task = crate::test_support::new_task_handle("初回読込対象").unwrap();
    let task_id = task.get_id().unwrap();
    source.start_new_project(task).unwrap();
    source.save().unwrap();
    fs::remove_file(storage_dir.path.join(".revision")).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path_str());

    let outcome = repository.reload_if_changed(now).unwrap();

    assert_eq!(outcome, RepositoryReloadOutcome::Reloaded);
    assert!(repository.get_by_id(task_id).unwrap().is_some());
}

#[test]
fn test_reload_if_changed_revision一致ならyamlを再読込せずclock同期する() {
    let storage_dir = TestStorageDir::new();
    let before = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let after = before + Duration::hours(2);
    let mut source = TaskRepository::new(storage_dir.path_str());
    source.sync_clock(before).unwrap();
    let task = crate::test_support::new_task_handle("cache対象").unwrap();
    task.set_start_time(before - Duration::hours(1)).unwrap();
    task.set_pending_until(before + Duration::hours(1)).unwrap();
    task.set_orig_status(Status::Pending).unwrap();
    let task_id = task.get_id().unwrap();
    source.start_new_project(task).unwrap();
    source.save().unwrap();
    let project_yaml_path = storage_dir
        .project_dir_path("20260813", "cache対象", task_id)
        .join("project.yaml");
    let mut repository = TaskRepository::new(storage_dir.path_str());
    assert_eq!(
        repository.reload_if_changed(before).unwrap(),
        RepositoryReloadOutcome::Reloaded
    );
    assert_eq!(
        repository
            .get_by_id(task_id)
            .unwrap()
            .unwrap()
            .get_status()
            .unwrap(),
        Status::Pending
    );
    fs::write(&project_yaml_path, "project: [").unwrap();

    let outcome = repository.reload_if_changed(after).unwrap();

    assert_eq!(outcome, RepositoryReloadOutcome::Cached);
    assert_eq!(
        repository
            .get_by_id(task_id)
            .unwrap()
            .unwrap()
            .get_status()
            .unwrap(),
        Status::Todo
    );
}

#[test]
fn test_reload_if_changed外部save後だけ1回reloadする() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let mut source = TaskRepository::new(storage_dir.path_str());
    source.sync_clock(now).unwrap();
    let task = crate::test_support::new_task_handle("外部更新対象").unwrap();
    let task_id = task.get_id().unwrap();
    source.start_new_project(task).unwrap();
    source.save().unwrap();
    let mut cached = TaskRepository::new(storage_dir.path_str());
    cached.reload_if_changed(now).unwrap();
    let mut external = TaskRepository::new(storage_dir.path_str());
    external.reload_if_changed(now).unwrap();
    external
        .get_by_id(task_id)
        .unwrap()
        .unwrap()
        .set_estimated_work_seconds(45 * 60)
        .unwrap();
    external.save().unwrap();

    assert_eq!(
        cached.reload_if_changed(now).unwrap(),
        RepositoryReloadOutcome::Reloaded
    );
    assert_eq!(
        cached
            .get_by_id(task_id)
            .unwrap()
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        45 * 60
    );
    assert_eq!(
        cached.reload_if_changed(now).unwrap(),
        RepositoryReloadOutcome::Cached
    );
}

#[test]
fn test_reload_if_changed新processはrevision一致でも停止中の直接編集をloadする() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let mut source = TaskRepository::new(storage_dir.path_str());
    source.sync_clock(now).unwrap();
    let task = crate::test_support::new_task_handle("停止中編集対象").unwrap();
    let task_id = task.get_id().unwrap();
    source.start_new_project(task).unwrap();
    source.save().unwrap();
    let original_revision = fs::read(storage_dir.path.join(".revision")).unwrap();
    let original_revision_id =
        Uuid::parse_str(std::str::from_utf8(&original_revision).unwrap().trim_end()).unwrap();
    source
        .get_by_id(task_id)
        .unwrap()
        .unwrap()
        .set_estimated_work_seconds(50 * 60)
        .unwrap();
    source.save().unwrap();
    fs::write(storage_dir.path.join(".revision"), original_revision).unwrap();
    let mut restarted = TaskRepository::new(storage_dir.path_str());
    restarted.storage_revision.set(Some(original_revision_id));

    let outcome = restarted.reload_if_changed(now).unwrap();

    assert_eq!(outcome, RepositoryReloadOutcome::Reloaded);
    assert_eq!(
        restarted
            .get_by_id(task_id)
            .unwrap()
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        50 * 60
    );
}

#[test]
#[ignore = "manual save performance measurement"]
fn benchmark_save_2172project中1件変更を2秒未満で処理する() {
    use std::time::{Duration as StdDuration, Instant};

    let source_storage_dir = std::env::var("SCHRONU_BENCHMARK_STORAGE")
        .expect("SCHRONU_BENCHMARK_STORAGE must point to a task storage copy source");
    let storage_dir = TestStorageDir::new();
    let source_storage_path = Path::new(&source_storage_dir);
    for entry in WalkDir::new(source_storage_path) {
        let entry = entry.unwrap();
        if entry.file_name() != "project.yaml" {
            continue;
        }
        let relative_path = entry.path().strip_prefix(source_storage_path).unwrap();
        let copied_path = storage_dir.path.join(relative_path);
        fs::create_dir_all(copied_path.parent().unwrap()).unwrap();
        fs::create_dir_all(copied_path.parent().unwrap().join("markdown")).unwrap();
        fs::copy(entry.path(), copied_path).unwrap();
    }

    let now = Local::now();
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository.load().unwrap();
    assert_eq!(repository.get_all_projects().len(), 2_172);
    let changed_task = (*repository
        .get_all_projects()
        .first()
        .expect("benchmark storage must contain a project"))
    .clone();
    changed_task
        .set_priority(changed_task.get_priority().unwrap() + 1)
        .unwrap();

    let started_at = Instant::now();
    repository.save().unwrap();
    let elapsed = started_at.elapsed();

    eprintln!("save benchmark elapsed: {elapsed:?}");
    assert!(elapsed < StdDuration::from_secs(2));
}

#[test]
fn test_get_by_id_キャッシュから取得する() {
    let mut task_repository = TaskRepository::new("");
    let root_task = crate::test_support::new_task_handle("親タスク").unwrap();
    let child_task = root_task.create_as_last_child(crate::test_support::new_task_attr("子タスク"));
    let child_task_id = child_task.get_id().unwrap();

    task_repository
        .cache_task_and_descendants(&root_task)
        .unwrap();
    task_repository
        .projects
        .push(Project::new(root_task, "".to_string(), "".to_string(), 5));

    let actual = task_repository.get_by_id(child_task_id).unwrap().unwrap();

    assert_eq!(actual.get_name().unwrap(), "子タスク");
    assert!(task_repository
        .id_to_task_map
        .borrow()
        .contains_key(&child_task_id));
}

#[test]
fn test_get_by_id_実行中に追加された子タスクを検索してキャッシュする() {
    let mut task_repository = TaskRepository::new("");
    let root_task = crate::test_support::new_task_handle("親タスク").unwrap();

    task_repository
        .cache_task_and_descendants(&root_task)
        .unwrap();
    task_repository.projects.push(Project::new(
        root_task.clone(),
        "".to_string(),
        "".to_string(),
        5,
    ));

    let child_task = root_task.create_as_last_child(crate::test_support::new_task_attr("子タスク"));
    let child_task_id = child_task.get_id().unwrap();
    assert!(!task_repository
        .id_to_task_map
        .borrow()
        .contains_key(&child_task_id));

    let actual = task_repository.get_by_id(child_task_id).unwrap().unwrap();

    assert_eq!(actual.get_name().unwrap(), "子タスク");
    assert!(task_repository
        .id_to_task_map
        .borrow()
        .contains_key(&child_task_id));
}

#[test]
fn test_get_by_id_未知のidならnoneを返す() {
    let mut task_repository = TaskRepository::new("");
    let root_task = crate::test_support::new_task_handle("親タスク").unwrap();
    task_repository
        .cache_task_and_descendants(&root_task)
        .unwrap();
    add_project(&mut task_repository, root_task);

    let actual = task_repository.get_by_id(Uuid::new_v4());

    assert_eq!(actual.unwrap(), None);
}

#[test]
fn test_get_highest_priority_leaf_task_id_締切なし同士では優先度が高いタスクを選ぶ() {
    let mut task_repository = TaskRepository::new("");
    let low_priority_task = crate::test_support::new_task_handle("低優先度タスク").unwrap();
    low_priority_task.set_priority(1).unwrap();
    let high_priority_task = crate::test_support::new_task_handle("高優先度タスク").unwrap();
    high_priority_task.set_priority(9).unwrap();
    let high_priority_task_id = high_priority_task.get_id().unwrap();

    add_project(&mut task_repository, high_priority_task);
    add_project(&mut task_repository, low_priority_task);

    let actual = task_repository.get_highest_priority_leaf_task_id(&[]);

    assert_eq!(actual.unwrap(), Some(high_priority_task_id));
}

#[test]
fn test_get_highest_priority_leaf_task_id_伏せたtaskを除外して次候補を返す() {
    let mut task_repository = TaskRepository::new("");
    let lower = crate::test_support::new_task_handle("次候補").unwrap();
    lower.set_priority(1).unwrap();
    let tucked = crate::test_support::new_task_handle("伏せる最優先候補").unwrap();
    tucked.set_priority(9).unwrap();
    let lower_id = lower.get_id().unwrap();
    let tucked_id = tucked.get_id().unwrap();

    add_project(&mut task_repository, tucked);
    add_project(&mut task_repository, lower);

    let actual = task_repository.get_highest_priority_leaf_task_id(&[tucked_id]);

    assert_eq!(actual.unwrap(), Some(lower_id));
}

#[test]
fn test_get_highest_priority_leaf_task_id_pending中のタスクは選ばない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let mut task_repository = TaskRepository::new("");
    task_repository.sync_clock(now).unwrap();

    let active_task = crate::test_support::new_task_handle("着手可能タスク").unwrap();
    active_task.set_priority(1).unwrap();
    let active_task_id = active_task.get_id().unwrap();

    let pending_task = pending_task_with_until("Pendingタスク", now + Duration::days(1));
    pending_task.set_priority(99).unwrap();

    add_project(&mut task_repository, active_task);
    add_project(&mut task_repository, pending_task);

    let actual = task_repository.get_highest_priority_leaf_task_id(&[]);

    assert_eq!(actual.unwrap(), Some(active_task_id));
}

#[test]
fn test_get_highest_priority_leaf_task_id_締切あり同士では優先度より締切日時を先に見る() {
    let mut task_repository = TaskRepository::new("");
    let high_priority_late_deadline_task =
        crate::test_support::new_task_handle("高優先度だが締切が遅いタスク").unwrap();
    high_priority_late_deadline_task.set_priority(99).unwrap();
    high_priority_late_deadline_task
        .set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 5, 11, 20, 0, 0).unwrap()))
        .unwrap();

    let low_priority_early_deadline_task =
        crate::test_support::new_task_handle("低優先度だが締切が早いタスク").unwrap();
    low_priority_early_deadline_task.set_priority(1).unwrap();
    low_priority_early_deadline_task
        .set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 5, 10, 20, 0, 0).unwrap()))
        .unwrap();
    let low_priority_early_deadline_task_id = low_priority_early_deadline_task.get_id().unwrap();

    add_project(&mut task_repository, high_priority_late_deadline_task);
    add_project(&mut task_repository, low_priority_early_deadline_task);

    let actual = task_repository.get_highest_priority_leaf_task_id(&[]);

    assert_eq!(actual.unwrap(), Some(low_priority_early_deadline_task_id));
}

#[test]
fn test_get_defer_candidate_leaf_task_id_完成閾値より前だけrecent扱いする() {
    let mut task_repository = TaskRepository::new("");
    task_repository
        .sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap())
        .unwrap();
    let recent_task = task_with_start_time(
        "閾値より前",
        Local.with_ymd_and_hms(2026, 5, 11, 5, 59, 59).unwrap(),
    );
    let boundary_task = task_with_start_time(
        "閾値ちょうど",
        Local.with_ymd_and_hms(2026, 5, 11, 6, 0, 0).unwrap(),
    );
    let recent_task_id = recent_task.get_id().unwrap();

    add_project(&mut task_repository, boundary_task);
    add_project(&mut task_repository, recent_task);

    let recent_threshold = Local.with_ymd_and_hms(2026, 5, 11, 6, 0, 0).unwrap();
    let actual = task_repository.get_defer_candidate_leaf_task_id(recent_threshold, &[]);

    assert_eq!(actual.unwrap(), Some(recent_task_id));
}

#[test]
fn test_get_defer_candidate_leaf_task_id_伏せたtaskを除外して次候補を返す() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let mut task_repository = TaskRepository::new("");
    task_repository.sync_clock(now).unwrap();
    let tucked = task_with_start_time("伏せる最低優先度候補", now);
    tucked.set_priority(1).unwrap();
    let higher = task_with_start_time("次候補", now);
    higher.set_priority(9).unwrap();
    let tucked_id = tucked.get_id().unwrap();
    let higher_id = higher.get_id().unwrap();

    add_project(&mut task_repository, tucked);
    add_project(&mut task_repository, higher);

    let recent_threshold = Local.with_ymd_and_hms(2026, 5, 11, 6, 0, 0).unwrap();
    let actual = task_repository.get_defer_candidate_leaf_task_id(recent_threshold, &[tucked_id]);

    assert_eq!(actual.unwrap(), Some(higher_id));
}

#[test]
fn test_get_defer_candidate_leaf_task_id_10日後相当の完成閾値を使用する() {
    let mut task_repository = TaskRepository::new("");
    task_repository
        .sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap())
        .unwrap();
    let recent_task = task_with_start_time(
        "10日指定でrecent",
        Local.with_ymd_and_hms(2026, 5, 21, 5, 59, 59).unwrap(),
    );
    let boundary_task = task_with_start_time(
        "10日指定の閾値ちょうど",
        Local.with_ymd_and_hms(2026, 5, 21, 6, 0, 0).unwrap(),
    );
    let recent_task_id = recent_task.get_id().unwrap();

    add_project(&mut task_repository, boundary_task);
    add_project(&mut task_repository, recent_task);

    let recent_threshold = Local.with_ymd_and_hms(2026, 5, 21, 6, 0, 0).unwrap();
    let actual = task_repository.get_defer_candidate_leaf_task_id(recent_threshold, &[]);

    assert_eq!(actual.unwrap(), Some(recent_task_id));
}

#[test]
fn test_get_defer_candidate_leaf_task_id_対象範囲外までpending済みのタスクは候補から除外する() {
    let mut task_repository = TaskRepository::new("");
    task_repository
        .sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap())
        .unwrap();
    let pending_task = pending_task_with_until(
        "100日後までpending済み",
        Local.with_ymd_and_hms(2026, 8, 18, 6, 0, 0).unwrap(),
    );
    let todo_task = task_with_start_time(
        "通常のTodo",
        Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
    );
    let todo_task_id = todo_task.get_id().unwrap();

    add_project(&mut task_repository, pending_task);
    add_project(&mut task_repository, todo_task);

    let recent_threshold = Local.with_ymd_and_hms(2026, 5, 11, 6, 0, 0).unwrap();
    let actual = task_repository.get_defer_candidate_leaf_task_id(recent_threshold, &[]);

    assert_eq!(actual.unwrap(), Some(todo_task_id));
}

#[test]
fn test_get_defer_candidate_leaf_task_id_対象範囲外のstart_timeを持つタスクは候補から除外する() {
    let mut task_repository = TaskRepository::new("");
    task_repository
        .sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap())
        .unwrap();
    let future_task = task_with_start_time(
        "遠い未来に開始するTodo",
        Local.with_ymd_and_hms(2026, 12, 19, 6, 0, 0).unwrap(),
    );
    let todo_task = task_with_start_time(
        "通常のTodo",
        Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
    );
    let todo_task_id = todo_task.get_id().unwrap();

    add_project(&mut task_repository, future_task);
    add_project(&mut task_repository, todo_task);

    let recent_threshold = Local.with_ymd_and_hms(2026, 5, 11, 6, 0, 0).unwrap();
    let actual = task_repository.get_defer_candidate_leaf_task_id(recent_threshold, &[]);

    assert_eq!(actual.unwrap(), Some(todo_task_id));
}

#[test]
fn test_get_defer_candidate_leaf_task_id_対象範囲外までpending済みのタスクしかなければnoneを返す() {
    let mut task_repository = TaskRepository::new("");
    task_repository
        .sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap())
        .unwrap();
    let pending_task = pending_task_with_until(
        "100日後までpending済み",
        Local.with_ymd_and_hms(2026, 8, 18, 6, 0, 0).unwrap(),
    );

    add_project(&mut task_repository, pending_task);

    let recent_threshold = Local.with_ymd_and_hms(2026, 5, 11, 6, 0, 0).unwrap();
    let actual = task_repository.get_defer_candidate_leaf_task_id(recent_threshold, &[]);

    assert_eq!(actual.unwrap(), None);
}

#[test]
fn test_get_defer_candidate_leaf_task_id_対象範囲外のstart_timeを持つタスクしかなければnoneを返す()
{
    let mut task_repository = TaskRepository::new("");
    task_repository
        .sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap())
        .unwrap();
    let future_task = task_with_start_time(
        "遠い未来に開始するTodo",
        Local.with_ymd_and_hms(2026, 12, 19, 6, 0, 0).unwrap(),
    );

    add_project(&mut task_repository, future_task);

    let recent_threshold = Local.with_ymd_and_hms(2026, 5, 11, 6, 0, 0).unwrap();
    let actual = task_repository.get_defer_candidate_leaf_task_id(recent_threshold, &[]);

    assert_eq!(actual.unwrap(), None);
}

#[test]
fn test_get_defer_candidate_leaf_task_id_pending_untilが閾値より前なら候補に残す() {
    let mut task_repository = TaskRepository::new("");
    task_repository
        .sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap())
        .unwrap();
    let pending_task = pending_task_with_until(
        "閾値より前までpending",
        Local.with_ymd_and_hms(2026, 5, 11, 5, 59, 59).unwrap(),
    );
    let pending_task_id = pending_task.get_id().unwrap();

    add_project(&mut task_repository, pending_task);

    let recent_threshold = Local.with_ymd_and_hms(2026, 5, 11, 6, 0, 0).unwrap();
    let actual = task_repository.get_defer_candidate_leaf_task_id(recent_threshold, &[]);

    assert_eq!(actual.unwrap(), Some(pending_task_id));
}

#[test]
fn test_get_defer_candidate_leaf_task_id_pending_untilが閾値ちょうどなら候補から除外する() {
    let mut task_repository = TaskRepository::new("");
    task_repository
        .sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap())
        .unwrap();
    let pending_task = pending_task_with_until(
        "閾値ちょうどまでpending",
        Local.with_ymd_and_hms(2026, 5, 11, 6, 0, 0).unwrap(),
    );

    add_project(&mut task_repository, pending_task);

    let recent_threshold = Local.with_ymd_and_hms(2026, 5, 11, 6, 0, 0).unwrap();
    let actual = task_repository.get_defer_candidate_leaf_task_id(recent_threshold, &[]);

    assert_eq!(actual.unwrap(), None);
}
