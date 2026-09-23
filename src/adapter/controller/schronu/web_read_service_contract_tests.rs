use super::{
    CompleteSessionRequest, DeferPlanRequest, DeferTaskRequest, RecordSessionRequest, WebReadError,
    WebService,
};
use crate::adapter::gateway::schronu_config::SchronuConfig;
use crate::adapter::gateway::storage_lock::{LockMode, StorageLock, StorageLockErrorKind};
use crate::adapter::gateway::storage_transaction_test_support::{
    FaultRule, PathMatcher, RecordingIo, RecordingOperation,
};
use crate::adapter::gateway::task_repository::TaskRepository;
use crate::application::interface::TaskRepositoryTrait;
use crate::application::task_use_case::DeferMode;
use crate::entity::task::{Status, TaskAttr, TaskHandle};
use chrono::{Duration, Local, NaiveDate, TimeZone};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
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
        self.seed_fixed_task_with_actual(now, 5 * 60, false)
    }

    fn seed_unconstrained_task(&self, now: chrono::DateTime<Local>) -> Uuid {
        let task_id = Uuid::from_u128(0x2026_0905_0003);
        let task = TaskHandle::with_identity("defer service task", task_id, now).unwrap();
        task.set_estimated_work_seconds(30 * 60).unwrap();

        let mut repository = TaskRepository::new(self.storage.to_str().unwrap());
        repository.sync_clock(now).unwrap();
        repository.load().unwrap();
        repository.start_new_project(task).unwrap();
        repository.save().unwrap();
        task_id
    }

    fn seed_fixed_tasks(&self, now: chrono::DateTime<Local>, count: usize) -> Vec<Uuid> {
        let mut repository = TaskRepository::new(self.storage.to_str().unwrap());
        repository.sync_clock(now).unwrap();
        repository.load().unwrap();
        let ids = (0..count)
            .map(|index| {
                let task_id = Uuid::new_v4();
                let task = TaskHandle::with_identity("paged service task", task_id, now).unwrap();
                task.set_estimated_work_seconds(60).unwrap();
                task.set_start_time(now + Duration::minutes(index as i64 * 2))
                    .unwrap();
                task.set_fixed_start(true).unwrap();
                if index == 0 {
                    task.set_deadline_time_opt(Some(now + Duration::hours(1)))
                        .unwrap();
                }
                repository.start_new_project(task).unwrap();
                task_id
            })
            .collect::<Vec<_>>();
        repository.save().unwrap();
        ids
    }

    fn seed_fixed_task_with_actual(
        &self,
        now: chrono::DateTime<Local>,
        actual_work_seconds: i64,
        completed: bool,
    ) -> Uuid {
        let task_id = Uuid::from_u128(0x2026_0905);
        let task = TaskHandle::with_identity("service task", task_id, now).unwrap();
        task.set_estimated_work_seconds(30 * 60).unwrap();
        task.set_actual_work_seconds(actual_work_seconds).unwrap();
        task.set_start_time(Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 0).unwrap())
            .unwrap();
        task.set_fixed_start(true).unwrap();
        task.set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 9, 5, 21, 0, 0).unwrap()))
            .unwrap();
        if completed {
            task.set_orig_status(Status::Done).unwrap();
        }

        let mut repository = TaskRepository::new(self.storage.to_str().unwrap());
        repository.sync_clock(now).unwrap();
        repository.load().unwrap();
        repository.start_new_project(task).unwrap();
        repository.save().unwrap();
        task_id
    }

    fn seed_repetition_task(&self, now: chrono::DateTime<Local>) -> (Uuid, Uuid) {
        let parent_id = Uuid::from_u128(0x2026_0905_0001);
        let child_id = Uuid::from_u128(0x2026_0905_0002);
        let parent = TaskHandle::with_identity("routine", parent_id, now).unwrap();
        parent.set_repetition_interval_days_opt(Some(7)).unwrap();
        parent.set_estimated_work_seconds(600).unwrap();
        let child = parent
            .create_child(TaskAttr::with_identity("occurrence", child_id, now))
            .unwrap();
        child.set_estimated_work_seconds(600).unwrap();
        child.set_actual_work_seconds(300).unwrap();

        let mut repository = TaskRepository::new(self.storage.to_str().unwrap());
        repository.sync_clock(now).unwrap();
        repository.start_new_project(parent).unwrap();
        repository.save().unwrap();
        (parent_id, child_id)
    }

    fn seed_parent_child_and_pending_leaf(
        &self,
        now: chrono::DateTime<Local>,
    ) -> (Uuid, Uuid, Uuid) {
        let parent_id = Uuid::new_v4();
        let child_id = Uuid::new_v4();
        let pending_id = Uuid::new_v4();
        let parent = TaskHandle::with_identity("all parent", parent_id, now).unwrap();
        parent.set_estimated_work_seconds(1_200).unwrap();
        let child = parent
            .create_child(TaskAttr::with_identity("all child", child_id, now))
            .unwrap();
        child.set_estimated_work_seconds(600).unwrap();
        let pending = TaskHandle::with_identity("pending leaf", pending_id, now).unwrap();
        pending.set_estimated_work_seconds(600).unwrap();
        pending.set_pending_until(now + Duration::days(1)).unwrap();

        let mut repository = TaskRepository::new(self.storage.to_str().unwrap());
        repository.sync_clock(now).unwrap();
        repository.load().unwrap();
        repository.start_new_project(parent).unwrap();
        repository.start_new_project(pending).unwrap();
        repository.save().unwrap();
        (parent_id, child_id, pending_id)
    }

    fn seed_task_split_by_fixed_blocker(&self, now: chrono::DateTime<Local>) -> Uuid {
        let task_id = Uuid::new_v4();
        let task = TaskHandle::with_identity("split all task", task_id, now).unwrap();
        task.set_estimated_work_seconds(3_600).unwrap();
        let blocker = TaskHandle::with_identity("fixed blocker", Uuid::new_v4(), now).unwrap();
        blocker.set_estimated_work_seconds(60).unwrap();
        blocker.set_start_time(now + Duration::minutes(30)).unwrap();
        blocker.set_fixed_start(true).unwrap();

        let mut repository = TaskRepository::new(self.storage.to_str().unwrap());
        repository.sync_clock(now).unwrap();
        repository.load().unwrap();
        repository.start_new_project(task).unwrap();
        repository.start_new_project(blocker).unwrap();
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

#[test]
fn 全件serviceは実repositoryの501segmentをsnapshot固定してpage解放する() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 6, 0, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_ids = fixture.seed_fixed_tasks(operation_now, 501);
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let first = service.list_all_tasks_at(operation_now, None).unwrap();
    assert_eq!(first.data.rows.len(), 500);
    assert_eq!(first.data.rows[0].segment_index, 0);
    assert_eq!(first.data.rows[499].segment_index, 499);
    assert_eq!(
        first
            .data
            .rows
            .iter()
            .map(|row| row.task.task_id.clone())
            .collect::<Vec<_>>(),
        task_ids[..500]
            .iter()
            .map(|id| id.hyphenated().to_string())
            .collect::<Vec<_>>()
    );
    assert!(first
        .data
        .rows
        .iter()
        .all(|row| row.is_leaf && row.schedule_date.len() == 10 && !row.deadline_label.is_empty()));
    assert!(first.data.rows[0].deadline_epoch_ms.is_some());
    let cursor = first.data.next_cursor.clone().unwrap();
    let cursor_id = cursor.split(':').next().unwrap();
    for invalid in [
        "missing-separator".to_owned(),
        "not-a-uuid:500".to_owned(),
        format!("{cursor_id}:nope"),
        format!("{cursor_id}:0"),
        format!("{cursor_id}:1"),
        format!("{cursor_id}:1000"),
        format!("{cursor_id}:500:extra"),
    ] {
        assert!(matches!(
            service.list_all_tasks_at(operation_now, Some(invalid)),
            Err(WebReadError::InvalidCursor)
        ));
    }

    let added_id = fixture.seed_unconstrained_task(operation_now + Duration::seconds(1));
    let second = service
        .list_all_tasks_at(operation_now + Duration::seconds(1), Some(cursor.clone()))
        .unwrap();
    assert_eq!(second.snapshot, first.snapshot);
    assert_eq!(second.data.rows.len(), 1);
    assert_eq!(second.data.rows[0].segment_index, 500);
    assert_eq!(
        second.data.rows[0].task.task_id,
        task_ids[500].hyphenated().to_string()
    );
    assert_ne!(
        second.data.rows[0].task.task_id,
        added_id.hyphenated().to_string()
    );
    assert_eq!(second.data.next_cursor, None);
    assert!(matches!(
        service.list_all_tasks_at(operation_now, Some(cursor)),
        Err(WebReadError::InvalidCursor)
    ));
}

