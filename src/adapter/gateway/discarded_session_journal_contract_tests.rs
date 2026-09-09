use super::storage_snapshot::{create_snapshot, restore_snapshot, verify_snapshot};
use super::storage_transaction_test_support::{
    FaultRule, PathMatcher, RecordingIo, RecordingOperation,
};
use super::task_repository::{DuplicateDiscardedSessionEventIdError, TaskRepository};
use crate::application::discarded_session_journal::AppendDiscardedSessionOutcome;
use crate::application::interface::{DiscardedSessionJournalTrait, TaskRepositoryTrait};
use crate::entity::discarded_session::{
    DiscardedSessionEvent, DiscardedSessionReason, DiscardedSessionSource,
};
use crate::entity::task::TaskHandle;
use chrono::{Duration, Local, TimeZone};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

struct TestStorage(PathBuf);

impl TestStorage {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("schronu-discarded-journal-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestStorage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn event(id: Uuid, name: &str) -> DiscardedSessionEvent {
    let started_at = Local.with_ymd_and_hms(2026, 9, 10, 5, 59, 0).unwrap();
    DiscardedSessionEvent::new(
        id,
        Uuid::from_u128(22),
        name.to_string(),
        started_at,
        started_at + Duration::seconds(61),
        DiscardedSessionSource::Web,
        DiscardedSessionReason::WebDiscardComplete,
    )
    .unwrap()
    .unwrap()
}

fn new_repository(storage: &TestStorage) -> TaskRepository {
    TaskRepository::new(storage.path().to_str().unwrap())
}

#[test]
fn 月別version付きyamlへ保存して再読込できる() {
    let storage = TestStorage::new();
    let mut repository = new_repository(&storage);
    repository.load().unwrap();
    let event = event(Uuid::from_u128(11), "開始時task名");

    assert_eq!(
        repository.append_discarded_session(event.clone()).unwrap(),
        AppendDiscardedSessionOutcome::Appended
    );
    assert!(repository.has_pending_changes().unwrap());
    repository.save().unwrap();
    assert!(!repository.has_pending_changes().unwrap());

    let journal_path = storage.path().join("discarded_sessions/2026-09.yaml");
    let yaml = fs::read_to_string(&journal_path).unwrap();
    assert!(yaml.contains("version: 1"));
    assert!(yaml.contains("task_name_at_start: 開始時task名"));
    assert!(storage.path().join(".revision").is_file());

    let mut reloaded = new_repository(&storage);
    reloaded.load().unwrap();
    assert_eq!(
        reloaded
            .discarded_sessions_on(chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap())
            .unwrap()
            .events(),
        &[event]
    );
}

#[test]
fn directory不存在は空journalとして扱う() {
    let storage = TestStorage::new();
    fs::create_dir_all(storage.path()).unwrap();
    let mut repository = new_repository(&storage);

    repository.load().unwrap();

    assert!(repository
        .discarded_sessions_on(chrono::NaiveDate::from_ymd_opt(2026, 9, 10).unwrap())
        .unwrap()
        .events()
        .is_empty());
}

#[test]
fn 同じidとpayloadは冪等で異なるpayloadは競合になる() {
    let storage = TestStorage::new();
    let mut repository = new_repository(&storage);
    repository.load().unwrap();
    let id = Uuid::from_u128(31);
    let original = event(id, "original");

    assert_eq!(
        repository
            .append_discarded_session(original.clone())
            .unwrap(),
        AppendDiscardedSessionOutcome::Appended
    );
    assert_eq!(
        repository.append_discarded_session(original).unwrap(),
        AppendDiscardedSessionOutcome::AlreadyPresent
    );
    let error = repository
        .append_discarded_session(event(id, "different"))
        .unwrap_err();
    assert_eq!(error.event_id(), id);
    assert_eq!(
        repository
            .discarded_sessions_on(chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap())
            .unwrap()
            .events()
            .len(),
        1
    );
}

#[test]
fn 未知versionと破損yamlはload_errorになり既存fileを変更しない() {
    for yaml in ["version: 2\nevents: []\n", "version: [\n"] {
        let storage = TestStorage::new();
        let journal_dir = storage.path().join("discarded_sessions");
        fs::create_dir_all(&journal_dir).unwrap();
        let path = journal_dir.join("2026-09.yaml");
        fs::write(&path, yaml).unwrap();
        let before = fs::read(&path).unwrap();
        let mut repository = new_repository(&storage);

        let error = repository.load().unwrap_err();

        assert!(error.to_string().contains("2026-09.yaml"));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}

#[test]
fn journal保存だけでもrevisionを更新しreload_if_changedで反映する() {
    let storage = TestStorage::new();
    let now = Local.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
    let mut cached = new_repository(&storage);
    cached.reload_if_changed(now).unwrap();
    let mut writer = new_repository(&storage);
    writer.reload_if_changed(now).unwrap();

    writer
        .append_discarded_session(event(Uuid::from_u128(41), "external"))
        .unwrap();
    writer.save().unwrap();
    cached.reload_if_changed(now).unwrap();

    assert_eq!(
        cached
            .discarded_sessions_on(chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap())
            .unwrap()
            .events()
            .len(),
        1
    );
}

#[test]
fn snapshot作成検証復元はjournalを全fileとして保持する() {
    let root = TestStorage::new();
    let storage = root.path().join("storage");
    let snapshot = root.path().join("snapshot");
    let restored = root.path().join("restored");
    fs::create_dir(&storage).unwrap();
    let mut repository = TaskRepository::new(storage.to_str().unwrap());
    repository.load().unwrap();
    let expected = event(Uuid::from_u128(51), "snapshot target");
    repository
        .append_discarded_session(expected.clone())
        .unwrap();
    repository.save().unwrap();

    let created = create_snapshot(&storage, &snapshot).unwrap();
    let verified = verify_snapshot(&snapshot).unwrap();
    let restored_summary = restore_snapshot(&snapshot, &restored).unwrap();

    assert_eq!(created.file_count(), 2);
    assert_eq!(verified.file_count(), 2);
    assert_eq!(restored_summary.file_count(), 2);
    let mut restored_repository = TaskRepository::new(restored.to_str().unwrap());
    restored_repository.load().unwrap();
    assert_eq!(
        restored_repository
            .discarded_sessions_on(chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap())
            .unwrap()
            .events(),
        &[expected]
    );
}

#[test]
fn taskとjournalのcommit失敗はどちらも未反映でpendingを維持する() {
    let storage = TestStorage::new();
    let now = Local.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
    let task_id = Uuid::from_u128(61);
    let task = TaskHandle::with_identity("atomic task", task_id, now).unwrap();
    let mut baseline = new_repository(&storage);
    baseline.sync_clock(now).unwrap();
    baseline.start_new_project(task).unwrap();
    baseline.save().unwrap();
    let revision_before = fs::read(storage.path().join(".revision")).unwrap();
    let project_path = fs::read_dir(storage.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("project.yaml"))
        .find(|path| path.is_file())
        .unwrap();
    let project_before = fs::read(&project_path).unwrap();
    let io = Arc::new(RecordingIo::new(vec![FaultRule {
        operation: RecordingOperation::WriteFile,
        path_matcher: PathMatcher::Any,
        occurrence: 2,
        error_kind: std::io::ErrorKind::Other,
        error_message: "injected pre-commit failure",
    }]));
    let mut repository =
        TaskRepository::new_with_storage_transaction_io(storage.path().to_str().unwrap(), io);
    repository.reload_if_changed(now).unwrap();
    repository
        .get_by_id(task_id)
        .unwrap()
        .unwrap()
        .set_estimated_work_seconds(60)
        .unwrap();
    repository
        .append_discarded_session(event(Uuid::from_u128(62), "atomic task"))
        .unwrap();

    let error = repository.save().unwrap_err();

    assert_eq!(
        error.save_failure_disposition(),
        Some(crate::application::interface::TaskRepositorySaveFailureDisposition::Retryable)
    );
    assert_eq!(fs::read(&project_path).unwrap(), project_before);
    assert_eq!(
        fs::read(storage.path().join(".revision")).unwrap(),
        revision_before
    );
    assert!(!storage
        .path()
        .join("discarded_sessions/2026-09.yaml")
        .exists());
    assert!(repository.has_pending_changes().unwrap());
}

#[test]
fn nestedのvalidとinvalidなjournal候補はruntimeとsnapshotでjournal扱いしない() {
    for (label, yaml) in [
        ("valid", "version: 1\nevents: []\n"),
        ("invalid", "version: [\n"),
    ] {
        let root = TestStorage::new();
        let storage = root.path().join(format!("storage-{label}"));
        let nested = storage.join("archive/discarded_sessions");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("2026-09.yaml"), yaml).unwrap();
        fs::create_dir_all(storage.join("discarded_sessions")).unwrap();
        fs::write(
            storage.join("discarded_sessions/notes.yaml"),
            "version: [\n",
        )
        .unwrap();
        let mut repository = TaskRepository::new(storage.to_str().unwrap());

        repository.load().unwrap();
        assert!(repository
            .discarded_sessions_on(chrono::NaiveDate::from_ymd_opt(2026, 9, 10).unwrap())
            .unwrap()
            .events()
            .is_empty());

        let snapshot = root.path().join(format!("snapshot-{label}"));
        create_snapshot(&storage, &snapshot).unwrap();
        verify_snapshot(&snapshot).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn saveはdiscarded_sessions_parent_symlinkの外部へ書かない() {
    use std::os::unix::fs::symlink;

    let root = TestStorage::new();
    let storage = root.path().join("storage-symlink");
    let outside = root.path().join("outside");
    fs::create_dir(&storage).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"unchanged").unwrap();
    symlink(&outside, storage.join("discarded_sessions")).unwrap();
    let mut repository = TaskRepository::new(storage.to_str().unwrap());
    repository
        .append_discarded_session(event(Uuid::from_u128(71), "outside guard"))
        .unwrap();

    assert!(repository.save().is_err());

    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"unchanged");
    assert!(!outside.join("2026-09.yaml").exists());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct SwapJournalParentIo {
    storage: PathBuf,
    outside: PathBuf,
    swapped: std::sync::atomic::AtomicBool,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl super::storage_transaction::StorageTransactionIo for SwapJournalParentIo {
    fn write_storage_file_anchored(
        &self,
        storage_dir_path: &Path,
        relative_path: &Path,
        transaction_id: Uuid,
        bytes: &[u8],
        permissions: Option<&fs::Permissions>,
    ) -> Result<bool, super::storage_transaction::StorageTransactionError> {
        let should_swap = relative_path.parent() == Some(Path::new("discarded_sessions"))
            && !self.swapped.swap(true, std::sync::atomic::Ordering::SeqCst);
        super::storage_transaction::write_storage_file_anchored_after_parent_open(
            storage_dir_path,
            relative_path,
            transaction_id,
            bytes,
            permissions,
            || {
                if should_swap {
                    let original = self.storage.join("discarded_sessions");
                    fs::rename(&original, self.storage.join("discarded_sessions-old")).unwrap();
                    std::os::unix::fs::symlink(&self.outside, original).unwrap();
                }
            },
        )?;
        Ok(true)
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn commit途中のparent_swapでもstorage外へ一byteも書かない() {
    let root = TestStorage::new();
    let storage = root.path().join("storage-parent-swap");
    let outside = root.path().join("outside-parent-swap");
    fs::create_dir(&storage).unwrap();
    fs::create_dir(storage.join("discarded_sessions")).unwrap();
    fs::create_dir(&outside).unwrap();
    let io = Arc::new(SwapJournalParentIo {
        storage: storage.clone(),
        outside: outside.clone(),
        swapped: std::sync::atomic::AtomicBool::new(false),
    });
    let mut repository =
        TaskRepository::new_with_storage_transaction_io(storage.to_str().unwrap(), io);
    repository
        .append_discarded_session(event(Uuid::from_u128(75), "swap guard"))
        .unwrap();

    assert!(repository.save().is_err());

    assert!(fs::read_dir(&outside).unwrap().next().is_none());
    assert!(fs::read_dir(storage.join("discarded_sessions-old"))
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn duplicate永続event_idは双方のpathとindexを診断する() {
    let storage = TestStorage::new();
    let journal_directory = storage.path().join("discarded_sessions");
    fs::create_dir(&journal_directory).unwrap();
    let id = Uuid::from_u128(81);
    let september_path = journal_directory.join("2026-09.yaml");
    let october_path = journal_directory.join("2026-10.yaml");
    write_event_journal(&september_path, &event_at(id, 2026, 9, 10, "first"));
    write_event_journal(&october_path, &event_at(id, 2026, 10, 10, "duplicate"));
    let mut repository = new_repository(&storage);

    let error = repository.load().unwrap_err();
    let duplicate = std::error::Error::source(&error)
        .and_then(|source| source.downcast_ref::<DuplicateDiscardedSessionEventIdError>())
        .unwrap();

    assert_eq!(duplicate.event_id(), id);
    assert_eq!(duplicate.first_path(), september_path);
    assert_eq!(duplicate.first_index(), 0);
    assert_eq!(duplicate.duplicate_path(), october_path);
    assert_eq!(duplicate.duplicate_index(), 0);
}

fn event_at(id: Uuid, year: i32, month: u32, day: u32, name: &str) -> DiscardedSessionEvent {
    let started_at = Local.with_ymd_and_hms(year, month, day, 12, 0, 0).unwrap();
    DiscardedSessionEvent::new(
        id,
        Uuid::from_u128(82),
        name.to_string(),
        started_at,
        started_at + Duration::seconds(1),
        DiscardedSessionSource::Cli,
        DiscardedSessionReason::CliNormalExit,
    )
    .unwrap()
    .unwrap()
}

fn write_event_journal(path: &Path, event: &DiscardedSessionEvent) {
    fs::write(
        path,
        format!(
            "version: 1\nevents:\n  - event_id: {}\n    task_id: {}\n    task_name_at_start: {}\n    started_at_epoch_ms: {}\n    ended_at_epoch_ms: {}\n    logical_date: {}\n    source: {}\n    reason: {}\n",
            event.event_id(),
            event.task_id(),
            event.task_name_at_start(),
            event.started_at_epoch_ms(),
            event.ended_at_epoch_ms(),
            event.logical_date(),
            event.source().as_str(),
            event.reason().as_str(),
        ),
    )
    .unwrap();
}
