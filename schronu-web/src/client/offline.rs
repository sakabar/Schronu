use super::local_time::{local_datetime, logical_date_with_offset, JAPAN_UTC_OFFSET_MINUTES};
use super::time_model::{
    buffer_timing_with_sessions, session_credit_start_epoch_ms, session_timing,
};
use super::work_sessions::WorkSession;
use crate::{OfflineBundle, OfflineOperation, OfflineOperationKind, ScheduledTaskRow};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A single IndexedDB value. Queue, projections and timer changes are committed together.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct OfflineState {
    pub bundle: OfflineBundle,
    pub sessions: Vec<WorkSession>,
    pub pending: Vec<OfflineOperation>,
    pub conflicts: BTreeMap<String, String>,
    recorded_task_names: BTreeMap<String, String>,
    completed: BTreeSet<String>,
    awaiting_refresh: BTreeSet<String>,
    /// The frozen server snapshot does not yet include these locally finished intervals.
    finished_credits: BTreeMap<String, i64>,
}

impl OfflineState {
    pub fn new(bundle: OfflineBundle, sessions: Vec<WorkSession>) -> Self {
        Self {
            bundle,
            sessions,
            pending: Vec::new(),
            conflicts: BTreeMap::new(),
            recorded_task_names: BTreeMap::new(),
            completed: BTreeSet::new(),
            awaiting_refresh: BTreeSet::new(),
            finished_credits: BTreeMap::new(),
        }
    }

    pub fn completed(&self, id: &str) -> bool {
        self.completed.contains(id)
    }

    pub fn recorded_task_name(&self, id: &str) -> Option<&str> {
        self.recorded_task_names.get(id).map(String::as_str)
    }

    pub fn awaiting_refresh(&self, id: &str) -> bool {
        self.awaiting_refresh.contains(id)
    }

    pub fn can_start(&self, row: &ScheduledTaskRow) -> bool {
        row.is_leaf
            && !row.occurrence.is_projected()
            && row.task.task_id.as_deref().is_some_and(|id| {
                !self.completed(id)
                    && !self.awaiting_refresh(id)
                    && !self.conflicts.contains_key(id)
                    && !self.sessions.iter().any(|active| active.task_id == id)
            })
    }

