use chrono::{Local, Timelike};
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;
use schronu::entity::task::TaskHandle;
use std::collections::BTreeMap;
use std::fs;
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
            "schronu-cli-runtime-contract-{}",
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
        let task = TaskHandle::with_identity("CLI error終了対象", Uuid::new_v4(), now).unwrap();
        task.set_estimated_work_seconds(45 * 60).unwrap();
        task.set_actual_work_seconds(15 * 60).unwrap();
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

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_schronu"))
            .args(args)
            .env("SCHRONU_STORAGE_DIR", &self.storage)
            .env("SCHRONU_CONFIG_PATH", &self.config)
            .output()
            .unwrap()
    }

    fn persistent_storage_bytes_excluding_process_lock(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let files = collect_persistent_storage_bytes_excluding_process_lock(&self.storage);
        assert!(files.contains_key(Path::new(".revision")));
        assert!(files.keys().any(|path| path.ends_with("project.yaml")));
        files
    }
}

impl Drop for CliFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn cli_processのfinish時刻errorは終了code1で永続dataを変更しない() {
    let fixture = CliFixture::seeded();
    let storage_before = fixture.persistent_storage_bytes_excluding_process_lock();

    let output = fixture.run(&["終", "invalid"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "[Error] 入力エラー: finished_at: 日時が不正です (コマンド: 終, 使い方: 終 [今|HH:MM[:SS] [日付]])\n"
    );
    assert_eq!(
        fixture.persistent_storage_bytes_excluding_process_lock(),
        storage_before
    );
}

#[test]
fn cli_processの余分なargumentはcanonical診断と終了code1で永続dataを変更しない() {
    let fixture = CliFixture::seeded();
    let storage_before = fixture.persistent_storage_bytes_excluding_process_lock();

    let output = fixture.run(&["flatten", "extra"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "[Error] 入力エラー: arguments: 引数の個数が正しくありません (コマンド: 平, 使い方: 平)\n"
    );
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

fn collect_persistent_storage_bytes_excluding_process_lock(
    storage: &Path,
) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect_directory_bytes(storage, storage, &mut files);
    files
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
        } else if file_type.is_file() && entry.file_name() != ".lock" {
            files.insert(
                path.strip_prefix(storage).unwrap().to_path_buf(),
                fs::read(path).unwrap(),
            );
        }
    }
}
