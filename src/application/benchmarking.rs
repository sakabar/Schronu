//! Diagnostic entrypoints used by the repository's custom scheduling benchmark.

use super::flatten_use_case::{flatten_tasks_with_metrics, FlattenResult};
use super::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use super::pack_use_case::{pack_tasks, PackResult};
use super::schedule_use_case::{get_schedule, ScheduledTaskView};
use super::scheduling_instrumentation::{capture_pack_metrics, capture_schedule_metrics};
pub use super::scheduling_instrumentation::{PackMetrics, ScheduleMetrics};
use super::scheduling_metrics::FlattenMetrics as InternalFlattenMetrics;
use super::task_use_case::ApplicationError;

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
    let (result, metrics) = capture_schedule_metrics(|| get_schedule(repository));
    Ok((result?, metrics))
}

pub fn pack_tasks_diagnostics(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<(PackResult, PackMetrics), ApplicationError> {
    let (result, metrics) = capture_pack_metrics(|| pack_tasks(repository, free_time_manager));
    Ok((result?, metrics))
}

pub fn flatten_tasks_diagnostics(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<(FlattenResult, FlattenMetrics), ApplicationError> {
    let mut metrics = InternalFlattenMetrics::default();
    let (result, schedule_metrics) = capture_schedule_metrics(|| {
        flatten_tasks_with_metrics(repository, free_time_manager, &mut metrics)
    });
    let mut metrics = FlattenMetrics::from(metrics);
    metrics.schedule = schedule_metrics;
    Ok((result?, metrics))
}

impl From<InternalFlattenMetrics> for FlattenMetrics {
    fn from(metrics: InternalFlattenMetrics) -> Self {
        Self {
            schedule: ScheduleMetrics::default(),
            overload_iteration_count: metrics.overload_iteration_count,
            candidate_trial_count: metrics.candidate_trial_count,
            override_clone_element_count: metrics.override_clone_element_count,
            full_schedule_scan_element_count: metrics.full_schedule_scan_element_count,
        }
    }
}
