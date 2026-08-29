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
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
    dependency_indices: Vec<Option<usize>>,
    remaining_seconds: i64,
    total_work_seconds: i64,
    effective_deadline: Option<DateTime<Local>>,
    completion_time: Option<DateTime<Local>>,
    completion_gate: bool,
}

struct Selection {
    index: usize,
    slack_boundary: Option<DateTime<Local>>,
}

/// 1回の候補評価で共有する、時刻境界のread-only view。
struct BoundaryContext<'a> {
    states: &'a [FlexibleState],
    fixed_slots: &'a [(DateTime<Local>, DateTime<Local>)],
    next_release: Option<DateTime<Local>>,
    frontier: Option<&'a SchedulerFrontier>,
    slack_index: Option<&'a SlackDemandIndex>,
}

type NormalReadyKey = (Reverse<i64>, bool, Option<DateTime<Local>>, usize, Uuid);
type ProtectedReadyKey = (DateTime<Local>, Reverse<i64>, usize, Uuid);

/// releaseとdependency完了を差分反映し、eventごとの全state走査を避ける。
#[derive(Clone)]
struct SchedulerFrontier {
    normal_ready: BTreeSet<(NormalReadyKey, usize)>,
    protected_ready: BTreeSet<(ProtectedReadyKey, usize)>,
    zero_ready: BTreeSet<(NormalReadyKey, usize)>,
    release_events: BTreeMap<DateTime<Local>, Vec<usize>>,
    unresolved_dependencies: Vec<usize>,
    dependency_end: Vec<Option<DateTime<Local>>>,
    dependents: Vec<Vec<usize>>,
    ready: Vec<bool>,
    incomplete_count: usize,
}

/// deadline需要を初期構築とsegment差分だけで維持する。
///
/// eventごとに全candidateから需要を作り直すと、segment数に比例して2乗走査へ
/// 戻る。group内残秒だけを更新し、slack式のprefix sumはdeadline数に限定する。
#[derive(Clone)]
struct SlackDemandIndex {
    current_time: DateTime<Local>,
    deadlines: Vec<DateTime<Local>>,
    remaining_by_group: Vec<i64>,
    group_by_state: Vec<Option<usize>>,
    slack_tree: SlackRangeTree,
    critical_groups: BTreeSet<usize>,
}

struct SpeculativeSlackChange {
    deactivated: Vec<(usize, i64)>,
}

const INACTIVE_SLACK: i64 = i64::MAX / 4;

/// active deadline groupのslack最小値をrange加算しながら保持する。
///
/// deadlineは実行中に増えないため、座標を初期化時に固定できる。これにより、
/// workで変化するdeadline prefixとidleで変化するsuffixを、各eventで全groupを
/// 作り直さずに更新できる。
#[derive(Clone)]
struct SlackRangeTree {
    len: usize,
    min: Vec<i64>,
    lazy: Vec<i64>,
}

impl SlackRangeTree {
    fn new(values: &[Option<i64>]) -> Self {
        let len = values.len();
        let capacity = len.next_power_of_two().max(1) * 4;
        let mut tree = Self {
            len,
            min: vec![INACTIVE_SLACK; capacity],
            lazy: vec![0; capacity],
        };
        if len > 0 {
            tree.build(1, 0, len, values);
        }
        tree
    }

    fn build(&mut self, node: usize, left: usize, right: usize, values: &[Option<i64>]) {
        if right - left == 1 {
            if let Some(value) = values[left] {
                self.min[node] = value;
            }
            return;
        }
        let middle = (left + right) / 2;
        self.build(node * 2, left, middle, values);
        self.build(node * 2 + 1, middle, right, values);
        self.pull(node);
    }

    fn range_add(
        &mut self,
        range: std::ops::Range<usize>,
        delta: i64,
        metrics: &mut ScheduleMetrics,
    ) {
        if range.start >= range.end || self.len == 0 || delta == 0 {
            return;
        }
        self.range_add_inner(1, 0, self.len, range.start, range.end, delta, metrics);
    }

    #[allow(clippy::too_many_arguments)]
    fn range_add_inner(
        &mut self,
        node: usize,
        left: usize,
        right: usize,
        query_left: usize,
        query_right: usize,
        delta: i64,
        metrics: &mut ScheduleMetrics,
    ) {
        metrics.record_slack_probes(1);
        if query_right <= left || right <= query_left {
            return;
        }
        if query_left <= left && right <= query_right {
            self.apply(node, delta);
            return;
        }
        self.push(node);
        let middle = (left + right) / 2;
        self.range_add_inner(
            node * 2,
            left,
            middle,
            query_left,
            query_right,
            delta,
            metrics,
        );
        self.range_add_inner(
            node * 2 + 1,
            middle,
            right,
            query_left,
            query_right,
            delta,
            metrics,
        );
        self.pull(node);
    }

    fn deactivate(&mut self, index: usize, metrics: &mut ScheduleMetrics) {
        if self.len > 0 {
            self.deactivate_inner(1, 0, self.len, index, metrics);
        }
    }

    fn activate(&mut self, index: usize, value: i64, metrics: &mut ScheduleMetrics) {
        self.activate_inner(1, 0, self.len, index, value, metrics);
    }

    fn activate_inner(
        &mut self,
        node: usize,
        left: usize,
        right: usize,
        index: usize,
        value: i64,
        metrics: &mut ScheduleMetrics,
    ) {
        metrics.record_slack_probes(1);
        if right - left == 1 {
            self.min[node] = value;
            self.lazy[node] = 0;
            return;
        }
        self.push(node);
        let middle = (left + right) / 2;
        if index < middle {
            self.activate_inner(node * 2, left, middle, index, value, metrics);
        } else {
            self.activate_inner(node * 2 + 1, middle, right, index, value, metrics);
        }
        self.pull(node);
    }