#[test]
fn 全件serviceの九個目開始は最古snapshotだけをfifoで失効させる() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 6, 0, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    fixture.seed_fixed_tasks(operation_now, 501);
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let cursors = (0..9)
        .map(|index| {
            service
                .list_all_tasks_at(operation_now + Duration::seconds(index), None)
                .unwrap()
                .data
                .next_cursor
                .unwrap()
        })
        .collect::<Vec<_>>();

    assert!(matches!(
        service.list_all_tasks_at(operation_now, Some(cursors[0].clone())),
        Err(WebReadError::InvalidCursor)
    ));
    for cursor in &cursors[1..] {
        assert!(service
            .list_all_tasks_at(operation_now, Some(cursor.clone()))
            .is_ok());
    }
}

#[test]
fn 全件serviceは実repositoryの親葉pending葉をschedule順のrowへ変換する() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 6, 0, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let (parent_id, child_id, pending_id) =
        fixture.seed_parent_child_and_pending_leaf(operation_now);
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let page = service.list_all_tasks_at(operation_now, None).unwrap().data;
    assert_eq!(
        page.rows
            .iter()
            .map(|row| row.segment_index)
            .collect::<Vec<_>>(),
        (0..page.rows.len()).collect::<Vec<_>>()
    );
    let parent = page
        .rows
        .iter()
        .find(|row| row.task.task_id == parent_id.hyphenated().to_string())
        .unwrap();
    let child = page
        .rows
        .iter()
        .find(|row| row.task.task_id == child_id.hyphenated().to_string())
        .unwrap();
    let pending = page
        .rows
        .iter()
        .find(|row| row.task.task_id == pending_id.hyphenated().to_string())
        .unwrap();
    assert!(!parent.is_leaf);
    assert!(child.is_leaf);
    assert!(pending.is_leaf);
    assert_eq!(pending.schedule_date.len(), 10);
}

