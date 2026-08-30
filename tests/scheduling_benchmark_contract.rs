#![cfg(feature = "benchmarking")]

#[path = "support/scheduling_benchmark_limits.rs"]
mod scheduling_benchmark_limits;
#[path = "support/scheduling_fixture.rs"]
mod scheduling_fixture;
#[path = "support/scheduling_harness.rs"]
mod scheduling_harness;

use scheduling_benchmark_limits::FLATTEN_BENCHMARK_CAPACITY_MINUTES;
use scheduling_fixture::{FixtureSize, SchedulingFixture};
use scheduling_harness::{SchedulingFreeTimeManager, SchedulingRepository};
use schronu::application::benchmarking::{
    flatten_tasks_diagnostics, get_schedule_diagnostics, pack_tasks_diagnostics,
};
use schronu::application::flatten_use_case::flatten_tasks;
use schronu::application::pack_use_case::pack_tasks;
use schronu::application::schedule_use_case::get_schedule;
use std::time::{Duration, Instant};

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
    assert!(metrics.dependency_candidate_probe_count > 0);
    assert!(metrics.selection_event_count > 0);
    assert!(metrics.selection_candidate_probe_count > 0);
    assert!(metrics.release_candidate_probe_count > 0);
    assert!(metrics.slack_probe_count > 0);
    assert_eq!(metrics.segment_count, actual.len());
    assert_eq!(metrics.sort_count, 2);
}

#[test]
fn typical_scheduleはslot探索とsortの上限内に収まる() {
    let fixture = SchedulingFixture::build(FixtureSize::Typical).unwrap();
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);

    let (_, metrics) = get_schedule_diagnostics(&repository).unwrap();

    assert_eq!(metrics.candidate_count, 1_755);
    // fixed windowとslack/release境界による15分超のfragment分割を含む出力を固定する。
    assert_eq!(metrics.segment_count, 1_768);
    assert_eq!(metrics.schedule_rebuild_count, 1);
    assert!(
        metrics.occupied_slot_probe_count <= 100_000,
        "occupied slot probes exceeded the deterministic limit: {}",
        metrics.occupied_slot_probe_count
    );
    assert!(
        metrics.sort_count == 2,
        "sorts exceeded the deterministic limit: {}",
        metrics.sort_count
    );
}

#[test]
fn distinct_deadline_scheduleはdeadline_groupを対数探索する() {
    const CANDIDATE_COUNT: usize = 512;
    let fixture = SchedulingFixture::distinct_deadlines(CANDIDATE_COUNT).unwrap();
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);

    let (schedule, metrics) = get_schedule_diagnostics(&repository).unwrap();

    assert_eq!(metrics.candidate_count, CANDIDATE_COUNT);
    assert_eq!(schedule.len(), CANDIDATE_COUNT);
    assert!(metrics.selection_event_count >= CANDIDATE_COUNT);
    assert!(
        metrics.slack_probe_count <= CANDIDATE_COUNT * 128,
        "distinct deadline probes exceeded the logarithmic index limit: {} > {}",
        metrics.slack_probe_count,
        CANDIDATE_COUNT * 128
    );
}

#[test]
fn atomic_release探索はready候補とfuture_releaseの直積にならない() {
    const READY_ATOMIC_COUNT: usize = 32;
    const FUTURE_RELEASE_COUNT: usize = 32;
    let fixture =
        SchedulingFixture::atomic_release_adversarial(READY_ATOMIC_COUNT, FUTURE_RELEASE_COUNT)
            .unwrap();
    let now = fixture.now;
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);

    let started = Instant::now();
    let (schedule, metrics) = get_schedule_diagnostics(&repository).unwrap();
    let elapsed = started.elapsed();
    let candidate_count = READY_ATOMIC_COUNT + FUTURE_RELEASE_COUNT;

    assert!(
        metrics.release_candidate_probe_count <= candidate_count * 4,
        "atomic release probes exceeded the linear limit: {} > {}",
        metrics.release_candidate_probe_count,
        candidate_count * 4
    );
    assert!(
        elapsed <= Duration::from_secs(2),
        "atomic release adversarial fixture exceeded wall limit: {elapsed:?}"
    );
    assert_eq!(schedule.len(), candidate_count + 1);
    assert_eq!(
        schedule
            .iter()
            .map(|task| task.scheduled_work_seconds)
            .sum::<i64>(),
        candidate_count as i64 * 8 * 60 * 60 + 60 * 60
    );
    let fixed = schedule
        .iter()
        .find(|task| task.task.fixed_start)
        .expect("the adversarial fixture contains one fixed boundary");
    assert_eq!(fixed.scheduled_start, now + chrono::Duration::hours(4));
    assert_eq!(fixed.scheduled_end, now + chrono::Duration::hours(5));
    assert!(
        schedule
            .iter()
            .filter(|task| !task.task.fixed_start)
            .all(|task| task.scheduled_start >= fixed.scheduled_end),
        "atomic work must not be lost or placed across the fixed boundary"
    );
}

