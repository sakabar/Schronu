use super::*;
use chrono::TimeZone;

#[test]
fn slack境界queryは正負交互でもdeadline数を走査しない() {
    const DEADLINE_COUNT: usize = 512;
    let values = (0..DEADLINE_COUNT)
        .map(|index| {
            let magnitude = i64::try_from(index + 1).unwrap();
            Some(if index % 2 == 0 {
                magnitude.saturating_neg()
            } else {
                magnitude
            })
        })
        .collect::<Vec<_>>();
    let tree = SlackRangeTree::new(&values);
    let mut metrics = ScheduleMetrics::default();

    let minimum = tree.range_min(0..DEADLINE_COUNT, &mut metrics);

    assert_eq!(minimum, Some(-511));
    assert!(
        metrics.slack_probe_count <= 32,
        "alternating slack query visited too many nodes: {}",
        metrics.slack_probe_count
    );
}

fn candidate(
    name: &str,
    first_available_time: DateTime<Local>,
    priority: i64,
    remaining_seconds: i64,
) -> TaskScheduleCandidate {
    let task = crate::test_support::new_task_handle(name).unwrap();
    TaskScheduleCandidate {
        id: task.get_id().unwrap(),
        task,
        first_available_time,
        priority,
        rank: 0,
        deadline_time: None,
        remaining_seconds,
        dependency_ids: vec![],
        atomic: false,
        fixed_start: false,
        fixed_start_time: first_available_time,
        estimated_work_seconds: remaining_seconds,
    }
}

fn fixed_candidate(
    name: &str,
    fixed_start_time: DateTime<Local>,
    estimated_work_seconds: i64,
    remaining_seconds: i64,
) -> TaskScheduleCandidate {
    let mut candidate = candidate(name, fixed_start_time, 0, remaining_seconds);
    candidate.fixed_start = true;
    candidate.fixed_start_time = fixed_start_time;
    candidate.estimated_work_seconds = estimated_work_seconds;
    candidate
}

fn segments_for(scheduled: &[ScheduledTask], task_id: Uuid) -> Vec<&ScheduledTask> {
    scheduled
        .iter()
        .filter(|segment| segment.id == task_id)
        .collect()
}

#[test]
fn window内で完了するfixedはtyped_completion_eventへ分類する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let fixed_start = now + Duration::hours(1);
    let window_end = fixed_start + Duration::minutes(30);
    let dependency_ids = vec![Uuid::new_v4(), Uuid::new_v4()];
    let mut fixed = fixed_candidate("fixed", fixed_start, 30 * 60, 10 * 60);
    fixed.dependency_ids = dependency_ids.clone();
    let task_id = fixed.id;
    let mut metrics = ScheduleMetrics::default();

    let prepared = classify_fixed_candidates(&[fixed], now, &mut metrics).unwrap();

    assert_eq!(prepared.pending.len(), 1);
    let SchedulingItem::Completion(event) = &prepared.pending[0] else {
        panic!("window内で完了するfixedをtask候補として残してはならない");
    };
    assert_eq!(
        event,
        &CompletionEvent {
            task_id,
            earliest_occurrence: window_end,
            dependency_ids,
        }
    );
}

#[test]
fn fixed予定同士は重複しても双方の指定時刻を保持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let first_start = now + Duration::hours(1);
    let second_start = now + Duration::minutes(90);
    let first = fixed_candidate("first", first_start, 60 * 60, 60 * 60);
    let second = fixed_candidate("second", second_start, 60 * 60, 60 * 60);
    let first_id = first.id;
    let second_id = second.id;

    let scheduled = schedule_tasks_by_priority(&[second, first], now).unwrap();

    assert_eq!(scheduled_start(&scheduled, first_id), first_start);
    assert_eq!(scheduled_start(&scheduled, second_id), second_start);
}

#[test]
fn 過去開始のfixed予定は元window内へ残作業を置き超過分を後続へ置く() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let start = now - Duration::hours(1);
    let fixed = fixed_candidate("past", start, 2 * 60 * 60, 2 * 60 * 60);
    let fixed_id = fixed.id;

    let scheduled = schedule_tasks_by_priority(&[fixed], now).unwrap();
    let segments = segments_for(&scheduled, fixed_id);

    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].scheduled_start, now);
    assert_eq!(segments[0].scheduled_end, now + Duration::hours(1));
    assert_eq!(segments[0].scheduled_work_seconds, 60 * 60);
    assert_eq!(segments[1].scheduled_start, now + Duration::hours(1));
    assert_eq!(segments[1].scheduled_end, now + Duration::hours(2));
    assert_eq!(segments[1].scheduled_work_seconds, 60 * 60);
    assert_eq!(segments[0].total_work_seconds, 2 * 60 * 60);
    assert_eq!(segments[1].total_work_seconds, 2 * 60 * 60);
}