#[test]
fn 全件serviceは同じtaskの複数segmentを別rowとして保持する() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 6, 0, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_task_split_by_fixed_blocker(operation_now);
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let page = service.list_all_tasks_at(operation_now, None).unwrap().data;
    let task_rows = page
        .rows
        .iter()
        .filter(|row| row.task.task_id == task_id.hyphenated().to_string())
        .collect::<Vec<_>>();
    assert_eq!(task_rows.len(), 2);
    assert_ne!(task_rows[0].segment_index, task_rows[1].segment_index);
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
fn serviceの4read操作は実storageを同期して同一snapshotとtyped_dataを返し保存しない() {
    let seeded_at = Local.with_ymd_and_hms(2026, 9, 5, 18, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 59).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(seeded_at);
    let before = fixture.persisted_bytes();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let bootstrap = service.bootstrap_at(operation_now).unwrap();
    let listed = service
        .list_tasks_at(operation_now, NaiveDate::from_ymd_opt(2026, 9, 5).unwrap())
        .unwrap();
    let all = service.list_all_tasks_at(operation_now, None).unwrap();
    let selected = service.auto_session_at(operation_now).unwrap();

    assert_eq!(
        bootstrap.observed_at_epoch_ms,
        operation_now.timestamp_millis()
    );
    assert_eq!(bootstrap.logical_date, "2026-09-05");
    assert_eq!(bootstrap.buffer_seconds, 20_041);
    assert_eq!(listed.snapshot, bootstrap);
    assert_eq!(all.snapshot, bootstrap);
    assert_eq!(selected.snapshot, bootstrap);
    assert_eq!(listed.data.len(), 1);
    assert_eq!(all.data.rows.len(), 1);
    assert_eq!(all.data.next_cursor, None);
    assert_eq!(all.data.rows[0].segment_index, 0);
    assert_eq!(all.data.rows[0].schedule_date, "2026-09-05");
    assert_eq!(
        all.data.rows[0].task.task_id,
        task_id.hyphenated().to_string()
    );
    assert_eq!(
        all.data.rows[0].deadline_label,
        listed.data[0].deadline_label
    );
    assert_eq!(
        all.data.rows[0].misses_deadline,
        listed.data[0].misses_deadline
    );
    assert_eq!(all.data.rows[0].is_leaf, listed.data[0].is_leaf);
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
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

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
    let mut service = WebService::new(fixture.storage.clone(), config);

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

#[test]
fn defer_taskは表示日が未来ならその翌日の論理日開始まで延期して1回保存する() {
    let seeded_at = Local.with_ymd_and_hms(2026, 9, 5, 18, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 59).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_unconstrained_task(seeded_at);
    let revision_before = fs::read(fixture.storage.join(".revision")).unwrap();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let response = service
        .defer_task_at(
            operation_now,
            DeferTaskRequest {
                task_id: task_id.to_string(),
                selected_logical_date: "2026-09-08".to_owned(),
                expected_plan: DeferPlanRequest {
                    mode: DeferMode::Normal,
                    requested_pending_until_epoch_ms: Local
                        .with_ymd_and_hms(2026, 9, 9, 6, 0, 0)
                        .unwrap()
                        .timestamp_millis(),
                    effective_pending_until_epoch_ms: None,
                    repetition_interval_days: None,
                },
            },
        )
        .unwrap();

    assert_eq!(
        response.observed_at_epoch_ms,
        operation_now.timestamp_millis()
    );
    assert_ne!(
        fs::read(fixture.storage.join(".revision")).unwrap(),
        revision_before
    );
    let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
    repository.reload_if_changed(operation_now).unwrap();
    let task = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        task.get_pending_until().unwrap(),
        Local.with_ymd_and_hms(2026, 9, 9, 6, 0, 0).unwrap()
    );
}

