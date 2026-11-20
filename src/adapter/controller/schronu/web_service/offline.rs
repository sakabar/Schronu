use super::error::WebReadCoreError;
use super::{
    apply_completion, apply_work_slice, build_scheduled_task_rows,
    build_server_snapshot_from_schedule, load_from_schedule, LoadDataDto, ScheduledTaskRowDto,
    ServerSnapshot, WebReadError, WebService,
};
use crate::adapter::controller::web_session_write::{
    prepare_add_actual_work_input, prepare_complete_task_input, prepare_work_slice_input,
    CompleteSessionRequest, RecordSessionRequest,
};
use crate::adapter::gateway::task_repository::ReceiptStatus;
use crate::adapter::gateway::task_repository::TaskRepository;
use crate::application::interface::TaskRepositoryTrait;
use crate::application::schedule_use_case::get_schedule;
use crate::application::task_use_case::AddActualWorkInput;
use crate::application::task_use_case::{
    add_actual_work, get_task, ApplicationError, CompleteTaskInput, CompleteWorkSliceInput,
};
use crate::entity::task::{Status, TaskTreeError};
use chrono::{DateTime, Local};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub struct OfflineBundleDto {
    pub snapshot: ServerSnapshot,
    pub days: BTreeMap<String, Vec<ScheduledTaskRowDto>>,
    pub load: LoadDataDto,
    pub splittable_task_ids: BTreeSet<String>,
    pub project_names: BTreeMap<String, String>,
    /// Saved parents that can receive a handoff, including their off-screen unfinished children.
    pub handoff_children: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OfflineOperationKind {
    Record,
    Split,
    Complete,
    CompleteWithoutRecording,
}

#[derive(Clone, Debug, Serialize)]
pub struct OfflineOperationDto {
    pub operation_id: Uuid,
    pub task_id: Uuid,
    pub kind: OfflineOperationKind,
    pub started_at_epoch_ms: i64,
    pub ended_at_epoch_ms: i64,
    pub expected_actual_work_seconds: i64,
    pub expected_estimated_work_seconds: Option<i64>,
}

enum SessionWrite {
    Record(AddActualWorkInput),
    Split(CompleteWorkSliceInput, i64),
    Complete(CompleteTaskInput),
}

impl OfflineOperationDto {
    fn prepare(&self, now: DateTime<Local>) -> Result<SessionWrite, WebReadError> {
        if self.operation_id.is_nil() || self.task_id.is_nil() {
            return Err(WebReadError::Application(ApplicationError::InvalidInput {
                field: "offline_operation",
                reason: "operation and task IDs must not be nil",
            }));
        }
        let record = RecordSessionRequest {
            task_id: self.task_id.to_string(),
            started_at_epoch_ms: self.started_at_epoch_ms,
            ended_at_epoch_ms: Some(self.ended_at_epoch_ms),
            expected_actual_work_seconds: self.expected_actual_work_seconds,
        };
        match (self.kind, self.expected_estimated_work_seconds) {
            (OfflineOperationKind::Record, None) => {
                prepare_add_actual_work_input(record, now).map(SessionWrite::Record)
            }
            (OfflineOperationKind::Split, Some(estimate)) if estimate >= 0 => {
                prepare_work_slice_input(record, now)
                    .map(|input| SessionWrite::Split(input, estimate))
            }
            (
                OfflineOperationKind::Complete | OfflineOperationKind::CompleteWithoutRecording,
                None,
            ) => prepare_complete_task_input(
                CompleteSessionRequest {
                    task_id: record.task_id,
                    started_at_epoch_ms: record.started_at_epoch_ms,
                    ended_at_epoch_ms: record.ended_at_epoch_ms,
                    expected_actual_work_seconds: record.expected_actual_work_seconds,
                    record_elapsed_seconds: self.kind == OfflineOperationKind::Complete,
                },
                now,
            )
            .map(SessionWrite::Complete),
            _ => {
                return Err(WebReadError::Application(ApplicationError::InvalidInput {
                    field: "expected_estimated_work_seconds",
                    reason: "a nonnegative expected estimate is required only for a split",
                }))
            }
        }
        .map_err(WebReadError::InvalidInput)
    }
}

impl WebService {
    pub fn prepare_offline_at(
        &mut self,
        now: DateTime<Local>,
    ) -> Result<OfflineBundleDto, WebReadError> {
        self.run_at(now, |repository, free_time_manager, offset| {
            // A single schedule under the read lock backs both horizons and every date.
            let schedule = get_schedule(repository).map_err(WebReadCoreError::Application)?;
            let load = load_from_schedule(repository, free_time_manager, &schedule, now, offset)?;
            let snapshot = build_server_snapshot_from_schedule(
                repository,
                free_time_manager,
                now,
                &schedule,
                offset,
            )?;
            let mut days = BTreeMap::new();
            for day in load.band_days.iter().take(7) {
                let rows = build_scheduled_task_rows(repository, &schedule, day.logical_date, now)?;
                days.insert(day.logical_date.to_string(), rows);
            }
            let mut bundle = OfflineBundleDto {
                snapshot,
                days,
                load,
                splittable_task_ids: BTreeSet::new(),
                project_names: BTreeMap::new(),
                handoff_children: BTreeMap::new(),
            };
            bundle.collect_task_metadata(repository).map_err(|error| {
                WebReadCoreError::Application(ApplicationError::TaskTree(error))
            })?;
            Ok(bundle)
        })
    }

    pub fn sync_offline_operation_at(
        &mut self,
        now: DateTime<Local>,
        request: OfflineOperationDto,
    ) -> Result<(), WebReadError> {
        let write = request.prepare(now)?;
        self.run_mutation_at(now, |repository, _, _| {
            match repository
                .stage_offline_receipt(request.operation_id, &request)
                .map_err(WebReadCoreError::ReceiptStorage)?
            {
                ReceiptStatus::AlreadyApplied => return Ok(((), false)),
                ReceiptStatus::PayloadConflict => return Err(WebReadCoreError::OfflineConflict),
                ReceiptStatus::New => {}
            }
            match write {
                SessionWrite::Record(input) => add_actual_work(repository, input).map(|_| ()),
                SessionWrite::Split(input, expected_estimate) => {
                    let task = get_task(repository, input.task_id)
                        .map_err(WebReadCoreError::Application)?
                        .ok_or(WebReadCoreError::Application(
                            ApplicationError::TaskNotFound(input.task_id),
                        ))?;
                    if task.estimated_work_seconds != expected_estimate {
                        return Err(WebReadCoreError::OfflineConflict);
                    }
                    apply_work_slice(repository, input, now)
                }
                SessionWrite::Complete(input) => {
                    let exclude_from_duration_statistics =
                        request.kind == OfflineOperationKind::CompleteWithoutRecording;
                    apply_completion(repository, input, exclude_from_duration_statistics, now)
                        .map(|_| ())
                }
            }
            .map_err(WebReadCoreError::Application)?;
            Ok(((), true))
        })
    }
}

impl OfflineBundleDto {
    fn collect_task_metadata(&mut self, repository: &TaskRepository) -> Result<(), TaskTreeError> {
        let saved_ids: BTreeSet<_> = self
            .days
            .values()
            .flatten()
            .filter_map(|row| row.task.task_id.as_deref())
            .collect();
        // Already running browser sessions may be outside the downloaded dates.
        let mut pending: Vec<_> = repository.get_all_projects().into_iter().cloned().collect();
        while let Some(task) = pending.pop() {
            let children = task.get_children()?;
            let id = task.get_id()?.to_string();
            if saved_ids.contains(id.as_str()) {
                self.project_names
                    .insert(id.clone(), task.root()?.get_name()?);
            }
            // Completing a child of a repetition template creates its next occurrence.
            if task.get_status()? != Status::Done && !task.is_repeating_task()? {
                if saved_ids.contains(id.as_str()) {
                    let mut unfinished = BTreeSet::new();
                    for child in &children {
                        if child.get_status()? != Status::Done {
                            unfinished.insert(child.get_id()?.to_string());
                        }
                    }
                    if !unfinished.is_empty() {
                        self.handoff_children.insert(id.clone(), unfinished);
                    }
                }
                self.splittable_task_ids.insert(id);
            }
            pending.extend(children);
        }
        Ok(())
    }
}
