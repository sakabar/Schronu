//! Diagnostic entrypoints used by the repository's custom scheduling benchmark.

use super::flatten_use_case::{flatten_tasks, FlattenResult};
use super::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use super::pack_use_case::{pack_tasks, PackResult};
use super::schedule_use_case::{get_schedule, ScheduledTaskView};
use super::scheduling_instrumentation::{
    capture_flatten_metrics, capture_pack_metrics, capture_schedule_metrics,
};
pub use super::scheduling_instrumentation::{FlattenMetrics, PackMetrics, ScheduleMetrics};
use super::task_use_case::ApplicationError;

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
    let (result, metrics) =
        capture_flatten_metrics(|| flatten_tasks(repository, free_time_manager));
    Ok((result?, metrics))
}
