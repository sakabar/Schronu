use super::*;
use crate::adapter::gateway::storage_transaction_test_support::{
    FaultRule, PathMatcher, RecordingIo, RecordingOperation,
};
use crate::application::interface::RepositoryReloadOutcome;
use crate::application::task_use_case::DeferMode;
use crate::entity::task::{Status, TaskHandle};
use chrono::{Duration, TimeZone};
use std::{fs, sync::Arc};

struct Fixture {
    root: PathBuf,
    now: DateTime<Local>,
    id: Uuid,
    service: WebService,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("schronu-cache-contract-{}", Uuid::new_v4()));
        let storage = root.join("storage");
        fs::create_dir_all(&storage).unwrap();
        let busy = root.join("busy.yaml");
        let mut yaml = String::from("days_of_week:\n");
        for day in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
            yaml.push_str(&format!("  - day_of_week: {day}\n    busy_time_slots:\n      - start_time: '20:00'\n        duration_minutes: 60\n        name: recurring unavailable interval\n"));
        }
        fs::write(&busy, yaml).unwrap();
        let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
        let id = Uuid::new_v4();
        let task = TaskHandle::with_identity("cache-contract-target", id, now).unwrap();
        task.set_estimated_work_seconds(600).unwrap();
        let mut repository = TaskRepository::new(storage.to_str().unwrap());
        repository.sync_clock(now).unwrap();
        repository.load().unwrap();
        repository.start_new_project(task).unwrap();
        repository.save().unwrap();
        let mut service = WebService::new(
            storage,
            SchronuConfig {
                busy_time_slots_yaml_path: busy,
                ..SchronuConfig::default()
            },
        );
        service.list_tasks_at(now, now.date_naive()).unwrap();
        Self {
            root,
            now,
            id,
            service,
        }
    }

    fn request(&self) -> RecordSessionRequest {
        RecordSessionRequest {
            task_id: self.id.to_string(),
            started_at_epoch_ms: self.now.timestamp_millis() - 60_000,
            ended_at_epoch_ms: Some(self.now.timestamp_millis()),
            expected_actual_work_seconds: 0,
        }
    }

    fn assert_cache_current(&mut self) {
        let persisted =
            fs::read_to_string(self.service.storage_directory.join(".revision")).unwrap();
        let repository = self.service.task_repository.as_mut().unwrap();
        assert_eq!(
            repository.repository_revision(),
            Some(Uuid::parse_str(persisted.trim()).unwrap())
        );
        assert_eq!(
            repository.reload_if_changed(self.now).unwrap(),
            RepositoryReloadOutcome::Cached
        );
    }

    fn fail_save(&mut self, post_commit: bool) {
        let io = Arc::new(RecordingIo::new(vec![FaultRule {
            operation: if post_commit {
                RecordingOperation::WriteFile
            } else {
                RecordingOperation::CreateDirectory
            },
            path_matcher: if post_commit {
                PathMatcher::FileNamePrefix(".project.yaml.")
            } else {
                PathMatcher::FileName(".schronu-transactions")
            },
            occurrence: 1,
            error_kind: std::io::ErrorKind::Other,
            error_message: "injected save failure",
        }]));
        self.service.set_mutation_repository_factory(move |path| {
            TaskRepository::new_with_storage_transaction_io(path, io.clone())
        });
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn successful_mutations_publish_committed_cache() {
    for operation in ["record", "complete", "defer", "create", "slice"] {
        let mut f = Fixture::new();
        let request = f.request();
        match operation {
            "record" => {
                f.service.record_session_at(f.now, request).unwrap();
            }
            "complete" => {
                f.service
                    .complete_session_at(
                        f.now,
                        CompleteSessionRequest {
                            task_id: request.task_id,
                            started_at_epoch_ms: request.started_at_epoch_ms,
                            ended_at_epoch_ms: request.ended_at_epoch_ms,
                            expected_actual_work_seconds: 0,
                            record_elapsed_seconds: true,
                        },
                    )
                    .unwrap();
            }
            "defer" => {
                f.service
                    .defer_task_at(
                        f.now,
                        DeferTaskRequest {
                            task_id: request.task_id,
                            selected_logical_date: f.now.date_naive().to_string(),
                            expected_plan: super::super::web_session_write::DeferPlanRequest {
                                mode: DeferMode::Normal,
                                requested_pending_until_epoch_ms: (f.now + Duration::days(1))
                                    .date_naive()
                                    .and_hms_opt(6, 0, 0)
                                    .unwrap()
                                    .and_local_timezone(Local)
                                    .single()
                                    .unwrap()
                                    .timestamp_millis(),
                                effective_pending_until_epoch_ms: None,
                                repetition_interval_days: None,
                            },
                        },
                    )
                    .unwrap();
            }
            "create" => {
                f.service
                    .create_task_at(f.now, "created-cache-target".to_owned(), Some(10))
                    .unwrap();
            }
            "slice" => {
                f.service.complete_work_slice_at(f.now, request).unwrap();
            }
            _ => unreachable!(),
        }
        f.assert_cache_current();
        let rows = f
            .service
            .list_tasks_at(f.now, f.now.date_naive())
            .unwrap()
            .data;
        let target = rows
            .iter()
            .find(|row| row.task.task_id.as_deref() == Some(f.id.to_string().as_str()));
        if operation == "record" {
            assert_eq!(target.unwrap().task.actual_work_seconds, 60);
        }
        if matches!(operation, "complete" | "defer") {
            assert!(target.is_none());
        }
        if operation == "slice" {
            assert_eq!(target.unwrap().task.estimated_work_seconds, 540);
        }
        if operation == "create" {
            let repository = f.service.task_repository.as_ref().unwrap();
            assert_eq!(repository.get_all_projects().len(), 2);
        }
    }
}

#[test]
fn external_save_after_handoff_is_reloaded() {
    let mut f = Fixture::new();
    f.service.record_session_at(f.now, f.request()).unwrap();
    f.assert_cache_current();
    let mut external = TaskRepository::new(f.service.storage_directory.to_str().unwrap());
    external.reload_if_changed(f.now).unwrap();
    external
        .get_by_id(f.id)
        .unwrap()
        .unwrap()
        .set_actual_work_seconds(120)
        .unwrap();
    external.save().unwrap();
    let rows = f
        .service
        .list_tasks_at(f.now, f.now.date_naive())
        .unwrap()
        .data;
    assert_eq!(
        rows.iter()
            .find(|row| row.task.task_id.as_deref() == Some(f.id.to_string().as_str()))
            .unwrap()
            .task
            .actual_work_seconds,
        120
    );
    f.assert_cache_current();
}

#[test]
fn failed_mutations_preserve_read_cache_and_retry_safety() {
    for failure in ["operation", "pre_commit", "post_commit"] {
        let mut f = Fixture::new();
        let before = f
            .service
            .task_repository
            .as_ref()
            .unwrap()
            .repository_revision();
        let mut request = f.request();
        if failure == "operation" {
            request.expected_actual_work_seconds = 1;
        } else {
            f.fail_save(failure == "post_commit");
        }
        assert!(f.service.record_session_at(f.now, request.clone()).is_err());
        let cache = f.service.task_repository.as_ref().unwrap();
        assert_eq!(cache.repository_revision(), before);
        let task = cache.get_by_id(f.id).unwrap().unwrap();
        assert_eq!(task.get_actual_work_seconds().unwrap(), 0);
        assert_eq!(task.get_status().unwrap(), Status::Todo);
        if failure == "post_commit" {
            assert!(matches!(
                f.service.record_session_at(f.now, request),
                Err(WebReadError::RepositoryPoisoned)
            ));
        } else {
            let rows = f
                .service
                .list_tasks_at(f.now, f.now.date_naive())
                .unwrap()
                .data;
            assert_eq!(rows[0].task.actual_work_seconds, 0);
            f.service.record_session_at(f.now, f.request()).unwrap();
            f.assert_cache_current();
            assert_eq!(
                f.service
                    .task_repository
                    .as_ref()
                    .unwrap()
                    .get_by_id(f.id)
                    .unwrap()
                    .unwrap()
                    .get_actual_work_seconds()
                    .unwrap(),
                60
            );
        }
    }
}
