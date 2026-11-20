use super::*;
use crate::{
    DeadlineDisplayKind, LoadData, RoutineLoadReport, ScheduleOccurrence, ScheduledTask,
    ServerSnapshot, TaskDisplayKind,
};

pub(crate) const TASK: &str = "00112233-4455-4677-8899-aabbccddeeff";
pub(crate) const PARENT: &str = "cbd0b1f8-4cf3-49d9-9222-1a1b96098f6e";

pub(crate) fn family() -> OfflineState {
    let mut state = sample();
    let mut parent = state.bundle.days["2026-11-17"][0].clone();
    parent.task.task_id = Some(PARENT.into());
    parent.task.task_name = "親".into();
    parent.task.pending_until_epoch_ms = Some(parent.schedule_start_epoch_ms + 86_400_000);
    parent.occurrence = ScheduleOccurrence::Actual {
        task_id: PARENT.into(),
    };
    parent.is_leaf = false;
    for (_, rows) in state.bundle.days.iter_mut().skip(1) {
        *rows = vec![parent.clone()];
    }
    state
        .bundle
        .handoff_children
        .insert(PARENT.into(), BTreeSet::from([TASK.into()]));
    state
}

#[test]
fn 最後の子の完了で別日の親に同時刻で移り解除後も再開できる() {
    for kind in [
        OfflineOperationKind::Complete,
        OfflineOperationKind::CompleteWithoutRecording,
    ] {
        let mut state = family();
        let start = state.bundle.snapshot.observed_at_epoch_ms;
        assert!(state.start(PARENT, start).is_err());
        state.start(TASK, start).unwrap();
        state
            .finish(TASK, kind, start + 60_000, "child".into())
            .unwrap();
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].task_id, PARENT);
        assert_eq!(state.sessions[0].started_at_epoch_ms, start + 60_000);
        assert_eq!(
            state.sessions[0].pending_until_epoch_ms,
            state.bundle.days["2026-11-18"][0]
                .task
                .pending_until_epoch_ms
        );
        let mut restored: OfflineState =
            serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
        restored.release(PARENT);
        for (_, rows) in restored.bundle.days.iter().skip(1) {
            assert!(restored.can_start(&rows[0]));
        }
        restored.start(PARENT, start + 120_000).unwrap();
        assert_eq!(restored.sessions[0].started_at_epoch_ms, start + 120_000);
    }
}

#[test]
fn 期間外の親や未完了の兄弟がいる親には移行しない() {
    let mut absent = family();
    absent.bundle.days.retain(|date, _| date == "2026-11-17");
    let mut sibling = family();
    sibling
        .bundle
        .handoff_children
        .get_mut(PARENT)
        .unwrap()
        .insert("offscreen-child".into());
    for mut state in [absent, sibling] {
        state.start(TASK, 0).unwrap();
        state
            .finish(
                TASK,
                OfflineOperationKind::Complete,
                60_000,
                "finish".into(),
            )
            .unwrap();
        assert!(state.sessions.is_empty());
        assert!(state.start(PARENT, 60_000).is_err());
    }
}

#[test]
fn 複数の子の完了で開始した親は各子の同期結果に従う() {
    let mut state = family();
    let sibling = "d6d94f35-67bb-4fb4-b412-0b3c402ce68d";
    let mut second = state.bundle.days["2026-11-17"][0].clone();
    second.task.task_id = Some(sibling.into());
    second.occurrence = ScheduleOccurrence::Actual {
        task_id: sibling.into(),
    };
    state
        .bundle
        .days
        .get_mut("2026-11-17")
        .unwrap()
        .push(second);
    state
        .bundle
        .handoff_children
        .get_mut(PARENT)
        .unwrap()
        .insert(sibling.into());

    state.start(TASK, 0).unwrap();
    state
        .finish(TASK, OfflineOperationKind::Complete, 1000, "first".into())
        .unwrap();
    assert!(state.sessions.is_empty());
    assert!(state.start(PARENT, 1000).is_err());
    state.start(sibling, 1000).unwrap();
    state
        .finish(
            sibling,
            OfflineOperationKind::Complete,
            2000,
            "second".into(),
        )
        .unwrap();
    assert_eq!(state.sessions[0].task_id, PARENT);
    state.mark_sync_conflict(TASK.into(), "実績競合".into());
    assert!(state.needs_review(PARENT));
    assert_eq!(state.review_task_name(PARENT), Some("親"));
    assert!(state
        .finish(PARENT, OfflineOperationKind::Record, 3000, "parent".into())
        .is_err());
}

#[test]
fn 非完了操作は親に移らず完了時も既存の親セッションを維持する() {
    let mut state = family();
    for kind in [OfflineOperationKind::Split, OfflineOperationKind::Record] {
        state.start(TASK, 0).unwrap();
        state
            .finish(TASK, kind, 60_000, format!("{kind:?}"))
            .unwrap();
        assert!(state.sessions.is_empty());
        assert!(state.start(PARENT, 60_000).is_err());
    }
    let parent = state.bundle.days["2026-11-18"][0]
        .task
        .actionable_task()
        .unwrap();
    let active = WorkSession::from_task(&parent, 3000);
    state.sessions.push(active.clone());
    state.start(TASK, 0).unwrap();
    state
        .finish(
            TASK,
            OfflineOperationKind::Complete,
            60_000,
            "finish".into(),
        )
        .unwrap();
    assert_eq!(state.sessions, [active]);
}