    fn point_value(&self, index: usize, metrics: &mut ScheduleMetrics) -> Option<i64> {
        self.range_min(index..index + 1, metrics)
    }

    fn deactivate_inner(
        &mut self,
        node: usize,
        left: usize,
        right: usize,
        index: usize,
        metrics: &mut ScheduleMetrics,
    ) {
        metrics.record_slack_probes(1);
        if right - left == 1 {
            self.min[node] = INACTIVE_SLACK;
            self.lazy[node] = 0;
            return;
        }
        self.push(node);
        let middle = (left + right) / 2;
        if index < middle {
            self.deactivate_inner(node * 2, left, middle, index, metrics);
        } else {
            self.deactivate_inner(node * 2 + 1, middle, right, index, metrics);
        }
        self.pull(node);
    }

    fn first_at_most(
        &self,
        range: std::ops::Range<usize>,
        threshold: i64,
        metrics: &mut ScheduleMetrics,
    ) -> Option<usize> {
        if range.start >= range.end || self.len == 0 {
            return None;
        }
        self.first_at_most_inner(1, 0, self.len, &range, threshold, 0, metrics)
    }

    #[allow(clippy::too_many_arguments)]
    fn first_at_most_inner(
        &self,
        node: usize,
        left: usize,
        right: usize,
        range: &std::ops::Range<usize>,
        threshold: i64,
        inherited_lazy: i64,
        metrics: &mut ScheduleMetrics,
    ) -> Option<usize> {
        metrics.record_slack_probes(1);
        if range.end <= left
            || right <= range.start
            || self.min[node].saturating_add(inherited_lazy) > threshold
        {
            return None;
        }
        if right - left == 1 {
            return Some(left);
        }
        let inherited_lazy = inherited_lazy.saturating_add(self.lazy[node]);
        let middle = (left + right) / 2;
        self.first_at_most_inner(
            node * 2,
            left,
            middle,
            range,
            threshold,
            inherited_lazy,
            metrics,
        )
        .or_else(|| {
            self.first_at_most_inner(
                node * 2 + 1,
                middle,
                right,
                range,
                threshold,
                inherited_lazy,
                metrics,
            )
        })
    }

    fn range_min(
        &self,
        range: std::ops::Range<usize>,
        metrics: &mut ScheduleMetrics,
    ) -> Option<i64> {
        if range.start >= range.end || self.len == 0 {
            return None;
        }
        let minimum = self.range_min_inner(1, 0, self.len, &range, 0, metrics);
        (minimum < INACTIVE_SLACK / 2).then_some(minimum)
    }

    fn range_min_inner(
        &self,
        node: usize,
        left: usize,
        right: usize,
        range: &std::ops::Range<usize>,
        inherited_lazy: i64,
        metrics: &mut ScheduleMetrics,
    ) -> i64 {
        metrics.record_slack_probes(1);
        if range.end <= left || right <= range.start {
            return INACTIVE_SLACK;
        }
        if range.start <= left && right <= range.end {
            return self.min[node].saturating_add(inherited_lazy);
        }
        if right - left == 1 {
            return INACTIVE_SLACK;
        }
        let inherited_lazy = inherited_lazy.saturating_add(self.lazy[node]);
        let middle = (left + right) / 2;
        let left_min = self.range_min_inner(node * 2, left, middle, range, inherited_lazy, metrics);
        let right_min =
            self.range_min_inner(node * 2 + 1, middle, right, range, inherited_lazy, metrics);
        left_min.min(right_min)
    }

    fn apply(&mut self, node: usize, delta: i64) {
        if self.min[node] < INACTIVE_SLACK / 2 {
            self.min[node] = self.min[node].saturating_add(delta);
            self.lazy[node] = self.lazy[node].saturating_add(delta);
        }
    }

    fn push(&mut self, node: usize) {
        let delta = self.lazy[node];
        if delta != 0 {
            self.apply(node * 2, delta);
            self.apply(node * 2 + 1, delta);
            self.lazy[node] = 0;
        }
    }

    fn pull(&mut self, node: usize) {
        self.min[node] = self.min[node * 2].min(self.min[node * 2 + 1]);
    }
}

impl SlackDemandIndex {
    fn new(
        states: &[FlexibleState],
        now: DateTime<Local>,
        fixed_slots: &[(DateTime<Local>, DateTime<Local>)],
        metrics: &mut ScheduleMetrics,
    ) -> Self {
        let mut deadlines = states
            .iter()
            .filter_map(|state| state.effective_deadline)
            .collect::<Vec<_>>();
        deadlines.sort_unstable();
        deadlines.dedup();
        let mut remaining_by_group = vec![0_i64; deadlines.len()];
        let group_by_state = states
            .iter()
            .map(|state| {
                state.effective_deadline.map(|deadline| {
                    metrics.record_slack_probes(1);
                    let group = deadlines
                        .binary_search(&deadline)
                        .expect("effective deadline was collected into the index");
                    remaining_by_group[group] =
                        remaining_by_group[group].saturating_add(state.remaining_seconds.max(0));
                    group
                })
            })
            .collect();
        let mut cumulative_demand = 0_i64;
        let raw_slacks = deadlines
            .iter()
            .enumerate()
            .map(|(group, deadline)| {
                cumulative_demand = cumulative_demand.saturating_add(remaining_by_group[group]);
                (remaining_by_group[group] > 0).then(|| {
                    available_seconds_until(now, *deadline, fixed_slots, metrics)
                        .saturating_sub(cumulative_demand)
                })
            })
            .collect::<Vec<_>>();
        let critical_groups = raw_slacks
            .iter()
            .enumerate()
            .filter_map(|(group, slack)| slack.is_some_and(|slack| slack <= 0).then_some(group))
            .collect();
        let slack_values = raw_slacks
            .into_iter()
            .map(|slack| slack.filter(|slack| *slack > 0))
            .collect::<Vec<_>>();
        Self {
            current_time: now,
            deadlines,
            remaining_by_group,
            group_by_state,
            slack_tree: SlackRangeTree::new(&slack_values),
            critical_groups,
        }
    }

