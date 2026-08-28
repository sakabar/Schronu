#![cfg(feature = "benchmarking")]

#[path = "support/scheduling_fixture.rs"]
mod scheduling_fixture;
#[path = "support/scheduling_harness.rs"]
mod scheduling_harness;

use scheduling_fixture::{FixtureSize, SchedulingFixture};
use scheduling_harness::{SchedulingFreeTimeManager, SchedulingRepository};
use schronu::application::benchmarking::{
    flatten_tasks_diagnostics, get_schedule_diagnostics, pack_tasks_diagnostics,
};
use schronu::application::flatten_use_case::flatten_tasks;
use schronu::application::pack_use_case::pack_tasks;
use schronu::application::schedule_use_case::get_schedule;

const DAILY_FREE_MINUTES: i64 = 8 * 60;

fn small_repository() -> SchedulingRepository {
    let fixture = SchedulingFixture::build(FixtureSize::Small).unwrap();
    SchedulingRepository::new(fixture.projects, fixture.now)
}

#[test]
fn benchmark_supportは全fixture規模と診断情報を公開する() {
    let supported_sizes = [
        FixtureSize::Small,
        FixtureSize::Typical,
        FixtureSize::Stress,
    ];
    assert_eq!(supported_sizes.len(), 3);

    let fixture = SchedulingFixture::build(FixtureSize::Small).unwrap();
    assert!(fixture.seed > 0);
    assert!(fixture.digest().unwrap() > 0);
    assert!(fixture.summary().unwrap().tasks > 0);
}

#[test]
fn schedule診断は通常経路と同じ結果を返し内部処理を計数する() {
    let expected = get_schedule(&small_repository()).unwrap();

    let (actual, metrics) = get_schedule_diagnostics(&small_repository()).unwrap();

    assert_eq!(actual, expected);
    assert_eq!(metrics.schedule_rebuild_count, 1);
    assert!(metrics.candidate_count > 0);
    assert_eq!(metrics.segment_count, actual.len());
    assert!(metrics.sort_count >= 3);
}

#[test]
fn typical_scheduleはslot探索とsortの上限内に収まる() {
    let fixture = SchedulingFixture::build(FixtureSize::Typical).unwrap();
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);

    let (_, metrics) = get_schedule_diagnostics(&repository).unwrap();

    assert_eq!(metrics.candidate_count, 1_755);
    assert_eq!(metrics.segment_count, 1_762);
    assert_eq!(metrics.schedule_rebuild_count, 1);
    assert!(
        metrics.occupied_slot_probe_count <= 20_000_000,
        "occupied slot probes exceeded the deterministic limit: {}",
        metrics.occupied_slot_probe_count
    );
    assert!(
        metrics.sort_count <= 4,
        "sorts exceeded the deterministic limit: {}",
        metrics.sort_count
    );
}

#[test]
fn pack診断は通常経路と同じ結果を返しschedule再構築を計数する() {
    let expected_repository = small_repository();
    let mut expected_free_time = SchedulingFreeTimeManager::new(DAILY_FREE_MINUTES);
    let expected = pack_tasks(&expected_repository, &mut expected_free_time).unwrap();
    let actual_repository = small_repository();
    let mut actual_free_time = SchedulingFreeTimeManager::new(DAILY_FREE_MINUTES);

    let (actual, metrics) =
        pack_tasks_diagnostics(&actual_repository, &mut actual_free_time).unwrap();

    assert_eq!(actual, expected);
    assert_eq!(
        actual_repository.task_states().unwrap(),
        expected_repository.task_states().unwrap()
    );
    assert!(metrics.schedule.schedule_rebuild_count > 0);
    assert!(metrics.schedule.candidate_count > 0);
    assert!(metrics.candidate_count > 0);
    assert!(metrics.placement_trial_count > 0);
}

#[test]
fn packは同一candidateの現在位置と日別余力に同じscheduleを使う() {
    let repository = small_repository();
    let mut free_time = SchedulingFreeTimeManager::new(DAILY_FREE_MINUTES);

    let (_, metrics) = pack_tasks_diagnostics(&repository, &mut free_time).unwrap();

    let rebuild_limit = 1 + metrics.candidate_count + metrics.placement_trial_count;
    assert!(
        metrics.schedule.schedule_rebuild_count <= rebuild_limit,
        "schedule rebuilds exceeded the per-candidate limit: {} > {}",
        metrics.schedule.schedule_rebuild_count,
        rebuild_limit
    );
}

#[test]
fn flatten診断は通常経路と同じ結果を返しschedule走査を計数する() {
    let expected_repository = small_repository();
    let mut expected_free_time = SchedulingFreeTimeManager::new(15);
    let expected = flatten_tasks(&expected_repository, &mut expected_free_time).unwrap();
    let actual_repository = small_repository();
    let mut actual_free_time = SchedulingFreeTimeManager::new(15);

    let (actual, metrics) =
        flatten_tasks_diagnostics(&actual_repository, &mut actual_free_time).unwrap();

    assert_eq!(actual, expected);
    assert_eq!(
        actual_repository.task_states().unwrap(),
        expected_repository.task_states().unwrap()
    );
    assert!(metrics.schedule.schedule_rebuild_count > 0);
    assert!(metrics.full_schedule_scan_element_count > 0);
    assert!(metrics.overload_iteration_count > 0);
    assert!(metrics.candidate_trial_count > 0);
    assert!(metrics.override_clone_element_count > 0);
}

#[test]
fn pack診断はatomic探索の1分前進量を計数する() {
    let repository = small_repository();
    let mut no_free_time = SchedulingFreeTimeManager::without_continuous_free_time(8 * 60);

    let (_, metrics) = pack_tasks_diagnostics(&repository, &mut no_free_time).unwrap();

    assert!(metrics.cursor_minute_advance_count > 0);
}
