use crate::entity::task::{TaskAttr, TaskHandle, TaskTreeError};
use chrono::Local;
use uuid::Uuid;

pub(crate) fn new_task_attr(name: &str) -> TaskAttr {
    TaskAttr::with_identity(name, Uuid::new_v4(), Local::now())
}

pub(crate) fn new_task_handle(name: &str) -> Result<TaskHandle, TaskTreeError> {
    TaskHandle::with_identity(name, Uuid::new_v4(), Local::now())
}
