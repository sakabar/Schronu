#![cfg(unix)]

use chrono::{Local, Timelike};
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;
use schronu::application::task_use_case::{validate_task_name, ApplicationError};
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use uuid::Uuid;

const COLUMN_COUNT: usize = 19;
const ALLOWED_NAME: &str = "  日本語  'single' \"double\" C:\\path  ";

struct InvalidCase {
    name: &'static str,
    reason: &'static str,
    spreadsheet_diagnostic: &'static str,
}

const INVALID_CASES: &[InvalidCase] = &[
    InvalidCase {
        name: " \u{3000} ",
        reason: "must not be blank",
        spreadsheet_diagnostic: "J列のtask名が空です",
    },
    InvalidCase {
        name: " -42 ",
        reason: "must not be an integer-only name",
        spreadsheet_diagnostic: "J列のtask名に整数だけは指定できません",
    },
    InvalidCase {
        name: "tab\tname",
        reason: "must not contain control characters",
        spreadsheet_diagnostic: "列数が不正です: 20列",
    },
    InvalidCase {
        name: "line\nname",
        reason: "must not contain control characters",
        spreadsheet_diagnostic: "列数が不正です: 10列",
    },
    InvalidCase {
        name: "escape\u{1b}name",
        reason: "must not contain control characters",
        spreadsheet_diagnostic: "J列のtask名にcontrol characterが含まれています",
    },
    InvalidCase {
        name: "c1\u{85}name",
        reason: "must not contain control characters",
        spreadsheet_diagnostic: "J列のtask名にcontrol characterが含まれています",
    },
];

struct ProductFixture {
    root: PathBuf,
    storage: PathBuf,
    config: PathBuf,
}

impl ProductFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "schronu-task-name-cross-boundary-{}",
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
        Self {
            root,
            storage,
            config,
        }
    }

    fn cli(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_schronu"));
        command
            .env("SCHRONU_STORAGE_DIR", &self.storage)
            .env("SCHRONU_CONFIG_PATH", &self.config);
        command
    }

    fn run_cli(&self, args: &[&str]) -> Output {
        self.cli().args(args).output().unwrap()
    }

    fn stored_names(&self) -> Vec<String> {
        let mut repository = TaskRepository::new(self.storage.to_str().unwrap());
        repository
            .sync_clock(Local::now().with_nanosecond(0).unwrap())
            .unwrap();
        repository.load().unwrap();
        repository
            .get_all_projects()
            .into_iter()
            .map(|task| task.get_name().unwrap())
            .collect()
    }
}

impl Drop for ProductFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn 許可task名は全公開境界で原文を保持する() {
    assert_eq!(validate_task_name(ALLOWED_NAME, "name"), Ok(()));

    let cli = ProductFixture::new();
    let output = cli.run_cli(&["新", ALLOWED_NAME]);
    assert_success(&output, "non-interactive CLI");
    assert_eq!(cli.stored_names(), vec![ALLOWED_NAME]);

    let yaml = YamlFixture::with_name(ALLOWED_NAME);
    assert_eq!(yaml.loaded_names().unwrap(), vec![ALLOWED_NAME]);

    let generated = run_generator(format!("{}\n", spreadsheet_row(ALLOWED_NAME)).as_bytes());
    assert_success(&generated, "Spreadsheet generator");
    let first_command = generated
        .stdout
        .split_inclusive(|byte| *byte == b'\n')
        .next()
        .expect("generator emits a create command");
    assert_eq!(
        first_command,
        concat!(r#"新 "  日本語  'single' \"double\" C:\\path  ""#, "\n").as_bytes()
    );
}

#[test]
fn 不正task名は全公開境界で同じ分類として拒否される() {
    for case in INVALID_CASES {
        assert_eq!(
            validate_task_name(case.name, "name"),
            Err(ApplicationError::InvalidInput {
                field: "name",
                reason: case.reason,
            }),
            "application name={:?}",
            case.name
        );

        let cli = ProductFixture::new();
        let output = cli.run_cli(&["新", case.name]);
        assert_eq!(output.status.code(), Some(1), "CLI name={:?}", case.name);
        assert!(output.stdout.is_empty(), "CLI name={:?}", case.name);
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains(&format!("invalid input for name: {}", case.reason)),
            "CLI name={:?}, stderr={}",
            case.name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(cli.stored_names().is_empty(), "CLI name={:?}", case.name);

        let yaml = YamlFixture::with_name(case.name);
        let yaml_error = yaml.loaded_names().unwrap_err();
        let diagnostic = error_chain(yaml_error.as_ref());
        assert!(
            diagnostic.contains(yaml.project_yaml.to_str().unwrap()),
            "YAML name={:?}, diagnostic={diagnostic}",
            case.name
        );
        assert!(
            diagnostic.contains(&format!("project.name: {}", case.reason)),
            "YAML name={:?}, diagnostic={diagnostic}",
            case.name
        );

        let spreadsheet = run_generator(format!("{}\n", spreadsheet_row(case.name)).as_bytes());
        assert!(
            !spreadsheet.status.success(),
            "Spreadsheet name={:?}",
            case.name
        );
        assert!(
            spreadsheet.stdout.is_empty(),
            "Spreadsheet name={:?}",
            case.name
        );
        let stderr = String::from_utf8_lossy(&spreadsheet.stderr);
        assert!(
            stderr.contains("line 1:") && stderr.contains(case.spreadsheet_diagnostic),
            "Spreadsheet name={:?}, stderr={stderr}",
            case.name
        );
    }
}

struct YamlFixture {
    root: PathBuf,
    project_yaml: PathBuf,
}

impl YamlFixture {
    fn with_name(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "schronu-task-name-cross-boundary-yaml-{}",
            Uuid::new_v4().hyphenated()
        ));
        let project_dir = root.join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let project_yaml = project_dir.join("project.yaml");
        fs::write(
            &project_yaml,
            format!("project:\n  name: \"{}\"\n", yaml_escape(name)),
        )
        .unwrap();
        Self { root, project_yaml }
    }

    fn loaded_names(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut repository = TaskRepository::new(self.root.to_str().unwrap());
        repository.load()?;
        repository
            .get_all_projects()
            .into_iter()
            .map(|task| task.get_name().map_err(|error| error.into()))
            .collect()
    }
}

impl Drop for YamlFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn yaml_escape(value: &str) -> String {
    value.chars().fold(String::new(), |mut escaped, character| {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04X}", character as u32));
            }
            character => escaped.push(character),
        }
        escaped
    })
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

fn spreadsheet_row(task_name: &str) -> String {
    let mut columns = vec![""; COLUMN_COUNT];
    columns[9] = task_name;
    columns[18] = "0:00:00";
    columns.join("\t")
}

fn run_generator(input: &[u8]) -> Output {
    let mut child = Command::new("zsh")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("shell/generate_command_from_spreadsheet.sh"),
        )
        .arg("--stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn assert_success(output: &Output, boundary: &str) {
    assert!(
        output.status.success(),
        "{boundary} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn error_chain(error: &(dyn Error + 'static)) -> String {
    let mut messages = Vec::new();
    let mut current = Some(error);
    while let Some(source) = current {
        messages.push(source.to_string());
        current = source.source();
    }
    messages.join("\ncaused by: ")
}
