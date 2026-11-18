use super::*;
use crate::adapter::controller::{OfflineOperationDto, OfflineOperationKind};
use chrono::DateTime;

fn operation(
    kind: OfflineOperationKind,
    task_id: Uuid,
    start: DateTime<Local>,
    seconds: i64,
    expected_actual: i64,
) -> OfflineOperationDto {
    OfflineOperationDto {
        operation_id: Uuid::new_v4(),
        task_id,
        kind,
        started_at_epoch_ms: start.timestamp_millis(),
        ended_at_epoch_ms: (start + Duration::seconds(seconds)).timestamp_millis(),
        expected_actual_work_seconds: expected_actual,
        expected_estimated_work_seconds: None,
    }
}

#[test]
fn 保存範囲と分割可否は同じ時点のオンライン一覧と負荷に一致する() {
    let fixture = WebReadServiceFixture::new();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 5, 59, 59).unwrap();
    let (routine, occurrence) = fixture.seed_repetition_task(now);
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());
    let bundle = service.prepare_offline_at(now).unwrap();
    assert_eq!(bundle.snapshot.logical_date, "2026-09-04");
    assert_eq!(bundle.days.len(), 7);
    assert_eq!(bundle.days.last_key_value().unwrap().0, "2026-09-10");
    assert_eq!(bundle.load.band_days.len(), 14);
    assert_eq!(bundle.load.routine_load.horizon_day_count, 28);
    assert_eq!(bundle.load, service.load_band_at(now).unwrap().data);
    assert!(bundle.splittable_task_ids.contains(&occurrence.to_string()));
    assert!(!bundle.splittable_task_ids.contains(&routine.to_string()));
    for (date, rows) in bundle.days {
        assert_eq!(
            rows,
            service
                .list_tasks_at(now, date.parse().unwrap())
                .unwrap()
                .data
        );
    }
}

#[test]
fn 記録分割完了の再送は終了時刻と回数を保持する() {
    let fixture = WebReadServiceFixture::new();
    let start = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 0).unwrap();
    let now = start + Duration::hours(5);
    let task_id = fixture.seed_unconstrained_task(start);
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());

    let record = operation(OfflineOperationKind::Record, task_id, start, 40, 0);
    let mut split = operation(
        OfflineOperationKind::Split,
        task_id,
        start + Duration::seconds(40),
        300,
        40,
    );
    split.expected_estimated_work_seconds = Some(1800);
    let complete = operation(
        OfflineOperationKind::Complete,
        task_id,
        start + Duration::seconds(340),
        180,
        40,
    );
    for op in [&record, &split, &complete] {
        service.sync_offline_operation_at(now, op.clone()).unwrap();
    }
    let revision = fs::read(fixture.storage.join(".revision")).unwrap();
    let mut restarted = WebService::new(fixture.storage.clone(), fixture.config());
    for op in [&record, &split, &complete] {
        restarted
            .sync_offline_operation_at(now, op.clone())
            .unwrap();
    }
    assert_eq!(
        revision,
        fs::read(fixture.storage.join(".revision")).unwrap()
    );
    let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
    repository.reload_if_changed(now).unwrap();
    let parent = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(parent.get_actual_work_seconds().unwrap(), 220);
    assert_eq!(parent.get_estimated_work_seconds().unwrap(), 1500);
    assert_eq!(
        parent.get_end_time_opt().unwrap(),
        Some(start + Duration::seconds(520))
    );
    let children = parent.get_children().unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_actual_work_seconds().unwrap(), 300);
    assert_eq!(
        children[0].get_end_time_opt().unwrap(),
        Some(start + Duration::seconds(340))
    );
}

#[test]
fn 競合した分割を確定せず内容が異なる再送を拒否する() {
    let fixture = WebReadServiceFixture::new();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 0).unwrap();
    let task_id = fixture.seed_unconstrained_task(now);
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());
    let mut split = operation(
        OfflineOperationKind::Split,
        task_id,
        now - Duration::seconds(60),
        60,
        0,
    );
    split.expected_estimated_work_seconds = Some(2000);
    assert!(matches!(
        service.sync_offline_operation_at(now, split.clone()),
        Err(WebReadError::OfflineConflict)
    ));
    split.expected_estimated_work_seconds = Some(1800);
    service
        .sync_offline_operation_at(now, split.clone())
        .unwrap();
    split.started_at_epoch_ms += 1000;
    assert!(matches!(
        service.sync_offline_operation_at(now, split),
        Err(WebReadError::OfflineConflict)
    ));
}

