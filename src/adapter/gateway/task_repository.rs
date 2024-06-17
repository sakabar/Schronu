use crate::adapter::gateway::yaml::yaml_to_task;
use crate::application::interface::TaskRepositoryTrait;
use crate::entity::task::extract_leaf_tasks_from_project;
use crate::entity::task::extract_leaf_tasks_from_project_with_pending;
use crate::entity::task::{task_to_yaml, Task};
use chrono::Duration;
use chrono::{DateTime, Local};
use linked_hash_map::LinkedHashMap;
use regex::Regex;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
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
    _project_dir_path: String,
    project_yaml_file_path: String,
    priority: i64,
}

impl Project {
    fn new(
        root_task: Task,
        _project_dir_path: String,
        project_yaml_file_path: String,
        priority: i64,
    ) -> Self {
        Self {
            root_task,
            _project_dir_path,
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
    fn get_project_storage_dir_name(&self) -> &str {
        &self.project_storage_dir_name
    }

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
                let project_dir_path: String = entry
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
                            project_dir_path,
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

    fn get_last_synced_time(&self) -> DateTime<Local> {
        self.last_synced_time
    }

    fn get_highest_priority_project(&mut self) -> Option<&Task> {
        // 副作用として、projectsを優先度の低い順に破壊的にソートする
        // 葉ノードを出力する際に優先度が高いものが下となり優先度が低いものが画面外(上)になるように、projectsは低い順に保持する
        // 最も優先度が高いprojectsが必要な場合はlast()で取得する
        self.projects.sort_by(|a, b| a.priority.cmp(&b.priority));

        self.projects
            .last()
            .and_then(|project| Some(&project.root_task))
    }

    fn get_highest_priority_leaf_task_id(&mut self) -> Option<Uuid> {
        // 副作用として、projectsを優先度の低い順に破壊的にソートする
        // 葉ノードを出力する際に優先度が高いものが下となり優先度が低いものが画面外(上)になるように、projectsは低い順に保持する
        // 最も優先度が高いprojectsが必要な場合はlast()で取得する
        self.projects.sort_by(|a, b| a.priority.cmp(&b.priority));

        // 優先度が低いPJ順に見て、返すべき葉タスクのid値を更新していく
        let mut ans = None;

        for project in &self.projects {
            let root_task = &project.root_task;

            let leaf_tasks: Vec<Task> = extract_leaf_tasks_from_project(&root_task);

            for leaf_task in leaf_tasks.iter() {
                let deadline_time_opt = leaf_task.get_deadline_time_opt();
                let neg_priority = -leaf_task.get_priority();
                let id = leaf_task.get_id();

                let tpl = (
                    deadline_time_opt.is_none(),
                    neg_priority,
                    deadline_time_opt,
                    id,
                );

                if ans.is_none() || tpl < ans.unwrap() {
                    ans = Some(tpl);
                }
            }
        }

        let ans_id = ans.map(|tpl| tpl.3);
        ans_id
    }

    // 優先度の低いタスクを未来に飛ばす用
    fn get_lowest_priority_leaf_task_id(&mut self) -> Option<Uuid> {
        // 副作用として、projectsを優先度の低い順に破壊的にソートする
        self.projects.sort_by(|a, b| a.priority.cmp(&b.priority));

        // 優先度が低いPJ順に見て、返すべき葉タスクのid値を更新していく
        let mut ans = None;

        for project in &self.projects {
            let root_task = &project.root_task;

            let leaf_tasks: Vec<Task> = extract_leaf_tasks_from_project_with_pending(&root_task);

            for leaf_task in leaf_tasks.iter() {
                let deadline_time_opt = leaf_task.get_deadline_time_opt();
                let first_available_time = leaf_task.first_available_time();
                let is_recent = first_available_time <= self.last_synced_time + Duration::days(10);
                let neg_priority = -leaf_task.get_priority();
                let id = leaf_task.get_id();

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

        let ans_id = ans.map(|tpl| tpl.5);
        ans_id
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

    fn start_new_project(&mut self, root_task: Task) {
        let project_name = root_task.get_name();

        let yyyymmdd = self.last_synced_time.format("%Y%m%d").to_string();

        // ディレクトリ名からはURLを除く (ディレクトリの区切りに使われうる "/" が入らないようにするため)
        let http_pattern = Regex::new(r"http.*").unwrap();
        let project_name_for_dir = http_pattern.replace(&project_name, "").replace("/", "-");

        let dir_name = format!("{}-{}", yyyymmdd, project_name_for_dir);
        let project_dir_path = Path::new(&self.project_storage_dir_name).join(dir_name);

        // project_dirを実際に生成する
        match fs::create_dir_all(&project_dir_path) {
            Ok(()) => {}
            Err(_) => {
                return;
            }
        }

        let markdown_dir_path = &project_dir_path.join("markdown");
        match fs::create_dir_all(&markdown_dir_path) {
            Ok(()) => {}
            Err(err) => {
                println!("{}", err);
                return;
            }
        }

        let project_yaml_file_path = project_dir_path.join("project.yaml");

        let priority = root_task.get_priority();

        match (project_dir_path.to_str(), project_yaml_file_path.to_str()) {
            (Some(project_dir_path_str), Some(project_yaml_file_path_str)) => {
                let project = Project::new(
                    root_task,
                    project_dir_path_str.to_string(),
                    project_yaml_file_path_str.to_string(),
                    priority,
                );

                self.projects.push(project);
            }
            _ => {}
        }
    }
}
