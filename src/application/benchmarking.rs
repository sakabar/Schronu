//! Diagnostic entrypoints used by the repository's custom scheduling benchmark.

use super::flatten_use_case::{flatten_tasks_with_metrics, FlattenResult};
use super::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use super::pack_use_case::{pack_tasks_with_metrics, PackResult};
use super::schedule_use_case::{get_schedule_with_metrics, ScheduledTaskView};
use super::scheduling_metrics::ScheduleMetrics as InternalScheduleMetrics;
use super::scheduling_metrics::{
    FlattenMetrics as InternalFlattenMetrics, PackMetrics as InternalPackMetrics,
};
use super::task_use_case::ApplicationError;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScheduleMetrics {
    pub candidate_count: usize,
    pub segment_count: usize,
    pub occupied_slot_probe_count: usize,
    pub dependency_candidate_probe_count: usize,
    pub sort_count: usize,
    pub schedule_rebuild_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackMetrics {
    pub schedule: ScheduleMetrics,
    pub candidate_count: usize,
    pub placement_trial_count: usize,
    pub cursor_minute_advance_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FlattenMetrics {
    pub schedule: ScheduleMetrics,
    pub overload_iteration_count: usize,
    pub candidate_trial_count: usize,
    pub override_clone_element_count: usize,
    pub full_schedule_scan_element_count: usize,
}

pub fn get_schedule_diagnostics(
    repository: &dyn TaskRepositoryTrait,
) -> Result<(Vec<ScheduledTaskView>, ScheduleMetrics), ApplicationError> {
    let mut metrics = InternalScheduleMetrics::default();
    let result = get_schedule_with_metrics(repository, &mut metrics)?;
    Ok((result, metrics.into()))
}

pub fn pack_tasks_diagnostics(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<(PackResult, PackMetrics), ApplicationError> {
    let mut metrics = InternalPackMetrics::default();
    let result = pack_tasks_with_metrics(repository, free_time_manager, &mut metrics)?;
    Ok((result, metrics.into()))
}

pub fn flatten_tasks_diagnostics(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<(FlattenResult, FlattenMetrics), ApplicationError> {
    let mut metrics = InternalFlattenMetrics::default();
    let result = flatten_tasks_with_metrics(repository, free_time_manager, &mut metrics)?;
    Ok((result, metrics.into()))
}

impl From<InternalScheduleMetrics> for ScheduleMetrics {
    fn from(metrics: InternalScheduleMetrics) -> Self {
        Self {
            candidate_count: metrics.candidate_count,
            segment_count: metrics.segment_count,
            occupied_slot_probe_count: metrics.occupied_slot_probe_count,
            dependency_candidate_probe_count: metrics.dependency_candidate_probe_count,
            sort_count: metrics.sort_count,
            schedule_rebuild_count: metrics.schedule_rebuild_count,
        }
    }
}

impl From<InternalPackMetrics> for PackMetrics {
    fn from(metrics: InternalPackMetrics) -> Self {
        Self {
            schedule: metrics.schedule.into(),
            candidate_count: metrics.candidate_count,
            placement_trial_count: metrics.placement_trial_count,
            cursor_minute_advance_count: metrics.cursor_minute_advance_count,
        }
    }
}

impl From<InternalFlattenMetrics> for FlattenMetrics {
    fn from(metrics: InternalFlattenMetrics) -> Self {
        Self {
            schedule: metrics.schedule.into(),
            overload_iteration_count: metrics.overload_iteration_count,
            candidate_trial_count: metrics.candidate_trial_count,
            override_clone_element_count: metrics.override_clone_element_count,
            full_schedule_scan_element_count: metrics.full_schedule_scan_element_count,
        }
    }
}
