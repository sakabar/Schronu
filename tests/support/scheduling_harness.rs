use chrono::{DateTime, Local};
use schronu::application::interface::{
    BusyTimeSlotLoadError, BusyTimeSlotRegistrationError, FreeTimeManagerTrait,
    ProjectRegistrationError, TaskRepositoryError, TaskRepositoryTrait,
};
#[cfg(test)]
use schronu::entity::task::Status;
use schronu::entity::task::{TaskHandle, TaskTreeError};
use std::collections::HashMap;
use uuid::Uuid;

pub struct SchedulingRepository {
    projects: Vec<TaskHandle>,
    now: DateTime<Local>,
    tasks_by_id: HashMap<Uuid, TaskHandle>,
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub struct TaskState {
    pub id: Uuid,
    pub status: Status,
    pub pending_until: DateTime<Local>,
}

impl SchedulingRepository {
    pub fn new(projects: Vec<TaskHandle>, now: DateTime<Local>) -> Self {
        let mut tasks_by_id = HashMap::new();
        for project in &projects {
            index_task(project, &mut tasks_by_id)
                .expect("synthetic scheduling fixture contains a valid task tree");
        }
        Self {
            projects,
            now,
            tasks_by_id,
        }
    }

    #[cfg(test)]
    pub fn task_states(&self) -> Result<Vec<TaskState>, TaskTreeError> {
        let mut states = self
            .tasks_by_id
            .values()
            .map(|task| {
                Ok(TaskState {
                    id: task.get_id()?,
                    status: task.get_orig_status()?,
                    pending_until: task.get_pending_until()?,
                })
            })
            .collect::<Result<Vec<_>, TaskTreeError>>()?;
        states.sort_by_key(|state| state.id);
        Ok(states)
    }
}

fn index_task(
    task: &TaskHandle,
    tasks_by_id: &mut HashMap<Uuid, TaskHandle>,
) -> Result<(), TaskTreeError> {
    tasks_by_id.insert(task.get_id()?, task.clone());
    for child in task.get_children()? {
        index_task(&child, tasks_by_id)?;
    }
    Ok(())
}

impl TaskRepositoryTrait for SchedulingRepository {
    fn get_project_storage_dir_name(&self) -> &str {
        "synthetic-scheduling-fixture"
    }

    fn get_all_projects(&self) -> Vec<&TaskHandle> {
        self.projects.iter().collect()
    }

    fn load(&mut self) -> Result<(), TaskRepositoryError> {
        Ok(())
    }

    fn save(&self) -> Result<(), TaskRepositoryError> {
        Ok(())
    }

    fn sync_clock(&mut self, now: DateTime<Local>) -> Result<(), TaskTreeError> {
        self.now = now;
        for project in &self.projects {
            project.sync_clock(now)?;
        }
        Ok(())
    }

    fn get_last_synced_time(&self) -> DateTime<Local> {
        self.now
    }

    fn get_highest_priority_project(&mut self) -> Option<&TaskHandle> {
        self.projects.first()
    }

    fn get_highest_priority_leaf_task_id(
        &mut self,
        _excluded_task_ids: &[Uuid],
    ) -> Result<Option<Uuid>, TaskTreeError> {
        Ok(None)
    }

    fn get_defer_candidate_leaf_task_id(
        &mut self,
        _recent_threshold: DateTime<Local>,
        _excluded_task_ids: &[Uuid],
    ) -> Result<Option<Uuid>, TaskTreeError> {
        Ok(None)
    }

    fn get_by_id(&self, id: Uuid) -> Result<Option<TaskHandle>, TaskTreeError> {
        Ok(self.tasks_by_id.get(&id).cloned())
    }

    fn start_new_project(&mut self, root_task: TaskHandle) -> Result<(), ProjectRegistrationError> {
        index_task(&root_task, &mut self.tasks_by_id)
            .map_err(ProjectRegistrationError::TaskTree)?;
        self.projects.push(root_task);
        Ok(())
    }
}

pub struct SchedulingFreeTimeManager {
    daily_free_minutes: i64,
    continuous_time_is_free: bool,
}

impl SchedulingFreeTimeManager {
    pub fn new(daily_free_minutes: i64) -> Self {
        Self {
            daily_free_minutes,
            continuous_time_is_free: true,
        }
    }

    #[cfg(test)]
    pub fn without_continuous_free_time(daily_free_minutes: i64) -> Self {
        Self {
            daily_free_minutes,
            continuous_time_is_free: false,
        }
    }
}

impl FreeTimeManagerTrait for SchedulingFreeTimeManager {
    fn get_free_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
        let duration_minutes = (*end - *start).num_minutes().max(0);
        if duration_minutes >= 12 * 60 {
            self.daily_free_minutes
        } else if self.continuous_time_is_free {
            duration_minutes
        } else {
            0
        }
    }

    fn get_busy_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
        (*end - *start).num_minutes() - self.get_free_minutes(start, end)
    }

    fn register_busy_time_slot(
        &mut self,
        _start: &DateTime<Local>,
        _end: &DateTime<Local>,
    ) -> Result<(), BusyTimeSlotRegistrationError> {
        Ok(())
    }

    fn load_busy_time_slots_from_file(
        &mut self,
        _busy_time_slots_file_path: &str,
    ) -> Result<(), BusyTimeSlotLoadError> {
        Ok(())
    }
}
