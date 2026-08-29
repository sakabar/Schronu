//! Schronuの予定配置policy。
//!
//! Schronuは「重要なtaskを先に進める」ことと「締切までに必要な容量を残す」
//! ことを分けて扱う。flexible taskは通常priority順だが、deadline `D`のslackが
//! 0以下になった時だけ、そのdeadline groupを保護する。
//!
//! `slack(D, t) = fixed予約を除く[t, D)の空き秒 - deadlineがD以下の未完了残秒`
//!
//! 用語:
//! - fixed: 指定開始を動かさず、flexibleに対しては予約区間となる予定。
//! - effective deadline: 明示deadlineと、依存先fixedの開始時刻のうち早い方。
//! - event: task完了、fixed開始、candidate release、またはslackが0になる時点。
//!
//! 入口関数は4 phase(分類、effective deadline計算、event駆動配置、表示sort)を
//! その順に読める形に保つ。主な不変条件はfixedを動かさないこと、fixed同士の
//! 重複を隠さないこと、作業秒を欠落・重複させないこと、依存完了前に着手しない
//! ことである。deadlineまたは依存が実現不能でもloopせず、決定的なfallback配置を
//! 返す。これにより上位層が既存の期限超過表示を行える。

use crate::application::scheduling_metrics::ScheduleMetrics;
use crate::entity::task::TaskHandle;
use chrono::{DateTime, Duration, Local};
use std::cmp::{max, Reverse};
use std::collections::{HashMap, HashSet};
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

#[derive(Clone)]
struct FlexibleState {
    candidate: TaskScheduleCandidate,
    remaining_seconds: i64,
    total_work_seconds: i64,
    effective_deadline: Option<DateTime<Local>>,
    completion_time: Option<DateTime<Local>>,
    completion_gate: bool,
}

struct Selection {
    index: usize,
    slacks: Vec<(DateTime<Local>, i64)>,
}

/// fixed開始をすべてのtransitive dependencyへ逆伝搬する。
///
/// ここでは開始時刻を変更せず、容量保護に使う内部deadlineだけを作る。
fn effective_deadlines(
    candidates: &[TaskScheduleCandidate],
) -> HashMap<Uuid, Option<DateTime<Local>>> {
    let candidates_by_id = candidates
        .iter()
        .map(|candidate| (candidate.id, candidate))
        .collect::<HashMap<_, _>>();
    let mut deadlines = candidates
        .iter()
        .map(|candidate| (candidate.id, candidate.deadline_time))
        .collect::<HashMap<_, _>>();

    for fixed in candidates.iter().filter(|candidate| candidate.fixed_start) {
        let mut stack = fixed.dependency_ids.clone();
        let mut visited = HashSet::new();
        while let Some(dependency_id) = stack.pop() {
            if !visited.insert(dependency_id) {
                continue;
            }
            deadlines.entry(dependency_id).and_modify(|deadline| {
                *deadline = Some(
                    deadline
                        .map(|explicit| explicit.min(fixed.fixed_start_time))
                        .unwrap_or(fixed.fixed_start_time),
                );
            });
            if let Some(dependency) = candidates_by_id.get(&dependency_id) {
                stack.extend(dependency.dependency_ids.iter().copied());
            }
        }
    }
    deadlines
}

fn dependencies_complete(state: &FlexibleState, states: &[FlexibleState]) -> bool {
    state.candidate.dependency_ids.iter().all(|dependency_id| {
        states
            .iter()
            .find(|dependency| dependency.candidate.id == *dependency_id)
            .and_then(|dependency| dependency.completion_time)
            .is_some()
    })
}

fn dependency_end(state: &FlexibleState, states: &[FlexibleState]) -> Option<DateTime<Local>> {
    state
        .candidate
        .dependency_ids
        .iter()
        .filter_map(|dependency_id| {
            states
                .iter()
                .find(|dependency| dependency.candidate.id == *dependency_id)
                .and_then(|dependency| dependency.completion_time)
        })
        .max()
}

fn release_time(state: &FlexibleState, states: &[FlexibleState]) -> Option<DateTime<Local>> {
    dependencies_complete(state, states).then(|| {
        max(
            state.candidate.first_available_time,
            dependency_end(state, states).unwrap_or(state.candidate.first_available_time),
        )
    })
}

/// `[start, deadline)`からfixed予約のunionを差し引いた秒数を返す。
fn available_seconds_until(
    start: DateTime<Local>,
    deadline: DateTime<Local>,
    fixed_slots: &[(DateTime<Local>, DateTime<Local>)],
) -> i64 {
    if deadline <= start {
        return 0;
    }
    let reserved = fixed_slots
        .iter()
        .map(|(fixed_start, fixed_end)| {
            let overlap_start = max(start, *fixed_start);
            let overlap_end = deadline.min(*fixed_end);
            (overlap_end - overlap_start).num_seconds().max(0)
        })
        .sum::<i64>();
    (deadline - start).num_seconds().saturating_sub(reserved)
}