#[test]
fn 親子情報のない旧形式から未送信記録を復元しても親を開始しない() {
    let mut state = family();
    state.start(TASK, 0).unwrap();
    state
        .finish(TASK, OfflineOperationKind::Record, 60_000, "record".into())
        .unwrap();
    let mut json = serde_json::to_value(state).unwrap();
    json["bundle"]
        .as_object_mut()
        .unwrap()
        .remove("handoff_children");
    let mut restored: OfflineState = serde_json::from_value(json).unwrap();
    restored.start(TASK, 100_000).unwrap();
    restored
        .finish(
            TASK,
            OfflineOperationKind::Complete,
            200_000,
            "child".into(),
        )
        .unwrap();
    assert!(restored.sessions.is_empty());
    assert_eq!(restored.pending.len(), 2);
    assert!(restored.start(PARENT, 200_000).is_err());
}

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
            project_names: BTreeMap::new(),
            handoff_children: BTreeMap::new(),
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
    assert_eq!(result.review_task_name(TASK), Some("対象タスク"));
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
fn completion_hides_all_actual_segments_but_keeps_the_next_occurrence_and_other_tasks() {
    for kind in [
        OfflineOperationKind::Complete,
        OfflineOperationKind::CompleteWithoutRecording,
    ] {
        let mut state = sample();
        let start = state.bundle.snapshot.observed_at_epoch_ms;
        let mut projected = state.bundle.days.values().next().unwrap()[0].clone();
        projected.task.task_id = None;
        projected.occurrence = ScheduleOccurrence::Projected {
            occurrence_key: "next".into(),
            source_task_id: TASK.into(),
        };
        let mut other = projected.clone();
        other.task.task_id = Some("other".into());
        other.occurrence = ScheduleOccurrence::Actual {
            task_id: "other".into(),
        };
        for rows in state.bundle.days.values_mut() {
            rows.extend([projected.clone(), other.clone()]);
        }
        state.start(TASK, start).unwrap();
        state
            .finish(TASK, kind, start + 60_000, "finish".into())
            .unwrap();
        for date in state.bundle.days.keys() {
            assert_eq!(
                state.visible_schedule(date).collect::<Vec<_>>(),
                [&projected, &other],
                "{kind:?} on {date}"
            );
        }
    }
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

#[test]
fn 同じ分の分割を別々に数え完了時刻の朝六時で日を分ける() {
    let mut state = sample();
    let start = chrono::DateTime::parse_from_rfc3339("2026-11-18T05:59:00+09:00")
        .unwrap()
        .timestamp_millis();
    let project = "集計対象プロジェクト";
    state
        .bundle
        .project_names
        .insert(TASK.into(), project.into());
    for (second, id) in [(5, "split-1"), (10, "split-2")] {
        let end = start + second * 1000;
        state.start(TASK, end - 2000).unwrap();
        state
            .finish(TASK, OfflineOperationKind::Split, end, id.into())
            .unwrap();
    }
    let splits = state.completions_on("2026-11-17");
    assert_eq!(splits.len(), 2);
    assert_ne!(splits[0].id, splits[1].id);
    for row in splits {
        assert_eq!(row.task_name, "対象タスク-20261118-0559");
        assert_eq!(row.actual_work_seconds, Some(2));
        assert_eq!(row.estimated_work_seconds, Some(2));
        assert_eq!(row.project_name.as_deref(), Some(project));
    }
    assert_eq!(state.visible_schedule("2026-11-18").count(), 1);
    state.start(TASK, start + 50_000).unwrap();
    state
        .finish(
            TASK,
            OfflineOperationKind::Complete,
            start + 60_000,
            "complete".into(),
        )
        .unwrap();
    let completed = state.completions_on("2026-11-18");
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].actual_work_seconds, Some(20));
    assert_eq!(completed[0].estimated_work_seconds, Some(3596));
    assert_eq!(state.visible_schedule("2026-11-18").count(), 0);
}

#[test]
fn 旧形式の未送信操作を復元し見積のない実績ゼロを区別する() {
    let mut state = sample();
    let start = state.bundle.snapshot.observed_at_epoch_ms;
    state.start(TASK, start).unwrap();
    state.sessions[0].actual_work_seconds_at_start = 0;
    state.sessions[0].estimated_work_seconds_at_start = 0;
    state
        .finish(
            TASK,
            OfflineOperationKind::CompleteWithoutRecording,
            start + 1000,
            "completion".into(),
        )
        .unwrap();
    assert_eq!(
        state.completions_on("2026-11-17")[0].actual_work_seconds,
        Some(1)
    );
    let mut json = serde_json::to_value(state).unwrap();
    json.as_object_mut().unwrap().remove("completion_history");
    json["bundle"]
        .as_object_mut()
        .unwrap()
        .remove("project_names");
    let mut legacy: OfflineState = serde_json::from_value(json).unwrap();
    legacy.bundle.days.clear();
    legacy.restore_completion_history();
    legacy.restore_completion_history();
    let rows = legacy.completions_on("2026-11-17");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task_name, "対象タスク");
    assert_eq!(rows[0].project_name, None);
    assert_eq!(rows[0].actual_work_seconds, None);
    assert_eq!(rows[0].estimated_work_seconds, None);
}
