use super::task_repository::TaskRepository;
use crate::application::discarded_session_journal::AppendDiscardedSessionOutcome;
use crate::application::interface::{DiscardedSessionJournalTrait, TaskRepositoryTrait};
use crate::entity::discarded_session::{
    DiscardedSessionEvent, DiscardedSessionReason, DiscardedSessionSource,
};
use chrono::{Duration, Local, TimeZone};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

struct TestStorage(PathBuf);

impl TestStorage {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "schronu-discarded-journal-{}",
            Uuid::new_v4()
        )))
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
            .events()
            .len(),
        1
    );
}