    fn critical_deadline(&self, metrics: &mut ScheduleMetrics) -> Option<DateTime<Local>> {
        metrics.record_slack_probes(1);
        self.critical_groups
            .first()
            .map(|group| self.deadlines[*group])
    }

    /// `worked`を仮に進めた時点の最早critical deadlineを、indexを変更せず返す。
    fn critical_deadline_after_work(
        &self,
        state_index: usize,
        worked_seconds: i64,
        metrics: &mut ScheduleMetrics,
    ) -> Option<DateTime<Local>> {
        let changed_end = self.group_by_state[state_index].unwrap_or(self.deadlines.len());
        let newly_critical = self
            .slack_tree
            .first_at_most(0..changed_end, worked_seconds, metrics);
        metrics.record_slack_probes(1);
        newly_critical
            .into_iter()
            .chain(self.critical_groups.first().copied())
            .min()
            .map(|group| self.deadlines[group])
    }

    /// 選択候補が保護group外で消費できる最小の正slackを返す。
    fn slack_boundary(
        &self,
        state_index: usize,
        now: DateTime<Local>,
        metrics: &mut ScheduleMetrics,
    ) -> Option<DateTime<Local>> {
        self.slack_seconds_before_state(state_index, None, metrics)
            .and_then(|seconds| now.checked_add_signed(Duration::seconds(seconds)))
    }

    /// `worked_state`を仮に進めたrelease時点でのslack境界を返す。
    fn slack_boundary_after_work(
        &self,
        state_index: usize,
        now: DateTime<Local>,
        worked_state: usize,
        worked_seconds: i64,
        metrics: &mut ScheduleMetrics,
    ) -> Option<DateTime<Local>> {
        self.slack_seconds_before_state(state_index, Some((worked_state, worked_seconds)), metrics)
            .and_then(|seconds| now.checked_add_signed(Duration::seconds(seconds)))
    }

    fn slack_seconds_before_state(
        &self,
        state_index: usize,
        worked: Option<(usize, i64)>,
        metrics: &mut ScheduleMetrics,
    ) -> Option<i64> {
        let boundary_end = self.group_by_state[state_index].unwrap_or(self.deadlines.len());
        let Some((worked_state, worked_seconds)) = worked else {
            return self.slack_tree.range_min(0..boundary_end, metrics);
        };
        let changed_end = self.group_by_state[worked_state].unwrap_or(self.deadlines.len());
        let changed_overlap = boundary_end.min(changed_end);
        let changed = self
            .slack_tree
            .range_min(0..changed_overlap, metrics)
            .map(|slack| slack.saturating_sub(worked_seconds));
        let unchanged = self
            .slack_tree
            .range_min(changed_overlap..boundary_end, metrics);
        let minimum = match (changed, unchanged) {
            (Some(changed), Some(unchanged)) => Some(changed.min(unchanged)),
            (changed, unchanged) => changed.or(unchanged),
        };
        // 選択済みatomicはguardより前のreleaseだけを仮想評価するため、この差分で
        // 新たなcritical groupを跨がない。
        debug_assert!(minimum.is_none_or(|slack| slack > 0));
        minimum.filter(|slack| *slack > 0)
    }

    /// slackはscheduleの進行中に増えない。0へ達したgroupをactive treeから外すと、
    /// 最小の正slackは符号分布に関係なく通常のrange minimumで得られる。各groupの
    /// 移動は高々1回なので、全deactivateの総計はO(N log N)である。
    fn decrease_range(
        &mut self,
        range: std::ops::Range<usize>,
        seconds: i64,
        metrics: &mut ScheduleMetrics,
    ) -> Vec<(usize, i64)> {
        if range.start >= range.end || seconds == 0 {
            return Vec::new();
        }
        self.slack_tree
            .range_add(range.clone(), seconds.saturating_neg(), metrics);
        let mut deactivated = Vec::new();
        while let Some(group) = self.slack_tree.first_at_most(range.clone(), 0, metrics) {
            let current = self
                .slack_tree
                .point_value(group, metrics)
                .expect("critical group is active before deactivation");
            deactivated.push((group, current.saturating_add(seconds)));
            self.slack_tree.deactivate(group, metrics);
            self.critical_groups.insert(group);
        }
        deactivated
    }

    /// 実作業ではcapacity減と、そのtaskを含むdeadline需要減が相殺される。
    /// したがってdeadlineなしなら全group、deadlineありならそれより前のprefixだけを減らす。
    fn record_work(
        &mut self,
        state_index: usize,
        work_seconds: i64,
        metrics: &mut ScheduleMetrics,
    ) {
        self.current_time += Duration::seconds(work_seconds);
        let changed_end = self.group_by_state[state_index].unwrap_or(self.deadlines.len());
        self.decrease_range(0..changed_end, work_seconds, metrics);
        if let Some(group) = self.group_by_state[state_index] {
            self.remaining_by_group[group] =
                self.remaining_by_group[group].saturating_sub(work_seconds);
            if self.remaining_by_group[group] == 0 {
                self.slack_tree.deactivate(group, metrics);
                self.critical_groups.remove(&group);
            }
        }
    }

