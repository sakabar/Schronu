#[path = "support/scheduling_fixture.rs"]
mod scheduling_fixture;
#[path = "support/scheduling_workload_profile.rs"]
mod scheduling_workload_profile;

use chrono::Duration;
use scheduling_fixture::{FixtureSize, FixtureSummary, SchedulingFixture};
use std::collections::HashSet;

#[test]
fn small_fixtureはschedule契約の最小構成を固定する() {
    let fixture = SchedulingFixture::build(FixtureSize::Small).unwrap();

    assert_eq!(fixture.projects.len(), 2);
    assert_eq!(fixture.summary().unwrap().tasks, 6);
    assert_eq!(fixture.summary().unwrap().active_atomic, 1);
    assert_eq!(fixture.summary().unwrap().active_deadline, 1);
}

#[test]
fn typical_fixtureは匿名化した実データ分布を固定する() {
    let fixture = SchedulingFixture::build(FixtureSize::Typical).unwrap();

    assert_eq!(
        fixture.summary().unwrap(),
        FixtureSummary {
            projects: 2_213,
            tasks: 26_378,
            leaves: 21_769,
            active_nodes: 1_749,
            active_leaves: 691,
            active_projects: 416,
            root_statuses: [1_803, 21, 389],
            leaf_statuses: [21_078, 308, 383],
            active_atomic: 119,
            active_deadline: 343,
            active_work_buckets: [31, 349, 223, 41, 47],
            project_depth_percentiles: [0, 3, 18, 210],
            project_size_percentiles: [1, 13, 263, 2_486],
        }
    );
}

#[test]
fn stress_fixtureはtypicalのtask規模と競合候補を4倍にする() {
    let typical = SchedulingFixture::build(FixtureSize::Typical)
        .unwrap()
        .summary()
        .unwrap();
    let stress = SchedulingFixture::build(FixtureSize::Stress)
        .unwrap()
        .summary()
        .unwrap();

    assert_eq!(stress.projects, typical.projects * 4);
    assert_eq!(stress.tasks, typical.tasks * 4);
    assert_eq!(stress.leaves, typical.leaves * 4);
    assert_eq!(stress.active_nodes, typical.active_nodes * 4);
    assert_eq!(stress.active_leaves, typical.active_leaves * 4);
    assert_eq!(stress.active_projects, typical.active_projects * 4);
    assert_eq!(
        stress.root_statuses,
        typical.root_statuses.map(|count| count * 4)
    );
    assert_eq!(
        stress.leaf_statuses,
        typical.leaf_statuses.map(|count| count * 4)
    );
    assert_eq!(stress.active_atomic, typical.active_atomic * 4);
    assert_eq!(stress.active_deadline, typical.active_deadline * 4);
    assert_eq!(
        stress.active_work_buckets,
        typical.active_work_buckets.map(|count| count * 4)
    );
    assert_eq!(stress.project_depth_percentiles, [0, 3, 18, 210]);
    assert_eq!(stress.project_size_percentiles, [1, 13, 263, 2_486]);
}

#[test]
#[ignore = "manual read-only anonymized workload profiling"]
fn 指定storageの匿名化集計はtypical_fixture契約と一致する() {
    let storage = std::env::var_os("SCHRONU_BENCHMARK_STORAGE")
        .expect("SCHRONU_BENCHMARK_STORAGE must point to task storage");
    let actual = scheduling_workload_profile::summarize_storage(std::path::Path::new(&storage))
        .expect("task storage is readable and contains valid project YAML");
    let expected = SchedulingFixture::build(FixtureSize::Typical)
        .unwrap()
        .summary()
        .unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn fixtureは固定seedから同一のidentityとdigestを生成する() {
    let first = SchedulingFixture::build(FixtureSize::Typical).unwrap();
    let second = SchedulingFixture::build(FixtureSize::Typical).unwrap();

    assert_eq!(first.seed, second.seed);
    assert_eq!(first.digest().unwrap(), second.digest().unwrap());
    assert_eq!(first.digest().unwrap(), 9_925_687_030_946_154_452);
}

#[test]
fn fixtureはsyntheticな識別情報と相対時刻だけを含む() {
    let fixture = SchedulingFixture::build(FixtureSize::Typical).unwrap();
    let mut ids = HashSet::new();
    for project in &fixture.projects {
        assert_synthetic(project, fixture.now, &mut ids);
    }
    assert_eq!(ids.len(), 26_378);
}

fn assert_synthetic(
    task: &schronu::entity::task::TaskHandle,
    now: chrono::DateTime<chrono::Local>,
    ids: &mut HashSet<uuid::Uuid>,
) {
    let name = task.get_name().unwrap();
    assert!(
        name.starts_with("fixture-project-") || name.starts_with("fixture-task-"),
        "fixture contains a non-synthetic name: {name}"
    );
    let id = task.get_id().unwrap();
    assert_eq!(&id.as_bytes()[..5], &[0x54, 0x44, 0x30, 0x31, 0x32]);
    assert!(ids.insert(id), "fixture UUIDs must be unique");

    let start_offset = task.get_start_time().unwrap() - now;
    assert!((Duration::zero()..=Duration::days(6)).contains(&start_offset));
    if let Some(deadline) = task.get_deadline_time_opt().unwrap() {
        let deadline_offset = deadline - now;
        assert!((Duration::days(1)..=Duration::days(14)).contains(&deadline_offset));
    }
    for child in task.get_children().unwrap() {
        assert_synthetic(&child, now, ids);
    }
}
