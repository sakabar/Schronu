use crate::adapter::gateway::yaml::{task_snapshot_to_yaml, yaml_to_task};
use crate::application::interface::{
    ProjectRegistrationError, RepositoryReloadOutcome, TaskRepositoryError,
    TaskRepositoryOperation as ApplicationRepositoryOperation, TaskRepositoryTrait,
};
use crate::entity::task::extract_leaf_tasks_from_project;
use crate::entity::task::extract_leaf_tasks_from_project_with_pending;
use crate::entity::task::{Status, TaskHandle, TaskTreeError};
use chrono::{DateTime, Local};
use linked_hash_map::LinkedHashMap;
use regex::Regex;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use walkdir::WalkDir;
use yaml_rust::{Yaml, YamlEmitter, YamlLoader};

const PROJECT_DIRECTORY_COMPONENT_MAX_BYTES: usize = 255;

pub struct TaskRepository {
    projects: Vec<Project>,
    project_storage_dir_name: String,
    last_synced_time: DateTime<Local>,
    id_to_task_map: RefCell<HashMap<Uuid, TaskHandle>>,
    storage_revision: Cell<Option<Uuid>>,
    has_loaded: bool,
}

struct Project {
    root_task: TaskHandle,
    project_dir_path: PathBuf,
    project_yaml_file_path: PathBuf,
    priority: i64,
    persisted_mutation_revision: Cell<Option<u64>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileRepositoryOperation {
    TraverseDirectory,
    ReadMetadata,
    OpenFile,
    ReadFile,
    ParseProject,
    ParseRevision,
    SerializeProject,
    CreateDirectory,
    CreateFile,
    WriteFile,
    SyncFile,
    SetPermissions,
    RenameFile,
}

#[derive(Debug)]
struct FileRepositoryError {
    operation: FileRepositoryOperation,
    path: PathBuf,
    source: std::io::Error,
}

impl FileRepositoryError {
    fn new(
        operation: FileRepositoryOperation,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for FileRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "file repository {:?} failed for {}: {}",
            self.operation,
            self.path.display(),
            self.source
        )
    }
}

impl Error for FileRepositoryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

trait AtomicSaveFile {
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    fn sync_all(&self) -> std::io::Result<()>;
}

impl AtomicSaveFile for File {
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        Write::write_all(self, bytes)
    }

    fn sync_all(&self) -> std::io::Result<()> {
        File::sync_all(self)
    }
}

fn write_and_sync_temporary_file(
    file: &mut dyn AtomicSaveFile,
    temporary_file_path: &Path,
    bytes: &[u8],
) -> Result<(), FileRepositoryError> {
    file.write_all(bytes).map_err(|error| {
        FileRepositoryError::new(
            FileRepositoryOperation::WriteFile,
            temporary_file_path,
            error,
        )
    })?;
    file.sync_all().map_err(|error| {
        FileRepositoryError::new(
            FileRepositoryOperation::SyncFile,
            temporary_file_path,
            error,
        )
    })
}

fn replace_file_atomically<F: AtomicSaveFile>(
    target_file_path: &Path,
    temporary_file_path: &Path,
    mut file: F,
    bytes: &[u8],
) -> Result<(), FileRepositoryError> {
    let write_result = write_and_sync_temporary_file(&mut file, temporary_file_path, bytes);
    drop(file);

    let result = write_result.and_then(|()| {
        fs::rename(temporary_file_path, target_file_path).map_err(|error| {
            FileRepositoryError::new(FileRepositoryOperation::RenameFile, target_file_path, error)
        })
    });

    if result.is_err() {
        let _ = fs::remove_file(temporary_file_path);
    }
    result
}

