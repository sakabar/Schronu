use crate::entity::task::Task;
use chrono::{DateTime, Local};
use uuid::Uuid;

pub trait TaskRepositoryTrait {
    fn get_all_projects(&self) -> Vec<&Task>;
    fn load(&mut self);
    fn save(&self);
    fn sync_clock(&mut self, now: DateTime<Local>);
    fn get_highest_priority_project(&mut self) -> Option<&Task>;
    fn get_by_id(&self, id: Uuid) -> Option<Task>;
}