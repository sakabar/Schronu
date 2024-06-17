use crate::entity::task::Task;
use chrono::{DateTime, Local};
use uuid::Uuid;

pub trait TaskRepositoryTrait {
    fn get_project_storage_dir_name(&self) -> &str;
    fn get_all_projects(&self) -> Vec<&Task>;
    fn load(&mut self);
    fn save(&self);
    fn sync_clock(&mut self, now: DateTime<Local>);
    fn get_last_synced_time(&self) -> DateTime<Local>;
    fn get_highest_priority_project(&mut self) -> Option<&Task>;
    fn get_highest_priority_leaf_task_id(&mut self) -> Option<Uuid>;
    fn get_lowest_priority_leaf_task_id(&mut self) -> Option<Uuid>;
    fn get_by_id(&self, id: Uuid) -> Option<Task>;
    fn start_new_project(&mut self, root_task: Task);
}

pub trait FreeTimeManagerTrait {
    fn get_free_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64;
    fn get_busy_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64;
    fn register_busy_time_slot(&mut self, start: &DateTime<Local>, end: &DateTime<Local>);
    fn load_busy_time_slots_from_file(
        &mut self,
        busy_time_slots_file_path: &str,
        now: &DateTime<Local>,
    );
}
