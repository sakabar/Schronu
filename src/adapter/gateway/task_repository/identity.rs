use crate::entity::task::{TaskHandle, TaskTreeError};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Eq, PartialEq)]
pub struct NilTaskIdError {
    project_yaml_file_path: PathBuf,
    task_path: String,
}

impl NilTaskIdError {
    pub(super) fn new(project_yaml_file_path: PathBuf, task_path: String) -> Self {
        Self {
            project_yaml_file_path,
            task_path,
        }
    }

    pub fn project_yaml_file_path(&self) -> &Path {
        &self.project_yaml_file_path
    }

    pub fn task_path(&self) -> &str {
        &self.task_path
    }
}

impl fmt::Display for NilTaskIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "nil task ID at {}:{}",
            self.project_yaml_file_path.display(),
            self.task_path
        )
    }
}

impl Error for NilTaskIdError {}

pub(super) struct TaskIdentityEntry {
    pub(super) task_id: Uuid,
    pub(super) task_path: String,
    pub(super) task: TaskHandle,
}

pub(super) fn task_identity_entries(
    root_task: &TaskHandle,
) -> Result<Vec<TaskIdentityEntry>, TaskTreeError> {
    fn collect(
        task: &TaskHandle,
        task_path: String,
        entries: &mut Vec<TaskIdentityEntry>,
    ) -> Result<(), TaskTreeError> {
        entries.push(TaskIdentityEntry {
            task_id: task.get_id()?,
            task_path: task_path.clone(),
            task: task.clone(),
        });
        for (index, child) in task.get_children()?.into_iter().enumerate() {
            collect(&child, format!("{task_path}.children[{index}]"), entries)?;
        }
        Ok(())
    }

    let mut entries = Vec::new();
    collect(root_task, "project".to_string(), &mut entries)?;
    Ok(entries)
}