#[test]
fn defer_taskは不正uuidを保存前に拒否する() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 59).unwrap();
    let fixture = WebReadServiceFixture::new();
    fixture.seed_fixed_task(operation_now - Duration::hours(1));
    let before = fixture.persisted_bytes();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    assert!(matches!(
        service.defer_task_at(
            operation_now,
            DeferTaskRequest {
                task_id: "not-a-uuid".to_owned(),
                selected_logical_date: "2026-09-05".to_owned(),
                expected_plan: DeferPlanRequest {
                    mode: DeferMode::Normal,
                    requested_pending_until_epoch_ms: operation_now.timestamp_millis(),
                    effective_pending_until_epoch_ms: None,
                    repetition_interval_days: None,
                },
            }
        ),
        Err(WebReadError::InvalidInput(_))
    ));
    assert_eq!(fixture.persisted_bytes(), before);
}

#[test]
fn defer_taskは一覧後にmodeが変わった場合に保存しない() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 59).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(operation_now - Duration::hours(1));
    let before = fixture.persisted_bytes();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    assert!(matches!(
        service.defer_task_at(
            operation_now,
            DeferTaskRequest {
                task_id: task_id.to_string(),
                selected_logical_date: "2026-09-05".to_owned(),
                expected_plan: DeferPlanRequest {
                    mode: DeferMode::Normal,
                    requested_pending_until_epoch_ms: Local
                        .with_ymd_and_hms(2026, 9, 6, 6, 0, 0)
                        .unwrap()
                        .timestamp_millis(),
                    effective_pending_until_epoch_ms: None,
                    repetition_interval_days: None,
                },
            }
        ),
        Err(WebReadError::Application(
            crate::application::task_use_case::ApplicationError::DeferPlanChanged {
                expected: DeferMode::Normal,
                actual: DeferMode::DeadlineLimited,
            }
        ))
    ));
    assert_eq!(fixture.persisted_bytes(), before);
}

#[test]
fn defer_taskはmodeが同じでも一覧後に延期先が変わった場合に保存しない() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 59).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_unconstrained_task(operation_now - Duration::hours(1));
    let before = fixture.persisted_bytes();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    assert!(matches!(
        service.defer_task_at(
            operation_now,
            DeferTaskRequest {
                task_id: task_id.to_string(),
                selected_logical_date: "2026-09-05".to_owned(),
                expected_plan: DeferPlanRequest {
                    mode: DeferMode::Normal,
                    requested_pending_until_epoch_ms: Local
                        .with_ymd_and_hms(2026, 9, 7, 6, 0, 0)
                        .unwrap()
                        .timestamp_millis(),
                    effective_pending_until_epoch_ms: None,
                    repetition_interval_days: None,
                },
            }
        ),
        Err(WebReadError::Application(
            crate::application::task_use_case::ApplicationError::DeferPlanChanged {
                expected: DeferMode::Normal,
                actual: DeferMode::Normal,
            }
        ))
    ));
    assert_eq!(fixture.persisted_bytes(), before);
}

#[test]
fn record_sessionは経過ミリ秒をfloor秒へ変換して実績を1回だけ保存する() {
    let seeded_at = Local.with_ymd_and_hms(2026, 9, 5, 18, 0, 0).unwrap();
    let operation_now =
        Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap() + Duration::milliseconds(999);
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(seeded_at);
    let revision_before = fs::read(fixture.storage.join(".revision")).unwrap();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let started_at_epoch_ms = operation_now.timestamp_millis() - 65_999;
    let ended_at_epoch_ms = operation_now.timestamp_millis() - 5_999;
    let response = service
        .record_session_at(
            operation_now,
            RecordSessionRequest {
                task_id: task_id.hyphenated().to_string(),
                started_at_epoch_ms,
                ended_at_epoch_ms: Some(ended_at_epoch_ms),
                expected_actual_work_seconds: 300,
            },
        )
        .unwrap();

    assert_eq!(response.data.actual_work_seconds, 360);
    assert_eq!(
        response.snapshot.observed_at_epoch_ms,
        operation_now.timestamp_millis()
    );
    assert_ne!(
        fs::read(fixture.storage.join(".revision")).unwrap(),
        revision_before
    );
    let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
    repository.reload_if_changed(operation_now).unwrap();
    assert_eq!(
        repository
            .get_by_id(task_id)
            .unwrap()
            .unwrap()
            .get_actual_work_seconds()
            .unwrap(),
        360
    );
}