#[test]
fn flexible予定はfixed区間のunionを避ける() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let first = fixed_candidate("fixed-first", now + Duration::hours(1), 60 * 60, 60 * 60);
    let second = fixed_candidate(
        "fixed-second",
        now + Duration::minutes(90),
        60 * 60,
        60 * 60,
    );
    let flexible = candidate("flexible", now, 99, 2 * 60 * 60);
    let flexible_id = flexible.id;

    let scheduled = schedule_tasks_by_priority(&[flexible, second, first], now).unwrap();
    let segments = segments_for(&scheduled, flexible_id);

    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].scheduled_start, now);
    assert_eq!(segments[0].scheduled_end, now + Duration::hours(1));
    assert_eq!(segments[1].scheduled_start, now + Duration::minutes(150));
    assert_eq!(segments[1].scheduled_end, now + Duration::minutes(210));
}

#[test]
fn future_fixedの元window超過分も作業秒数を欠落重複なく後続配置する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let start = now + Duration::hours(1);
    let fixed = fixed_candidate("excess", start, 60 * 60, 2 * 60 * 60);
    let fixed_id = fixed.id;

    let scheduled = schedule_tasks_by_priority(&[fixed], now).unwrap();
    let segments = segments_for(&scheduled, fixed_id);

    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].scheduled_start, start);
    assert_eq!(segments[0].scheduled_end, start + Duration::hours(1));
    assert_eq!(segments[1].scheduled_start, start + Duration::hours(1));
    assert_eq!(segments[1].scheduled_end, start + Duration::hours(2));
    assert_eq!(
        segments
            .iter()
            .map(|segment| segment.scheduled_work_seconds)
            .sum::<i64>(),
        2 * 60 * 60
    );
    assert!(segments
        .iter()
        .all(|segment| segment.total_work_seconds == 2 * 60 * 60));
}

#[test]
fn fixed予定はdependencyが未完了でも指定時刻を動かさない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let child = candidate("dependency", now, 99, 2 * 60 * 60);
    let mut fixed = fixed_candidate("fixed-parent", now + Duration::hours(1), 60 * 60, 60 * 60);
    fixed.dependency_ids = vec![child.id];
    let fixed_id = fixed.id;

    let scheduled = schedule_tasks_by_priority(&[fixed, child], now).unwrap();

    assert_eq!(
        scheduled_start(&scheduled, fixed_id),
        now + Duration::hours(1)
    );
}

#[test]
fn fixed完了はdependency完了後にgrandparentを解放する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let child = candidate("child", now, 0, 3 * 60 * 60);
    let child_id = child.id;
    let mut fixed = fixed_candidate("fixed", now + Duration::hours(1), 60 * 60, 60 * 60);
    fixed.dependency_ids = vec![child_id];
    let fixed_id = fixed.id;
    let mut grandparent = candidate("grandparent", now, 99, 60 * 60);
    grandparent.dependency_ids = vec![fixed_id];
    let grandparent_id = grandparent.id;

    let scheduled = schedule_tasks_by_priority(&[grandparent, fixed, child], now).unwrap();
    let fixed_segment = segments_for(&scheduled, fixed_id)[0];
    let child_completion = segments_for(&scheduled, child_id)
        .iter()
        .map(|segment| segment.scheduled_end)
        .max()
        .unwrap();
    let grandparent_start = scheduled_start(&scheduled, grandparent_id);

    assert_eq!(fixed_segment.scheduled_start, now + Duration::hours(1));
    assert_eq!(fixed_segment.scheduled_end, now + Duration::hours(2));
    assert!(grandparent_start >= child_completion);
    assert!(grandparent_start >= fixed_segment.scheduled_end);
}

#[test]
fn fixed超過分は元dependency完了後に下流を解放する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let child = candidate("child", now, 0, 3 * 60 * 60);
    let child_id = child.id;
    let mut fixed = fixed_candidate(
        "fixed-excess",
        now + Duration::hours(1),
        60 * 60,
        2 * 60 * 60,
    );
    fixed.dependency_ids = vec![child_id];
    let fixed_id = fixed.id;
    let mut grandparent = candidate("grandparent", now, 99, 60 * 60);
    grandparent.dependency_ids = vec![fixed_id];
    let grandparent_id = grandparent.id;

    let scheduled = schedule_tasks_by_priority(&[grandparent, fixed, child], now).unwrap();
    let fixed_segments = segments_for(&scheduled, fixed_id);
    let child_completion = segments_for(&scheduled, child_id)
        .iter()
        .map(|segment| segment.scheduled_end)
        .max()
        .unwrap();
    let fixed_completion = fixed_segments
        .iter()
        .map(|segment| segment.scheduled_end)
        .max()
        .unwrap();

    assert_eq!(fixed_segments.len(), 2);
    assert!(fixed_segments[1].scheduled_start >= child_completion);
    assert!(scheduled_start(&scheduled, grandparent_id) >= fixed_completion);
}

