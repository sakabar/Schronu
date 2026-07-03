use crate::adapter::gateway::yaml::yaml_to_task;
use crate::application::interface::TaskRepositoryTrait;
use crate::entity::datetime::get_next_morning_datetime;
use crate::entity::task::extract_leaf_tasks_from_project;
use crate::entity::task::extract_leaf_tasks_from_project_with_pending;
use crate::entity::task::{task_to_yaml, Status, Task};
use chrono::Duration;
use chrono::{DateTime, Local};
use linked_hash_map::LinkedHashMap;
use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
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
    id_to_task_map: RefCell<HashMap<Uuid, Task>>,
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
            id_to_task_map: RefCell::new(HashMap::new()),
        }
    }

    fn cache_task_and_descendants(&self, task: &Task) {
        self.id_to_task_map
            .borrow_mut()
            .insert(task.get_id(), task.clone());

        for child_task in task.get_children() {
            self.cache_task_and_descendants(&child_task);
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
                        self.cache_task_and_descendants(&root_task);
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
                    deadline_time_opt,
                    neg_priority,
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
    fn get_lowest_priority_leaf_task_id(&mut self, recent_days: i64) -> Option<Uuid> {
        // 副作用として、projectsを優先度の低い順に破壊的にソートする
        self.projects.sort_by(|a, b| a.priority.cmp(&b.priority));

        // 優先度が低いPJ順に見て、返すべき葉タスクのid値を更新していく
        let mut ans = None;
        let recent_threshold =
            get_next_morning_datetime(self.last_synced_time) + Duration::days(recent_days);

        for project in &self.projects {
            let root_task = &project.root_task;

            let leaf_tasks: Vec<Task> = extract_leaf_tasks_from_project_with_pending(&root_task);

            for leaf_task in leaf_tasks.iter() {
                if leaf_task.get_start_time() >= recent_threshold
                    || (leaf_task.get_orig_status() == Status::Pending
                        && leaf_task.get_pending_until() >= recent_threshold)
                {
                    continue;
                }

                let deadline_time_opt = leaf_task.get_deadline_time_opt();
                let first_available_time = leaf_task.first_available_time();
                let is_recent = first_available_time < recent_threshold;
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
        if let Some(task) = self.id_to_task_map.borrow().get(&id).cloned() {
            return Some(task);
        }

        for project in self.projects.iter() {
            let tmp = project.root_task.get_by_id(id);
            if let Some(task) = tmp {
                self.id_to_task_map.borrow_mut().insert(id, task.clone());
                return Some(task);
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

                self.cache_task_and_descendants(&project.root_task);
                self.projects.push(project);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::task::TaskAttr;
    use chrono::TimeZone;

    fn task_with_start_time(name: &str, start_time: DateTime<Local>) -> Task {
        let task = Task::new(name);
        task.set_start_time(start_time);
        task.set_priority(5);
        task
    }

    fn pending_task_with_until(name: &str, pending_until: DateTime<Local>) -> Task {
        let task = Task::new(name);
        task.set_start_time(DateTime::<Local>::MIN_UTC.into());
        task.set_pending_until(pending_until);
        task.set_orig_status(Status::Pending);
        task.set_priority(5);
        task
    }

    fn add_project(task_repository: &mut TaskRepository, root_task: Task) {
        task_repository
            .projects
            .push(Project::new(root_task, "".to_string(), "".to_string(), 5));
    }

    #[test]
    fn test_get_by_id_キャッシュから取得する() {
        let mut task_repository = TaskRepository::new("");
        let root_task = Task::new("親タスク");
        let child_task = root_task.create_as_last_child(TaskAttr::new("子タスク"));
        let child_task_id = child_task.get_id();

        task_repository.cache_task_and_descendants(&root_task);
        task_repository
            .projects
            .push(Project::new(root_task, "".to_string(), "".to_string(), 5));

        let actual = task_repository.get_by_id(child_task_id).unwrap();

        assert_eq!(actual.get_name(), "子タスク");
        assert!(task_repository
            .id_to_task_map
            .borrow()
            .contains_key(&child_task_id));
    }

    #[test]
    fn test_get_by_id_実行中に追加された子タスクを検索してキャッシュする() {
        let mut task_repository = TaskRepository::new("");
        let root_task = Task::new("親タスク");

        task_repository.cache_task_and_descendants(&root_task);
        task_repository.projects.push(Project::new(
            root_task.clone(),
            "".to_string(),
            "".to_string(),
            5,
        ));

        let child_task = root_task.create_as_last_child(TaskAttr::new("子タスク"));
        let child_task_id = child_task.get_id();
        assert!(!task_repository
            .id_to_task_map
            .borrow()
            .contains_key(&child_task_id));

        let actual = task_repository.get_by_id(child_task_id).unwrap();

        assert_eq!(actual.get_name(), "子タスク");
        assert!(task_repository
            .id_to_task_map
            .borrow()
            .contains_key(&child_task_id));
    }

    #[test]
    fn test_get_highest_priority_leaf_task_id_締切あり同士では優先度より締切日時を先に見る() {
        let mut task_repository = TaskRepository::new("");
        let high_priority_late_deadline_task = Task::new("高優先度だが締切が遅いタスク");
        high_priority_late_deadline_task.set_priority(99);
        high_priority_late_deadline_task
            .set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 5, 11, 20, 0, 0).unwrap()));

        let low_priority_early_deadline_task = Task::new("低優先度だが締切が早いタスク");
        low_priority_early_deadline_task.set_priority(1);
        low_priority_early_deadline_task
            .set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 5, 10, 20, 0, 0).unwrap()));
        let low_priority_early_deadline_task_id = low_priority_early_deadline_task.get_id();

        add_project(&mut task_repository, high_priority_late_deadline_task);
        add_project(&mut task_repository, low_priority_early_deadline_task);

        let actual = task_repository.get_highest_priority_leaf_task_id();

        assert_eq!(actual, Some(low_priority_early_deadline_task_id));
    }

    #[test]
    fn test_get_lowest_priority_leaf_task_id_0日指定は次の朝より前だけrecent扱いする() {
        let mut task_repository = TaskRepository::new("");
        task_repository.sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap());
        let recent_task = task_with_start_time(
            "閾値より前",
            Local.with_ymd_and_hms(2026, 5, 11, 5, 59, 59).unwrap(),
        );
        let boundary_task = task_with_start_time(
            "閾値ちょうど",
            Local.with_ymd_and_hms(2026, 5, 11, 6, 0, 0).unwrap(),
        );
        let recent_task_id = recent_task.get_id();

        add_project(&mut task_repository, boundary_task);
        add_project(&mut task_repository, recent_task);

        let actual = task_repository.get_lowest_priority_leaf_task_id(0);

        assert_eq!(actual, Some(recent_task_id));
    }

    #[test]
    fn test_get_lowest_priority_leaf_task_id_10日指定は次の朝から10日後を閾値にする() {
        let mut task_repository = TaskRepository::new("");
        task_repository.sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap());
        let recent_task = task_with_start_time(
            "10日指定でrecent",
            Local.with_ymd_and_hms(2026, 5, 21, 5, 59, 59).unwrap(),
        );
        let boundary_task = task_with_start_time(
            "10日指定の閾値ちょうど",
            Local.with_ymd_and_hms(2026, 5, 21, 6, 0, 0).unwrap(),
        );
        let recent_task_id = recent_task.get_id();

        add_project(&mut task_repository, boundary_task);
        add_project(&mut task_repository, recent_task);

        let actual = task_repository.get_lowest_priority_leaf_task_id(10);

        assert_eq!(actual, Some(recent_task_id));
    }

    #[test]
    fn test_get_lowest_priority_leaf_task_id_対象範囲外までpending済みのタスクは候補から除外する() {
        let mut task_repository = TaskRepository::new("");
        task_repository.sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap());
        let pending_task = pending_task_with_until(
            "100日後までpending済み",
            Local.with_ymd_and_hms(2026, 8, 18, 6, 0, 0).unwrap(),
        );
        let todo_task = task_with_start_time(
            "通常のTodo",
            Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
        );
        let todo_task_id = todo_task.get_id();

        add_project(&mut task_repository, pending_task);
        add_project(&mut task_repository, todo_task);

        let actual = task_repository.get_lowest_priority_leaf_task_id(0);

        assert_eq!(actual, Some(todo_task_id));
    }

    #[test]
    fn test_get_lowest_priority_leaf_task_id_対象範囲外のstart_timeを持つタスクは候補から除外する()
    {
        let mut task_repository = TaskRepository::new("");
        task_repository.sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap());
        let future_task = task_with_start_time(
            "遠い未来に開始するTodo",
            Local.with_ymd_and_hms(2026, 12, 19, 6, 0, 0).unwrap(),
        );
        let todo_task = task_with_start_time(
            "通常のTodo",
            Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
        );
        let todo_task_id = todo_task.get_id();

        add_project(&mut task_repository, future_task);
        add_project(&mut task_repository, todo_task);

        let actual = task_repository.get_lowest_priority_leaf_task_id(0);

        assert_eq!(actual, Some(todo_task_id));
    }

    #[test]
    fn test_get_lowest_priority_leaf_task_id_対象範囲外までpending済みのタスクしかなければnoneを返す(
    ) {
        let mut task_repository = TaskRepository::new("");
        task_repository.sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap());
        let pending_task = pending_task_with_until(
            "100日後までpending済み",
            Local.with_ymd_and_hms(2026, 8, 18, 6, 0, 0).unwrap(),
        );

        add_project(&mut task_repository, pending_task);

        let actual = task_repository.get_lowest_priority_leaf_task_id(0);

        assert_eq!(actual, None);
    }

    #[test]
    fn test_get_lowest_priority_leaf_task_id_対象範囲外のstart_timeを持つタスクしかなければnoneを返す(
    ) {
        let mut task_repository = TaskRepository::new("");
        task_repository.sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap());
        let future_task = task_with_start_time(
            "遠い未来に開始するTodo",
            Local.with_ymd_and_hms(2026, 12, 19, 6, 0, 0).unwrap(),
        );

        add_project(&mut task_repository, future_task);

        let actual = task_repository.get_lowest_priority_leaf_task_id(0);

        assert_eq!(actual, None);
    }

    #[test]
    fn test_get_lowest_priority_leaf_task_id_pending_untilが閾値より前なら候補に残す() {
        let mut task_repository = TaskRepository::new("");
        task_repository.sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap());
        let pending_task = pending_task_with_until(
            "閾値より前までpending",
            Local.with_ymd_and_hms(2026, 5, 11, 5, 59, 59).unwrap(),
        );
        let pending_task_id = pending_task.get_id();

        add_project(&mut task_repository, pending_task);

        let actual = task_repository.get_lowest_priority_leaf_task_id(0);

        assert_eq!(actual, Some(pending_task_id));
    }

    #[test]
    fn test_get_lowest_priority_leaf_task_id_pending_untilが閾値ちょうどなら候補から除外する() {
        let mut task_repository = TaskRepository::new("");
        task_repository.sync_clock(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap());
        let pending_task = pending_task_with_until(
            "閾値ちょうどまでpending",
            Local.with_ymd_and_hms(2026, 5, 11, 6, 0, 0).unwrap(),
        );

        add_project(&mut task_repository, pending_task);

        let actual = task_repository.get_lowest_priority_leaf_task_id(0);

        assert_eq!(actual, None);
    }
}