#[test]
fn record_sessionの二重送信は競合となり2回目は保存しない() {
    let seeded_at = Local.with_ymd_and_hms(2026, 9, 5, 18, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(seeded_at);
    let request = RecordSessionRequest {
        task_id: task_id.hyphenated().to_string(),
        started_at_epoch_ms: operation_now.timestamp_millis() - 60_000,
        ended_at_epoch_ms: None,
        expected_actual_work_seconds: 300,
    };
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());
    service
        .record_session_at(operation_now, request.clone())
        .unwrap();
    let after_first = fixture.persisted_bytes();

    let error = service
        .record_session_at(operation_now, request)
        .unwrap_err();

    assert!(matches!(
        error,
        WebReadError::Application(
            crate::application::task_use_case::ApplicationError::ActualWorkConflict { .. }
        )
    ));
    assert_eq!(fixture.persisted_bytes(), after_first);
}

#[test]
fn record_sessionはwire入力errorを分類して保存しない() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    fixture.seed_fixed_task(operation_now - Duration::hours(1));
    let before = fixture.persisted_bytes();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    for request in [
        RecordSessionRequest {
            task_id: "not-a-uuid".to_string(),
            started_at_epoch_ms: operation_now.timestamp_millis(),
            ended_at_epoch_ms: None,
            expected_actual_work_seconds: 0,
        },
        RecordSessionRequest {
            task_id: Uuid::new_v4().to_string(),
            started_at_epoch_ms: operation_now.timestamp_millis() + 1,
            ended_at_epoch_ms: None,
            expected_actual_work_seconds: 0,
        },
        RecordSessionRequest {
            task_id: Uuid::new_v4().to_string(),
            started_at_epoch_ms: i64::MIN,
            ended_at_epoch_ms: None,
            expected_actual_work_seconds: 0,
        },
        RecordSessionRequest {
            task_id: Uuid::new_v4().to_string(),
            started_at_epoch_ms: operation_now.timestamp_millis(),
            ended_at_epoch_ms: None,
            expected_actual_work_seconds: -1,
        },
        RecordSessionRequest {
            task_id: Uuid::new_v4().to_string(),
            started_at_epoch_ms: operation_now.timestamp_millis(),
            ended_at_epoch_ms: Some(operation_now.timestamp_millis() - 1),
            expected_actual_work_seconds: 0,
        },
        RecordSessionRequest {
            task_id: Uuid::new_v4().to_string(),
            started_at_epoch_ms: operation_now.timestamp_millis(),
            ended_at_epoch_ms: Some(i64::MAX),
            expected_actual_work_seconds: 0,
        },
    ] {
        assert!(matches!(
            service.record_session_at(operation_now, request),
            Err(WebReadError::InvalidInput(_))
        ));
    }
    assert_eq!(fixture.persisted_bytes(), before);
}

#[test]
fn record_sessionはbrowser時計がserverより進んでいてもclickまでの経過秒を保存する() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let browser_ended_at = operation_now + Duration::minutes(5);
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(operation_now - Duration::hours(1));
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let response = service
        .record_session_at(
            operation_now,
            RecordSessionRequest {
                task_id: task_id.to_string(),
                started_at_epoch_ms: browser_ended_at.timestamp_millis() - 60_000,
                ended_at_epoch_ms: Some(browser_ended_at.timestamp_millis()),
                expected_actual_work_seconds: 300,
            },
        )
        .unwrap();

    assert_eq!(response.data.actual_work_seconds, 360);
}

#[test]
fn record_sessionは日時として表現不能な開始epochを保存前に拒否する() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(operation_now - Duration::hours(1));
    let before = fixture.persisted_bytes();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let error = service
        .record_session_at(
            operation_now,
            RecordSessionRequest {
                task_id: task_id.to_string(),
                started_at_epoch_ms: i64::MIN / 2,
                ended_at_epoch_ms: None,
                expected_actual_work_seconds: 300,
            },
        )
        .unwrap_err();

    assert!(matches!(error, WebReadError::InvalidInput(_)));
    assert_eq!(fixture.persisted_bytes(), before);
}

#[test]
fn record_sessionはsnapshot失敗時のmutationを後続requestへ持ち越さない() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task_with_actual(operation_now - Duration::hours(1), 0, false);
    let before = fixture.persisted_bytes();
    let mut config = fixture.config();
    config.end_of_day_offset_minutes = i64::MAX;
    let mut service = WebService::new(fixture.storage.clone(), config);
    let request = RecordSessionRequest {
        task_id: task_id.to_string(),
        started_at_epoch_ms: operation_now.timestamp_millis() - 1_000,
        ended_at_epoch_ms: None,
        expected_actual_work_seconds: 0,
    };

    let first = service
        .record_session_at(operation_now, request.clone())
        .unwrap_err();
    let second = service
        .record_session_at(operation_now, request)
        .unwrap_err();

    assert!(matches!(
        first,
        WebReadError::Application(
            crate::application::task_use_case::ApplicationError::LogicalDateEndOutOfRange { .. }
        )
    ));
    assert!(matches!(
        second,
        WebReadError::Application(
            crate::application::task_use_case::ApplicationError::LogicalDateEndOutOfRange { .. }
        )
    ));
    assert_eq!(fixture.persisted_bytes(), before);
}

