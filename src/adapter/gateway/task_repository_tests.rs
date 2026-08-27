use super::*;
use crate::adapter::gateway::yaml::YamlConversionError;
use chrono::{Duration, TimeZone};
use std::path::PathBuf;

struct FailingAtomicSaveFile {
    write_error: bool,
    sync_error: bool,
}

impl AtomicSaveFile for FailingAtomicSaveFile {
    fn write_all(&mut self, _bytes: &[u8]) -> std::io::Result<()> {
        if self.write_error {
            Err(std::io::Error::other("test write failure"))
        } else {
            Ok(())
        }
    }

    fn sync_all(&self) -> std::io::Result<()> {
        if self.sync_error {
            Err(std::io::Error::other("test sync failure"))
        } else {
            Ok(())
        }
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

    fn project_dir_path(&self, date: &str, project_name: &str) -> PathBuf {
        self.path.join(format!("{date}-{project_name}"))
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

fn file_repository_error(error: &TaskRepositoryError) -> &FileRepositoryError {
    error
        .source()
        .and_then(|source| source.downcast_ref::<FileRepositoryError>())
        .expect("repository error source must be FileRepositoryError")
}

fn task_with_start_time(name: &str, start_time: DateTime<Local>) -> TaskHandle {
    let task = crate::test_support::new_task_handle(name).unwrap();
    task.set_start_time(start_time).unwrap();
    task.set_priority(5).unwrap();
    task
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
    let project_dir_path = storage_dir.project_dir_path("20260811", "保存対象");
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
fn test_save_directory作成失敗を型付きerrorで返す() {
    let storage_dir = TestStorageDir::new();
    fs::write(&storage_dir.path, b"not a directory").unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let mut task_repository = TaskRepository::new(storage_dir.path_str());
    task_repository.sync_clock(now).unwrap();
    task_repository
        .start_new_project(crate::test_support::new_task_handle("保存失敗対象").unwrap())
        .unwrap();
    let expected_project_dir = storage_dir.project_dir_path("20260811", "保存失敗対象");

    let actual = task_repository.save().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Save);
    let source = file_repository_error(&actual);
    assert_eq!(source.operation, FileRepositoryOperation::CreateDirectory);
    assert_eq!(source.path, expected_project_dir);
    assert!(source.source.raw_os_error().is_some());
}

#[cfg(unix)]
#[test]
fn test_save_project_yaml_read失敗でもatomic_writeを試す() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let mut task_repository = TaskRepository::new(storage_dir.path_str());
    task_repository.sync_clock(now).unwrap();
    task_repository
        .start_new_project(crate::test_support::new_task_handle("read失敗対象").unwrap())
        .unwrap();
    let project_yaml_path = storage_dir
        .project_dir_path("20260811", "read失敗対象")
        .join("project.yaml");
    fs::create_dir_all(&project_yaml_path).unwrap();

    let actual = task_repository.save().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Save);
    let source = file_repository_error(&actual);
    assert_eq!(source.operation, FileRepositoryOperation::RenameFile);
    assert_eq!(source.path, project_yaml_path);
    assert!(source.path.is_dir());
}

#[test]
fn test_write_file_atomically_既存fileを置換してtemporary_fileを残さない() {
    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    let target_file_path = storage_dir.path.join("project.yaml");
    let temporary_file_path = storage_dir.path.join("project.yaml.test.tmp");
    fs::write(&target_file_path, b"old").unwrap();

    write_file_atomically_with_temporary_path(&target_file_path, &temporary_file_path, b"new")
        .unwrap();

    assert_eq!(fs::read(&target_file_path).unwrap(), b"new");
    assert!(!temporary_file_path.exists());
}

#[cfg(unix)]
#[test]
fn test_write_file_atomically_if_changed_同一内容なら置換しない() {
    use std::os::unix::fs::MetadataExt;

    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    let target_file_path = storage_dir.path.join("project.yaml");
    let temporary_file_path = storage_dir.path.join("project.yaml.test.tmp");
    fs::write(&target_file_path, b"same").unwrap();
    let original_inode = fs::metadata(&target_file_path).unwrap().ino();

    let replaced = write_file_atomically_if_changed_with_temporary_path(
        &target_file_path,
        &temporary_file_path,
        b"same",
    )
    .unwrap();

    assert!(!replaced);
    assert_eq!(
        fs::metadata(&target_file_path).unwrap().ino(),
        original_inode
    );
    assert!(!temporary_file_path.exists());
}

#[test]
fn test_write_file_atomically_if_changed_変更内容なら置換する() {
    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    let target_file_path = storage_dir.path.join("project.yaml");
    let temporary_file_path = storage_dir.path.join("project.yaml.test.tmp");
    fs::write(&target_file_path, b"old").unwrap();

    let replaced = write_file_atomically_if_changed_with_temporary_path(
        &target_file_path,
        &temporary_file_path,
        b"new",
    )
    .unwrap();

    assert!(replaced);
    assert_eq!(fs::read(&target_file_path).unwrap(), b"new");
    assert!(!temporary_file_path.exists());
}

#[test]
fn test_write_file_atomically_if_changed_新規fileを作成する() {
    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    let target_file_path = storage_dir.path.join("project.yaml");
    let temporary_file_path = storage_dir.path.join("project.yaml.test.tmp");

    let replaced = write_file_atomically_if_changed_with_temporary_path(
        &target_file_path,
        &temporary_file_path,
        b"new",
    )
    .unwrap();

    assert!(replaced);
    assert_eq!(fs::read(&target_file_path).unwrap(), b"new");
    assert!(!temporary_file_path.exists());
}

#[cfg(unix)]
#[test]
fn test_write_file_atomically_if_changed_read失敗でもatomic_writeを試す() {
    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    let target_file_path = storage_dir.path.join("project.yaml");
    let temporary_file_path = storage_dir.path.join("project.yaml.test.tmp");
    fs::create_dir(&target_file_path).unwrap();

    let actual = write_file_atomically_if_changed_with_temporary_path(
        &target_file_path,
        &temporary_file_path,
        b"new",
    )
    .unwrap_err();

    assert_eq!(actual.operation, FileRepositoryOperation::RenameFile);
    assert_eq!(actual.path, target_file_path);
    assert!(target_file_path.is_dir());
    assert!(!temporary_file_path.exists());
}

#[cfg(unix)]
#[test]
fn test_write_file_atomically_既存fileのpermissionを維持する() {
    use std::os::unix::fs::PermissionsExt;

    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    let target_file_path = storage_dir.path.join("project.yaml");
    let temporary_file_path = storage_dir.path.join("project.yaml.test.tmp");
    fs::write(&target_file_path, b"old").unwrap();
    fs::set_permissions(&target_file_path, fs::Permissions::from_mode(0o600)).unwrap();

    write_file_atomically_with_temporary_path(&target_file_path, &temporary_file_path, b"new")
        .unwrap();

    let mode = fs::metadata(&target_file_path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn test_write_file_atomically_temporary_file作成失敗時に既存fileを維持する() {
    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    let target_file_path = storage_dir.path.join("project.yaml");
    let temporary_file_path = storage_dir.path.join("project.yaml.test.tmp");
    fs::write(&target_file_path, b"old").unwrap();
    fs::create_dir(&temporary_file_path).unwrap();

    let actual =
        write_file_atomically_with_temporary_path(&target_file_path, &temporary_file_path, b"new")
            .unwrap_err();

    assert_eq!(actual.operation, FileRepositoryOperation::CreateFile);
    assert_eq!(actual.path, temporary_file_path);
    assert_eq!(fs::read(&target_file_path).unwrap(), b"old");
}

#[test]
fn test_replace_file_atomically_write失敗とsync失敗時に既存fileを維持する() {
    for (write_error, sync_error, expected_operation) in [
        (true, false, FileRepositoryOperation::WriteFile),
        (false, true, FileRepositoryOperation::SyncFile),
    ] {
        let storage_dir = TestStorageDir::new();
        fs::create_dir_all(&storage_dir.path).unwrap();
        let target_file_path = storage_dir.path.join("project.yaml");
        let temporary_file_path = storage_dir.path.join("project.yaml.test.tmp");
        fs::write(&target_file_path, b"old").unwrap();
        fs::write(&temporary_file_path, b"temporary").unwrap();
        let file = FailingAtomicSaveFile {
            write_error,
            sync_error,
        };

        let actual = replace_file_atomically(&target_file_path, &temporary_file_path, file, b"new")
            .unwrap_err();

        assert_eq!(actual.operation, expected_operation);
        assert_eq!(actual.path, temporary_file_path);
        assert_eq!(fs::read(&target_file_path).unwrap(), b"old");
        assert!(!temporary_file_path.exists());
    }
}

#[test]
fn test_write_file_atomically_rename失敗時にtemporary_fileを削除する() {
    let storage_dir = TestStorageDir::new();
    fs::create_dir_all(&storage_dir.path).unwrap();
    let target_file_path = storage_dir.path.join("project.yaml");
    let temporary_file_path = storage_dir.path.join("project.yaml.test.tmp");
    fs::create_dir(&target_file_path).unwrap();

    let actual =
        write_file_atomically_with_temporary_path(&target_file_path, &temporary_file_path, b"new")
            .unwrap_err();

    assert_eq!(actual.operation, FileRepositoryOperation::RenameFile);
    assert_eq!(actual.path, target_file_path);
    assert!(target_file_path.is_dir());
    assert!(!temporary_file_path.exists());
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
    repository.start_new_project(changed_task.clone()).unwrap();
    repository.start_new_project(unchanged_task).unwrap();
    repository.save().unwrap();

    let changed_yaml_path = storage_dir
        .project_dir_path("20260811", "変更対象")
        .join("project.yaml");
    let unchanged_yaml_path = storage_dir
        .project_dir_path("20260811", "未変更対象")
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
    repository.start_new_project(changed_task.clone()).unwrap();
    repository.start_new_project(unchanged_task).unwrap();
    repository.save().unwrap();
    let unchanged_dir = storage_dir.project_dir_path("20260813", "未変更対象");
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
    source
        .start_new_project(crate::test_support::new_task_handle("読込済み").unwrap())
        .unwrap();
    source.save().unwrap();

    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    repository.load().unwrap();
    let loaded_dir = storage_dir.project_dir_path("20260813", "読込済み");
    fs::remove_dir_all(&loaded_dir).unwrap();
    repository
        .start_new_project(crate::test_support::new_task_handle("新規").unwrap())
        .unwrap();

    repository.save().unwrap();

    assert!(!loaded_dir.exists());
    assert!(storage_dir
        .project_dir_path("20260813", "新規")
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
    let project_yaml_path = storage_dir
        .project_dir_path("20260813", "再試行対象")
        .join("project.yaml");
    let old_bytes = fs::read(&project_yaml_path).unwrap();
    fs::remove_file(&project_yaml_path).unwrap();
    fs::create_dir(&project_yaml_path).unwrap();
    task.set_estimated_work_seconds(30 * 60).unwrap();

    assert!(repository.save().is_err());
    fs::remove_dir(&project_yaml_path).unwrap();
    fs::write(&project_yaml_path, old_bytes).unwrap();
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
fn test_save_project失敗時はdisk_revisionだけを先に進める() {
    let storage_dir = TestStorageDir::new();
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let revision_path = storage_dir.path.join(".revision");
    let mut repository = TaskRepository::new(storage_dir.path_str());
    repository.sync_clock(now).unwrap();
    let task = crate::test_support::new_task_handle("失敗対象").unwrap();
    repository.start_new_project(task.clone()).unwrap();
    repository.save().unwrap();
    let previous_revision = repository.storage_revision.get().unwrap();
    let project_yaml_path = storage_dir
        .project_dir_path("20260813", "失敗対象")
        .join("project.yaml");
    fs::remove_file(&project_yaml_path).unwrap();
    fs::create_dir(&project_yaml_path).unwrap();
    task.set_estimated_work_seconds(30 * 60).unwrap();

    assert!(repository.save().is_err());

    let disk_revision =
        Uuid::parse_str(fs::read_to_string(&revision_path).unwrap().trim()).unwrap();
    assert_ne!(disk_revision, previous_revision);
    assert_eq!(repository.storage_revision.get(), Some(previous_revision));
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
    repository
        .start_new_project(crate::test_support::new_task_handle("保存対象").unwrap())
        .unwrap();

    let actual = repository.save().unwrap_err();

    assert_eq!(actual.operation(), ApplicationRepositoryOperation::Save);
    let source = file_repository_error(&actual);
    assert_eq!(source.operation, FileRepositoryOperation::ReadMetadata);
    assert_eq!(source.path, revision_path);
    assert_eq!(fs::read_to_string(target_path).unwrap(), target_content);
    assert!(!storage_dir
        .project_dir_path("20260813", "保存対象")
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
        .project_dir_path("20260813", "cache対象")
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