fn write_file_atomically_with_temporary_path(
    target_file_path: &Path,
    temporary_file_path: &Path,
    bytes: &[u8],
) -> Result<(), FileRepositoryError> {
    let existing_permissions = match fs::metadata(target_file_path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(FileRepositoryError::new(
                FileRepositoryOperation::ReadMetadata,
                target_file_path,
                error,
            ));
        }
    };
    let file = File::create(temporary_file_path).map_err(|error| {
        FileRepositoryError::new(
            FileRepositoryOperation::CreateFile,
            temporary_file_path,
            error,
        )
    })?;
    if let Some(permissions) = existing_permissions {
        if let Err(error) = file.set_permissions(permissions) {
            drop(file);
            let _ = fs::remove_file(temporary_file_path);
            return Err(FileRepositoryError::new(
                FileRepositoryOperation::SetPermissions,
                temporary_file_path,
                error,
            ));
        }
    }
    replace_file_atomically(target_file_path, temporary_file_path, file, bytes)
}

fn write_file_atomically_if_changed_with_temporary_path(
    target_file_path: &Path,
    temporary_file_path: &Path,
    bytes: &[u8],
) -> Result<bool, FileRepositoryError> {
    match fs::read(target_file_path) {
        Ok(existing_bytes) if existing_bytes == bytes => return Ok(false),
        Ok(_) => {}
        Err(_) => {}
    }

    write_file_atomically_with_temporary_path(target_file_path, temporary_file_path, bytes)?;
    Ok(true)
}

fn write_file_atomically(
    target_file_path: &Path,
    bytes: &[u8],
) -> Result<bool, FileRepositoryError> {
    let file_name = target_file_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project.yaml");
    let temporary_file_path = target_file_path
        .with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4().hyphenated()));
    write_file_atomically_if_changed_with_temporary_path(
        target_file_path,
        &temporary_file_path,
        bytes,
    )
}

fn open_project_file(
    project_yaml_file_path: &Path,
) -> Result<(File, PathBuf), FileRepositoryError> {
    let canonical_path = fs::canonicalize(project_yaml_file_path).map_err(|error| {
        FileRepositoryError::new(
            FileRepositoryOperation::OpenFile,
            project_yaml_file_path,
            error,
        )
    })?;
    let file = File::open(&canonical_path).map_err(|error| {
        FileRepositoryError::new(
            FileRepositoryOperation::OpenFile,
            project_yaml_file_path,
            error,
        )
    })?;
    Ok((file, canonical_path))
}

fn project_directory_name(date: &str, project_name: &str, project_id: Uuid) -> String {
    // ディレクトリ名からはURLを除く (ディレクトリの区切りに使われうる "/" が入らないようにするため)
    let http_pattern = Regex::new(r"http.*").expect("project name URL regex must be valid");
    let project_name_for_dir = http_pattern.replace(project_name, "").replace('/', "-");
    let prefix = format!("{date}-");
    let identity_suffix = format!("-{project_id}");
    let max_name_bytes =
        PROJECT_DIRECTORY_COMPONENT_MAX_BYTES.saturating_sub(prefix.len() + identity_suffix.len());
    let mut name_end = project_name_for_dir.len().min(max_name_bytes);
    while !project_name_for_dir.is_char_boundary(name_end) {
        name_end -= 1;
    }
    let project_name_for_dir = &project_name_for_dir[..name_end];

    format!("{prefix}{project_name_for_dir}{identity_suffix}")
}

impl Project {
    fn new(
        root_task: TaskHandle,
        project_dir_path: impl Into<PathBuf>,
        project_yaml_file_path: impl Into<PathBuf>,
        priority: i64,
    ) -> Self {
        Self {
            root_task,
            project_dir_path: project_dir_path.into(),
            project_yaml_file_path: project_yaml_file_path.into(),
            priority,
            persisted_mutation_revision: Cell::new(None),
        }
    }

    fn mark_clean(&self) -> Result<(), TaskTreeError> {
        self.persisted_mutation_revision
            .set(Some(self.root_task.get_persistent_mutation_revision()?));
        Ok(())
    }

    fn needs_save(&self) -> Result<bool, TaskTreeError> {
        Ok(self.persisted_mutation_revision.get()
            != Some(self.root_task.get_persistent_mutation_revision()?))
    }
}