/// deadlineごとの累積需要を引い、容量が尽きる最初のdeadlineを返す。
fn deadline_slacks(
    states: &[FlexibleState],
    now: DateTime<Local>,
    fixed_slots: &[(DateTime<Local>, DateTime<Local>)],
) -> Vec<(DateTime<Local>, i64)> {
    let mut demand_by_deadline = states
        .iter()
        .filter(|state| state.remaining_seconds > 0)
        .filter_map(|state| {
            state
                .effective_deadline
                .map(|deadline| (deadline, state.remaining_seconds))
        })
        .collect::<Vec<_>>();
    demand_by_deadline.sort_unstable_by_key(|(deadline, _)| *deadline);

    // deadlineごとの全候補再走査はtask数の2乗になる。sort後のprefix sumなら
    // 同じ累積需要を1回の走査で得られ、数式の意味もそのまま読める。
    let mut result = Vec::new();
    let mut cumulative_demand = 0_i64;
    let mut index = 0;
    while let Some((deadline, _)) = demand_by_deadline.get(index).copied() {
        while let Some((same_deadline, seconds)) = demand_by_deadline.get(index).copied() {
            if same_deadline != deadline {
                break;
            }
            cumulative_demand = cumulative_demand.saturating_add(seconds);
            index += 1;
        }
        result.push((
            deadline,
            available_seconds_until(now, deadline, fixed_slots).saturating_sub(cumulative_demand),
        ));
    }
    result
}

fn normal_selection_key(
    state: &FlexibleState,
) -> (Reverse<i64>, bool, Option<DateTime<Local>>, usize, Uuid) {
    (
        Reverse(state.candidate.priority),
        state.effective_deadline.is_none(),
        state.effective_deadline,
        state.candidate.rank,
        state.candidate.id,
    )
}

fn protected_selection_key(state: &FlexibleState) -> (DateTime<Local>, Reverse<i64>, usize, Uuid) {
    (
        state
            .effective_deadline
            .expect("protected group always has an effective deadline"),
        Reverse(state.candidate.priority),
        state.candidate.rank,
        state.candidate.id,
    )
}

fn fixed_slot_containing(
    now: DateTime<Local>,
    fixed_slots: &[(DateTime<Local>, DateTime<Local>)],
) -> Option<(DateTime<Local>, DateTime<Local>)> {
    fixed_slots
        .iter()
        .find(|(start, end)| *start <= now && now < *end)
        .copied()
}

fn next_fixed_start(
    now: DateTime<Local>,
    fixed_slots: &[(DateTime<Local>, DateTime<Local>)],
) -> Option<DateTime<Local>> {
    fixed_slots
        .iter()
        .map(|(start, _)| *start)
        .find(|start| *start > now)
}

fn next_release_event(states: &[FlexibleState], now: DateTime<Local>) -> Option<DateTime<Local>> {
    states
        .iter()
        .filter(|state| state.completion_time.is_none())
        .filter_map(|state| release_time(state, states))
        .filter(|release| *release > now)
        .min()
}

/// 選択taskがdeadline保護対象外なら、最小の正slackが0になる時刻を返す。
fn slack_boundary(
    selected: &FlexibleState,
    slacks: &[(DateTime<Local>, i64)],
    now: DateTime<Local>,
) -> Option<DateTime<Local>> {
    slacks
        .iter()
        .filter(|(deadline, slack)| {
            *slack > 0
                && selected
                    .effective_deadline
                    .is_none_or(|selected_deadline| selected_deadline > *deadline)
        })
        .filter_map(|(_, slack)| checked_segment_end(selected.candidate.id, now, *slack).ok())
        .min()
}

fn segment_boundary(
    selected: &FlexibleState,
    states: &[FlexibleState],
    now: DateTime<Local>,
    fixed_slots: &[(DateTime<Local>, DateTime<Local>)],
    slacks: &[(DateTime<Local>, i64)],
) -> Result<DateTime<Local>, SchedulingPolicyError> {
    let completion = checked_segment_end(selected.candidate.id, now, selected.remaining_seconds)?;
    Ok([
        Some(completion),
        next_fixed_start(now, fixed_slots),
        next_release_event(states, now),
        slack_boundary(selected, slacks, now),
    ]
    .into_iter()
    .flatten()
    .min()
    .expect("task completion is always a boundary"))
}