    /// release待ちで使わなかったfree capacityだけを、未来のdeadlineへ反映する。
    /// fixed区間のskipはcapacityを消費しないため、このmethodを呼ばない。
    fn record_idle(
        &mut self,
        old_now: DateTime<Local>,
        new_now: DateTime<Local>,
        metrics: &mut ScheduleMetrics,
    ) {
        self.apply_idle_decrease(old_now, new_now, metrics);
        self.current_time = new_now;
    }

    fn begin_speculative_idle(
        &mut self,
        old_now: DateTime<Local>,
        new_now: DateTime<Local>,
        metrics: &mut ScheduleMetrics,
    ) -> SpeculativeSlackChange {
        let deactivated = self.apply_idle_decrease(old_now, new_now, metrics);
        self.current_time = new_now;
        SpeculativeSlackChange { deactivated }
    }

    fn apply_idle_decrease(
        &mut self,
        old_now: DateTime<Local>,
        new_now: DateTime<Local>,
        metrics: &mut ScheduleMetrics,
    ) -> Vec<(usize, i64)> {
        let elapsed = (new_now - old_now).num_seconds().max(0);
        let first_after_old = self
            .deadlines
            .partition_point(|deadline| *deadline <= old_now);
        let first_future = self
            .deadlines
            .partition_point(|deadline| *deadline <= new_now);
        // jump中にdeadlineを越えたgroupは、そのdeadlineまでのcapacity差分だけを
        // 反映する。各deadlineを越えるのはschedule全体で1回なので、このpoint更新を
        // 加えても総計はO(deadline数 log deadline数)に収まる。
        let mut deactivated = Vec::new();
        for group in first_after_old..first_future {
            let seconds = (self.deadlines[group] - old_now).num_seconds().max(0);
            deactivated.extend(self.decrease_range(group..group + 1, seconds, metrics));
        }
        deactivated.extend(self.decrease_range(
            first_future..self.deadlines.len(),
            elapsed,
            metrics,
        ));
        deactivated
    }

    fn restore_speculative_idle(
        &mut self,
        old_now: DateTime<Local>,
        new_now: DateTime<Local>,
        change: SpeculativeSlackChange,
        metrics: &mut ScheduleMetrics,
    ) {
        let elapsed = (new_now - old_now).num_seconds().max(0);
        let first_after_old = self
            .deadlines
            .partition_point(|deadline| *deadline <= old_now);
        let first_future = self
            .deadlines
            .partition_point(|deadline| *deadline <= new_now);
        for group in first_after_old..first_future {
            let seconds = (self.deadlines[group] - old_now).num_seconds().max(0);
            self.slack_tree
                .range_add(group..group + 1, seconds, metrics);
        }
        self.slack_tree
            .range_add(first_future..self.deadlines.len(), elapsed, metrics);
        for (group, original_slack) in change.deactivated {
            self.critical_groups.remove(&group);
            self.slack_tree.activate(group, original_slack, metrics);
        }
        self.current_time = old_now;
    }

    fn record_fixed_skip(&mut self, new_now: DateTime<Local>) {
        self.current_time = new_now;
    }
}

impl SchedulerFrontier {
    fn new(states: &[FlexibleState]) -> Self {
        let mut unresolved_dependencies = vec![0; states.len()];
        let mut dependents = vec![Vec::new(); states.len()];
        for (index, state) in states.iter().enumerate() {
            unresolved_dependencies[index] = state.dependency_indices.len();
            for dependency in state.dependency_indices.iter().flatten() {
                dependents[*dependency].push(index);
            }
        }
        let mut frontier = Self {
            normal_ready: BTreeSet::new(),
            protected_ready: BTreeSet::new(),
            zero_ready: BTreeSet::new(),
            release_events: BTreeMap::new(),
            unresolved_dependencies,
            dependency_end: vec![None; states.len()],
            dependents,
            ready: vec![false; states.len()],
            incomplete_count: states.len(),
        };
        for (index, state) in states.iter().enumerate() {
            if frontier.unresolved_dependencies[index] == 0 {
                frontier
                    .release_events
                    .entry(state.candidate.first_available_time)
                    .or_default()
                    .push(index);
            }
        }
        frontier
    }

    fn promote_releases(
        &mut self,
        now: DateTime<Local>,
        states: &[FlexibleState],
        metrics: &mut ScheduleMetrics,
    ) {
        while self
            .release_events
            .first_key_value()
            .is_some_and(|(release, _)| *release <= now)
        {
            let (_, indices) = self
                .release_events
                .pop_first()
                .expect("a checked release event exists");
            for index in indices {
                metrics.record_release_candidate_probe();
                if states[index].completion_time.is_some() || self.ready[index] {
                    continue;
                }
                self.ready[index] = true;
                let normal_key = normal_selection_key(&states[index]);
                if states[index].remaining_seconds == 0 {
                    self.zero_ready.insert((normal_key, index));
                } else {
                    self.normal_ready.insert((normal_key, index));
                    if states[index].effective_deadline.is_some() {
                        self.protected_ready
                            .insert((protected_selection_key(&states[index]), index));
                    }
                }
            }
        }
    }

