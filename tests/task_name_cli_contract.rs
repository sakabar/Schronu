use chrono::{Local, Timelike};
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;
use schronu::entity::task::TaskHandle;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use uuid::Uuid;

struct CliFixture {
    root: PathBuf,
    storage: PathBuf,
    config: PathBuf,
}

impl CliFixture {
    fn seeded() -> Self {
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

        let now = Local::now().with_nanosecond(0).unwrap();
        let task = TaskHandle::with_identity("既存task", Uuid::new_v4(), now).unwrap();
        let mut repository = TaskRepository::new(storage.to_str().unwrap());
        repository.sync_clock(now).unwrap();
        repository.load().unwrap();
        repository.start_new_project(task).unwrap();
        repository.save().unwrap();

        Self {
            root,
            storage,
            config,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_schronu"));
        command
            .args(args)
            .env("SCHRONU_STORAGE_DIR", &self.storage)
            .env("SCHRONU_CONFIG_PATH", &self.config);
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn stored_project_names(&self) -> Vec<String> {
        let mut repository = TaskRepository::new(self.storage.to_str().unwrap());
        repository.sync_clock(Local::now()).unwrap();
        repository.load().unwrap();
        repository
            .get_all_projects()
            .into_iter()
            .map(|task| task.get_name().unwrap())
            .collect()
    }

    fn persistent_storage_bytes_excluding_process_lock(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        collect_directory_bytes(&self.storage, &self.storage, &mut files);
        files.remove(Path::new(".lock"));
        files
    }
}

impl Drop for CliFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn 非対話cliはosのtask名argvを原文のまま保存する() {
    let task_names = [
        "内部  連続 空白 日本語 'single' \"double\" C:\\temp",
        "  前後 空白 日本語 '引用' \"二重\" C:\\path  ",
    ];

    for task_name in task_names {
        let fixture = CliFixture::seeded();

        let output = fixture.run(&["新", task_name]);

        assert!(
            output.status.success(),
            "task_name={task_name:?}, stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stored_names = fixture.stored_project_names();
        assert!(
            stored_names
                .iter()
                .any(|stored_name| stored_name == task_name),
            "task_name={task_name:?}, stored_names={stored_names:?}"
        );
    }
}

#[test]
fn 非対話cliはcontrol名を入力errorにしてstorageを変更しない() {
    for task_name in ["ESC\u{1b}name", "tab\tname"] {
        let fixture = CliFixture::seeded();
        let storage_before = fixture.persistent_storage_bytes_excluding_process_lock();

        let output = fixture.run(&["新", task_name]);

        assert_eq!(output.status.code(), Some(1), "task_name={task_name:?}");
        assert!(output.stdout.is_empty(), "task_name={task_name:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.starts_with("[Error] ")
                && (stderr.contains("入力エラー") || stderr.contains("invalid input")),
            "task_name={task_name:?}, stderr={stderr:?}"
        );
        assert_eq!(
            fixture.persistent_storage_bytes_excluding_process_lock(),
            storage_before,
            "task_name={task_name:?}"
        );
    }
}

#[test]
fn 非対話cliのnul名はos境界で拒否されstorageを変更しない() {
    let fixture = CliFixture::seeded();
    let storage_before = fixture.persistent_storage_bytes_excluding_process_lock();

    let error = fixture.command(&["新", "NUL\0name"]).output().unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        fixture.persistent_storage_bytes_excluding_process_lock(),
        storage_before
    );
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

fn collect_directory_bytes(
    storage: &Path,
    directory: &Path,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
) {
    let mut entries = fs::read_dir(directory)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type().unwrap();
        if file_type.is_dir() {
            collect_directory_bytes(storage, &path, files);
        } else if file_type.is_file() {
            files.insert(
                path.strip_prefix(storage).unwrap().to_path_buf(),
                fs::read(path).unwrap(),
            );
        }
    }
}
