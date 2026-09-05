use super::*;

pub(super) struct RepositoryLoadBuilder {
    last_synced_time: DateTime<Local>,
    projects: Vec<Project>,
    canonical_project_paths: HashMap<PathBuf, PathBuf>,
}

impl RepositoryLoadBuilder {
    pub(super) fn new(last_synced_time: DateTime<Local>) -> Self {
        Self {
            last_synced_time,
            projects: Vec::new(),
            canonical_project_paths: HashMap::new(),
        }
    }

    pub(super) fn push(
        &mut self,
        path: PathBuf,
        canonical_path: PathBuf,
        bytes: &[u8],
    ) -> Result<(), TaskRepositoryError> {
        let text = std::str::from_utf8(bytes).map_err(|error| {
            TaskRepositoryError::new(
                ApplicationRepositoryOperation::Load,
                FileRepositoryError::new(
                    FileRepositoryOperation::ReadFile,
                    &path,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                ),
            )
        })?;
        self.projects.push(TaskRepository::parse_project(
            self.last_synced_time,
            path,
            canonical_path,
            text,
            &mut self.canonical_project_paths,
        )?);
        Ok(())
    }

    pub(super) fn finish(
        self,
        storage_revision: Option<Uuid>,
    ) -> Result<LoadedRepositoryState, TaskRepositoryError> {
        TaskRepository::finish_loaded_state(self.projects, storage_revision)
    }
}

pub(super) fn parse_storage_revision(
    path: &Path,
    bytes: &[u8],
) -> Result<Uuid, FileRepositoryError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        FileRepositoryError::new(
            FileRepositoryOperation::ReadFile,
            path,
            std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        )
    })?;
    Uuid::parse_str(text.trim()).map_err(|error| {
        FileRepositoryError::new(
            FileRepositoryOperation::ParseRevision,
            path,
            std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        )
    })
}

impl TaskRepository {
    fn index_task_and_descendants(
        task: &TaskHandle,
        project_yaml_file_path: &Path,
        task_path: String,
        task_locations: &mut HashMap<Uuid, TaskLocation>,
        id_to_task_map: &mut HashMap<Uuid, TaskHandle>,
    ) -> Result<(), TaskRepositoryError> {
        let task_id = task.get_id().map_err(|error| {
            TaskRepositoryError::new(ApplicationRepositoryOperation::Load, error)
        })?;
        let location = TaskLocation {
            project_yaml_file_path: project_yaml_file_path.to_path_buf(),
            task_path,
        };
        if let Some(first) = task_locations.insert(task_id, location.clone()) {
            return Err(TaskRepositoryError::new(
                ApplicationRepositoryOperation::Load,
                DuplicateTaskIdError {
                    task_id,
                    first,
                    duplicate: location,
                },
            ));
        }
        id_to_task_map.insert(task_id, task.clone());

        for (index, child_task) in task
            .get_children()
            .map_err(|error| TaskRepositoryError::new(ApplicationRepositoryOperation::Load, error))?
            .into_iter()
            .enumerate()
        {
            Self::index_task_and_descendants(
                &child_task,
                project_yaml_file_path,
                format!("{}.children[{index}]", location.task_path),
                task_locations,
                id_to_task_map,
            )?;
        }
        Ok(())
    }

    fn parse_project(
        last_synced_time: DateTime<Local>,
        project_yaml_file_path: PathBuf,
        canonical_project_yaml_path: PathBuf,
        text: &str,
        canonical_project_paths: &mut HashMap<PathBuf, PathBuf>,
    ) -> Result<Project, TaskRepositoryError> {
        let project_dir_path = project_yaml_file_path
            .parent()
            .ok_or_else(|| {
                TaskRepositoryError::new(
                    ApplicationRepositoryOperation::Load,
                    FileRepositoryError::new(
                        FileRepositoryOperation::ParseProject,
                        &project_yaml_file_path,
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "project.yaml must have a parent directory",
                        ),
                    ),
                )
            })?
            .to_path_buf();
        let docs = YamlLoader::load_from_str(text).map_err(|error| {
            TaskRepositoryError::new(
                ApplicationRepositoryOperation::Load,
                FileRepositoryError::new(
                    FileRepositoryOperation::ParseProject,
                    &project_yaml_file_path,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                ),
            )
        })?;
        let project_yaml = docs
            .first()
            .map(|doc| &doc["project"])
            .filter(|yaml| yaml.as_hash().is_some())
            .ok_or_else(|| {
                TaskRepositoryError::new(
                    ApplicationRepositoryOperation::Load,
                    FileRepositoryError::new(
                        FileRepositoryOperation::ParseProject,
                        &project_yaml_file_path,
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "project document must contain a project mapping",
                        ),
                    ),
                )
            })?;
        let root_task = yaml_to_task(project_yaml, last_synced_time).map_err(|error| {
            TaskRepositoryError::new(
                ApplicationRepositoryOperation::Load,
                FileRepositoryError::new(
                    FileRepositoryOperation::ParseProject,
                    &project_yaml_file_path,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                ),
            )
        })?;
        if let Some(first_project_yaml_path) = canonical_project_paths
            .insert(canonical_project_yaml_path, project_yaml_file_path.clone())
        {
            return Err(TaskRepositoryError::new(
                ApplicationRepositoryOperation::Load,
                FileRepositoryError::new(
                    FileRepositoryOperation::ParseProject,
                    &project_yaml_file_path,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "project YAML paths resolve to the same canonical path: {} and {}",
                            first_project_yaml_path.display(),
                            project_yaml_file_path.display()
                        ),
                    ),
                ),
            ));
        }
        let priority = root_task.get_priority().map_err(|error| {
            TaskRepositoryError::new(ApplicationRepositoryOperation::Load, error)
        })?;
        let project = Project::new(
            root_task,
            project_dir_path,
            project_yaml_file_path,
            priority,
        );
        project.mark_clean().map_err(|error| {
            TaskRepositoryError::new(ApplicationRepositoryOperation::Load, error)
        })?;
        Ok(project)
    }

    fn finish_loaded_state(
        projects: Vec<Project>,
        storage_revision: Option<Uuid>,
    ) -> Result<LoadedRepositoryState, TaskRepositoryError> {
        let mut task_locations = HashMap::new();
        let mut id_to_task_map = HashMap::new();
        for project in &projects {
            Self::index_task_and_descendants(
                &project.root_task,
                &project.project_yaml_file_path,
                "project".to_string(),
                &mut task_locations,
                &mut id_to_task_map,
            )?;
        }
        Ok(LoadedRepositoryState {
            projects,
            id_to_task_map,
            storage_revision,
        })
    }
}