#[test]
fn record_sessionは未知taskと完了済みtaskと競合と加算overflowで保存しない() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();

    for (actual_work_seconds, completed, task_id, expected_kind) in [
        (300, false, Uuid::new_v4(), "not_found"),
        (300, true, Uuid::from_u128(0x2026_0905), "completed"),
        (300, false, Uuid::from_u128(0x2026_0905), "conflict"),
        (i64::MAX, false, Uuid::from_u128(0x2026_0905), "overflow"),
    ] {
        let fixture = WebReadServiceFixture::new();
        fixture.seed_fixed_task_with_actual(
            operation_now - Duration::hours(1),
            actual_work_seconds,
            completed,
        );
        let before = fixture.persisted_bytes();
        let mut service = WebService::new(fixture.storage.clone(), fixture.config());
        let expected = if expected_kind == "conflict" {
            actual_work_seconds - 1
        } else {
            actual_work_seconds
        };
        let started_at_epoch_ms = if expected_kind == "overflow" {
            operation_now.timestamp_millis() - 1_000
        } else {
            operation_now.timestamp_millis()
        };

        let error = service
            .record_session_at(
                operation_now,
                RecordSessionRequest {
                    task_id: task_id.to_string(),
                    started_at_epoch_ms,
                    ended_at_epoch_ms: None,
                    expected_actual_work_seconds: expected,
                },
            )
            .unwrap_err();

        let application_error = match error {
            WebReadError::Application(error) => error,
            other => panic!("unexpected error: {other:?}"),
        };
        assert!(matches!(
            (expected_kind, application_error),
            (
                "not_found",
                crate::application::task_use_case::ApplicationError::TaskNotFound(_)
            ) | (
                "completed",
                crate::application::task_use_case::ApplicationError::TaskAlreadyCompleted(_),
            ) | (
                "conflict",
                crate::application::task_use_case::ApplicationError::ActualWorkConflict { .. },
            ) | (
                "overflow",
                crate::application::task_use_case::ApplicationError::InvalidInput {
                    field: "additional_actual_work_seconds",
                    reason: "actual work seconds overflow",
                },
            )
        ));
        assert_eq!(fixture.persisted_bytes(), before);
    }
}

#[test]
fn complete_sessionは経過秒加算と完了を1回の保存で反映してsnapshotだけを返す() {
    let seeded_at = Local.with_ymd_and_hms(2026, 9, 5, 18, 0, 0).unwrap();
    let operation_now =
        Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap() + Duration::milliseconds(999);
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(seeded_at);
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let started_at_epoch_ms = operation_now.timestamp_millis() - 65_999;
    let ended_at_epoch_ms = operation_now.timestamp_millis() - 5_999;
    let response = service
        .complete_session_at(
            operation_now,
            CompleteSessionRequest {
                task_id: task_id.to_string(),
                started_at_epoch_ms,
                ended_at_epoch_ms: Some(ended_at_epoch_ms),
                expected_actual_work_seconds: 300,
                record_elapsed_seconds: true,
            },
        )
        .unwrap();

    assert_eq!(
        response.observed_at_epoch_ms,
        operation_now.timestamp_millis()
    );
    assert_eq!(response.logical_date, "2026-09-05");
    let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
    repository.reload_if_changed(operation_now).unwrap();
    let completed = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(completed.get_actual_work_seconds().unwrap(), 360);
    assert_eq!(completed.get_status().unwrap(), Status::Done);
    assert_eq!(
        completed
            .get_end_time_opt()
            .unwrap()
            .map(|finished_at| finished_at.timestamp()),
        Some(ended_at_epoch_ms / 1_000)
    );
}

