use super::runtime::EXPECTED_TODAY_PLAIN_TEXT;
use super::{TodayTextError, TodayTextService};
use crate::adapter::gateway::schronu_config::SchronuConfig;
use crate::adapter::gateway::storage_lock::{LockMode, StorageLock, StorageLockErrorKind};
use crate::adapter::gateway::task_repository::TaskRepository;
use crate::application::interface::{TaskRepositoryOperation, TaskRepositoryTrait};
use crate::entity::task::{ProjectCategory, TaskHandle};
use chrono::{Duration, Local, TimeZone};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

struct TodayTextFixture {
    root: PathBuf,
    storage: PathBuf,
    busy_time_slots: PathBuf,
}

impl TodayTextFixture {
    fn empty() -> Self {
        let root = std::env::temp_dir().join(format!(
            "schronu-today-text-contract-{}",
            Uuid::new_v4().hyphenated()
        ));
        let storage = root.join("storage");
        fs::create_dir_all(&storage).unwrap();
        let busy_time_slots = root.join("busy_time_slots.yaml");
        fs::write(&busy_time_slots, busy_time_slots_yaml()).unwrap();
        Self {
            root,
            storage,
            busy_time_slots,
        }
    }

    fn seeded(now: chrono::DateTime<Local>) -> Self {
        let fixture = Self::empty();
        let task = TaskHandle::with_identity("Web表示契約task", Uuid::from_u128(0x2026_0811), now)
            .unwrap();
        task.set_estimated_work_seconds(30 * 60).unwrap();
        task.set_start_time(now + Duration::hours(1)).unwrap();
        task.set_fixed_start(true).unwrap();
        task.set_deadline_time_opt(Some(now + Duration::hours(2)))
            .unwrap();
        task.set_priority(7).unwrap();
        task.set_project_category_opt(Some(ProjectCategory::Investment))
            .unwrap();

        let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
        repository.sync_clock(now).unwrap();
        repository.load().unwrap();
        repository.start_new_project(task).unwrap();
        repository.save().unwrap();
        fixture
    }

    fn config(&self) -> SchronuConfig {
        SchronuConfig {
            busy_time_slots_yaml_path: self.busy_time_slots.clone(),
            ..SchronuConfig::default()
        }
    }
}

impl Drop for TodayTextFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn busy_time_slots_yaml() -> String {
    let mut yaml = String::from("days_of_week:\n");
    for day_of_week in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
        yaml.push_str(&format!(
            "  - day_of_week: {day_of_week}\n    busy_time_slots: []\n"
        ));
    }
    yaml
}

#[test]
fn render_atは既存の今display_modelをplain全文へ描画する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let fixture = TodayTextFixture::seeded(now);
    let mut service = TodayTextService::new(fixture.storage.clone(), fixture.config());

    let actual = service.render_at(now).unwrap();

    assert!(!actual.contains("\x1b["));
    assert_eq!(actual, EXPECTED_TODAY_PLAIN_TEXT);
}

#[test]
fn render_atはstorage_lock競合を元error付きで分類する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let fixture = TodayTextFixture::empty();
    let _cli_lock = StorageLock::acquire(&fixture.storage, LockMode::Cli).unwrap();
    let mut service = TodayTextService::new(fixture.storage.clone(), fixture.config());

    let error = service.render_at(now).unwrap_err();

    match error {
        TodayTextError::Lock(source) => {
            assert_eq!(source.kind(), StorageLockErrorKind::Contended);
            assert_eq!(source.path(), fixture.storage.join(".lock"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn render_atはrepository読込失敗を元error付きで分類する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let fixture = TodayTextFixture::empty();
    let invalid_project_directory = fixture.storage.join("invalid-project");
    fs::create_dir(&invalid_project_directory).unwrap();
    fs::write(
        invalid_project_directory.join("project.yaml"),
        "not: a project\n",
    )
    .unwrap();
    let mut service = TodayTextService::new(fixture.storage.clone(), fixture.config());

    let error = service.render_at(now).unwrap_err();

    match error {
        TodayTextError::Repository(source) => {
            assert_eq!(source.operation(), TaskRepositoryOperation::Load);
            assert!(source.to_string().contains("project.yaml"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn render_atはbusy_time_slot読込失敗を元error付きで分類する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let fixture = TodayTextFixture::empty();
    let missing_path = fixture.root.join("missing-busy-time-slots.yaml");
    let config = SchronuConfig {
        busy_time_slots_yaml_path: missing_path.clone(),
        ..fixture.config()
    };
    let mut service = TodayTextService::new(fixture.storage.clone(), config);

    let error = service.render_at(now).unwrap_err();

    match error {
        TodayTextError::BusyTimeSlots(source) => {
            assert_eq!(source.path(), Path::new(&missing_path));
            assert_eq!(source.field_path(), "$");
            assert!(source.source().is_some());
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
