use chrono::{Local, Timelike};
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;
use schronu::entity::task::TaskHandle;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use uuid::Uuid;

pub(super) struct CliFixture {
    root: PathBuf,
    pub(super) storage: PathBuf,
    config: PathBuf,
}

impl CliFixture {
    pub(super) fn new(seed_existing_project: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "schronu-task-name-cli-contract-{}",
            Uuid::new_v4().hyphenated()
        ));
        let storage = root.join("storage");
        fs::create_dir_all(&storage).unwrap();
        let busy_time_slots = root.join("busy_time_slots.yaml");
        fs::write(&busy_time_slots, valid_busy_time_slots_yaml()).unwrap();
        let config = root.join("schronu.yaml");
        fs::write(
            &config,
            format!("busy_time_slots_yaml_path: {}\n", busy_time_slots.display()),
        )
        .unwrap();
        let fixture = Self {
            root,
            storage,
            config,
        };
        if seed_existing_project {
            let now = Local::now().with_nanosecond(0).unwrap();
            let task = TaskHandle::with_identity("既存task", Uuid::new_v4(), now).unwrap();
            let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
            repository.sync_clock(now).unwrap();
            repository.load().unwrap();
            repository.start_new_project(task).unwrap();
            repository.save().unwrap();
        }
        fixture
    }

    pub(super) fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_schronu"));
        command
            .args(args)
            .env("SCHRONU_STORAGE_DIR", &self.storage)
            .env("SCHRONU_CONFIG_PATH", &self.config);
        command
    }

    pub(super) fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    pub(super) fn stored_project_names(&self) -> Vec<String> {
        let mut repository = TaskRepository::new(self.storage.to_str().unwrap());
        repository.sync_clock(Local::now()).unwrap();
        repository.load().unwrap();
        repository
            .get_all_projects()
            .into_iter()
            .map(|task| task.get_name().unwrap())
            .collect()
    }
}

impl Drop for CliFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn valid_busy_time_slots_yaml() -> String {
    let mut yaml = String::from("days_of_week:\n");
    for day_of_week in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
        yaml.push_str(&format!(
            "  - day_of_week: {day_of_week}\n    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: recurring-unavailable-time\n"
        ));
    }
    yaml
}
