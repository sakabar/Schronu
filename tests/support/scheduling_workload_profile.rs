use crate::scheduling_fixture::{summarize_projects, FixtureSummary};
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;
use std::path::Path;

pub fn summarize_storage(storage: &Path) -> Result<FixtureSummary, String> {
    let storage = storage.to_path_buf();
    std::thread::Builder::new()
        .name("scheduling-workload-profile".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || summarize_storage_on_large_stack(&storage))
        .map_err(|error| format!("workload profiler thread creation failed: {error}"))?
        .join()
        .map_err(|_| "workload profiler thread panicked".to_string())?
}

fn summarize_storage_on_large_stack(storage: &Path) -> Result<FixtureSummary, String> {
    let storage = storage
        .to_str()
        .ok_or_else(|| "task storage path must be valid UTF-8".to_string())?;
    let mut repository = TaskRepository::new(storage);
    repository
        .load()
        .map_err(|error| format!("task storage load failed: {error}"))?;
    let projects = repository
        .get_all_projects()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    summarize_projects(&projects).map_err(|error| format!("task tree summary failed: {error}"))
}