#[test]
fn zero_remainingのfixed予定は指定時刻に決定的な点を返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let start = now + Duration::hours(1);
    let fixed = fixed_candidate("done-fixed", start, 60 * 60, 0);
    let fixed_id = fixed.id;

    let first = schedule_tasks_by_priority(std::slice::from_ref(&fixed), now).unwrap();
    let second = schedule_tasks_by_priority(&[fixed], now).unwrap();

    assert_eq!(segments_for(&first, fixed_id).len(), 1);
    assert_eq!(segments_for(&first, fixed_id)[0].scheduled_start, start);
    assert_eq!(
        segments_for(&first, fixed_id)[0].scheduled_end,
        start + Duration::hours(1)
    );
    assert_eq!(segments_for(&first, fixed_id)[0].scheduled_work_seconds, 0);
    assert_eq!(first[0].scheduled_start, second[0].scheduled_start);
    assert_eq!(first[0].scheduled_end, second[0].scheduled_end);
}

#[test]
fn fixed予定は残作業がwindowより短くても予約区間全体を可視化する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let start = now + Duration::hours(1);
    let fixed = fixed_candidate("partly-done", start, 60 * 60, 15 * 60);
    let fixed_id = fixed.id;

    let scheduled = schedule_tasks_by_priority(&[fixed], now).unwrap();
    let segment = segments_for(&scheduled, fixed_id)[0];

    assert_eq!(segment.scheduled_start, start);
    assert_eq!(segment.scheduled_end, start + Duration::hours(1));
    assert_eq!(segment.scheduled_work_seconds, 15 * 60);
    assert_eq!(segment.total_work_seconds, 15 * 60);
}

#[test]
fn fixed_window終了が日時範囲外ならtaskと入力値を保持するerrorを返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let fixed = fixed_candidate("overflow", now, i64::MAX, i64::MAX);
    let fixed_id = fixed.id;

    assert_eq!(
        schedule_tasks_by_priority(&[fixed], now).err(),
        Some(SchedulingPolicyError {
            task_id: fixed_id,
            start_time: now,
            work_seconds: i64::MAX,
        })
    );
}

#[test]
fn flexibleとatomicの終了が日時範囲外なら入力値を保持するerrorを返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for atomic in [false, true] {
        let mut candidate = candidate("overflow", now, 0, i64::MAX);
        candidate.atomic = atomic;
        let task_id = candidate.id;

        assert_eq!(
            schedule_tasks_by_priority(&[candidate], now).err(),
            Some(SchedulingPolicyError {
                task_id,
                start_time: now,
                work_seconds: i64::MAX,
            }),
            "atomic={atomic}"
        );
    }
}

#[test]
fn speculative選択errorはfrontierとslackを双方復元する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let boundary = now + Duration::minutes(1);
    let selected = candidate("selected", now, 0, 10 * 60);
    let mut overflowing = candidate("overflowing", boundary, 1, i64::MAX);
    overflowing.atomic = true;
    let overflow_id = overflowing.id;
    let states = [selected, overflowing]
        .into_iter()
        .map(|candidate| FlexibleState {
            total_work_seconds: candidate.remaining_seconds,
            effective_deadline: None,
            remaining_seconds: candidate.remaining_seconds,
            candidate,
            dependency_indices: Vec::new(),
            completion_time: None,
        })
        .collect::<Vec<_>>();
    let mut metrics = ScheduleMetrics::default();
    let mut frontier = SchedulerFrontier::new(&states);
    frontier.promote_releases(now, &states, &mut metrics);
    let mut slack_index = SlackDemandIndex::new(&states, now, &[], &mut metrics);

    let error = select_at_speculative_boundary(
        &states,
        now,
        boundary,
        &[],
        &mut frontier,
        &mut slack_index,
        &mut metrics,
    )
    .err();

    assert_eq!(
        error,
        Some(SchedulingPolicyError {
            task_id: overflow_id,
            start_time: boundary,
            work_seconds: i64::MAX,
        })
    );
    assert_eq!(slack_index.current_time, now);
    assert_eq!(frontier.next_release(), Some(boundary));
    assert!(!frontier.ready[1]);
}

#[test]
fn task完了でfrontier世代が変わるとatomic予測cacheを無効化する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let child = candidate("child", now, 2, 60);
    let child_id = child.id;
    let mut dependent = candidate("dependent", now, 1, 60);
    dependent.dependency_ids = vec![child_id];
    let preemptor = candidate("preemptor", now + Duration::hours(1), 3, 60);
    let states = [child, dependent, preemptor]
        .into_iter()
        .map(|candidate| FlexibleState {
            total_work_seconds: candidate.remaining_seconds,
            effective_deadline: None,
            remaining_seconds: candidate.remaining_seconds,
            candidate,
            dependency_indices: Vec::new(),
            completion_time: None,
        })
        .collect::<Vec<_>>();
    let mut states = states;
    states[1].dependency_indices = vec![Some(DependencyNode::Task(0))];
    let mut metrics = ScheduleMetrics::default();
    let mut frontier = SchedulerFrontier::new(&states);
    frontier.promote_releases(now, &states, &mut metrics);
    let mut cache = AtomicReleasePredictionCache::default();
    cache.insert(
        AtomicReleasePrediction {
            release: now + Duration::hours(1),
            preemptor_index: 2,
            critical_deadline: None,
            protected_mode: false,
            frontier_generation: frontier.generation,
        },
        &mut metrics,
    );

    frontier.complete(0, now + Duration::minutes(1), &states, &mut metrics);
    cache.retain_future_preemptors_for_generation(
        now,
        &states,
        Some(frontier.generation),
        &mut metrics,
    );

    assert!(cache.entries.is_empty());
    assert_eq!(
        frontier.next_release(),
        Some(now + Duration::minutes(1)),
        "dependency completion must add a new release overlay"
    );
}