#[test]
fn commit後の障害から復旧した直後の再送でも子は一つだけ作られる() {
    let fixture = WebReadServiceFixture::new();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 0).unwrap();
    let task_id = fixture.seed_unconstrained_task(now);
    let mut split = operation(
        OfflineOperationKind::Split,
        task_id,
        now - Duration::seconds(60),
        60,
        0,
    );
    split.expected_estimated_work_seconds = Some(1800);
    let io = Arc::new(RecordingIo::new(vec![FaultRule {
        operation: RecordingOperation::WriteFile,
        path_matcher: PathMatcher::FileNamePrefix("..schronu-offline-receipts.json."),
        occurrence: 1,
        error_kind: std::io::ErrorKind::Other,
        error_message: "injected receipt publication failure",
    }]));
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());
    service.set_mutation_repository_factory(move |storage_path| {
        TaskRepository::new_with_storage_transaction_io(storage_path, io.clone())
    });
    assert!(matches!(
        service.sync_offline_operation_at(now, split.clone()),
        Err(WebReadError::RepositoryStateUncertain(_))
    ));
    let mut restarted = WebService::new(fixture.storage.clone(), fixture.config());
    restarted.sync_offline_operation_at(now, split).unwrap();
    let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
    repository.reload_if_changed(now).unwrap();
    let task = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(task.get_children().unwrap().len(), 1);
    assert_eq!(task.get_estimated_work_seconds().unwrap(), 1740);
}

#[test]
fn 完了の再送で次回の繰返タスクを重複生成しない() {
    let fixture = WebReadServiceFixture::new();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 0).unwrap();
    let (routine, occurrence) = fixture.seed_repetition_task(now);
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());
    let complete = operation(
        OfflineOperationKind::Complete,
        occurrence,
        now - Duration::seconds(30),
        30,
        300,
    );
    service
        .sync_offline_operation_at(now, complete.clone())
        .unwrap();
    let mut restarted = WebService::new(fixture.storage.clone(), fixture.config());
    restarted.sync_offline_operation_at(now, complete).unwrap();
    let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
    repository.reload_if_changed(now).unwrap();
    let children = repository
        .get_by_id(routine)
        .unwrap()
        .unwrap()
        .get_children()
        .unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(
        repository
            .get_by_id(occurrence)
            .unwrap()
            .unwrap()
            .get_actual_work_seconds()
            .unwrap(),
        330
    );
}

#[test]
fn current_restoreは実績と処理済み記録を同じ世代へ戻す() {
    use crate::adapter::gateway::storage_snapshot::{
        create_snapshot, restore_current_snapshot, verify_snapshot,
    };
    let fixture = WebReadServiceFixture::new();
    let now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 0).unwrap();
    let task_id = fixture.seed_unconstrained_task(now);
    let old = fixture.root.join("before-record");
    let new = fixture.root.join("after-record");
    create_snapshot(&fixture.storage, &old).unwrap();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());
    let record = operation(
        OfflineOperationKind::Record,
        task_id,
        now - Duration::seconds(60),
        60,
        0,
    );
    service
        .sync_offline_operation_at(now, record.clone())
        .unwrap();
    create_snapshot(&fixture.storage, &new).unwrap();
    for (source, pre_backup_name, expected_actual) in [
        (&old, "current-before-old-restore", 0),
        (&new, "current-before-new-restore", 60),
    ] {
        let backup = fixture.root.join(pre_backup_name);
        {
            let lock = StorageLock::acquire(&fixture.storage, LockMode::Cli).unwrap();
            restore_current_snapshot(&fixture.storage, source, &backup, &lock).unwrap();
        }
        verify_snapshot(&backup).unwrap();
        let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
        repository.reload_if_changed(now).unwrap();
        let actual = |repo: &TaskRepository| {
            repo.get_by_id(task_id)
                .unwrap()
                .unwrap()
                .get_actual_work_seconds()
                .unwrap()
        };
        assert_eq!(actual(&repository), expected_actual);
        let before = repository.repository_revision();
        service
            .sync_offline_operation_at(now, record.clone())
            .unwrap();
        repository.reload_if_changed(now).unwrap();
        assert_eq!(actual(&repository), 60);
        // The replay reapplies after rollback to the old generation, and
        // needs no save when the restored generation already contains it.
        assert_eq!(
            before == repository.repository_revision(),
            expected_actual == 60
        );
    }
}