impl TaskRepository {
    pub fn new(project_storage_dir_name: &str) -> Self {
        Self {
            projects: vec![],
            project_storage_dir_name: project_storage_dir_name.to_string(),
            last_synced_time: DateTime::<Local>::MIN_UTC.into(),
            id_to_task_map: RefCell::new(HashMap::new()),
            storage_revision: Cell::new(None),
            has_loaded: false,
        }
    }

    fn cache_task_and_descendants(&self, task: &TaskHandle) -> Result<(), TaskTreeError> {
        self.id_to_task_map
            .borrow_mut()
            .insert(task.get_id()?, task.clone());

        for child_task in task.get_children()? {
            self.cache_task_and_descendants(&child_task)?;
        }
        Ok(())
    }

    fn sync_task_and_descendants(
        task: &TaskHandle,
        now: DateTime<Local>,
    ) -> Result<(), TaskTreeError> {
        task.sync_clock(now)?;
        for child_task in task.get_children()? {
            Self::sync_task_and_descendants(&child_task, now)?;
        }
        Ok(())
    }

    fn storage_revision_path(&self) -> PathBuf {
        Path::new(&self.project_storage_dir_name).join(".revision")
    }

    fn read_storage_revision(&self) -> Result<Option<Uuid>, FileRepositoryError> {
        let revision_path = self.storage_revision_path();
        match fs::symlink_metadata(&revision_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(FileRepositoryError::new(
                    FileRepositoryOperation::ReadMetadata,
                    revision_path,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "storage revision must not be a symbolic link",
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(FileRepositoryError::new(
                    FileRepositoryOperation::ReadMetadata,
                    revision_path,
                    error,
                ));
            }
        }

        let revision_text = fs::read_to_string(&revision_path).map_err(|error| {
            FileRepositoryError::new(FileRepositoryOperation::ReadFile, &revision_path, error)
        })?;
        Uuid::parse_str(revision_text.trim())
            .map(Some)
            .map_err(|error| {
                FileRepositoryError::new(
                    FileRepositoryOperation::ParseRevision,
                    revision_path,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                )
            })
    }

    fn serialize_project(project: &Project) -> Result<Vec<u8>, TaskRepositoryError> {
        let snapshot = project.root_task.snapshot().map_err(|error| {
            TaskRepositoryError::new(ApplicationRepositoryOperation::Save, error)
        })?;
        let task_yaml = task_snapshot_to_yaml(&snapshot);
        let mut project_hash = LinkedHashMap::new();
        project_hash.insert(Yaml::String(String::from("project")), task_yaml);
        let doc = Yaml::Hash(project_hash);
        let mut out = String::new();
        YamlEmitter::new(&mut out).dump(&doc).map_err(|error| {
            TaskRepositoryError::new(
                ApplicationRepositoryOperation::Save,
                FileRepositoryError::new(
                    FileRepositoryOperation::SerializeProject,
                    &project.project_yaml_file_path,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                ),
            )
        })?;
        out.push('\n');
        Ok(out.into_bytes())
    }
}

impl TaskRepositoryTrait for TaskRepository {
    fn get_project_storage_dir_name(&self) -> &str {
        &self.project_storage_dir_name
    }

    fn get_all_projects(&self) -> Vec<&TaskHandle> {
        self.projects
            .iter()
            .map(|project| &project.root_task)
            .collect()
    }

