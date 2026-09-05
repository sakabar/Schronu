use super::{WebReadError, WebReadService};
use crate::adapter::gateway::schronu_config::SchronuConfig;
use crate::adapter::gateway::storage_lock::{LockMode, StorageLock, StorageLockErrorKind};
use crate::adapter::gateway::task_repository::TaskRepository;
use crate::application::interface::TaskRepositoryTrait;
use crate::entity::task::TaskHandle;
use chrono::{Local, NaiveDate, TimeZone};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

struct WebReadServiceFixture {
    root: PathBuf,
    storage: PathBuf,
    busy_time_slots: PathBuf,
}

impl WebReadServiceFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "schronu-web-read-service-{}",
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

    fn config(&self) -> SchronuConfig {
        SchronuConfig {
            busy_time_slots_yaml_path: self.busy_time_slots.clone(),
            end_of_day_offset_minutes: 120,
            ..SchronuConfig::default()
        }
    }

    fn seed_fixed_task(&self, now: chrono::DateTime<Local>) -> Uuid {
        let task_id = Uuid::from_u128(0x2026_0905);
        let task = TaskHandle::with_identity("service task", task_id, now).unwrap();
        task.set_estimated_work_seconds(30 * 60).unwrap();
        task.set_actual_work_seconds(5 * 60).unwrap();
        task.set_start_time(Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 0).unwrap())
            .unwrap();
        task.set_fixed_start(true).unwrap();
        task.set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 9, 5, 21, 0, 0).unwrap()))
            .unwrap();

        let mut repository = TaskRepository::new(self.storage.to_str().unwrap());
        repository.sync_clock(now).unwrap();
        repository.load().unwrap();
        repository.start_new_project(task).unwrap();
        repository.save().unwrap();
        task_id
    }

    fn persisted_bytes(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut paths = vec![self.storage.join(".revision")];
        for entry in fs::read_dir(&self.storage).unwrap() {
            let path = entry.unwrap().path().join("project.yaml");
            if path.is_file() {
                paths.push(path);
            }
        }
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect()
    }
}

impl Drop for WebReadServiceFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn busy_time_slots_yaml() -> String {
    let mut yaml = String::from("days_of_week:\n");
    for day_of_week in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
        yaml.push_str(&format!(
            "  - day_of_week: {day_of_week}\n    busy_time_slots:\n      - start_time: '20:00'\n        duration_minutes: 60\n        name: fixed rest\n"
        ));
    }
    yaml
}

#[test]
fn serviceの3read操作は実storageを同期して同一snapshotとtyped_dataを返し保存しない() {
    let seeded_at = Local.with_ymd_and_hms(2026, 9, 5, 18, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 59).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(seeded_at);
    let before = fixture.persisted_bytes();
    let mut service = WebReadService::new(fixture.storage.clone(), fixture.config());

    let bootstrap = service.bootstrap_at(operation_now).unwrap();
    let listed = service
        .list_tasks_at(operation_now, NaiveDate::from_ymd_opt(2026, 9, 5).unwrap())
        .unwrap();
    let selected = service.auto_session_at(operation_now).unwrap();

    assert_eq!(
        bootstrap.observed_at_epoch_ms,
        operation_now.timestamp_millis()
    );
    assert_eq!(bootstrap.logical_date, "2026-09-05");
    assert_eq!(bootstrap.buffer_seconds, 20_041);
    assert_eq!(listed.snapshot, bootstrap);
    assert_eq!(selected.snapshot, bootstrap);
    assert_eq!(listed.data.len(), 1);
    assert_eq!(
        listed.data[0].task.task_id,
        task_id.hyphenated().to_string()
    );
    assert_eq!(listed.data[0].task.actual_work_seconds, 300);
    assert_eq!(
        listed.data[0].schedule_start_epoch_ms,
        operation_now.timestamp_millis()
    );
    assert_eq!(
        selected.data.unwrap().task_id,
        task_id.hyphenated().to_string()
    );
    assert_eq!(fixture.persisted_bytes(), before);
}

#[test]
fn serviceはweb_lock競合をrepository読込前にtyped_errorで返す() {
    let now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 59).unwrap();
    let fixture = WebReadServiceFixture::new();
    let _cli_lock = StorageLock::acquire(&fixture.storage, LockMode::Cli).unwrap();
    let mut service = WebReadService::new(fixture.storage.clone(), fixture.config());

    let error = service.bootstrap_at(now).unwrap_err();

    match error {
        WebReadError::Lock(source) => {
            assert_eq!(source.kind(), StorageLockErrorKind::Contended);
            assert_eq!(source.path(), fixture.storage.join(".lock"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn serviceはbusy_time_slot読込失敗を元情報付きtyped_errorで返す() {
    let now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 59).unwrap();
    let fixture = WebReadServiceFixture::new();
    let missing = fixture.root.join("missing.yaml");
    let config = SchronuConfig {
        busy_time_slots_yaml_path: missing.clone(),
        ..fixture.config()
    };
    let mut service = WebReadService::new(fixture.storage.clone(), config);

    let error = service.bootstrap_at(now).unwrap_err();

    match error {
        WebReadError::BusyTimeSlots(source) => {
            assert_eq!(source.path(), missing);
            assert_eq!(source.field_path(), "$");
            assert!(source.source().is_some());
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