#[cfg(feature = "benchmarking")]
#[test]
fn schedule_tasks_by_priorityは依存待ちcandidateを毎回走査しない() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let child = candidate("child", now, 0, 60);
    let child_id = child.id;
    let mut candidates = (0..100)
        .map(|index| {
            let mut parent = candidate(&format!("parent-{index}"), now, 98, 0);
            parent.rank = 1;
            parent.dependency_ids = vec![child_id];
            parent
        })
        .collect::<Vec<_>>();
    candidates.push(child);
    let mut metrics = ScheduleMetrics::default();

    schedule_tasks_by_priority_with_metrics(&candidates, now, &mut metrics).unwrap();

    assert!(
        metrics.dependency_candidate_probe_count <= candidates.len(),
        "dependency readiness probes exceeded one per candidate: {} > {}",
        metrics.dependency_candidate_probe_count,
        candidates.len()
    );
}

#[test]
fn schedule_tasks_by_priorityはmissing_dependencyをready候補の後にfallback配置する() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let ready = candidate("ready", now, 0, 60);
    let ready_id = ready.id;
    let mut blocked = candidate("blocked", now, 98, 60);
    blocked.dependency_ids = vec![Uuid::nil()];
    let blocked_id = blocked.id;

    let scheduled = schedule_tasks_by_priority(&[blocked, ready], now).unwrap();

    assert_eq!(scheduled_start(&scheduled, ready_id), now);
    assert_eq!(
        scheduled_start(&scheduled, blocked_id),
        now + Duration::seconds(60)
    );
}

#[test]
fn schedule_tasks_by_priorityは重複dependencyを1回の完了で解決する() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let child = candidate("child", now, 0, 60);
    let child_id = child.id;
    let mut parent = candidate("parent", now, 98, 60);
    parent.dependency_ids = vec![child_id, child_id];
    let parent_id = parent.id;

    let scheduled = schedule_tasks_by_priority(&[parent, child], now).unwrap();

    assert_eq!(scheduled_start(&scheduled, child_id), now);
    assert_eq!(
        scheduled_start(&scheduled, parent_id),
        now + Duration::seconds(60)
    );
}

#[test]
fn schedule_tasks_by_priorityはcycle時にsort順先頭からfallback配置する() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let mut first = candidate("first", now, 98, 60);
    let mut second = candidate("second", now, 0, 60);
    first.dependency_ids = vec![second.id];
    second.dependency_ids = vec![first.id];
    let first_id = first.id;
    let second_id = second.id;

    let scheduled = schedule_tasks_by_priority(&[second, first], now).unwrap();

    assert_eq!(scheduled_start(&scheduled, first_id), now);
    assert_eq!(
        scheduled_start(&scheduled, second_id),
        now + Duration::seconds(60)
    );
}

#[test]
fn zero_workだけのdependency_cycleも決定的な完了点を返す() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let mut first = candidate("first", now, 98, 0);
    let mut second = candidate("second", now, 0, 0);
    first.id = Uuid::from_u128(1);
    second.id = Uuid::from_u128(2);
    first.dependency_ids = vec![second.id];
    second.dependency_ids = vec![first.id];

    let scheduled = schedule_tasks_by_priority(&[second, first], now).unwrap();

    assert_eq!(scheduled.len(), 2);
    assert_eq!(scheduled[0].id, Uuid::from_u128(1));
    assert_eq!(scheduled[0].scheduled_start, now);
    assert_eq!(scheduled[1].id, Uuid::from_u128(2));
    assert_eq!(scheduled[1].scheduled_start, now);
}

fn scheduled_start(scheduled: &[ScheduledTask], task_id: Uuid) -> DateTime<Local> {
    scheduled
        .iter()
        .find(|task| task.id == task_id)
        .expect("candidate is scheduled")
        .scheduled_start
}