/// atomicは途中eventを跨がず、1segmentで完了できる場合だけ開始する。
fn atomic_fits(
    selected: &FlexibleState,
    states: &[FlexibleState],
    now: DateTime<Local>,
    fixed_slots: &[(DateTime<Local>, DateTime<Local>)],
    slacks: &[(DateTime<Local>, i64)],
) -> Result<bool, SchedulingPolicyError> {
    if !selected.candidate.atomic {
        return Ok(true);
    }
    let completion = checked_segment_end(selected.candidate.id, now, selected.remaining_seconds)?;
    Ok(completion == segment_boundary(selected, states, now, fixed_slots, slacks)?)
}

/// ready候補の選択規則を唯一の場所に集約する。
///
/// 最早のcritical deadlineがあればそこまでの候補を保護し、それ以外は
/// priorityを最優先する。先頭atomicが入らない場合は、入る候補まで決定的に送る。
fn select_next_candidate(
    states: &[FlexibleState],
    ready_indices: &[usize],
    now: DateTime<Local>,
    fixed_slots: &[(DateTime<Local>, DateTime<Local>)],
) -> Result<Option<Selection>, SchedulingPolicyError> {
    let slacks = deadline_slacks(states, now, fixed_slots);
    let critical_deadline = slacks
        .iter()
        .find(|(_, slack)| *slack <= 0)
        .map(|(deadline, _)| *deadline);
    let mut ordered = ready_indices.to_vec();
    let protected_ready = critical_deadline.is_some_and(|critical| {
        ordered.iter().any(|index| {
            states[*index]
                .effective_deadline
                .is_some_and(|deadline| deadline <= critical)
        })
    });
    if protected_ready {
        let critical = critical_deadline.expect("protected mode requires a critical deadline");
        ordered.retain(|index| {
            states[*index]
                .effective_deadline
                .is_some_and(|deadline| deadline <= critical)
        });
        ordered.sort_by_key(|index| protected_selection_key(&states[*index]));
    } else {
        ordered.sort_by_key(|index| normal_selection_key(&states[*index]));
    }

    for index in ordered {
        if atomic_fits(&states[index], states, now, fixed_slots, &slacks)? {
            return Ok(Some(Selection { index, slacks }));
        }
    }
    Ok(None)
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
    // Phase 1: fixedを先に分類し、約束時刻がflexibleの選択結果に影響されない
    // 形で予約する。
    let prepared = classify_fixed_candidates(candidates, last_synced_time, metrics)?;

    // Phase 2: fixed開始をdependencyのeffective deadlineにすることで、fixed本体を
    // 動かさずにその準備容量だけを保護する。
    let effective_deadlines_by_id = effective_deadlines(candidates);
    let mut states = prepared
        .pending
        .into_iter()
        .map(|candidate| {
            metrics.record_dependency_candidate_probe();
            let remaining_seconds = candidate.remaining_seconds.max(0);
            FlexibleState {
                total_work_seconds: prepared
                    .total_work_seconds_by_id
                    .get(&candidate.id)
                    .copied()
                    .unwrap_or(remaining_seconds),
                effective_deadline: effective_deadlines_by_id
                    .get(&candidate.id)
                    .copied()
                    .flatten(),
                completion_gate: prepared.completion_gate_ids.contains(&candidate.id),
                candidate,
                remaining_seconds,
                completion_time: None,
            }
        })
        .collect::<Vec<_>>();
    let fixed_slots = prepared.occupied_fixed;
    let mut scheduled_tasks = prepared.scheduled_fixed;
    let mut now = last_synced_time;

    // Phase 3: eventとeventの間だけを配置し、releaseやslack境界で必ず再選択する。
    while states.iter().any(|state| state.completion_time.is_none()) {
        if let Some((_, fixed_end)) = fixed_slot_containing(now, &fixed_slots) {
            now = fixed_end;
            continue;
        }

        // 0秒taskもdependencyのcompletion gateとして意味があるため、選択前に
        // 固定点で完了させる。
        let zero_ready = states.iter().enumerate().find_map(|(index, state)| {
            (state.completion_time.is_none()
                && state.remaining_seconds == 0
                && release_time(state, &states).is_some_and(|release| release <= now))
            .then_some(index)
        });
        if let Some(index) = zero_ready {
            if !states[index].completion_gate {
                metrics.record_segment();
                scheduled_tasks.push(to_scheduled_task(
                    &states[index].candidate,
                    now,
                    now,
                    0,
                    states[index].total_work_seconds,
                ));
            }
            states[index].completion_time = Some(now);
            continue;
        }

        let ready_indices = states
            .iter()
            .enumerate()
            .filter(|(_, state)| state.completion_time.is_none() && state.remaining_seconds > 0)
            .filter(|(_, state)| release_time(state, &states).is_some_and(|release| release <= now))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();

        let selection = select_next_candidate(&states, &ready_indices, now, &fixed_slots)?;
        let Some(selection) = selection else {
            let next_release = next_release_event(&states, now);
            let next_fixed = next_fixed_start(now, &fixed_slots);
            if let Some(next_event) = [next_release, next_fixed].into_iter().flatten().min() {
                now = next_event;
                continue;
            }

            // missing dependencyやcycleでも、上位層が問題を可視化できるscheduleを
            // 返す。UUID順入力に依らない通常選択がfallback順となる。
            let fallback = states
                .iter()
                .enumerate()
                .filter(|(_, state)| state.completion_time.is_none() && state.remaining_seconds > 0)
                .min_by_key(|(_, state)| normal_selection_key(state))
                .map(|(index, _)| index);
            if let Some(index) = fallback {
                let release = states[index].candidate.first_available_time;
                now = max(now, release);
                let fallback_slacks = deadline_slacks(&states, now, &fixed_slots);
                schedule_selected_segment(
                    index,
                    &mut states,
                    &mut scheduled_tasks,
                    &mut now,
                    &fixed_slots,
                    &fallback_slacks,
                    metrics,
                    true,
                )?;
                continue;
            }
            break;
        };

        schedule_selected_segment(
            selection.index,
            &mut states,
            &mut scheduled_tasks,
            &mut now,
            &fixed_slots,
            &selection.slacks,
            metrics,
            false,
        )?;
    }

    // Phase 4: 表示順は選択policyと分離し、同一入力から常に同一結果を返す。
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

#[allow(clippy::too_many_arguments)]
fn schedule_selected_segment(
    selected_index: usize,
    states: &mut [FlexibleState],
    scheduled_tasks: &mut Vec<ScheduledTask>,
    now: &mut DateTime<Local>,
    fixed_slots: &[(DateTime<Local>, DateTime<Local>)],
    slacks: &[(DateTime<Local>, i64)],
    metrics: &mut ScheduleMetrics,
    ignore_dependencies: bool,
) -> Result<(), SchedulingPolicyError> {
    let mut boundary =
        segment_boundary(&states[selected_index], states, *now, fixed_slots, slacks)?;
    if states[selected_index].candidate.atomic {
        boundary = checked_segment_end(
            states[selected_index].candidate.id,
            *now,
            states[selected_index].remaining_seconds,
        )?;
    }
    let work_seconds_before_boundary_adjustment = (boundary - *now).num_seconds();
    let after_split = states[selected_index]
        .remaining_seconds
        .saturating_sub(work_seconds_before_boundary_adjustment);
    if !states[selected_index].candidate.atomic
        && after_split > 0
        && (work_seconds_before_boundary_adjustment <= MIN_SPLIT_SEGMENT_SECONDS
            || after_split <= MIN_SPLIT_SEGMENT_SECONDS)
    {
        let fixed_boundary = next_fixed_start(*now, fixed_slots) == Some(boundary);
        let guard_boundary =
            slack_boundary(&states[selected_index], slacks, *now) == Some(boundary);
        let ready_at_boundary = states
            .iter()
            .enumerate()
            .filter(|(_, state)| state.completion_time.is_none() && state.remaining_seconds > 0)
            .filter(|(_, state)| {
                release_time(state, states).is_some_and(|release| release <= boundary)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let release_preempts =
            select_next_candidate(states, &ready_at_boundary, boundary, fixed_slots)?
                .is_some_and(|selection| selection.index != selected_index);
        if fixed_boundary || guard_boundary || release_preempts {
            // fixed、deadline guard、より優先されるreleaseは越えない。短い
            // fragmentは作らず、event後に再配置する。
            *now = boundary;
            return Ok(());
        }
        // release後も同じtaskが選ばれるなら、見かけだけの数分segmentを
        // 作らず、そのまま完了させる。
        boundary = checked_segment_end(
            states[selected_index].candidate.id,
            *now,
            states[selected_index].remaining_seconds,
        )?;
    }

    let work_seconds = (boundary - *now).num_seconds();
    let scheduled_start = *now;
    let total_work_seconds = states[selected_index].total_work_seconds;
    let candidate = states[selected_index].candidate.clone();
    metrics.record_segment();
    scheduled_tasks.push(to_scheduled_task(
        &candidate,
        scheduled_start,
        boundary,
        work_seconds,
        total_work_seconds,
    ));
    states[selected_index].remaining_seconds -= work_seconds;
    *now = boundary;
    if states[selected_index].remaining_seconds == 0 {
        states[selected_index].completion_time = Some(boundary);
    } else if ignore_dependencies {
        // fallbackは1segment進めることでcycleを解く。残作業は通常event loopへ戻す。
        states[selected_index].candidate.dependency_ids.clear();
    }
    Ok(())
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