#[test]
fn atomic候補間でrelease予測を共有する() {
    const READY_ATOMIC_COUNT: usize = 64;
    const FUTURE_RELEASE_COUNT: usize = 32;
    let fixture = SchedulingFixture::atomic_release_prediction_adversarial(
        READY_ATOMIC_COUNT,
        FUTURE_RELEASE_COUNT,
    )
    .unwrap();
    let now = fixture.now;
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);

    let started = Instant::now();
    let (schedule, metrics) = get_schedule_diagnostics(&repository).unwrap();
    let elapsed = started.elapsed();
    let candidate_count = READY_ATOMIC_COUNT + FUTURE_RELEASE_COUNT;
    let probe_limit = candidate_count * 12;

    assert!(
        metrics.release_candidate_probe_count <= probe_limit,
        "atomic prediction probes exceeded the shared-timeline limit: {} > {}",
        metrics.release_candidate_probe_count,
        probe_limit
    );
    assert!(
        elapsed <= Duration::from_secs(2),
        "atomic prediction adversarial fixture exceeded wall limit: {elapsed:?}"
    );
    assert_eq!(schedule.len(), candidate_count);
    assert_eq!(
        schedule
            .iter()
            .map(|task| task.scheduled_work_seconds)
            .sum::<i64>(),
        candidate_count as i64 * 60 * 60
    );
    let preemptor = schedule
        .iter()
        .find(|task| task.task.name == "fixture-prediction-future-0031")
        .expect("the last future release is the common preemptor");
    assert_eq!(
        preemptor.scheduled_start,
        now + chrono::Duration::seconds(FUTURE_RELEASE_COUNT as i64)
    );
}

#[test]
fn atomic_release予測をevent間で再利用する() {
    let measure = |ready_atomic_count, future_release_count| {
        let fixture = SchedulingFixture::atomic_release_prediction_adversarial(
            ready_atomic_count,
            future_release_count,
        )
        .unwrap();
        let repository = SchedulingRepository::new(fixture.projects, fixture.now);
        let started = Instant::now();
        let metrics = get_schedule_diagnostics(&repository).unwrap().1;
        (metrics, started.elapsed())
    };

    let (small_metrics, _) = measure(64, 32);
    let (large_metrics, large_elapsed) = measure(256, 128);
    let small_probes = small_metrics.release_candidate_probe_count;
    let large_probes = large_metrics.release_candidate_probe_count;
    let large_candidate_count = 256 + 128;

    assert!(
        large_probes <= small_probes * 6,
        "event-scale release probes grew faster than the input: {large_probes} > {}",
        small_probes * 6
    );
    assert!(
        large_probes <= large_candidate_count * 12,
        "large event-scale release probes exceeded the linear limit: {large_probes} > {}",
        large_candidate_count * 12
    );
    assert_eq!(small_metrics.atomic_release_cache_peak_entry_count, 1);
    assert_eq!(large_metrics.atomic_release_cache_peak_entry_count, 1);
    assert!(small_metrics.atomic_release_cache_probe_count > 0);
    assert!(
        large_metrics.atomic_release_cache_probe_count
            <= small_metrics.atomic_release_cache_probe_count * 6,
        "event-scale cache probes grew faster than the input: {} > {}",
        large_metrics.atomic_release_cache_probe_count,
        small_metrics.atomic_release_cache_probe_count * 6
    );
    assert!(
        large_metrics.atomic_release_cache_probe_count <= large_candidate_count * 12,
        "large event-scale cache probes exceeded the linear limit: {} > {}",
        large_metrics.atomic_release_cache_probe_count,
        large_candidate_count * 12
    );
    assert!(
        large_metrics.atomic_release_cache_probe_count
            <= large_metrics.selection_candidate_probe_count
                + large_metrics.selection_event_count * 2,
        "cache bookkeeping exceeded candidate/event work: {} > {}",
        large_metrics.atomic_release_cache_probe_count,
        large_metrics.selection_candidate_probe_count + large_metrics.selection_event_count * 2
    );
    assert!(
        large_elapsed <= Duration::from_secs(2),
        "large event-scale fixture exceeded wall limit: {large_elapsed:?}"
    );
}