#[test]
fn schedule_tasks_by_priority_5分以下の空き時間には分割しない() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let low = candidate(
        "低優先度",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
        87,
        20 * 60,
    );
    let low_id = low.task.get_id().unwrap();
    let high = candidate(
        "高優先度",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 5, 0).unwrap(),
        88,
        60 * 60,
    );

    let actual = schedule_tasks_by_priority(&[low, high], now).unwrap();
    let low_segments = actual
        .iter()
        .filter(|scheduled| scheduled.task.get_id().unwrap() == low_id)
        .collect::<Vec<_>>();

    assert_eq!(low_segments.len(), 1);
    assert_eq!(
        low_segments[0].scheduled_start,
        Local.with_ymd_and_hms(2026, 5, 10, 14, 5, 0).unwrap()
    );
    assert_eq!(low_segments[0].scheduled_work_seconds, 20 * 60);
}

#[test]
fn schedule_tasks_by_priority_6分の空き時間には分割する() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let low = candidate(
        "低優先度",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
        87,
        20 * 60,
    );
    let low_id = low.task.get_id().unwrap();
    let high = candidate(
        "高優先度",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 6, 0).unwrap(),
        88,
        60 * 60,
    );

    let actual = schedule_tasks_by_priority(&[low, high], now).unwrap();
    let low_segments = actual
        .iter()
        .filter(|scheduled| scheduled.task.get_id().unwrap() == low_id)
        .collect::<Vec<_>>();

    assert_eq!(low_segments.len(), 2);
    assert_eq!(
        low_segments[0].scheduled_start,
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap()
    );
    assert_eq!(low_segments[0].scheduled_work_seconds, 6 * 60);
    assert_eq!(
        low_segments[1].scheduled_start,
        Local.with_ymd_and_hms(2026, 5, 10, 14, 6, 0).unwrap()
    );
    assert_eq!(low_segments[1].scheduled_work_seconds, 14 * 60);
}

#[test]
fn schedule_tasks_by_priority_後半が5分以下になる分割はしない() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let low = candidate(
        "低優先度",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
        87,
        20 * 60,
    );
    let low_id = low.task.get_id().unwrap();
    let high = candidate(
        "高優先度",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 15, 0).unwrap(),
        88,
        60 * 60,
    );

    let actual = schedule_tasks_by_priority(&[low, high], now).unwrap();
    let low_segments = actual
        .iter()
        .filter(|scheduled| scheduled.task.get_id().unwrap() == low_id)
        .collect::<Vec<_>>();

    assert_eq!(low_segments.len(), 1);
    assert_eq!(
        low_segments[0].scheduled_start,
        Local.with_ymd_and_hms(2026, 5, 10, 14, 15, 0).unwrap()
    );
    assert_eq!(low_segments[0].scheduled_work_seconds, 20 * 60);
}

#[test]
fn schedule_tasks_by_priority_残り5分以下のtask自体は配置する() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let blocker = candidate("blocker", now, 88, 60 * 60);
    let task = candidate(
        "5分task",
        Local.with_ymd_and_hms(2026, 5, 10, 12, 55, 0).unwrap(),
        87,
        5 * 60,
    );
    let task_id = task.task.get_id().unwrap();

    let actual = schedule_tasks_by_priority(&[blocker, task], now).unwrap();
    let scheduled = actual
        .iter()
        .find(|scheduled| scheduled.task.get_id().unwrap() == task_id)
        .unwrap();

    assert_eq!(
        scheduled.scheduled_start,
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap()
    );
    assert_eq!(scheduled.scheduled_work_seconds, 5 * 60);
}

#[test]
fn schedule_tasks_by_priority_atomic_taskは依存終了後の連続枠に配置する() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let child = candidate("子", now, 98, 60 * 60);
    let child_id = child.task.get_id().unwrap();
    let blocker = candidate(
        "blocker",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 30, 0).unwrap(),
        97,
        60 * 60,
    );
    let mut parent = candidate("atomic親", now, 89, 2 * 60 * 60);
    parent.rank = 1;
    parent.atomic = true;
    parent.dependency_ids = vec![child_id];
    let parent_id = parent.task.get_id().unwrap();

    let actual = schedule_tasks_by_priority(&[parent, blocker, child], now).unwrap();
    let scheduled = actual
        .iter()
        .find(|scheduled| scheduled.task.get_id().unwrap() == parent_id)
        .unwrap();

    assert_eq!(
        scheduled.scheduled_start,
        Local.with_ymd_and_hms(2026, 5, 10, 14, 30, 0).unwrap()
    );
    assert_eq!(
        scheduled.scheduled_end,
        Local.with_ymd_and_hms(2026, 5, 10, 16, 30, 0).unwrap()
    );
}

