use super::{CompleteSessionRequest, DiscardSessionRequest, WebService};
use crate::adapter::gateway::schronu_config::SchronuConfig;
use crate::adapter::gateway::task_repository::TaskRepository;
use crate::application::interface::{DiscardedSessionJournalTrait, TaskRepositoryTrait};
use crate::entity::discarded_session::DiscardedSessionReason;
use crate::entity::task::{Status, TaskHandle};
use chrono::{Local, NaiveDate, TimeZone};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

struct Fixture {
    root: PathBuf,
    storage: PathBuf,
    busy: PathBuf,
    task_id: Uuid,
}

impl Fixture {
    fn new(now: chrono::DateTime<Local>) -> Self {
        let root = std::env::temp_dir().join(format!("schronu-web-discard-{}", Uuid::new_v4()));
        let storage = root.join("storage");
        fs::create_dir_all(&storage).unwrap();
        let busy = root.join("busy.yaml");
        fs::write(&busy, "[]\n").unwrap();
        let task_id = Uuid::from_u128(0x2026_0910);
        let task = TaskHandle::with_identity("開始時の名前", task_id, now).unwrap();
        task.set_estimated_work_seconds(600).unwrap();
        task.set_actual_work_seconds(120).unwrap();
        let mut repository = TaskRepository::new(storage.to_str().unwrap());
        repository.sync_clock(now).unwrap();
        repository.start_new_project(task).unwrap();
        repository.save().unwrap();
        Self { root, storage, busy, task_id }
    }

    fn service(&self) -> WebService {
        WebService::new(
            self.storage.clone(),
            SchronuConfig {
                busy_time_slots_yaml_path: self.busy.clone(),
                ..SchronuConfig::default()
            },
        )
    }

    fn repository(&self, now: chrono::DateTime<Local>) -> TaskRepository {
        let mut repository = TaskRepository::new(self.storage.to_str().unwrap());
        repository.reload_if_changed(now).unwrap();
        repository
    }
}

impl Drop for Fixture {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.root); }
}

#[test]
fn discard_sessionは1秒以上だけjournalへ保存しtaskを変えない() {
    let now = Local.with_ymd_and_hms(2026, 9, 10, 9, 0, 2).unwrap();
    let fixture = Fixture::new(now);
    let mut service = fixture.service();
    service.discard_session_at(now, DiscardSessionRequest {
        event_id: Uuid::from_u128(1).to_string(),
        task_id: fixture.task_id.to_string(),
        task_name_at_start: "開始時の名前".to_owned(),
        started_at_epoch_ms: now.timestamp_millis() - 1_999,
        ended_at_epoch_ms: now.timestamp_millis(),
    }).unwrap();

    let repository = fixture.repository(now);
    let task = repository.get_by_id(fixture.task_id).unwrap().unwrap();
    assert_eq!(task.get_actual_work_seconds().unwrap(), 120);
    assert_eq!(task.get_status().unwrap(), Status::Todo);
    let summary = repository.discarded_sessions_on(NaiveDate::from_ymd_opt(2026, 9, 10).unwrap()).unwrap();
    assert_eq!(summary.total_seconds(), 1);
    assert_eq!(summary.events()[0].reason(), DiscardedSessionReason::WebDiscardRelease);
}

#[test]
fn discard_sessionの0秒は成功no_opになる() {
    let now = Local.with_ymd_and_hms(2026, 9, 10, 9, 0, 0).unwrap();
    let fixture = Fixture::new(now);
    fixture.service().discard_session_at(now, DiscardSessionRequest {
        event_id: Uuid::from_u128(2).to_string(), task_id: fixture.task_id.to_string(),
        task_name_at_start: "開始時の名前".to_owned(), started_at_epoch_ms: now.timestamp_millis() - 999,
        ended_at_epoch_ms: now.timestamp_millis(),
    }).unwrap();
    assert_eq!(fixture.repository(now).discarded_sessions_on(NaiveDate::from_ymd_opt(2026, 9, 10).unwrap()).unwrap().total_seconds(), 0);
}

#[test]
fn complete_sessionの破棄はtask完了とjournalを同じtransactionへ保存する() {
    let now = Local.with_ymd_and_hms(2026, 9, 10, 9, 1, 0).unwrap();
    let fixture = Fixture::new(now);
    fixture.service().complete_session_at(now, CompleteSessionRequest {
        task_id: fixture.task_id.to_string(), started_at_epoch_ms: now.timestamp_millis() - 60_000,
        ended_at_epoch_ms: Some(now.timestamp_millis()), expected_actual_work_seconds: 120,
        record_elapsed_seconds: false, discard_event_id: Some(Uuid::from_u128(3).to_string()),
        task_name_at_start: "開始時の名前".to_owned(),
    }).unwrap();
    let repository = fixture.repository(now);
    assert_eq!(repository.get_by_id(fixture.task_id).unwrap().unwrap().get_status().unwrap(), Status::Done);
    let summary = repository.discarded_sessions_on(NaiveDate::from_ymd_opt(2026, 9, 10).unwrap()).unwrap();
    assert_eq!(summary.total_seconds(), 60);
    assert_eq!(summary.events()[0].reason(), DiscardedSessionReason::WebDiscardComplete);
}

#[test]
fn list_discarded_sessionsはsnapshotと共通集計をwire向けdtoで返す() {
    let now = Local.with_ymd_and_hms(2026, 9, 10, 9, 2, 0).unwrap();
    let fixture = Fixture::new(now);
    let mut service = fixture.service();
    service.discard_session_at(now, DiscardSessionRequest {
        event_id: Uuid::from_u128(4).to_string(), task_id: fixture.task_id.to_string(),
        task_name_at_start: "開始時の名前".to_owned(), started_at_epoch_ms: now.timestamp_millis() - 30_000,
        ended_at_epoch_ms: now.timestamp_millis(),
    }).unwrap();
    let result = service.list_discarded_sessions_at(now, NaiveDate::from_ymd_opt(2026, 9, 10).unwrap()).unwrap();
    assert_eq!(result.data.total_seconds, 30);
    assert_eq!(result.data.task_totals[0].task_name, "開始時の名前");
    assert_eq!(result.data.events[0].event_id, Uuid::from_u128(4).to_string());
    assert_eq!(result.data.events[0].reason, "web_discard_release");
}
