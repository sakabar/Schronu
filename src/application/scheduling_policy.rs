//! Schronuの予定配置policy。
//!
//! fixed予定を通常候補と同じ順序で処理すると、先に配置されたtaskによって約束の
//! 開始時刻が動いてしまう。このmoduleではfixed区間を最初に予約し、残りの候補だけを
//! 従来のdeadline・priority順で配置する。fixed予定が元windowへ収まらない場合は、
//! window内の作業と後続の超過分へ分けるが、両者の作業秒数の合計は必ず元の残秒とする。

use crate::application::scheduling_metrics::ScheduleMetrics;
use crate::entity::task::TaskHandle;
use chrono::{DateTime, Duration, Local};
use std::cmp::{max, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};
use uuid::Uuid;

const MIN_SPLIT_SEGMENT_SECONDS: i64 = 5 * 60;

#[derive(Clone)]
pub(super) struct TaskScheduleCandidate {
    pub(super) id: Uuid,
    pub(super) task: TaskHandle,
    pub(super) first_available_time: DateTime<Local>,
    pub(super) priority: i64,
    pub(super) rank: usize,
    pub(super) deadline_time: Option<DateTime<Local>>,
    pub(super) remaining_seconds: i64,
    pub(super) dependency_ids: Vec<Uuid>,
    pub(super) atomic: bool,
    pub(super) fixed_start: bool,
    pub(super) fixed_start_time: DateTime<Local>,
    pub(super) estimated_work_seconds: i64,
}

