use crate::entity::task::{TaskAttr, TaskHandle, TaskTreeError};
use chrono::{DateTime, Local, TimeZone};
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
    TaskAttr::with_identity(name, next_task_id(), task_time())
}

pub(crate) fn new_task_handle(name: &str) -> Result<TaskHandle, TaskTreeError> {
    TaskHandle::with_identity(name, next_task_id(), task_time())
}
