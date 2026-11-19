use super::*;
use crate::{
    DeadlineDisplayKind, LoadData, RoutineLoadReport, ScheduleOccurrence, ScheduledTask,
    ServerSnapshot, TaskDisplayKind,
};

const TASK: &str = "00112233-4455-4677-8899-aabbccddeeff";

fn sample() -> OfflineState {
    let first_ms = chrono::DateTime::parse_from_rfc3339("2026-11-18T04:00:00+09:00")
        .unwrap()
        .timestamp_millis();
    let row = ScheduledTaskRow {
        task: ScheduledTask {
            task_id: Some(TASK.to_owned()),
            task_name: "対象タスク".to_owned(),
            estimated_work_seconds: 3_600,
            actual_work_seconds: 10,
            start_time_epoch_ms: None,
            pending_until_epoch_ms: None,
            duration_stddev_seconds: None,
            estimate_success_percentage: None,
        },
        display_task_name: None,
        advanceable_days: 0,
        occurrence: ScheduleOccurrence::Actual {
            task_id: TASK.to_owned(),
        },
        schedule_start_epoch_ms: first_ms,
        schedule_end_epoch_ms: first_ms + 1_800_000,
        deadline_epoch_ms: None,
        deadline_label: String::new(),
        misses_deadline: false,
        task_display_kind: TaskDisplayKind::NonRepetitive,
        deadline_display_kind: DeadlineDisplayKind::None,
        is_leaf: true,
        defer_plan: None,
    };
    let snapshot = ServerSnapshot {
        logical_date: "2026-11-17".into(),
        observed_at_epoch_ms: first_ms,
        buffer_seconds: 10_000,
    };
    let days = (17..=23)
        .map(|day| (format!("2026-11-{day}"), vec![row.clone()]))
        .collect();
    let load = LoadData {
        band_days: vec![],
        routine_load: RoutineLoadReport {
            start_date: "2026-11-17".into(),
            end_date: "2026-12-14".into(),
            horizon_day_count: 28,
            full_day_available_seconds_by_date: Default::default(),
            forecast_by_date: Default::default(),
            rows: vec![],
        },
    };
    OfflineState::new(
        OfflineBundle {
            snapshot,
            days,
            load,
            splittable_task_ids: BTreeSet::from([TASK.into()]),
        },
        vec![],
    )
}

#[test]
fn split_restart_and_complete_share_all_segments_and_preserve_capture_times_across_reload() {
    let mut result = sample();
    let original_load = result.bundle.load.clone();
    let start = result.bundle.snapshot.observed_at_epoch_ms;
    result.start(TASK, start).unwrap();
    assert!(result.start(TASK, start).is_err());
    result
        .finish(
            TASK,
            OfflineOperationKind::Split,
            start + 1_500_123,
            "op1".into(),
        )
        .unwrap();
    assert!(result.sessions.is_empty());
    assert!(result
        .bundle
        .days
        .values()
        .flatten()
        .all(|r| r.task.estimated_work_seconds == 2_100 && r.task.actual_work_seconds == 10));
    result = serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
    result.start(TASK, start + 3_600_000).unwrap();
    assert_eq!(result.sessions[0].estimated_work_seconds_at_start, 2_100);
    result
        .finish(
            TASK,
            OfflineOperationKind::Record,
            start + 3_720_999,
            "op2".into(),
        )
        .unwrap();
    result.start(TASK, start + 4_000_000).unwrap();
    result
        .finish(
            TASK,
            OfflineOperationKind::Complete,
            start + 4_010_555,
            "op3".into(),
        )
        .unwrap();
    assert!(result.start(TASK, start + 5_000_000).is_err());
    assert!(result.completed(TASK));
    assert_eq!(result.pending.len(), 3);
    assert_eq!(result.recorded_task_name(TASK), Some("対象タスク"));
    assert_eq!(
        result.pending[0].expected_estimated_work_seconds,
        Some(3_600)
    );
    assert_eq!(result.pending[0].ended_at_epoch_ms, start + 1_500_123);
    assert_eq!(result.pending[2].expected_actual_work_seconds, 130);
    assert_eq!(result.pending[2].ended_at_epoch_ms, start + 4_010_555);
    assert_eq!(result.bundle.load, original_load);
}

#[test]
fn discard_completion_preserves_prior_work_and_removes_only_the_discarded_buffer_credit() {
    let mut state = sample();
    let start = state.bundle.snapshot.observed_at_epoch_ms;
    // Ten seconds were already saved. Record the rest of the first 30 minutes offline.
    let recorded = start + 1_790_000;
    state.start(TASK, start).unwrap();
    state
        .finish(
            TASK,
            OfflineOperationKind::Record,
            recorded,
            "record".into(),
        )
        .unwrap();
    let confirmed = recorded + 1_200_000;
    let expected_buffer = state.display_buffer_seconds(confirmed);
    state.start(TASK, recorded).unwrap();
    assert_eq!(
        state.display_buffer_seconds(confirmed),
        expected_buffer + 1200
    );
    state
        .finish(
            TASK,
            OfflineOperationKind::CompleteWithoutRecording,
            confirmed,
            "discard".into(),
        )
        .unwrap();
    let restored: OfflineState =
        serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
    assert!(restored.sessions.is_empty());
    assert!(restored.completed(TASK));
    assert_eq!(restored.display_buffer_seconds(confirmed), expected_buffer);
    assert!(restored
        .bundle
        .days
        .values()
        .flatten()
        .all(|row| { row.task.actual_work_seconds == 1800 && !restored.can_start(row) }));
    assert_eq!(restored.pending.len(), 2);
    let op = &restored.pending[1];
    assert_eq!(
        serde_json::to_value(op.kind).unwrap(),
        "complete_without_recording"
    );
    assert_eq!(op.expected_actual_work_seconds, 1800);
    assert_eq!(op.ended_at_epoch_ms, confirmed);
}