#[test]
fn schedule_tasks_by_priority_高優先度task間の隙間を優先度順に埋める() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let lunch = candidate("昼食", now, 88, 60 * 60);
    let lunch_id = lunch.task.get_id().unwrap();
    let priority_88 = candidate(
        "優先度88",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
        87,
        4 * 60 * 60,
    );
    let priority_88_id = priority_88.task.get_id().unwrap();
    let priority_87 = candidate(
        "優先度87",
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
        86,
        60 * 60,
    );
    let priority_87_id = priority_87.task.get_id().unwrap();
    let dinner = candidate(
        "夕食",
        Local.with_ymd_and_hms(2026, 5, 10, 18, 0, 0).unwrap(),
        88,
        60 * 60,
    );
    let dinner_id = dinner.task.get_id().unwrap();

    let actual =
        schedule_tasks_by_priority(&[priority_87, dinner, priority_88, lunch], now).unwrap();
    let start = |id| {
        actual
            .iter()
            .find(|scheduled| scheduled.task.get_id().unwrap() == id)
            .unwrap()
            .scheduled_start
    };

    assert_eq!(start(lunch_id), now);
    assert_eq!(
        start(priority_88_id),
        Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap()
    );
    assert_eq!(
        start(priority_87_id),
        Local.with_ymd_and_hms(2026, 5, 10, 17, 0, 0).unwrap()
    );
    assert_eq!(
        start(dinner_id),
        Local.with_ymd_and_hms(2026, 5, 10, 18, 0, 0).unwrap()
    );
}

#[test]
fn schedule_tasks_by_priority_親は子の実schedule終了後に配置する() {
    let now = Local.with_ymd_and_hms(2026, 5, 10, 14, 0, 0).unwrap();
    let blocker = candidate("blocker", now, 89, 60 * 60);
    let child = candidate("子", now, 0, 60);
    let child_id = child.task.get_id().unwrap();
    let mut parent = candidate("親", now, 98, 0);
    parent.rank = 1;
    parent.dependency_ids = vec![child_id];
    let parent_id = parent.task.get_id().unwrap();

    let actual = schedule_tasks_by_priority(&[parent, blocker, child], now).unwrap();
    let start = |id| {
        actual
            .iter()
            .find(|scheduled| scheduled.task.get_id().unwrap() == id)
            .unwrap()
            .scheduled_start
    };

    assert_eq!(
        start(child_id),
        Local.with_ymd_and_hms(2026, 5, 10, 15, 0, 0).unwrap()
    );
    assert_eq!(
        start(parent_id),
        Local.with_ymd_and_hms(2026, 5, 10, 15, 1, 0).unwrap()
    );
}

#[test]
fn deadline_slackに余裕がある間は高priorityの長時間taskを先行する() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut deadline = candidate("deadline", now, 1, 60 * 60);
    deadline.deadline_time = Some(now + Duration::hours(4));
    let deadline_id = deadline.id;
    let important = candidate("important", now, 99, 2 * 60 * 60);
    let important_id = important.id;

    let scheduled = schedule_tasks_by_priority(&[deadline, important], now).unwrap();

    assert_eq!(scheduled_start(&scheduled, important_id), now);
    assert_eq!(
        scheduled_start(&scheduled, deadline_id),
        now + Duration::hours(2)
    );
}

#[test]
fn 完了済みdeadlineは後続のdeadlineなしtaskを分割しない() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut deadline = candidate("deadline", now, 99, 10 * 60);
    deadline.deadline_time = Some(now + Duration::hours(1));
    let flexible = candidate("flexible", now, 1, 2 * 60 * 60);
    let flexible_id = flexible.id;

    let scheduled = schedule_tasks_by_priority(&[deadline, flexible], now).unwrap();
    let flexible_segments = segments_for(&scheduled, flexible_id);

    assert_eq!(flexible_segments.len(), 1);
    assert_eq!(
        flexible_segments[0].scheduled_start,
        now + Duration::minutes(10)
    );
    assert_eq!(
        flexible_segments[0].scheduled_end,
        now + Duration::minutes(130)
    );
}

#[test]
fn deadline_slackが0になる境界でdeadline_taskへ切り替える() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut deadline = candidate("deadline", now, 1, 60 * 60);
    deadline.deadline_time = Some(now + Duration::hours(3));
    let deadline_id = deadline.id;
    let important = candidate("important", now, 99, 4 * 60 * 60);
    let important_id = important.id;

    let scheduled = schedule_tasks_by_priority(&[deadline, important], now).unwrap();
    let important_segments = segments_for(&scheduled, important_id);

    assert_eq!(important_segments.len(), 2);
    assert_eq!(important_segments[0].scheduled_start, now);
    assert_eq!(
        important_segments[0].scheduled_end,
        now + Duration::hours(2)
    );
    assert_eq!(
        scheduled_start(&scheduled, deadline_id),
        now + Duration::hours(2)
    );
}

#[test]
fn slack境界が5分の後半fragmentを作る場合はdeadline_taskを先行する() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut deadline = candidate("deadline", now, 1, 60 * 60);
    deadline.deadline_time = Some(now + Duration::hours(3));
    let deadline_id = deadline.id;
    let important = candidate("important", now, 99, 2 * 60 * 60 + 5 * 60);
    let important_id = important.id;

    let scheduled = schedule_tasks_by_priority(&[important, deadline], now).unwrap();

    assert_eq!(scheduled_start(&scheduled, deadline_id), now);
    assert_eq!(
        scheduled_start(&scheduled, important_id),
        now + Duration::hours(1)
    );
}