    pub fn start(&mut self, id: &str, now: i64) -> Result<(), &'static str> {
        let task = self
            .bundle
            .days
            .values()
            .flatten()
            .find(|row| row.task.task_id.as_deref() == Some(id) && self.can_start(row))
            .and_then(|row| row.task.actionable_task())
            .ok_or("このタスクを開始できません。")?;
        self.sessions.push(WorkSession::from_task(&task, now));
        Ok(())
    }

    pub fn finish(
        &mut self,
        id: &str,
        kind: OfflineOperationKind,
        now: i64,
        operation_id: String,
    ) -> Result<(), &'static str> {
        if self.conflicts.contains_key(id) || self.awaiting_refresh(id) {
            return Err("同期結果を確認してください。");
        }
        let index = self
            .sessions
            .iter()
            .position(|session| session.task_id == id)
            .ok_or("対象のセッションが見つかりません。")?;
        let session = &self.sessions[index];
        let elapsed_seconds = || {
            now.checked_sub(session.started_at_epoch_ms)
                .filter(|elapsed| *elapsed >= 0)
                .map(|elapsed| elapsed / 1000)
                .ok_or("開始時刻より後に操作してください。")
        };
        let actual = session.actual_work_seconds_at_start;
        let estimate = session.estimated_work_seconds_at_start;
        let (new_actual, new_estimate) = match kind {
            OfflineOperationKind::Split => {
                let seconds = elapsed_seconds()?;
                if !self.bundle.splittable_task_ids.contains(id) || seconds == 0 {
                    return Err("このセッションは分割できません。");
                }
                (actual, estimate - seconds.min(estimate))
            }
            OfflineOperationKind::Record | OfflineOperationKind::Complete => (
                actual
                    .checked_add(elapsed_seconds()?)
                    .ok_or("実績の上限を超えました。")?,
                estimate,
            ),
            OfflineOperationKind::CompleteWithoutRecording => (actual, estimate),
        };
        if kind != OfflineOperationKind::CompleteWithoutRecording {
            let (credit_start, credit_end) = protected_interval(session, now);
            let credit = buffer_timing_with_sessions(now, 0, now, &[(credit_start, credit_end)])
                .display_buffer_seconds;
            self.finished_credits.insert(
                operation_id.clone(),
                i64::try_from(credit).map_err(|_| "実績の上限を超えました。")?,
            );
        }
        self.recorded_task_names
            .insert(id.to_owned(), session.task_name.clone());
        self.pending.push(OfflineOperation {
            operation_id,
            task_id: id.to_owned(),
            kind,
            started_at_epoch_ms: session.started_at_epoch_ms,
            ended_at_epoch_ms: now,
            expected_actual_work_seconds: actual,
            expected_estimated_work_seconds: (kind == OfflineOperationKind::Split)
                .then_some(estimate),
        });
        self.sessions.remove(index);
        if matches!(
            kind,
            OfflineOperationKind::Complete | OfflineOperationKind::CompleteWithoutRecording
        ) {
            self.completed.insert(id.to_owned());
        }
        for row in
            self.bundle.days.values_mut().flatten().filter(|row| {
                !row.occurrence.is_projected() && row.task.task_id.as_deref() == Some(id)
            })
        {
            row.task.actual_work_seconds = new_actual;
            row.task.estimated_work_seconds = new_estimate;
        }
        Ok(())
    }

    pub fn restart(&mut self, id: &str, now: i64) -> Result<(), &'static str> {
        let session = self
            .sessions
            .iter_mut()
            .find(|s| s.task_id == id)
            .ok_or("対象のセッションが見つかりません。")?;
        session.started_at_epoch_ms = now;
        session.buffer_credit_start_epoch_ms = None;
        Ok(())
    }

    pub fn release(&mut self, id: &str) {
        self.sessions.retain(|s| s.task_id != id);
    }

    pub fn discard_conflict(&mut self, id: &str) {
        if self.conflicts.remove(id).is_none() {
            return;
        }
        self.pending.retain(|op| {
            if op.task_id == id {
                self.finished_credits.remove(&op.operation_id);
                false
            } else {
                true
            }
        });
        // Projected actual/estimate can no longer seed fresh work until the next server read.
        self.completed.remove(id);
        self.recorded_task_names.remove(id);
        self.awaiting_refresh.insert(id.to_owned());
    }

    /// Rebase unrelated tasks to the fresh read, keeping each unsent task at its local projection.
    pub fn rebase(&mut self, mut fresh: OfflineBundle) {
        let pending_ids: BTreeSet<_> = self.pending.iter().map(|op| op.task_id.as_str()).collect();
        let projected: BTreeMap<_, _> = self
            .bundle
            .days
            .values()
            .flatten()
            .filter(|row| !row.occurrence.is_projected())
            .filter_map(|row| {
                let id = row.task.task_id.as_deref()?;
                pending_ids.contains(id).then_some((
                    id,
                    (
                        row.task.actual_work_seconds,
                        row.task.estimated_work_seconds,
                    ),
                ))
            })
            .collect();
        for row in fresh
            .days
            .values_mut()
            .flatten()
            .filter(|r| !r.occurrence.is_projected())
        {
            if let Some((actual, estimate)) =
                row.task.task_id.as_deref().and_then(|id| projected.get(id))
            {
                row.task.actual_work_seconds = *actual;
                row.task.estimated_work_seconds = *estimate;
            }
        }
        self.recorded_task_names
            .retain(|id, _| pending_ids.contains(id.as_str()));
        self.completed
            .retain(|id| pending_ids.contains(id.as_str()));
        self.awaiting_refresh.clear();
        let outstanding: BTreeSet<_> = self
            .pending
            .iter()
            .map(|op| op.operation_id.as_str())
            .collect();
        self.finished_credits
            .retain(|id, _| outstanding.contains(id.as_str()));
        self.bundle = fresh;
    }

    pub fn display_buffer_seconds(&self, now: i64) -> i128 {
        let snapshot = &self.bundle.snapshot;
        let intervals = self
            .sessions
            .iter()
            .map(|session| protected_interval(session, now))
            .collect::<Vec<_>>();
        buffer_timing_with_sessions(
            snapshot.observed_at_epoch_ms,
            snapshot.buffer_seconds,
            now,
            &intervals,
        )
        .display_buffer_seconds
            + self
                .finished_credits
                .values()
                .map(|&n| i128::from(n))
                .sum::<i128>()
    }

    pub fn current_logical_date(now: i64) -> Option<chrono::NaiveDate> {
        logical_date_with_offset(now, &|_| Some(JAPAN_UTC_OFFSET_MINUTES))
    }

    pub fn prior_day_estimate(&self, now: i64) -> bool {
        Self::current_logical_date(now)
            .is_some_and(|date| date.to_string() != self.bundle.snapshot.logical_date)
    }
}

fn protected_interval(session: &WorkSession, now: i64) -> (i64, Option<i64>) {
    let start = session_credit_start_epoch_ms(
        session.started_at_epoch_ms,
        session.start_time_epoch_ms,
        session.pending_until_epoch_ms,
        now,
        &|_| Some(JAPAN_UTC_OFFSET_MINUTES),
    )
    .max(
        session
            .buffer_credit_start_epoch_ms
            .unwrap_or(session.started_at_epoch_ms),
    );
    let timing = session_timing(
        session.started_at_epoch_ms,
        session.estimated_work_seconds_at_start,
        session.actual_work_seconds_at_start,
        now,
    );
    (start, timing.estimated_completion_epoch_ms)
}

pub fn format_jst(epoch: i64, format: &str) -> String {
    format_at_offset(epoch, Some(JAPAN_UTC_OFFSET_MINUTES), format)
}

pub fn format_at_offset(epoch: i64, offset: Option<i32>, format: &str) -> String {
    offset
        .and_then(|minutes| local_datetime(epoch, minutes))
        .map(|dt| dt.format(format).to_string())
        .unwrap_or_else(|| "—".to_owned())
}

#[cfg(test)]
mod tests;