    fn next_release(&self) -> Option<DateTime<Local>> {
        self.release_events.first_key_value().map(|(time, _)| *time)
    }

    fn first_zero(&self) -> Option<usize> {
        self.zero_ready.first().map(|(_, index)| *index)
    }

    fn remove_ready(&mut self, index: usize, state: &FlexibleState) {
        if !self.ready[index] {
            return;
        }
        self.ready[index] = false;
        let normal_key = normal_selection_key(state);
        self.zero_ready.remove(&(normal_key, index));
        self.normal_ready.remove(&(normal_key, index));
        if state.effective_deadline.is_some() {
            self.protected_ready
                .remove(&(protected_selection_key(state), index));
        }
    }

    fn complete(
        &mut self,
        index: usize,
        completion_time: DateTime<Local>,
        states: &[FlexibleState],
        metrics: &mut ScheduleMetrics,
    ) {
        self.remove_ready(index, &states[index]);
        self.incomplete_count = self.incomplete_count.saturating_sub(1);
        for dependent in self.dependents[index].iter().copied() {
            metrics.record_release_candidate_probe();
            if self.unresolved_dependencies[dependent] == 0 {
                continue;
            }
            self.unresolved_dependencies[dependent] -= 1;
            self.dependency_end[dependent] = Some(
                self.dependency_end[dependent]
                    .map(|current| current.max(completion_time))
                    .unwrap_or(completion_time),
            );
            if self.unresolved_dependencies[dependent] == 0 {
                let release = states[dependent]
                    .candidate
                    .first_available_time
                    .max(self.dependency_end[dependent].unwrap_or(completion_time));
                self.release_events
                    .entry(release)
                    .or_default()
                    .push(dependent);
            }
        }
    }

    fn force_ready(&mut self, index: usize, state: &FlexibleState) {
        self.unresolved_dependencies[index] = 0;
        if self.ready[index] {
            return;
        }
        self.ready[index] = true;
        let normal_key = normal_selection_key(state);
        if state.remaining_seconds == 0 {
            self.zero_ready.insert((normal_key, index));
        } else {
            self.normal_ready.insert((normal_key, index));
            if state.effective_deadline.is_some() {
                self.protected_ready
                    .insert((protected_selection_key(state), index));
            }
        }
    }
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
    state.dependency_indices.iter().all(|dependency_index| {
        dependency_index
            .and_then(|index| states[index].completion_time)
            .is_some()
    })
}

