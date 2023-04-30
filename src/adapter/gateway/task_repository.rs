use crate::adapter::gateway::yaml::yaml_to_task;
use crate::application::interface::TaskRepositoryTrait;
use crate::entity::task::{task_to_yaml, Task};
use chrono::{DateTime, Local};
use linked_hash_map::LinkedHashMap;
use std::fs::File;
use std::io::prelude::*;
use uuid::Uuid;
use walkdir::WalkDir;
use yaml_rust::{Yaml, YamlEmitter, YamlLoader};

pub struct TaskRepository {
    projects: Vec<Project>,
    project_storage_dir_name: String,
    last_synced_time: DateTime<Local>,
}

struct Project {
    root_task: Task,
    _project_dir_name: String,
    project_yaml_file_path: String,
    priority: i64,
}

impl Project {
    fn new(
        root_task: Task,
        _project_dir_name: String,
        project_yaml_file_path: String,
        priority: i64,
    ) -> Self {
        Self {
            root_task,
            _project_dir_name,
            project_yaml_file_path,
            priority,
        }
    }
}

impl TaskRepository {
    pub fn new(project_storage_dir_name: &str) -> Self {
        Self {
            projects: vec![],
            project_storage_dir_name: project_storage_dir_name.to_string(),
            last_synced_time: DateTime::<Local>::MIN_UTC.into(),
        }
    }
}

impl TaskRepositoryTrait for TaskRepository {
    fn get_all_projects(&self) -> Vec<&Task> {
        self.projects
            .iter()
            .map(|project| &project.root_task)
            .collect()
    }

    fn load(&mut self) {
        for entry in WalkDir::new(self.project_storage_dir_name.as_str())
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_name() == "project.yaml" {
                let project_yaml_file_path: String =
                    entry.path().to_str().map(|s| s.to_string()).unwrap();
                let project_dir_name: String = entry
                    .path()
                    .parent()
                    .and_then(|name| name.to_str().map(|s| s.to_string()))
                    .unwrap();
                let mut file = File::open(entry.path()).unwrap();
                let mut text = String::new();
                file.read_to_string(&mut text).unwrap();

                match YamlLoader::load_from_str(text.as_str()) {
                    Err(_) => {
                        panic!("Error occured in {:?}", entry.path());
                    }
                    Ok(docs) => {
                        let project_yaml: &Yaml = &docs[0]["project"];
                        let root_task: Task = yaml_to_task(project_yaml, self.last_synced_time);
                        let priority = root_task.get_priority();
                        let project = Project::new(
                            root_task,
                            project_dir_name,
                            project_yaml_file_path,
                            priority,
                        );
                        self.projects.push(project);
                    }
                }
            }
        }
    }

    fn save(&self) {
        for project in self.projects.iter() {
            let root_task = &project.root_task;
            let task_yaml = task_to_yaml(root_task);

            let mut project_hash = LinkedHashMap::new();
            project_hash.insert(Yaml::String(String::from("project")), task_yaml);
            let doc = Yaml::Hash(project_hash);

            let mut out_str = String::new();
            let mut emitter = YamlEmitter::new(&mut out_str);
            emitter.dump(&doc).unwrap();

            out_str += "\n";

            let mut file = File::create(project.project_yaml_file_path.as_str()).unwrap();
            file.write_all(out_str.as_bytes()).unwrap();
        }
    }

    fn sync_clock(&mut self, now: DateTime<Local>) {
        self.last_synced_time = now;

        // TODO
        // これ、本来はprojectsの中に伝搬させていくべきだ。
    }

    fn get_highest_priority_project(&mut self) -> Option<&Task> {
        // 副作用として、projectsを優先度の高い順に破壊的にソートする
        self.projects.sort_by(|a, b| b.priority.cmp(&a.priority));

        self.projects
            .first()
            .and_then(|project| Some(&project.root_task))
    }

    fn get_by_id(&self, id: Uuid) -> Option<Task> {
        for project in self.projects.iter() {
            let tmp = project.root_task.get_by_id(id);
            if tmp.is_some() {
                return tmp;
            }
        }

        None
    }
}
