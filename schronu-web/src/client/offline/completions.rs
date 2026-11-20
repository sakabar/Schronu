use super::{format_jst, OfflineState};
use crate::client::completed::{CompletionId, CompletionRow};
use crate::{OfflineOperation, OfflineOperationKind};

impl OfflineState {
    pub(crate) fn completions_on(&self, date: &str) -> Vec<CompletionRow> {
        let Ok(date) = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
            return Vec::new();
        };
        self.completion_history
            .values()
            .filter(|row| Self::current_logical_date(row.completed_at_epoch_ms) == Some(date))
            .cloned()
            .collect()
    }

    /// Materialize older pending operations before a sync checkpoint can remove them.
    pub(crate) fn restore_completion_history(&mut self) {
        for operation in &self.pending {
            if !self
                .completion_history
                .contains_key(&operation.operation_id)
            {
                if let Some(row) = self.completion_row(operation, None) {
                    self.completion_history
                        .insert(operation.operation_id.clone(), row);
                }
            }
        }
    }

    pub(super) fn completion_row(
        &self,
        operation: &OfflineOperation,
        session_estimate: Option<i64>,
    ) -> Option<CompletionRow> {
        use OfflineOperationKind::*;
        if operation.kind == Record {
            return None;
        }
        let id = &operation.task_id;
        let scheduled =
            self.bundle.days.values().flatten().find(|row| {
                !row.occurrence.is_projected() && row.task.task_id.as_ref() == Some(id)
            });
        let name = self
            .review_task_name(id)
            .or_else(|| scheduled.map(|row| row.task.task_name.as_str()))
            .unwrap_or(id);
        let elapsed = || {
            operation
                .ended_at_epoch_ms
                .checked_sub(operation.started_at_epoch_ms)
                .filter(|ms| *ms >= 0)
                .map(|ms| ms / 1000)
        };
        let kind = scheduled.map(|row| row.task_display_kind);
        let (row_id, task_name, actual, estimate, display_kind) = match operation.kind {
            Split => {
                let seconds = elapsed()?;
                let estimate = operation
                    .expected_estimated_work_seconds
                    .map(|expected| expected.min(seconds).max(1));
                let suffix = format_jst(operation.ended_at_epoch_ms, "%Y%m%d-%H%M");
                (
                    CompletionId::OfflineSplit(operation.operation_id.clone()),
                    format!("{name}-{suffix}"),
                    seconds,
                    estimate,
                    kind.filter(|kind| *kind != crate::TaskDisplayKind::Fixed),
                )
            }
            Complete | CompleteWithoutRecording => {
                let actual = if operation.kind == Complete {
                    operation
                        .expected_actual_work_seconds
                        .checked_add(elapsed()?)?
                } else {
                    operation.expected_actual_work_seconds
                };
                let estimate = session_estimate
                    .or_else(|| scheduled.map(|row| row.task.estimated_work_seconds))
                    .map(|seconds| seconds.max(1));
                (
                    CompletionId::Task(id.clone()),
                    name.to_owned(),
                    actual,
                    estimate,
                    kind,
                )
            }
            Record => return None,
        };
        Some(CompletionRow {
            id: row_id,
            task_name,
            project_name: self.bundle.project_names.get(id).cloned(),
            task_display_kind: display_kind,
            completed_at_epoch_ms: operation.ended_at_epoch_ms,
            actual_work_seconds: if actual == 0 { estimate } else { Some(actual) },
            estimated_work_seconds: estimate,
        })
    }
}