    fn load(&mut self) -> Result<(), TaskRepositoryError> {
        let storage_revision = self.read_storage_revision().map_err(|error| {
            TaskRepositoryError::new(ApplicationRepositoryOperation::Load, error)
        })?;
        let mut loaded_projects = Vec::new();
        let mut canonical_project_paths = HashMap::new();
        for entry_result in WalkDir::new(self.project_storage_dir_name.as_str()).sort_by_file_name()
        {
            let entry = entry_result.map_err(|error| {
                let path = error
                    .path()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from(&self.project_storage_dir_name));
                let reason = error.to_string();
                let io_error = error
                    .into_io_error()
                    .unwrap_or_else(|| std::io::Error::other(reason));
                TaskRepositoryError::new(
                    ApplicationRepositoryOperation::Load,
                    FileRepositoryError::new(
                        FileRepositoryOperation::TraverseDirectory,
                        path,
                        io_error,
                    ),
                )
            })?;
            if entry.file_name() == "project.yaml" {
                let project_yaml_file_path = entry.path().to_path_buf();
                let project_dir_path = entry
                    .path()
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
                let (mut file, canonical_project_yaml_path) =
                    open_project_file(&project_yaml_file_path).map_err(|error| {
                        TaskRepositoryError::new(ApplicationRepositoryOperation::Load, error)
                    })?;
                let mut text = String::new();
                file.read_to_string(&mut text).map_err(|error| {
                    TaskRepositoryError::new(
                        ApplicationRepositoryOperation::Load,
                        FileRepositoryError::new(
                            FileRepositoryOperation::ReadFile,
                            &project_yaml_file_path,
                            error,
                        ),
                    )
                })?;

                let docs = YamlLoader::load_from_str(&text).map_err(|error| {
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
                let root_task =
                    yaml_to_task(project_yaml, self.last_synced_time).map_err(|error| {
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
                loaded_projects.push(project);
            }
        }

        self.projects = loaded_projects;
        self.id_to_task_map.borrow_mut().clear();
        for project in &self.projects {
            self.cache_task_and_descendants(&project.root_task)
                .map_err(|error| {
                    TaskRepositoryError::new(ApplicationRepositoryOperation::Load, error)
                })?;
        }
        self.storage_revision.set(storage_revision);
        self.has_loaded = true;
        Ok(())
    }

    fn reload_if_changed(
        &mut self,
        now: DateTime<Local>,
    ) -> Result<RepositoryReloadOutcome, TaskRepositoryError> {
        let storage_revision = self.read_storage_revision().map_err(|error| {
            TaskRepositoryError::new(ApplicationRepositoryOperation::Load, error)
        })?;
        if self.has_loaded && storage_revision == self.storage_revision.get() {
            self.sync_clock(now).map_err(|error| {
                TaskRepositoryError::new(ApplicationRepositoryOperation::Load, error)
            })?;
            return Ok(RepositoryReloadOutcome::Cached);
        }

        self.last_synced_time = now;
        self.load()?;
        Ok(RepositoryReloadOutcome::Reloaded)
    }

    fn has_pending_changes(&self) -> Result<bool, TaskTreeError> {
        self.projects
            .iter()
            .map(Project::needs_save)
            .collect::<Result<Vec<_>, _>>()
            .map(|needs_save| needs_save.into_iter().any(|needs_save| needs_save))
    }

    fn save(&self) -> Result<(), TaskRepositoryError> {
        let projects_to_save = self
            .projects
            .iter()
            .map(|project| project.needs_save().map(|needs_save| (project, needs_save)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| TaskRepositoryError::new(ApplicationRepositoryOperation::Save, error))?
            .into_iter()
            .filter_map(|(project, needs_save)| needs_save.then_some(project))
            .collect::<Vec<_>>();

        let mut prepared_writes = Vec::new();
        for project in &projects_to_save {
            let bytes = Self::serialize_project(project)?;
            let unchanged = fs::read(&project.project_yaml_file_path)
                .is_ok_and(|existing_bytes| existing_bytes == bytes);
            if !unchanged {
                prepared_writes.push((*project, bytes));
            }
        }

        if prepared_writes.is_empty() {
            for project in projects_to_save {
                project.mark_clean().map_err(|error| {
                    TaskRepositoryError::new(ApplicationRepositoryOperation::Save, error)
                })?;
            }
            return Ok(());
        }

        for (project, _) in &prepared_writes {
            fs::create_dir_all(&project.project_dir_path).map_err(|error| {
                TaskRepositoryError::new(
                    ApplicationRepositoryOperation::Save,
                    FileRepositoryError::new(
                        FileRepositoryOperation::CreateDirectory,
                        &project.project_dir_path,
                        error,
                    ),
                )
            })?;
            let markdown_dir_path = project.project_dir_path.join("markdown");
            fs::create_dir_all(&markdown_dir_path).map_err(|error| {
                TaskRepositoryError::new(
                    ApplicationRepositoryOperation::Save,
                    FileRepositoryError::new(
                        FileRepositoryOperation::CreateDirectory,
                        &markdown_dir_path,
                        error,
                    ),
                )
            })?;
        }
        let revision_path = self.storage_revision_path();
        if fs::symlink_metadata(&revision_path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(TaskRepositoryError::new(
                ApplicationRepositoryOperation::Save,
                FileRepositoryError::new(
                    FileRepositoryOperation::ReadMetadata,
                    revision_path,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "storage revision must not be a symbolic link",
                    ),
                ),
            ));
        }
        let new_storage_revision = Uuid::new_v4();
        let revision_text = format!("{new_storage_revision}\n");
        write_file_atomically(&revision_path, revision_text.as_bytes()).map_err(|error| {
            TaskRepositoryError::new(ApplicationRepositoryOperation::Save, error)
        })?;

        for (project, bytes) in prepared_writes {
            write_file_atomically(&project.project_yaml_file_path, &bytes).map_err(|error| {
                TaskRepositoryError::new(ApplicationRepositoryOperation::Save, error)
            })?;
        }

        for project in projects_to_save {
            project.mark_clean().map_err(|error| {
                TaskRepositoryError::new(ApplicationRepositoryOperation::Save, error)
            })?;
        }
        self.storage_revision.set(Some(new_storage_revision));
        Ok(())
    }

    fn sync_clock(&mut self, now: DateTime<Local>) -> Result<(), TaskTreeError> {
        self.last_synced_time = now;
        for project in &self.projects {
            Self::sync_task_and_descendants(&project.root_task, now)?;
        }
        Ok(())
    }

    fn get_last_synced_time(&self) -> DateTime<Local> {
        self.last_synced_time
    }

    fn get_highest_priority_project(&mut self) -> Option<&TaskHandle> {
        // 副作用として、projectsを優先度の低い順に破壊的にソートする
        // 葉ノードを出力する際に優先度が高いものが下となり優先度が低いものが画面外(上)になるように、projectsは低い順に保持する
        // 最も優先度が高いprojectsが必要な場合はlast()で取得する
        self.projects.sort_by_key(|a| a.priority);

        self.projects.last().map(|project| &project.root_task)
    }

    fn get_highest_priority_leaf_task_id(
        &mut self,
        excluded_task_ids: &[Uuid],
    ) -> Result<Option<Uuid>, TaskTreeError> {
        // 副作用として、projectsを優先度の低い順に破壊的にソートする
        // 葉ノードを出力する際に優先度が高いものが下となり優先度が低いものが画面外(上)になるように、projectsは低い順に保持する
        // 最も優先度が高いprojectsが必要な場合はlast()で取得する
        self.projects.sort_by_key(|a| a.priority);

        // 優先度が低いPJ順に見て、返すべき葉タスクのid値を更新していく
        let mut ans = None;

        for project in &self.projects {
            let root_task = &project.root_task;

            let leaf_tasks = extract_leaf_tasks_from_project(root_task)?;

            for leaf_task in leaf_tasks.iter() {
                let deadline_time_opt = leaf_task.get_deadline_time_opt()?;
                let neg_priority = !leaf_task.get_priority()?;
                let id = leaf_task.get_id()?;
                if excluded_task_ids.contains(&id) {
                    continue;
                }

                let tpl = (
                    deadline_time_opt.is_none(),
                    deadline_time_opt,
                    neg_priority,
                    id,
                );

                if ans.is_none() || tpl < ans.unwrap() {
                    ans = Some(tpl);
                }
            }
        }

        Ok(ans.map(|tpl| tpl.3))
    }

    // 優先度の低いタスクを未来に飛ばすための先送り候補選択用
    fn get_defer_candidate_leaf_task_id(
        &mut self,
        recent_threshold: DateTime<Local>,
        excluded_task_ids: &[Uuid],
    ) -> Result<Option<Uuid>, TaskTreeError> {
        // 副作用として、projectsを優先度の低い順に破壊的にソートする
        self.projects.sort_by_key(|a| a.priority);

        // 優先度が低いPJ順に見て、返すべき葉タスクのid値を更新していく
        let mut ans = None;
        for project in &self.projects {
            let root_task = &project.root_task;

            let leaf_tasks = extract_leaf_tasks_from_project_with_pending(root_task)?;

            for leaf_task in leaf_tasks.iter() {
                if leaf_task.get_start_time()? >= recent_threshold
                    || (leaf_task.get_orig_status()? == Status::Pending
                        && leaf_task.get_pending_until()? >= recent_threshold)
                {
                    continue;
                }

                let deadline_time_opt = leaf_task.get_deadline_time_opt()?;
                let first_available_time = leaf_task.first_available_time()?;
                let is_recent = first_available_time < recent_threshold;
                let neg_priority = !leaf_task.get_priority()?;
                let id = leaf_task.get_id()?;
                if excluded_task_ids.contains(&id) {
                    continue;
                }

                // 優先度が低いほど大さい値になる
                let tpl = (
                    deadline_time_opt.is_none(),
                    is_recent,
                    neg_priority,
                    deadline_time_opt,
                    first_available_time,
                    id,
                );

                if ans.is_none() || tpl > ans.unwrap() {
                    ans = Some(tpl);
                }
            }
        }

        Ok(ans.map(|tpl| tpl.5))
    }

    fn get_by_id(&self, id: Uuid) -> Result<Option<TaskHandle>, TaskTreeError> {
        if let Some(task) = self.id_to_task_map.borrow().get(&id).cloned() {
            return Ok(Some(task));
        }

        for project in self.projects.iter() {
            let tmp = project.root_task.get_by_id(id)?;
            if let Some(task) = tmp {
                self.id_to_task_map.borrow_mut().insert(id, task.clone());
                return Ok(Some(task));
            }
        }

        Ok(None)
    }

    fn start_new_project(&mut self, root_task: TaskHandle) -> Result<(), ProjectRegistrationError> {
        let project_name = root_task
            .get_name()
            .map_err(ProjectRegistrationError::TaskTree)?;
        let project_id = root_task
            .get_id()
            .map_err(ProjectRegistrationError::TaskTree)?;
        let priority = root_task
            .get_priority()
            .map_err(ProjectRegistrationError::TaskTree)?;

        let yyyymmdd = self.last_synced_time.format("%Y%m%d").to_string();
        let dir_name = project_directory_name(&yyyymmdd, &project_name, project_id);
        let project_dir_path = Path::new(&self.project_storage_dir_name).join(dir_name);
        let project_yaml_file_path = project_dir_path.join("project.yaml");

        for project in &self.projects {
            if project
                .root_task
                .get_by_id(project_id)
                .map_err(ProjectRegistrationError::TaskTree)?
                .is_some()
            {
                return Err(ProjectRegistrationError::DuplicateTaskId(project_id));
            }
            if project.project_dir_path == project_dir_path {
                return Err(ProjectRegistrationError::DuplicateStoragePath(
                    project_dir_path,
                ));
            }
        }

        let project = Project::new(
            root_task,
            project_dir_path,
            project_yaml_file_path,
            priority,
        );

        self.cache_task_and_descendants(&project.root_task)
            .map_err(ProjectRegistrationError::TaskTree)?;
        self.projects.push(project);
        Ok(())
    }
}

#[cfg(test)]
#[path = "task_repository_tests.rs"]
mod tests;