fn dependency_end(state: &FlexibleState, states: &[FlexibleState]) -> Option<DateTime<Local>> {
    state
        .dependency_indices
        .iter()
        .filter_map(|dependency_index| {
            dependency_index.and_then(|index| states[index].completion_time)
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
    metrics: &mut ScheduleMetrics,
) -> i64 {
    if deadline <= start {
        return 0;
    }
    let reserved = fixed_slots
        .iter()
        .inspect(|_| metrics.record_occupied_slot_probe())
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
    metrics: &mut ScheduleMetrics,
) -> Vec<(DateTime<Local>, i64)> {
    let mut demand_by_deadline = states
        .iter()
        .inspect(|_| metrics.record_slack_probes(1))
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
        metrics.record_slack_probes(1);
        while let Some((same_deadline, seconds)) = demand_by_deadline.get(index).copied() {
            if same_deadline != deadline {
                break;
            }
            cumulative_demand = cumulative_demand.saturating_add(seconds);
            index += 1;
        }
        result.push((
            deadline,
            available_seconds_until(now, deadline, fixed_slots, metrics)
                .saturating_sub(cumulative_demand),
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

fn critical_deadline(slacks: &[(DateTime<Local>, i64)]) -> Option<DateTime<Local>> {
    slacks
        .iter()
        .find(|(_, slack)| *slack <= 0)
        .map(|(deadline, _)| *deadline)
}

fn ordered_ready_candidates(
    states: &[FlexibleState],
    ready_indices: &[usize],
    critical_deadline: Option<DateTime<Local>>,
) -> Vec<usize> {
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
    ordered
}

/// atomicを実際に中断する候補がreleaseされる最初の時刻を返す。
fn next_preempting_release(
    selected_index: usize,
    selected: &FlexibleState,
    now: DateTime<Local>,
    slack_guard: Option<DateTime<Local>>,
    context: &BoundaryContext<'_>,
    metrics: &mut ScheduleMetrics,
) -> Result<Option<DateTime<Local>>, SchedulingPolicyError> {
    let states = context.states;
    let fixed_slots = context.fixed_slots;
    if let (Some(frontier), Some(slack_index)) = (context.frontier, context.slack_index) {
        let mut additional_normal = BTreeSet::new();
        let mut additional_protected = BTreeSet::new();
        for (release, released_indices) in frontier.release_events.range(now..) {
            if slack_guard.is_some_and(|guard| *release >= guard) {
                // current guardへ先に到達するatomicは、その時点で再選択される。
                // guard後のreleaseを仮想評価すると、既にcriticalへ移るgroupを
                // active treeへ残したまま扱うことになり、存在しない境界を作る。
                return Ok(None);
            }
            for index in released_indices {
                metrics.record_release_candidate_probe();
                if states[*index].remaining_seconds == 0 {
                    continue;
                }
                additional_normal.insert((normal_selection_key(&states[*index]), *index));
                if states[*index].effective_deadline.is_some() {
                    additional_protected.insert((protected_selection_key(&states[*index]), *index));
                }
            }
            let worked = (*release - now)
                .num_seconds()
                .max(0)
                .min(selected.remaining_seconds);
            if worked == selected.remaining_seconds {
                return Ok(None);
            }
            let critical =
                slack_index.critical_deadline_after_work(selected_index, worked, metrics);
            let protected_ready = critical.is_some_and(|critical| {
                frontier
                    .protected_ready
                    .union(&additional_protected)
                    .next()
                    .is_some_and(|((deadline, ..), _)| *deadline <= critical)
            });
            let mut virtual_selected = selected.clone();
            virtual_selected.remaining_seconds -= worked;
            if protected_ready {
                let critical = critical.expect("protected deadline exists");
                for (_, index) in frontier
                    .protected_ready
                    .union(&additional_protected)
                    .take_while(|((deadline, ..), _)| *deadline <= critical)
                {
                    metrics.record_selection_candidate_probe();
                    let candidate = if *index == selected_index {
                        &virtual_selected
                    } else {
                        &states[*index]
                    };
                    let slack_guard = slack_index.slack_boundary_after_work(
                        *index,
                        *release,
                        selected_index,
                        worked,
                        metrics,
                    );
                    if candidate_fits_without_future_release(
                        candidate,
                        *release,
                        fixed_slots,
                        slack_guard,
                    )? {
                        if *index != selected_index {
                            return Ok(Some(*release));
                        }
                        break;
                    }
                }
            } else {
                for (_, index) in frontier.normal_ready.union(&additional_normal) {
                    metrics.record_selection_candidate_probe();
                    let candidate = if *index == selected_index {
                        &virtual_selected
                    } else {
                        &states[*index]
                    };
                    let slack_guard = slack_index.slack_boundary_after_work(
                        *index,
                        *release,
                        selected_index,
                        worked,
                        metrics,
                    );
                    if candidate_fits_without_future_release(
                        candidate,
                        *release,
                        fixed_slots,
                        slack_guard,
                    )? {
                        if *index != selected_index {
                            return Ok(Some(*release));
                        }
                        break;
                    }
                }
            }
        }
        return Ok(None);
    }

    let mut releases = states
        .iter()
        .inspect(|_| metrics.record_release_candidate_probe())
        .filter(|state| state.completion_time.is_none())
        .filter_map(|state| release_time(state, states))
        .filter(|release| *release > now)
        .collect::<Vec<_>>();
    releases.sort_unstable();
    releases.dedup();
    for release in releases {
        if slack_guard.is_some_and(|guard| release >= guard) {
            return Ok(None);
        }
        let mut virtual_states = states.to_vec();
        let elapsed_work_seconds = (release - now).num_seconds().max(0);
        if let Some(running) = virtual_states
            .iter_mut()
            .find(|state| state.candidate.id == selected.candidate.id)
        {
            let worked = elapsed_work_seconds.min(running.remaining_seconds);
            running.remaining_seconds -= worked;
            if running.remaining_seconds == 0 {
                running.completion_time =
                    Some(checked_segment_end(running.candidate.id, now, worked)?);
            }
        }

        // release時点までcurrent atomicが継続した仮想状態で再選択する。
        // 開始時の全残秒のままでは、実際は完走できるatomicを誤って延期する。
        let ready = virtual_states
            .iter()
            .inspect(|_| metrics.record_release_candidate_probe())
            .enumerate()
            .filter(|(_, state)| state.completion_time.is_none() && state.remaining_seconds > 0)
            .filter(|(_, state)| {
                release_time(state, &virtual_states)
                    .is_some_and(|candidate_release| candidate_release <= release)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let slacks = deadline_slacks(&virtual_states, release, fixed_slots, metrics);
        for index in ordered_ready_candidates(&virtual_states, &ready, critical_deadline(&slacks)) {
            metrics.record_selection_candidate_probe();
            if candidate_fits_without_future_release(
                &virtual_states[index],
                release,
                fixed_slots,
                slack_boundary(&virtual_states[index], &slacks, release),
            )? {
                if virtual_states[index].candidate.id != selected.candidate.id {
                    return Ok(Some(release));
                }
                break;
            }
        }
    }
    Ok(None)
}

/// release時点で候補が実際に選択可能かを判定する。さらに未来のreleaseまで
/// 再帰的に追うと選択判定が循環するため、fixedとslackの硬い境界だけを見る。
fn candidate_fits_without_future_release(
    candidate: &FlexibleState,
    now: DateTime<Local>,
    fixed_slots: &[(DateTime<Local>, DateTime<Local>)],
    slack_guard: Option<DateTime<Local>>,
) -> Result<bool, SchedulingPolicyError> {
    if !candidate.candidate.atomic {
        return Ok(true);
    }
    let completion = checked_segment_end(candidate.candidate.id, now, candidate.remaining_seconds)?;
    let hard_boundary = [next_fixed_start(now, fixed_slots), slack_guard]
        .into_iter()
        .flatten()
        .min();
    Ok(hard_boundary.is_none_or(|boundary| completion <= boundary))
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
    selected_index: usize,
    selected: &FlexibleState,
    now: DateTime<Local>,
    slack_guard: Option<DateTime<Local>>,
    context: &BoundaryContext<'_>,
    metrics: &mut ScheduleMetrics,
) -> Result<DateTime<Local>, SchedulingPolicyError> {
    let completion = checked_segment_end(selected.candidate.id, now, selected.remaining_seconds)?;
    let release = if selected.candidate.atomic {
        next_preempting_release(selected_index, selected, now, slack_guard, context, metrics)?
    } else {
        context.next_release
    };
    Ok([
        Some(completion),
        next_fixed_start(now, context.fixed_slots),
        release,
        slack_guard,
    ]
    .into_iter()
    .flatten()
    .min()
    .expect("task completion is always a boundary"))
}

/// atomicは途中eventを跨がず、1segmentで完了できる場合だけ開始する。
fn atomic_fits(
    selected_index: usize,
    selected: &FlexibleState,
    now: DateTime<Local>,
    slack_guard: Option<DateTime<Local>>,
    context: &BoundaryContext<'_>,
    metrics: &mut ScheduleMetrics,
) -> Result<bool, SchedulingPolicyError> {
    if !selected.candidate.atomic {
        return Ok(true);
    }
    let completion = checked_segment_end(selected.candidate.id, now, selected.remaining_seconds)?;
    Ok(completion
        == segment_boundary(selected_index, selected, now, slack_guard, context, metrics)?)
}

/// slack guardで極端に短いfragmentができる候補は、時間を捨ててから
/// 切り替えるのではなく、この選択時点で後順に送る。
fn fits_split_contract(
    selected_index: usize,
    selected: &FlexibleState,
    now: DateTime<Local>,
    slack_guard: Option<DateTime<Local>>,
    context: &BoundaryContext<'_>,
    metrics: &mut ScheduleMetrics,
) -> Result<bool, SchedulingPolicyError> {
    if selected.candidate.atomic {
        return atomic_fits(selected_index, selected, now, slack_guard, context, metrics);
    }
    let Some(guard) = slack_guard else {
        return Ok(true);
    };
    if next_fixed_start(now, context.fixed_slots).is_some_and(|event| event <= guard)
        || context.next_release.is_some_and(|event| event <= guard)
    {
        // guardより前のeventで必ず再選択する。その手前で作れる有用な
        // segmentを、将来のguardの形だけを理由に捨ててはならない。
        return Ok(true);
    }
    let completion = checked_segment_end(selected.candidate.id, now, selected.remaining_seconds)?;
    if guard >= completion {
        return Ok(true);
    }
    let before_guard = (guard - now).num_seconds();
    let after_guard = selected.remaining_seconds.saturating_sub(before_guard);
    Ok(before_guard > MIN_SPLIT_SEGMENT_SECONDS && after_guard > MIN_SPLIT_SEGMENT_SECONDS)
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
    frontier: Option<&SchedulerFrontier>,
    slack_index: Option<&SlackDemandIndex>,
    metrics: &mut ScheduleMetrics,
) -> Result<Option<Selection>, SchedulingPolicyError> {
    metrics.record_selection_event();
    let fallback_slacks = slack_index
        .is_none()
        .then(|| deadline_slacks(states, now, fixed_slots, metrics));
    let critical = match slack_index {
        Some(index) => index.critical_deadline(metrics),
        None => critical_deadline(
            fallback_slacks
                .as_deref()
                .expect("fallback slacks exist without an index"),
        ),
    };
    if let Some(index) = slack_index {
        debug_assert_eq!(index.current_time, now, "slack index time diverged");
    }
    let next_release = frontier
        .and_then(SchedulerFrontier::next_release)
        .or_else(|| {
            frontier
                .is_none()
                .then(|| next_release_event(states, now))
                .flatten()
        });
    let boundary_context = BoundaryContext {
        states,
        fixed_slots,
        next_release,
        frontier,
        slack_index,
    };
    if let Some(frontier) = frontier {
        let protected_ready = critical.is_some_and(|critical| {
            frontier
                .protected_ready
                .first()
                .is_some_and(|((deadline, ..), _)| *deadline <= critical)
        });
        if protected_ready {
            let critical = critical.expect("protected mode requires a critical deadline");
            for ((deadline, ..), index) in &frontier.protected_ready {
                if *deadline > critical {
                    break;
                }
                metrics.record_selection_candidate_probe();
                let slack_guard =
                    slack_index.and_then(|indexer| indexer.slack_boundary(*index, now, metrics));
                if fits_split_contract(
                    *index,
                    &states[*index],
                    now,
                    slack_guard,
                    &boundary_context,
                    metrics,
                )? {
                    return Ok(Some(Selection {
                        index: *index,
                        slack_boundary: slack_guard,
                    }));
                }
            }
        } else {
            for (_, index) in &frontier.normal_ready {
                metrics.record_selection_candidate_probe();
                let slack_guard =
                    slack_index.and_then(|indexer| indexer.slack_boundary(*index, now, metrics));
                if fits_split_contract(
                    *index,
                    &states[*index],
                    now,
                    slack_guard,
                    &boundary_context,
                    metrics,
                )? {
                    return Ok(Some(Selection {
                        index: *index,
                        slack_boundary: slack_guard,
                    }));
                }
            }
        }
    } else {
        for index in ordered_ready_candidates(states, ready_indices, critical) {
            metrics.record_selection_candidate_probe();
            let fallback_slacks = fallback_slacks
                .as_deref()
                .expect("fallback selection has calculated slacks");
            let slack_guard = slack_boundary(&states[index], fallback_slacks, now);
            if fits_split_contract(
                index,
                &states[index],
                now,
                slack_guard,
                &boundary_context,
                metrics,
            )? {
                return Ok(Some(Selection {
                    index,
                    slack_boundary: slack_guard,
                }));
            }
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
                dependency_indices: Vec::new(),
                remaining_seconds,
                completion_time: None,
            }
        })
        .collect::<Vec<_>>();
    let index_by_id = states
        .iter()
        .enumerate()
        .map(|(index, state)| (state.candidate.id, index))
        .collect::<HashMap<_, _>>();
    for state in &mut states {
        state.dependency_indices = state
            .candidate
            .dependency_ids
            .iter()
            .map(|dependency_id| index_by_id.get(dependency_id).copied())
            .collect();
    }
    let fixed_slots = prepared.occupied_fixed;
    let mut scheduled_tasks = prepared.scheduled_fixed;
    let mut now = last_synced_time;
    let mut slack_index = SlackDemandIndex::new(&states, now, &fixed_slots, metrics);
    let mut frontier = SchedulerFrontier::new(&states);

    // Phase 3: eventとeventの間だけを配置し、releaseやslack境界で必ず再選択する。
    while frontier.incomplete_count > 0 {
        frontier.promote_releases(now, &states, metrics);
        if let Some((_, fixed_end)) = fixed_slot_containing(now, &fixed_slots) {
            slack_index.record_fixed_skip(fixed_end);
            now = fixed_end;
            continue;
        }

        // 0秒taskもdependencyのcompletion gateとして意味があるため、選択前に
        // 固定点で完了させる。
        if let Some(index) = frontier.first_zero() {
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
            frontier.complete(index, now, &states, metrics);
            continue;
        }

        let selection = select_next_candidate(
            &states,
            &[],
            now,
            &fixed_slots,
            Some(&frontier),
            Some(&slack_index),
            metrics,
        )?;
        let Some(selection) = selection else {
            let next_release = frontier.next_release();
            let next_fixed = next_fixed_start(now, &fixed_slots);
            if let Some(next_event) = [next_release, next_fixed].into_iter().flatten().min() {
                slack_index.record_idle(now, next_event, metrics);
                now = next_event;
                continue;
            }

            // missing dependencyやcycleでも、上位層が問題を可視化できるscheduleを
            // 返す。UUID順入力に依らない通常選択がfallback順となる。
            let fallback = states
                .iter()
                .enumerate()
                .filter(|(_, state)| {
                    metrics.record_selection_candidate_probe();
                    state.completion_time.is_none()
                })
                .min_by_key(|(_, state)| normal_selection_key(state))
                .map(|(index, _)| index);
            if let Some(index) = fallback {
                let release = states[index].candidate.first_available_time;
                let fallback_start = max(now, release);
                slack_index.record_idle(now, fallback_start, metrics);
                now = fallback_start;
                frontier.force_ready(index, &states[index]);
                if states[index].remaining_seconds == 0 {
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
                    frontier.complete(index, now, &states, metrics);
                    continue;
                }
                let fallback_guard = slack_index.slack_boundary(index, now, metrics);
                schedule_selected_segment(
                    index,
                    &mut states,
                    &mut scheduled_tasks,
                    &mut now,
                    &fixed_slots,
                    fallback_guard,
                    metrics,
                    true,
                    &mut slack_index,
                    &mut frontier,
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
            selection.slack_boundary,
            metrics,
            false,
            &mut slack_index,
            &mut frontier,
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
    slack_guard: Option<DateTime<Local>>,
    metrics: &mut ScheduleMetrics,
    ignore_dependencies: bool,
    slack_index: &mut SlackDemandIndex,
    frontier: &mut SchedulerFrontier,
) -> Result<(), SchedulingPolicyError> {
    let boundary_context = BoundaryContext {
        states,
        fixed_slots,
        next_release: frontier.next_release(),
        frontier: Some(frontier),
        slack_index: Some(slack_index),
    };
    let mut boundary = segment_boundary(
        selected_index,
        &states[selected_index],
        *now,
        slack_guard,
        &boundary_context,
        metrics,
    )?;
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
        let guard_boundary = slack_guard == Some(boundary);
        let mut boundary_frontier = frontier.clone();
        boundary_frontier.promote_releases(boundary, states, metrics);
        let speculative_slack = slack_index.begin_speculative_idle(*now, boundary, metrics);
        let release_preempts = select_next_candidate(
            states,
            &[],
            boundary,
            fixed_slots,
            Some(&boundary_frontier),
            Some(slack_index),
            metrics,
        )?
        .is_some_and(|selection| selection.index != selected_index);
        let release_boundary = frontier.next_release() == Some(boundary);
        let guard_releases_protected_task = guard_boundary && release_boundary && release_preempts;
        if fixed_boundary
            || (!guard_releases_protected_task && (guard_boundary || release_preempts))
        {
            // fixed、deadline guard、より優先されるreleaseは越えない。短い
            // fragmentは作らず、event後に再配置する。
            *now = boundary;
            return Ok(());
        }
        slack_index.restore_speculative_idle(*now, boundary, speculative_slack, metrics);
        if guard_releases_protected_task {
            // 保護taskがguard時刻までreleaseされない場合、それより前に切り替える
            // 選択肢はない。使える時間をidleにするより、境界までの作業を保存する。
        } else {
            // release後も同じtaskが選ばれるなら、見かけだけの数分segmentを
            // 作らず、そのまま完了させる。
            boundary = checked_segment_end(
                states[selected_index].candidate.id,
                *now,
                states[selected_index].remaining_seconds,
            )?;
        }
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
    slack_index.record_work(selected_index, work_seconds, metrics);
    *now = boundary;
    if states[selected_index].remaining_seconds == 0 {
        states[selected_index].completion_time = Some(boundary);
        frontier.complete(selected_index, boundary, states, metrics);
    } else if ignore_dependencies {
        // fallbackは1segment進めることでcycleを解く。残作業は通常event loopへ戻す。
        states[selected_index].candidate.dependency_ids.clear();
        states[selected_index].dependency_indices.clear();
        frontier.force_ready(selected_index, &states[selected_index]);
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