#[test]
fn 短fragment判定はfrontier全体を複製しない() {
    const FRAGMENT_COUNT: usize = 128;
    let fixture = SchedulingFixture::short_fragment_frontier_adversarial(FRAGMENT_COUNT).unwrap();
    let now = fixture.now;
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);

    let started = Instant::now();
    let (schedule, _) = get_schedule_diagnostics(&repository).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(schedule.len(), FRAGMENT_COUNT + 1);
    assert_eq!(
        schedule
            .iter()
            .map(|task| task.scheduled_work_seconds)
            .sum::<i64>(),
        (FRAGMENT_COUNT as i64 * 2 + 1) * 60,
        "speculative promotion must neither lose nor duplicate work"
    );
    assert!(
        schedule
            .iter()
            .filter(|task| task.task.name.starts_with("fixture-fragment-release-"))
            .all(|task| task.scheduled_start == task.first_available_time),
        "each released high-priority task must be selected at its release"
    );
    let long = schedule
        .iter()
        .find(|task| task.task.name == "fixture-fragment-long")
        .expect("the adversarial fixture contains the long task");
    assert_eq!(
        long.scheduled_start,
        now + chrono::Duration::seconds(FRAGMENT_COUNT as i64 * 2 * 60 + 60),
        "restored releases must be promoted exactly once before the long task"
    );
    assert!(
        elapsed <= Duration::from_secs(2),
        "short fragment adversarial fixture exceeded wall limit: {elapsed:?}"
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
fn packはrepository未変更ならcandidate間でscheduleを再利用する() {
    let repository = small_repository();
    let mut no_free_time = SchedulingFreeTimeManager::new(0);

    let (result, metrics) = pack_tasks_diagnostics(&repository, &mut no_free_time).unwrap();

    assert!(result.packed_tasks.is_empty());
    assert!(result.skipped_tasks.len() > 1);
    assert_eq!(metrics.placement_trial_count, 0);
    assert_eq!(
        metrics.schedule.schedule_rebuild_count, 1,
        "an unchanged repository should reuse one schedule snapshot"
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
    assert_eq!(
        metrics.schedule.candidate_count, 5,
        "flatten should build immutable schedule candidates once"
    );
    assert!(
        metrics.schedule.dependency_candidate_probe_count
            <= metrics.schedule.candidate_count * metrics.schedule.schedule_rebuild_count,
        "dependency readiness should inspect each candidate at most once per rebuild"
    );
    assert!(metrics.full_schedule_scan_element_count > 0);
    assert!(metrics.overload_iteration_count > 0);
    assert!(metrics.candidate_trial_count > 0);
    assert_eq!(metrics.override_clone_element_count, 0);
}

#[test]
fn flatten性能契約は専用過負荷fixtureで移動経路を実行する() {
    let repository = small_repository();
    let mut constrained_free_time = SchedulingFreeTimeManager::new(15);

    let (result, metrics) =
        flatten_tasks_diagnostics(&repository, &mut constrained_free_time).unwrap();

    assert!(metrics.overload_iteration_count > 0);
    assert!(metrics.candidate_trial_count > 0);
    assert!(!result.flattened_tasks.is_empty() || !result.unresolved_overloads.is_empty());
}

#[test]
fn stress_flattenはstress規模の過負荷経路を実行する() {
    let fixture = SchedulingFixture::stress_flatten().unwrap();
    assert!(fixture.summary().unwrap().tasks >= 105_512);
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);
    let mut free_time = SchedulingFreeTimeManager::new(FLATTEN_BENCHMARK_CAPACITY_MINUTES);

    let (_, metrics) = flatten_tasks_diagnostics(&repository, &mut free_time).unwrap();

    assert!(metrics.overload_iteration_count > 0);
    assert!(metrics.candidate_trial_count > 0);
}

#[test]
fn pack診断はatomic探索の1分前進量を計数する() {
    let repository = small_repository();
    let mut no_free_time = SchedulingFreeTimeManager::without_continuous_free_time(8 * 60);

    let (_, metrics) = pack_tasks_diagnostics(&repository, &mut no_free_time).unwrap();

    assert!(metrics.cursor_minute_advance_count > 0);
}

#[test]
fn typicalとstressは決定論的な処理回数上限内に収まる() {
    for size in [FixtureSize::Typical, FixtureSize::Stress] {
        assert_fixture_counter_bounds(size);
    }
}

fn assert_fixture_counter_bounds(size: FixtureSize) {
    let fixture = SchedulingFixture::build(size).unwrap();
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);
    let (_, schedule) = get_schedule_diagnostics(&repository).unwrap();
    assert!(schedule.occupied_slot_probe_count <= schedule.candidate_count * 20);
    assert!(schedule.dependency_candidate_probe_count <= schedule.candidate_count);
    assert_eq!(schedule.sort_count, 2);
    assert!(
        schedule.selection_event_count <= schedule.segment_count * 3,
        "selection events exceeded the per-segment limit: {} > {}",
        schedule.selection_event_count,
        schedule.segment_count * 3
    );
    assert!(
        schedule.selection_candidate_probe_count <= schedule.candidate_count * 16,
        "selection candidate probes exceeded the indexed limit: {} > {}",
        schedule.selection_candidate_probe_count,
        schedule.candidate_count * 16
    );
    assert!(
        // atomicの将来release評価も含め、fixture比率上はcandidate当たり32回以内に保つ。
        schedule.release_candidate_probe_count <= schedule.candidate_count * 32,
        "release candidate probes exceeded the indexed limit: {} > {}",
        schedule.release_candidate_probe_count,
        schedule.candidate_count * 32
    );
    assert!(
        schedule.slack_probe_count <= schedule.candidate_count * 128,
        "slack probes exceeded the linear input limit: {} > {}",
        schedule.slack_probe_count,
        schedule.candidate_count * 128
    );

    let fixture = SchedulingFixture::build(size).unwrap();
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);
    let mut pack_free_time = SchedulingFreeTimeManager::new(DAILY_FREE_MINUTES);
    let (pack_result, pack) = pack_tasks_diagnostics(&repository, &mut pack_free_time).unwrap();
    let mut packed_count = pack_result.packed_tasks.len();
    let mut placement_trial_count = pack.placement_trial_count;
    let mut cursor_minute_advance_count = pack.cursor_minute_advance_count;
    let mut schedule_rebuild_count = pack.schedule.schedule_rebuild_count;
    for _ in 0..pack_probe_scale(size) {
        let fixture = SchedulingFixture::build(FixtureSize::Small).unwrap();
        let repository = SchedulingRepository::new(fixture.projects, fixture.now);
        let mut free_time = SchedulingFreeTimeManager::new(DAILY_FREE_MINUTES);
        let (result, metrics) = pack_tasks_diagnostics(&repository, &mut free_time).unwrap();
        packed_count += result.packed_tasks.len();
        placement_trial_count += metrics.placement_trial_count;
        cursor_minute_advance_count += metrics.cursor_minute_advance_count;
        schedule_rebuild_count += metrics.schedule.schedule_rebuild_count;

        let fixture = SchedulingFixture::build(FixtureSize::Small).unwrap();
        let repository = SchedulingRepository::new(fixture.projects, fixture.now);
        let mut no_continuous_free_time =
            SchedulingFreeTimeManager::without_continuous_free_time(DAILY_FREE_MINUTES);
        let (result, metrics) =
            pack_tasks_diagnostics(&repository, &mut no_continuous_free_time).unwrap();
        packed_count += result.packed_tasks.len();
        placement_trial_count += metrics.placement_trial_count;
        cursor_minute_advance_count += metrics.cursor_minute_advance_count;
        schedule_rebuild_count += metrics.schedule.schedule_rebuild_count;
    }
    let probe_scale = pack_probe_scale(size);
    assert!((probe_scale..=probe_scale * 4).contains(&packed_count));
    assert!((probe_scale * 2..=probe_scale * 4).contains(&placement_trial_count));
    assert!((1..=probe_scale * 1_000).contains(&cursor_minute_advance_count));
    assert!(schedule_rebuild_count <= 1 + probe_scale * 6);

    let fixture = SchedulingFixture::build(size).unwrap();
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);
    let mut flatten_free_time = SchedulingFreeTimeManager::new(FLATTEN_BENCHMARK_CAPACITY_MINUTES);
    let (_, flatten) = flatten_tasks_diagnostics(&repository, &mut flatten_free_time).unwrap();
    // 過負荷分岐そのものは専用fixtureで固定し、規模fixtureでは上限だけを測る。
    assert!(flatten.overload_iteration_count <= 64);
    assert!(flatten.candidate_trial_count <= 128);
    assert_eq!(flatten.override_clone_element_count, 0);
    assert_eq!(flatten.schedule.candidate_count, schedule.candidate_count);
    assert!(
        flatten.schedule.dependency_candidate_probe_count
            <= flatten.schedule.candidate_count * flatten.schedule.schedule_rebuild_count
    );
    assert!(
        flatten.schedule.occupied_slot_probe_count
            <= flatten.schedule.candidate_count * flatten.schedule.schedule_rebuild_count * 20
    );
    assert!(flatten.full_schedule_scan_element_count <= flatten.schedule.segment_count * 6);
}

fn pack_probe_scale(size: FixtureSize) -> usize {
    match size {
        FixtureSize::Small | FixtureSize::Typical => 1,
        FixtureSize::Stress => 4,
    }
}