#[test]
fn slack境界とdeadline_releaseが同時でも境界までの作業を捨てない() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut deadline = candidate("deadline", now + Duration::hours(2), 1, 60 * 60);
    deadline.deadline_time = Some(now + Duration::hours(3));
    let deadline_id = deadline.id;
    let important = candidate("important", now, 99, 2 * 60 * 60 + 5 * 60);
    let important_id = important.id;

    let scheduled = schedule_tasks_by_priority(&[deadline, important], now).unwrap();
    let important_segments = segments_for(&scheduled, important_id);

    assert_eq!(important_segments[0].scheduled_start, now);
    assert_eq!(
        important_segments[0].scheduled_end,
        now + Duration::hours(2)
    );
    assert_eq!(
        scheduled_start(&scheduled, deadline_id),
        now + Duration::hours(2)
    );
    assert_eq!(
        important_segments
            .iter()
            .map(|segment| segment.scheduled_work_seconds)
            .sum::<i64>(),
        2 * 60 * 60 + 5 * 60
    );
}

#[test]
fn deadline_slackは早いdeadlineまでの需要を後続deadlineへ累積する() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut first = candidate("first-deadline", now, 1, 60 * 60);
    first.deadline_time = Some(now + Duration::hours(3));
    let first_id = first.id;
    let mut second = candidate("second-deadline", now, 2, 2 * 60 * 60);
    second.deadline_time = Some(now + Duration::hours(4));
    let second_id = second.id;
    let important = candidate("important", now, 99, 3 * 60 * 60);
    let important_id = important.id;

    let scheduled = schedule_tasks_by_priority(&[second, important, first], now).unwrap();

    assert_eq!(scheduled_start(&scheduled, important_id), now);
    assert_eq!(
        scheduled_start(&scheduled, first_id),
        now + Duration::hours(1)
    );
    assert_eq!(
        scheduled_start(&scheduled, second_id),
        now + Duration::hours(2)
    );
}

#[test]
fn future_candidateのrelease時刻でsegmentを切り再選択する() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut deadline = candidate("deadline", now, 1, 60 * 60);
    deadline.deadline_time = Some(now + Duration::hours(3));
    let deadline_id = deadline.id;
    let running = candidate("running", now, 50, 3 * 60 * 60);
    let running_id = running.id;
    let released = candidate("released", now + Duration::hours(1), 99, 60 * 60);
    let released_id = released.id;

    let scheduled = schedule_tasks_by_priority(&[deadline, released, running], now).unwrap();
    let running_segments = segments_for(&scheduled, running_id);

    assert_eq!(running_segments[0].scheduled_start, now);
    assert_eq!(running_segments[0].scheduled_end, now + Duration::hours(1));
    assert_eq!(
        scheduled_start(&scheduled, released_id),
        now + Duration::hours(1)
    );
    assert_eq!(
        scheduled_start(&scheduled, deadline_id),
        now + Duration::hours(2)
    );
}

#[test]
fn fixedの全transitive_dependencyへ開始時刻をeffective_deadlineとして伝搬する() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let dependency = candidate("dependency", now, 1, 60 * 60);
    let dependency_id = dependency.id;
    let mut intermediate = candidate("intermediate", now, 1, 0);
    intermediate.dependency_ids = vec![dependency_id];
    let intermediate_id = intermediate.id;
    let mut fixed = fixed_candidate("fixed", now + Duration::hours(2), 60 * 60, 60 * 60);
    fixed.dependency_ids = vec![intermediate_id];
    let important = candidate("important", now, 99, 3 * 60 * 60);
    let important_id = important.id;

    let scheduled =
        schedule_tasks_by_priority(&[fixed, important, intermediate, dependency], now).unwrap();

    assert_eq!(scheduled_start(&scheduled, important_id), now);
    assert_eq!(
        scheduled_start(&scheduled, dependency_id),
        now + Duration::hours(1)
    );
}

#[test]
fn priority_tieはeffective_deadline_rank_uuidで入力順に依存せず決定する() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut earlier = candidate("earlier", now, 50, 60);
    earlier.id = Uuid::from_u128(2);
    earlier.deadline_time = Some(now + Duration::hours(10));
    let mut later = candidate("later", now, 50, 60);
    later.id = Uuid::from_u128(1);
    later.deadline_time = Some(now + Duration::hours(11));

    let forward = schedule_tasks_by_priority(&[later.clone(), earlier.clone()], now).unwrap();
    let reverse = schedule_tasks_by_priority(&[earlier, later], now).unwrap();

    assert_eq!(forward[0].id, Uuid::from_u128(2));
    assert_eq!(
        forward
            .iter()
            .map(|segment| (segment.id, segment.scheduled_start, segment.scheduled_end))
            .collect::<Vec<_>>(),
        reverse
            .iter()
            .map(|segment| (segment.id, segment.scheduled_start, segment.scheduled_end))
            .collect::<Vec<_>>()
    );
}