#[derive(Clone)]
pub(super) struct ScheduledTask {
    pub(super) id: Uuid,
    pub(super) task: TaskHandle,
    pub(super) first_available_time: DateTime<Local>,
    pub(super) scheduled_start: DateTime<Local>,
    pub(super) scheduled_end: DateTime<Local>,
    pub(super) scheduled_work_seconds: i64,
    pub(super) total_work_seconds: i64,
    pub(super) priority: i64,
    pub(super) rank: usize,
    pub(super) deadline_time: Option<DateTime<Local>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SchedulingPolicyError {
    pub(super) task_id: Uuid,
    pub(super) start_time: DateTime<Local>,
    pub(super) work_seconds: i64,
}

struct PreparedCandidates {
    pending: Vec<TaskScheduleCandidate>,
    scheduled_fixed: Vec<ScheduledTask>,
    occupied_fixed: Vec<(DateTime<Local>, DateTime<Local>)>,
    completion_gate_ids: HashSet<Uuid>,
    total_work_seconds_by_id: HashMap<Uuid, i64>,
}

/// fixed候補を元window内の作業と後続の超過分へ分類する。
///
/// fixed同士の重複は解消しない。各taskの指定時刻を守ることが、資源の二重予約を
/// 隠さず利用者へ示すために必要だからである。一方、flexible候補へ渡す占有区間だけは
/// union化し、重複時間を二重に差し引かない。
fn classify_fixed_candidates(
    candidates: &[TaskScheduleCandidate],
    last_synced_time: DateTime<Local>,
    metrics: &mut ScheduleMetrics,
) -> Result<PreparedCandidates, SchedulingPolicyError> {
    let mut pending = Vec::with_capacity(candidates.len());
    let mut scheduled_fixed = Vec::new();
    let mut occupied_fixed = Vec::new();
    let mut completion_gate_ids = HashSet::new();
    let mut total_work_seconds_by_id = HashMap::new();

    for candidate in candidates {
        if !candidate.fixed_start {
            pending.push(candidate.clone());
            continue;
        }

        let total_work_seconds = candidate.remaining_seconds.max(0);
        total_work_seconds_by_id.insert(candidate.id, total_work_seconds);
        let fixed_segment_start = max(candidate.fixed_start_time, last_synced_time);
        let original_window_end = checked_segment_end(
            candidate.id,
            candidate.fixed_start_time,
            candidate.estimated_work_seconds.max(0),
        )?;
        let fixed_capacity_seconds = if original_window_end > fixed_segment_start {
            (original_window_end - fixed_segment_start).num_seconds()
        } else {
            0
        };
        let fixed_work_seconds = total_work_seconds.min(fixed_capacity_seconds);

        // 表示区間と衝突判定区間を同じ予約windowにする。作業済みのmeetingでも予約は
        // 消えず、実作業量はscheduled_work_secondsとして区間長と独立に保持される。
        if original_window_end > fixed_segment_start {
            insert_occupied_slot(
                &mut occupied_fixed,
                (fixed_segment_start, original_window_end),
                metrics,
            );
            metrics.record_segment();
            scheduled_fixed.push(to_scheduled_task(
                candidate,
                fixed_segment_start,
                original_window_end,
                fixed_work_seconds,
                total_work_seconds,
            ));
        } else if total_work_seconds == 0 {
            // zero remainingも1つの決定的な点として残し、候補消失を防ぐ。
            metrics.record_segment();
            scheduled_fixed.push(to_scheduled_task(
                candidate,
                fixed_segment_start,
                fixed_segment_start,
                0,
                0,
            ));
        }

        let excess_seconds = total_work_seconds - fixed_work_seconds;
        if excess_seconds > 0 {
            let mut excess = candidate.clone();
            excess.remaining_seconds = excess_seconds;
            excess.fixed_start = false;
            excess.first_available_time = max(original_window_end, last_synced_time);
            pending.push(excess);
        } else {
            // 表示windowは先に確定しても、下流dependencyの解放は上流完了後である。
            // 0秒gateをpendingへ残し、segmentを追加表示せずcompletionだけを伝える。
            let mut completion_gate = candidate.clone();
            completion_gate.remaining_seconds = 0;
            completion_gate.fixed_start = false;
            completion_gate.first_available_time = max(original_window_end, last_synced_time);
            completion_gate_ids.insert(candidate.id);
            pending.push(completion_gate);
        }
    }

    Ok(PreparedCandidates {
        pending,
        scheduled_fixed,
        occupied_fixed,
        completion_gate_ids,
        total_work_seconds_by_id,
    })
}

/// segment終了を表現できない場合に、taskと原因となる開始・秒数を失わず返す。
fn checked_segment_end(
    task_id: Uuid,
    start_time: DateTime<Local>,
    work_seconds: i64,
) -> Result<DateTime<Local>, SchedulingPolicyError> {
    Duration::try_seconds(work_seconds)
        .and_then(|duration| start_time.checked_add_signed(duration))
        .ok_or(SchedulingPolicyError {
            task_id,
            start_time,
            work_seconds,
        })
}

fn find_earliest_non_overlapping_start(
    task_id: Uuid,
    first_available_time: DateTime<Local>,
    remaining_seconds: i64,
    occupied_slots: &[(DateTime<Local>, DateTime<Local>)],
    metrics: &mut ScheduleMetrics,
) -> Result<DateTime<Local>, SchedulingPolicyError> {
    let mut start = first_available_time;
    let mut index = occupied_slots.partition_point(|(_, occupied_end)| {
        metrics.record_occupied_slot_probe();
        *occupied_end <= start
    });
    while let Some((occupied_start, occupied_end)) = occupied_slots.get(index) {
        let end = checked_segment_end(task_id, start, remaining_seconds)?;
        metrics.record_occupied_slot_probe();
        if end <= *occupied_start {
            break;
        }
        if start < *occupied_end && *occupied_start < end {
            start = *occupied_end;
        }
        index += 1;
    }
    Ok(start)
}

fn find_next_occupied_slot(
    start: DateTime<Local>,
    occupied_slots: &[(DateTime<Local>, DateTime<Local>)],
    metrics: &mut ScheduleMetrics,
) -> Option<(DateTime<Local>, DateTime<Local>)> {
    let index = occupied_slots.partition_point(|(occupied_start, _)| {
        metrics.record_occupied_slot_probe();
        *occupied_start < start
    });
    occupied_slots
        .get(index)
        .filter(|(occupied_start, occupied_end)| {
            metrics.record_occupied_slot_probe();
            start < *occupied_end && start <= *occupied_start
        })
        .copied()
}

fn find_occupied_slot_containing(
    datetime: DateTime<Local>,
    occupied_slots: &[(DateTime<Local>, DateTime<Local>)],
    metrics: &mut ScheduleMetrics,
) -> Option<(DateTime<Local>, DateTime<Local>)> {
    let index = occupied_slots.partition_point(|(_, occupied_end)| {
        metrics.record_occupied_slot_probe();
        *occupied_end <= datetime
    });
    occupied_slots
        .get(index)
        .filter(|(occupied_start, occupied_end)| {
            metrics.record_occupied_slot_probe();
            datetime >= *occupied_start && datetime < *occupied_end
        })
        .copied()
}

fn find_occupied_slot_starting_at(
    datetime: DateTime<Local>,
    occupied_slots: &[(DateTime<Local>, DateTime<Local>)],
    metrics: &mut ScheduleMetrics,
) -> Option<(DateTime<Local>, DateTime<Local>)> {
    let index = occupied_slots.partition_point(|(occupied_start, _)| {
        metrics.record_occupied_slot_probe();
        *occupied_start < datetime
    });
    occupied_slots
        .get(index)
        .filter(|(occupied_start, _)| {
            metrics.record_occupied_slot_probe();
            *occupied_start == datetime
        })
        .copied()
}

fn insert_occupied_slot(
    occupied_slots: &mut Vec<(DateTime<Local>, DateTime<Local>)>,
    mut slot: (DateTime<Local>, DateTime<Local>),
    metrics: &mut ScheduleMetrics,
) {
    let first_merged = occupied_slots.partition_point(|(_, existing_end)| {
        metrics.record_occupied_slot_probe();
        *existing_end < slot.0
    });
    let mut past_merged = first_merged;
    while let Some((existing_start, existing_end)) = occupied_slots.get(past_merged) {
        metrics.record_occupied_slot_probe();
        if *existing_start > slot.1 {
            break;
        }
        slot.0 = slot.0.min(*existing_start);
        slot.1 = slot.1.max(*existing_end);
        past_merged += 1;
    }
    occupied_slots.splice(first_merged..past_merged, [slot]);
}

#[cfg(test)]
fn schedule_tasks_by_priority(
    candidates: &[TaskScheduleCandidate],
    last_synced_time: DateTime<Local>,
) -> Result<Vec<ScheduledTask>, SchedulingPolicyError> {
    schedule_tasks_by_priority_with_metrics(
        candidates,
        last_synced_time,
        &mut ScheduleMetrics::default(),
    )
}

pub(super) fn schedule_tasks_by_priority_with_metrics(
    candidates: &[TaskScheduleCandidate],
    last_synced_time: DateTime<Local>,
    metrics: &mut ScheduleMetrics,
) -> Result<Vec<ScheduledTask>, SchedulingPolicyError> {
    // fixed区間を先に予約しなければ、通常候補の処理順が約束時刻を動かしてしまう。
    let prepared = classify_fixed_candidates(candidates, last_synced_time, metrics)?;
    let mut pending_candidates = prepared.pending;
    metrics.record_sort();
    pending_candidates.sort_by(|a, b| {
        (
            a.deadline_time.is_none(),
            a.deadline_time,
            Reverse(a.priority),
            a.first_available_time,
            a.rank,
            a.id,
        )
            .cmp(&(
                b.deadline_time.is_none(),
                b.deadline_time,
                Reverse(b.priority),
                b.first_available_time,
                b.rank,
                b.id,
            ))
    });

    let candidate_index_by_id = pending_candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| (candidate.id, index))
        .collect::<HashMap<_, _>>();
    let mut dependent_indices_by_id = HashMap::<Uuid, Vec<usize>>::new();
    let mut unresolved_dependency_counts = Vec::with_capacity(pending_candidates.len());
    let mut ready_indices = BinaryHeap::<Reverse<usize>>::new();
    for (index, candidate) in pending_candidates.iter().enumerate() {
        metrics.record_dependency_candidate_probe();
        let unresolved_count = candidate.dependency_ids.len();
        unresolved_dependency_counts.push(unresolved_count);
        if unresolved_count == 0 {
            ready_indices.push(Reverse(index));
        }
        for dependency_id in &candidate.dependency_ids {
            if candidate_index_by_id.contains_key(dependency_id) {
                dependent_indices_by_id
                    .entry(*dependency_id)
                    .or_default()
                    .push(index);
            }
        }
    }
    let mut pending_candidates = pending_candidates.into_iter().map(Some).collect::<Vec<_>>();
    let mut remaining_candidate_count = pending_candidates.len();
    let mut occupied_slots = prepared.occupied_fixed;
    let mut scheduled_tasks = prepared.scheduled_fixed;
    let mut scheduled_end_by_id = HashMap::new();