#[test]
fn complete_sessionはbrowser時計がserverより進んでいても未来の完了時刻を保存しない() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let browser_ended_at = operation_now + Duration::minutes(5);

    for (record_elapsed_seconds, expected_actual_work_seconds) in [(true, 360), (false, 300)] {
        let fixture = WebReadServiceFixture::new();
        let task_id = fixture.seed_fixed_task(operation_now - Duration::hours(1));
        let mut service = WebService::new(fixture.storage.clone(), fixture.config());

        service
            .complete_session_at(
                operation_now,
                CompleteSessionRequest {
                    task_id: task_id.to_string(),
                    started_at_epoch_ms: browser_ended_at.timestamp_millis() - 60_000,
                    ended_at_epoch_ms: Some(browser_ended_at.timestamp_millis()),
                    expected_actual_work_seconds: 300,
                    record_elapsed_seconds,
                },
            )
            .unwrap();

        let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
        repository.reload_if_changed(operation_now).unwrap();
        let completed = repository.get_by_id(task_id).unwrap().unwrap();
        assert_eq!(
            completed.get_actual_work_seconds().unwrap(),
            expected_actual_work_seconds
        );
        assert_eq!(
            completed
                .get_end_time_opt()
                .unwrap()
                .map(|finished_at| finished_at.timestamp()),
            Some(operation_now.timestamp())
        );
    }
}

#[test]
fn complete_sessionは計測破棄指定時に実績を加算せずtaskを完了する() {
    let seeded_at = Local.with_ymd_and_hms(2026, 9, 5, 18, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(seeded_at);
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    service
        .complete_session_at(
            operation_now,
            CompleteSessionRequest {
                task_id: task_id.to_string(),
                started_at_epoch_ms: i64::MAX,
                ended_at_epoch_ms: None,
                expected_actual_work_seconds: 300,
                record_elapsed_seconds: false,
            },
        )
        .unwrap();

    let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
    repository.reload_if_changed(operation_now).unwrap();
    let completed = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(completed.get_actual_work_seconds().unwrap(), 300);
    assert_eq!(completed.get_status().unwrap(), Status::Done);
    assert_eq!(
        completed
            .get_end_time_opt()
            .unwrap()
            .map(|finished_at| finished_at.timestamp()),
        Some(operation_now.timestamp())
    );
}

#[test]
fn complete_sessionは計測破棄指定でも期待実績競合時に状態を変更しない() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(operation_now - Duration::hours(1));
    let before = fixture.persisted_bytes();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let error = service
        .complete_session_at(
            operation_now,
            CompleteSessionRequest {
                task_id: task_id.to_string(),
                started_at_epoch_ms: i64::MAX,
                ended_at_epoch_ms: Some(operation_now.timestamp_millis() + 300_000),
                expected_actual_work_seconds: 299,
                record_elapsed_seconds: false,
            },
        )
        .unwrap_err();

    assert!(matches!(
        error,
        WebReadError::Application(
            crate::application::task_use_case::ApplicationError::ActualWorkConflict { .. }
        )
    ));
    assert_eq!(fixture.persisted_bytes(), before);
}

#[test]
fn complete_sessionは期待実績競合時にtaskと永続dataを変更しない() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(operation_now - Duration::hours(1));
    let before = fixture.persisted_bytes();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let error = service
        .complete_session_at(
            operation_now,
            CompleteSessionRequest {
                task_id: task_id.to_string(),
                started_at_epoch_ms: operation_now.timestamp_millis() + 240_000,
                ended_at_epoch_ms: Some(operation_now.timestamp_millis() + 300_000),
                expected_actual_work_seconds: 299,
                record_elapsed_seconds: true,
            },
        )
        .unwrap_err();

    assert!(matches!(
        error,
        WebReadError::Application(
            crate::application::task_use_case::ApplicationError::ActualWorkConflict { .. }
        )
    ));
    assert_eq!(fixture.persisted_bytes(), before);
}

#[test]
fn complete_sessionは記録方針にかかわらず反復task生成と元task完了を同じ保存で反映する() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    for (record_elapsed_seconds, expected_actual_work_seconds) in [(true, 360), (false, 300)] {
        let fixture = WebReadServiceFixture::new();
        let (parent_id, child_id) =
            fixture.seed_repetition_task(operation_now - Duration::hours(1));
        let mut service = WebService::new(fixture.storage.clone(), fixture.config());

        service
            .complete_session_at(
                operation_now,
                CompleteSessionRequest {
                    task_id: child_id.to_string(),
                    started_at_epoch_ms: operation_now.timestamp_millis() - 60_000,
                    ended_at_epoch_ms: None,
                    expected_actual_work_seconds: 300,
                    record_elapsed_seconds,
                },
            )
            .unwrap();

        let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
        repository.reload_if_changed(operation_now).unwrap();
        let parent = repository.get_by_id(parent_id).unwrap().unwrap();
        let children = parent.get_children().unwrap();
        assert_eq!(children.len(), 2);
        let completed = repository.get_by_id(child_id).unwrap().unwrap();
        assert_eq!(completed.get_status().unwrap(), Status::Done);
        assert_eq!(
            completed.get_actual_work_seconds().unwrap(),
            expected_actual_work_seconds
        );
        assert!(children
            .iter()
            .any(|child| child.get_id().unwrap() != child_id));
    }
}

