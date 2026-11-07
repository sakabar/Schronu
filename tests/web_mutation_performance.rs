//! Manual storage-backed timing of the Web mutation -> list -> load sequence.
//! Run with TZ=Asia/Tokyo cargo test --locked --release --test web_mutation_performance -- --ignored --nocapture

#[allow(dead_code)]
#[path = "support/scheduling_fixture.rs"]
mod scheduling_fixture;

use chrono::{Duration, Local, TimeZone};
use scheduling_fixture::{FixtureSize, SchedulingFixture};
use schronu::adapter::controller::{
    CompleteSessionRequest, DeferPlanRequest, DeferTaskRequest, RecordSessionRequest, WebService,
};
use schronu::adapter::gateway::schronu_config::SchronuConfig;
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;
use schronu::application::task_use_case::DeferMode;
use schronu::entity::task::TaskHandle;
use std::{fs, path::PathBuf, time::Instant};
use uuid::Uuid;

struct TemporaryStorage(PathBuf);

impl Drop for TemporaryStorage {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove synthetic storage");
    }
}

// Only copies the synthetic directory created by this test; fixture setup is untimed.
fn copy_directory(source: &std::path::Path, destination: &std::path::Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &path);
        } else {
            assert!(entry.file_type().unwrap().is_file());
            fs::copy(entry.path(), path).unwrap();
        }
    }
}

fn timed<T>(operation: impl FnOnce() -> T) -> (T, f64) {
    let start = Instant::now();
    let value = operation();
    (value, start.elapsed().as_secs_f64() * 1000.0)
}

#[test]
#[ignore = "manual release-mode storage-backed performance measurement"]
fn measure_web_mutation_sequence() {
    for size in [
        FixtureSize::Small,
        FixtureSize::Typical,
        FixtureSize::Stress,
    ] {
        let mut seed_storage: Option<TemporaryStorage> = None;
        for operation in ["record", "complete", "defer"] {
            for sample in 0..3 {
                let fixture = SchedulingFixture::build(size).unwrap();
                let summary = fixture.summary().unwrap();
                let now = fixture.now;
                let root = TemporaryStorage(
                    std::env::temp_dir()
                        .join(format!("schronu-web-performance-{}", Uuid::new_v4())),
                );
                let storage = root.0.join("storage");
                fs::create_dir_all(&storage).unwrap();
                let busy = root.0.join("busy.yaml");
                let mut yaml = String::from("days_of_week:\n");
                for day in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
                    yaml.push_str(&format!("  - day_of_week: {day}\n    busy_time_slots:\n      - start_time: '20:00'\n        duration_minutes: 60\n        name: recurring unavailable interval\n"));
                }
                fs::write(&busy, yaml).unwrap();
                let target_id = Uuid::from_u128(0xfeed_1000);
                let target =
                    TaskHandle::with_identity("measurement-target", target_id, now).unwrap();
                target.set_estimated_work_seconds(600).unwrap();
                if let Some(seed) = &seed_storage {
                    copy_directory(&seed.0, &storage);
                } else {
                    let mut repository = TaskRepository::new(storage.to_str().unwrap());
                    repository.sync_clock(now).unwrap();
                    repository.load().unwrap();
                    for project in fixture.projects {
                        repository.start_new_project(project).unwrap();
                    }
                    repository.start_new_project(target).unwrap();
                    repository.save().unwrap();
                    drop(repository);
                    let seed = TemporaryStorage(
                        std::env::temp_dir()
                            .join(format!("schronu-web-performance-seed-{}", Uuid::new_v4())),
                    );
                    copy_directory(&storage, &seed.0);
                    seed_storage = Some(seed);
                }
                let mut service = WebService::new(
                    storage.clone(),
                    SchronuConfig {
                        busy_time_slots_yaml_path: busy,
                        end_of_day_offset_minutes: 120,
                        ..SchronuConfig::default()
                    },
                );
                // Establish the same warm service state as a loaded Web screen.
                let (_, bootstrap_ms) = timed(|| service.bootstrap_at(now).unwrap());
                service.list_tasks_at(now, now.date_naive()).unwrap();
                service.load_band_at(now).unwrap();
                let (_, mutation_ms) = timed(|| match operation {
                    "record" => {
                        let result = service
                            .record_session_at(
                                now,
                                RecordSessionRequest {
                                    task_id: target_id.to_string(),
                                    started_at_epoch_ms: now.timestamp_millis() - 60_000,
                                    ended_at_epoch_ms: Some(now.timestamp_millis()),
                                    expected_actual_work_seconds: 0,
                                },
                            )
                            .unwrap();
                        assert_eq!(result.data.actual_work_seconds, 60);
                    }
                    "complete" => {
                        service
                            .complete_session_at(
                                now,
                                CompleteSessionRequest {
                                    task_id: target_id.to_string(),
                                    started_at_epoch_ms: now.timestamp_millis() - 60_000,
                                    ended_at_epoch_ms: Some(now.timestamp_millis()),
                                    expected_actual_work_seconds: 0,
                                    record_elapsed_seconds: true,
                                },
                            )
                            .unwrap();
                    }
                    "defer" => {
                        let tomorrow = now.date_naive() + Duration::days(1);
                        let pending = Local
                            .from_local_datetime(&tomorrow.and_hms_opt(6, 0, 0).unwrap())
                            .single()
                            .unwrap();
                        service
                            .defer_task_at(
                                now,
                                DeferTaskRequest {
                                    task_id: target_id.to_string(),
                                    selected_logical_date: now.date_naive().to_string(),
                                    expected_plan: DeferPlanRequest {
                                        mode: DeferMode::Normal,
                                        requested_pending_until_epoch_ms: pending
                                            .timestamp_millis(),
                                        effective_pending_until_epoch_ms: None,
                                        repetition_interval_days: None,
                                    },
                                },
                            )
                            .unwrap();
                    }
                    _ => unreachable!(),
                });
                let (_, list_ms) = timed(|| service.list_tasks_at(now, now.date_naive()).unwrap());
                let (_, load_ms) = timed(|| service.load_band_at(now).unwrap());
                // Diagnostic controls outside the measured user operation sequence.
                let (_, cached_list_ms) =
                    timed(|| service.list_tasks_at(now, now.date_naive()).unwrap());
                let mut fresh = TaskRepository::new(storage.to_str().unwrap());
                let (_, fresh_reload_ms) = timed(|| fresh.reload_if_changed(now).unwrap());
                let (_, schedule_ms) = timed(|| {
                    schronu::application::schedule_use_case::get_schedule(&fresh).unwrap()
                });
                println!("web_sequence size={size:?} tasks={} projects={} operation={operation} sample={sample} bootstrap_ms={bootstrap_ms:.3} mutation_ms={mutation_ms:.3} list_ms={list_ms:.3} load_ms={load_ms:.3} total_ms={:.3} cached_list_ms={cached_list_ms:.3} fresh_reload_ms={fresh_reload_ms:.3} schedule_ms={schedule_ms:.3}", summary.tasks + 1, summary.projects + 1, mutation_ms + list_ms + load_ms);
            }
        }
    }
}
