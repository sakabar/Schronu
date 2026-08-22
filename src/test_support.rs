use crate::application::interface::{
    BusyTimeSlotLoadError, BusyTimeSlotRegistrationError, FreeTimeManagerTrait,
    TaskRepositoryError, TaskRepositoryTrait,
};
use crate::entity::task::{TaskAttr, TaskHandle, TaskTreeError};
use chrono::{DateTime, Local, TimeZone};
use std::cell::Cell;
use uuid::Uuid;

fn next_task_id() -> Uuid {
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    Uuid::from_u128(u128::from(SEQUENCE.fetch_add(1, Ordering::Relaxed)))
}

fn task_time() -> DateTime<Local> {
    Local.with_ymd_and_hms(2100, 1, 1, 0, 0, 0).unwrap()
}

pub(crate) fn new_task_attr(name: &str) -> TaskAttr {
    new_task_attr_at(name, task_time())
}

pub(crate) fn new_task_attr_at(name: &str, now: DateTime<Local>) -> TaskAttr {
    TaskAttr::with_identity(name, next_task_id(), now)
}

pub(crate) fn new_task_handle(name: &str) -> Result<TaskHandle, TaskTreeError> {
    new_task_handle_at(name, task_time())
}

pub(crate) fn new_task_handle_at(
    name: &str,
    now: DateTime<Local>,
) -> Result<TaskHandle, TaskTreeError> {
    TaskHandle::with_identity(name, next_task_id(), now)
}

pub(crate) struct TestTaskRepository {
    projects: Vec<TaskHandle>,
    now: DateTime<Local>,
    highest_priority_leaf_task_id: Option<Uuid>,
    save_count: Cell<usize>,
}

impl TestTaskRepository {
    pub(crate) fn new(projects: Vec<TaskHandle>, now: DateTime<Local>) -> Self {
        Self {
            projects,
            now,
            highest_priority_leaf_task_id: None,
            save_count: Cell::new(0),
        }
    }

    pub(crate) fn projects(&self) -> &[TaskHandle] {
        &self.projects
    }

    pub(crate) fn save_count(&self) -> usize {
        self.save_count.get()
    }

    pub(crate) fn set_highest_priority_leaf_task_id(&mut self, task_id: Option<Uuid>) {
        self.highest_priority_leaf_task_id = task_id;
    }

    pub(crate) fn highest_priority_leaf_task_id(&self) -> Option<Uuid> {
        self.highest_priority_leaf_task_id
    }
}

impl TaskRepositoryTrait for TestTaskRepository {
    fn get_project_storage_dir_name(&self) -> &str {
        "unused"
    }

    fn get_all_projects(&self) -> Vec<&TaskHandle> {
        self.projects.iter().collect()
    }

    fn load(&mut self) -> Result<(), TaskRepositoryError> {
        Ok(())
    }

    fn save(&self) -> Result<(), TaskRepositoryError> {
        self.save_count.set(self.save_count.get() + 1);
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

    fn get_highest_priority_leaf_task_id(&mut self) -> Result<Option<Uuid>, TaskTreeError> {
        Ok(self.highest_priority_leaf_task_id)
    }

    fn get_defer_candidate_leaf_task_id(
        &mut self,
        _recent_threshold: DateTime<Local>,
    ) -> Result<Option<Uuid>, TaskTreeError> {
        Ok(None)
    }

    fn get_by_id(&self, id: Uuid) -> Result<Option<TaskHandle>, TaskTreeError> {
        for task in &self.projects {
            if let Some(found) = task.get_by_id(id)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    fn start_new_project(&mut self, root_task: TaskHandle) -> Result<(), TaskTreeError> {
        self.projects.push(root_task);
        Ok(())
    }
}

pub(crate) struct TestFreeTimeManager {
    daily_free_minutes: i64,
    blocked_interval: Option<(DateTime<Local>, DateTime<Local>)>,
}

impl TestFreeTimeManager {
    pub(crate) fn new(daily_free_minutes: i64) -> Self {
        Self {
            daily_free_minutes,
            blocked_interval: None,
        }
    }

    pub(crate) fn with_blocked_interval(
        daily_free_minutes: i64,
        start: DateTime<Local>,
        end: DateTime<Local>,
    ) -> Self {
        Self {
            daily_free_minutes,
            blocked_interval: Some((start, end)),
        }
    }
}

impl FreeTimeManagerTrait for TestFreeTimeManager {
    fn get_free_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
        let duration_minutes = (*end - *start).num_minutes().max(0);
        if duration_minutes >= 12 * 60 {
            return self.daily_free_minutes;
        }

        let blocked_minutes = self
            .blocked_interval
            .map(|(blocked_start, blocked_end)| {
                let overlap_start = (*start).max(blocked_start);
                let overlap_end = (*end).min(blocked_end);
                (overlap_end - overlap_start).num_minutes().max(0)
            })
            .unwrap_or(0);
        duration_minutes - blocked_minutes
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