#[test]
fn atomicが次eventまでに入らなければ入るready候補を先に配置する() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut atomic = candidate("atomic", now, 99, 2 * 60 * 60);
    atomic.atomic = true;
    let atomic_id = atomic.id;
    let flexible = candidate("flexible", now, 1, 30 * 60);
    let flexible_id = flexible.id;
    let fixed = fixed_candidate("fixed", now + Duration::hours(1), 60 * 60, 60 * 60);

    let scheduled = schedule_tasks_by_priority(&[fixed, flexible, atomic], now).unwrap();

    assert_eq!(scheduled_start(&scheduled, flexible_id), now);
    assert_eq!(
        scheduled_start(&scheduled, atomic_id),
        now + Duration::hours(2)
    );
    assert_eq!(segments_for(&scheduled, atomic_id).len(), 1);
}

#[test]
fn 低priority_candidateのfuture_releaseは高priority_atomicを延期しない() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut atomic = candidate("atomic", now, 99, 2 * 60 * 60);
    atomic.atomic = true;
    let atomic_id = atomic.id;
    let released = candidate("released", now + Duration::hours(1), 1, 60 * 60);
    let released_id = released.id;

    let scheduled = schedule_tasks_by_priority(&[released, atomic], now).unwrap();

    assert_eq!(scheduled_start(&scheduled, atomic_id), now);
    assert_eq!(
        scheduled_start(&scheduled, released_id),
        now + Duration::hours(2)
    );
}

#[test]
fn 連続枠に入らないfuture_atomicは実行中atomicを中断しない() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut running = candidate("running", now, 50, 60 * 60);
    running.atomic = true;
    let running_id = running.id;
    let mut released = candidate("released", now + Duration::minutes(30), 99, 2 * 60 * 60);
    released.atomic = true;
    let released_id = released.id;
    let fixed = fixed_candidate("fixed", now + Duration::hours(1), 60 * 60, 60 * 60);

    let scheduled = schedule_tasks_by_priority(&[fixed, released, running], now).unwrap();

    assert_eq!(scheduled_start(&scheduled, running_id), now);
    assert_eq!(
        scheduled_start(&scheduled, released_id),
        now + Duration::hours(2)
    );
}

#[test]
fn atomic継続候補はrelease時の残秒で判定する() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut running = candidate("running", now, 99, 60 * 60);
    running.atomic = true;
    let running_id = running.id;
    let released = candidate("released", now + Duration::minutes(30), 1, 60 * 60);
    let released_id = released.id;
    let fixed = fixed_candidate("fixed", now + Duration::hours(1), 60 * 60, 60 * 60);

    let scheduled = schedule_tasks_by_priority(&[fixed, released, running], now).unwrap();

    assert_eq!(scheduled_start(&scheduled, running_id), now);
    assert_eq!(
        scheduled_start(&scheduled, released_id),
        now + Duration::hours(2)
    );
}

#[test]
fn 実現不能なdeadlineでも決定的に期限超過scheduleを返す() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut impossible = candidate("impossible", now, 1, 2 * 60 * 60);
    impossible.deadline_time = Some(now + Duration::hours(1));
    impossible.id = Uuid::from_u128(1);

    let first = schedule_tasks_by_priority(std::slice::from_ref(&impossible), now).unwrap();
    let second = schedule_tasks_by_priority(&[impossible], now).unwrap();

    assert_eq!(first[0].scheduled_start, now);
    assert_eq!(first[0].scheduled_end, now + Duration::hours(2));
    assert!(first[0].scheduled_end > first[0].deadline_time.unwrap());
    assert_eq!(first[0].scheduled_start, second[0].scheduled_start);
    assert_eq!(first[0].scheduled_end, second[0].scheduled_end);
}

#[test]
fn preemptionとfixed衝突後もtaskごとの作業秒数を保存する() {
    let now = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let mut deadline = candidate("deadline", now, 1, 60 * 60);
    deadline.deadline_time = Some(now + Duration::hours(4));
    let important = candidate("important", now, 99, 4 * 60 * 60);
    let fixed = fixed_candidate("fixed", now + Duration::hours(2), 60 * 60, 60 * 60);
    let expected = [
        (deadline.id, deadline.remaining_seconds),
        (important.id, important.remaining_seconds),
        (fixed.id, fixed.remaining_seconds),
    ];

    let scheduled = schedule_tasks_by_priority(&[deadline, important, fixed], now).unwrap();

    for (id, expected_seconds) in expected {
        assert_eq!(
            segments_for(&scheduled, id)
                .iter()
                .map(|segment| segment.scheduled_work_seconds)
                .sum::<i64>(),
            expected_seconds,
            "work seconds changed for {id}"
        );
    }
}