#[test]
fn can_only_start_actual_leaf_and_invalid_end_retains_session() {
    let mut state = sample();
    let start = state.bundle.snapshot.observed_at_epoch_ms;
    for day in state.bundle.days.values_mut() {
        day[0].occurrence = ScheduleOccurrence::Projected {
            occurrence_key: "見込み".to_owned(),
            source_task_id: TASK.to_owned(),
        };
    }
    assert!(state.start(TASK, start).is_err());
    let row = &mut state.bundle.days.values_mut().next().unwrap()[0];
    row.occurrence = ScheduleOccurrence::Actual {
        task_id: TASK.into(),
    };
    row.is_leaf = false;
    assert!(state.start(TASK, start).is_err());
    state.bundle.days.values_mut().next().unwrap()[0].is_leaf = true;
    state.start(TASK, start).unwrap();
    let before = state.clone();
    assert!(state
        .finish(
            TASK,
            OfflineOperationKind::Split,
            start + 999,
            "too-short".into()
        )
        .is_err());
    assert!(state
        .finish(TASK, OfflineOperationKind::Record, start - 1, "back".into())
        .is_err());
    assert_eq!(state, before);
    state.bundle.splittable_task_ids.clear();
    assert!(state
        .finish(
            TASK,
            OfflineOperationKind::Split,
            start + 10_000,
            "restricted".into()
        )
        .is_err());
    state.restart(TASK, start + 100_000).unwrap();
    assert_eq!(state.sessions[0].started_at_epoch_ms, start + 100_000);
    state.release(TASK);
    assert!(state.sessions.is_empty());
    assert!(state.pending.is_empty());
}

#[test]
fn ended_work_credit_is_preserved_after_reload_and_the_six_oclock_boundary_is_jst() {
    let mut state = sample();
    let now = state.bundle.snapshot.observed_at_epoch_ms;
    state.start(TASK, now).unwrap();
    assert_eq!(state.display_buffer_seconds(now + 60_000), 10_000);
    state
        .finish(
            TASK,
            OfflineOperationKind::Record,
            now + 60_000,
            "local".into(),
        )
        .unwrap();
    let state: OfflineState =
        serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(state.display_buffer_seconds(now + 90_000), 9_970);
    assert!(!state.prior_day_estimate(now + 7_199_999));
    assert!(state.prior_day_estimate(now + 7_200_000));
    assert_eq!(format_jst(now + 18_000_000, "%H:%M"), "09:00");
}

#[test]
fn conflict_keeps_queue_and_projection_until_explicit_discard_then_requires_refresh() {
    let mut state = sample();
    let now = state.bundle.snapshot.observed_at_epoch_ms;
    let mut fresh = state.bundle.clone();
    state.start(TASK, now).unwrap();
    state
        .finish(
            TASK,
            OfflineOperationKind::Split,
            now + 120_000,
            "conflict".into(),
        )
        .unwrap();
    state
        .conflicts
        .insert(TASK.to_owned(), "見積が変更されています。".into());
    for row in fresh.days.values_mut().flatten() {
        row.task.estimated_work_seconds = 5_000;
    }
    state.rebase(fresh.clone());
    assert_eq!(
        state.bundle.days.values().next().unwrap()[0]
            .task
            .estimated_work_seconds,
        3_480
    );
    assert!(state.start(TASK, now + 125_000).is_err());
    state.discard_conflict(TASK);
    assert!(state.pending.is_empty());
    assert!(state.start(TASK, now + 125_000).is_err());
    state.rebase(fresh);
    state.start(TASK, now + 125_000).unwrap();
    assert_eq!(state.sessions[0].estimated_work_seconds_at_start, 5_000);
}

#[test]
fn preexisting_timer_gets_credit_once_until_receipt_and_fresh_snapshot() {
    let mut state = sample();
    let observed = state.bundle.snapshot.observed_at_epoch_ms;
    state.start(TASK, observed - 60_000).unwrap();
    assert_eq!(state.display_buffer_seconds(observed), 10_060);
    state
        .finish(
            TASK,
            OfflineOperationKind::Record,
            observed + 30_000,
            "carry-in".into(),
        )
        .unwrap();
    assert_eq!(state.display_buffer_seconds(observed + 30_000), 10_060);
    state.pending.clear();
    // Until we have a new snapshot, receipt alone must not remove the finished credit.
    assert_eq!(state.display_buffer_seconds(observed + 30_000), 10_060);
    let mut refreshed = state.bundle.clone();
    refreshed.snapshot.observed_at_epoch_ms = observed + 30_000;
    refreshed.snapshot.buffer_seconds = 10_060;
    state.rebase(refreshed);
    assert_eq!(state.display_buffer_seconds(observed + 30_000), 10_060);
}