    while remaining_candidate_count > 0 {
        let index = ready_indices
            .pop()
            .map(|Reverse(index)| index)
            .filter(|index| pending_candidates[*index].is_some())
            .or_else(|| pending_candidates.iter().position(Option::is_some))
            .expect("remaining candidate count guarantees a pending candidate");
        let candidate = pending_candidates[index]
            .take()
            .expect("ready candidate is pending");
        remaining_candidate_count -= 1;
        let dependency_end = candidate
            .dependency_ids
            .iter()
            .filter_map(|id| scheduled_end_by_id.get(id))
            .max()
            .copied()
            .unwrap_or(last_synced_time);
        let mut segment_start = find_earliest_non_overlapping_start(
            candidate.id,
            max(
                max(candidate.first_available_time, last_synced_time),
                dependency_end,
            ),
            0,
            &occupied_slots,
            metrics,
        )?;
        let mut remaining_seconds = candidate.remaining_seconds;
        let total_work_seconds = prepared
            .total_work_seconds_by_id
            .get(&candidate.id)
            .copied()
            .unwrap_or(remaining_seconds);
        let mut candidate_scheduled_end = segment_start;

        if remaining_seconds == 0 {
            if !prepared.completion_gate_ids.contains(&candidate.id) {
                metrics.record_segment();
                scheduled_tasks.push(to_scheduled_task(
                    &candidate,
                    segment_start,
                    segment_start,
                    0,
                    total_work_seconds,
                ));
            }
        } else if candidate.atomic {
            let start = find_earliest_non_overlapping_start(
                candidate.id,
                segment_start,
                remaining_seconds,
                &occupied_slots,
                metrics,
            )?;
            let end = checked_segment_end(candidate.id, start, remaining_seconds)?;
            metrics.record_segment();
            scheduled_tasks.push(to_scheduled_task(
                &candidate,
                start,
                end,
                remaining_seconds,
                total_work_seconds,
            ));
            insert_occupied_slot(&mut occupied_slots, (start, end), metrics);
            candidate_scheduled_end = end;
        } else {
            while remaining_seconds > 0 {
                segment_start = find_earliest_non_overlapping_start(
                    candidate.id,
                    segment_start,
                    0,
                    &occupied_slots,
                    metrics,
                )?;
                let uninterrupted_end =
                    checked_segment_end(candidate.id, segment_start, remaining_seconds)?;
                let segment_end =
                    match find_next_occupied_slot(segment_start, &occupied_slots, metrics) {
                        Some((occupied_start, _)) if occupied_start < uninterrupted_end => {
                            occupied_start
                        }
                        _ => uninterrupted_end,
                    };
                let work_seconds = (segment_end - segment_start).num_seconds();
                if work_seconds <= 0 {
                    segment_start =
                        find_occupied_slot_containing(segment_start, &occupied_slots, metrics)
                            .map(|(_, end)| end)
                            .map(Ok)
                            .unwrap_or_else(|| {
                                checked_segment_end(candidate.id, segment_start, 1)
                            })?;
                    continue;
                }
                let after_split = remaining_seconds - work_seconds;
                if segment_end < uninterrupted_end
                    && (work_seconds <= MIN_SPLIT_SEGMENT_SECONDS
                        || after_split <= MIN_SPLIT_SEGMENT_SECONDS)
                {
                    segment_start =
                        find_occupied_slot_starting_at(segment_end, &occupied_slots, metrics)
                            .map(|(_, end)| end)
                            .unwrap_or(segment_end);
                    continue;
                }
                metrics.record_segment();
                scheduled_tasks.push(to_scheduled_task(
                    &candidate,
                    segment_start,
                    segment_end,
                    work_seconds,
                    total_work_seconds,
                ));
                insert_occupied_slot(&mut occupied_slots, (segment_start, segment_end), metrics);
                remaining_seconds -= work_seconds;
                candidate_scheduled_end = segment_end;
                segment_start = segment_end;
            }
        }
        scheduled_end_by_id.insert(candidate.id, candidate_scheduled_end);
        if let Some(dependent_indices) = dependent_indices_by_id.get(&candidate.id) {
            for dependent_index in dependent_indices {
                let unresolved_count = &mut unresolved_dependency_counts[*dependent_index];
                *unresolved_count = unresolved_count.saturating_sub(1);
                if *unresolved_count == 0 && pending_candidates[*dependent_index].is_some() {
                    ready_indices.push(Reverse(*dependent_index));
                }
            }
        }
    }

    metrics.record_sort();
    scheduled_tasks.sort_by(|a, b| {
        (
            a.scheduled_start,
            a.deadline_time.is_none(),
            Reverse(a.priority),
            a.rank,
            a.id,
        )
            .cmp(&(
                b.scheduled_start,
                b.deadline_time.is_none(),
                Reverse(b.priority),
                b.rank,
                b.id,
            ))
    });
    Ok(scheduled_tasks)
}

fn to_scheduled_task(
    candidate: &TaskScheduleCandidate,
    scheduled_start: DateTime<Local>,
    scheduled_end: DateTime<Local>,
    scheduled_work_seconds: i64,
    total_work_seconds: i64,
) -> ScheduledTask {
    ScheduledTask {
        id: candidate.id,
        task: candidate.task.clone(),
        first_available_time: candidate.first_available_time,
        scheduled_start,
        scheduled_end,
        scheduled_work_seconds,
        total_work_seconds,
        priority: candidate.priority,
        rank: candidate.rank,
        deadline_time: candidate.deadline_time,
    }
}

#[cfg(test)]
#[path = "scheduling_policy_tests.rs"]
mod tests;