#[test]
fn complete_sessionは計測破棄指定でも未完了childがあれば保存しない() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let parent_id = fixture.seed_fixed_task(operation_now - Duration::hours(1));
    let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
    repository.reload_if_changed(operation_now).unwrap();
    repository
        .get_by_id(parent_id)
        .unwrap()
        .unwrap()
        .create_child(TaskAttr::with_identity(
            "undone child",
            Uuid::from_u128(0x2026_0905_0003),
            operation_now,
        ))
        .unwrap();
    repository.save().unwrap();
    let before = fixture.persisted_bytes();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let error = service
        .complete_session_at(
            operation_now,
            CompleteSessionRequest {
                task_id: parent_id.to_string(),
                started_at_epoch_ms: i64::MAX,
                ended_at_epoch_ms: None,
                expected_actual_work_seconds: 300,
                record_elapsed_seconds: false,
            },
        )
        .unwrap_err();

    assert!(matches!(
        error,
        WebReadError::Application(
            crate::application::task_use_case::ApplicationError::HasUndoneChildren(id)
        ) if id == parent_id
    ));
    assert_eq!(fixture.persisted_bytes(), before);
}

#[test]
fn complete_sessionは計測破棄指定のcommit前保存失敗後に同一requestを再試行できる() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(operation_now - Duration::hours(1));
    let request = CompleteSessionRequest {
        task_id: task_id.to_string(),
        started_at_epoch_ms: i64::MAX,
        ended_at_epoch_ms: None,
        expected_actual_work_seconds: 300,
        record_elapsed_seconds: false,
    };
    let io = Arc::new(RecordingIo::new(vec![FaultRule {
        operation: RecordingOperation::CreateDirectory,
        path_matcher: PathMatcher::FileName(".schronu-transactions"),
        occurrence: 1,
        error_kind: std::io::ErrorKind::Other,
        error_message: "injected pre-commit failure",
    }]));
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());
    service.set_mutation_repository_factory(move |storage_path| {
        TaskRepository::new_with_storage_transaction_io(storage_path, io.clone())
    });

    assert!(matches!(
        service.complete_session_at(operation_now, request.clone()),
        Err(WebReadError::RepositorySaveFailed(_))
    ));
    service.complete_session_at(operation_now, request).unwrap();

    let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
    repository.reload_if_changed(operation_now).unwrap();
    let completed = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(completed.get_actual_work_seconds().unwrap(), 300);
    assert_eq!(completed.get_status().unwrap(), Status::Done);
}

#[test]
fn record_sessionはcommit前save失敗後に同一requestを再試行できる() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(operation_now - Duration::hours(1));
    let request = RecordSessionRequest {
        task_id: task_id.to_string(),
        started_at_epoch_ms: operation_now.timestamp_millis() - 60_000,
        ended_at_epoch_ms: None,
        expected_actual_work_seconds: 300,
    };
    let io = Arc::new(RecordingIo::new(vec![FaultRule {
        operation: RecordingOperation::CreateDirectory,
        path_matcher: PathMatcher::FileName(".schronu-transactions"),
        occurrence: 1,
        error_kind: std::io::ErrorKind::Other,
        error_message: "injected pre-commit failure",
    }]));
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());
    service.set_mutation_repository_factory(move |storage_path| {
        TaskRepository::new_with_storage_transaction_io(storage_path, io.clone())
    });

    assert!(matches!(
        service.record_session_at(operation_now, request.clone()),
        Err(WebReadError::RepositorySaveFailed(_))
    ));
    assert_eq!(
        service
            .record_session_at(operation_now, request)
            .unwrap()
            .data
            .actual_work_seconds,
        360
    );
}

#[test]
fn record_sessionはcommit後save失敗で後続mutationを拒否する() {
    let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let task_id = fixture.seed_fixed_task(operation_now - Duration::hours(1));
    let request = RecordSessionRequest {
        task_id: task_id.to_string(),
        started_at_epoch_ms: operation_now.timestamp_millis() - 60_000,
        ended_at_epoch_ms: None,
        expected_actual_work_seconds: 300,
    };
    let io = Arc::new(RecordingIo::new(vec![FaultRule {
        operation: RecordingOperation::WriteFile,
        path_matcher: PathMatcher::FileNamePrefix(".project.yaml."),
        occurrence: 1,
        error_kind: std::io::ErrorKind::Other,
        error_message: "injected post-commit failure",
    }]));
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());
    service.set_mutation_repository_factory(move |storage_path| {
        TaskRepository::new_with_storage_transaction_io(storage_path, io.clone())
    });

    assert!(matches!(
        service.record_session_at(operation_now, request.clone()),
        Err(WebReadError::RepositoryStateUncertain(_))
    ));
    assert!(matches!(
        service.record_session_at(operation_now, request),
        Err(WebReadError::RepositoryPoisoned)
    ));
}
