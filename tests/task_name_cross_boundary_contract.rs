#![cfg(unix)]

use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;
use schronu::application::task_use_case::{validate_task_name, ApplicationError};
use std::error::Error;
use std::process::Output;

#[path = "task_name_contract_support/cli.rs"]
mod cli_support;
#[path = "task_name_contract_support/spreadsheet.rs"]
mod spreadsheet_support;
#[path = "task_name_contract_support/yaml.rs"]
mod yaml_support;

use cli_support::CliFixture;
use spreadsheet_support::{run_generator, spreadsheet_row};
use yaml_support::{error_chain, TestStorageDir};

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

#[test]
fn 許可task名は全公開境界で原文を保持する() {
    assert_eq!(validate_task_name(ALLOWED_NAME, "name"), Ok(()));

    let cli = CliFixture::new(false);
    let output = cli.run(&["新", ALLOWED_NAME]);
    assert_success(&output, "non-interactive CLI");
    assert_eq!(cli.stored_project_names(), vec![ALLOWED_NAME]);

    let yaml = TestStorageDir::new();
    yaml.write_project(&format!(
        "project:\n  name: \"{}\"\n",
        yaml_escape(ALLOWED_NAME)
    ));
    assert_eq!(loaded_yaml_names(&yaml).unwrap(), vec![ALLOWED_NAME]);

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

        let cli = CliFixture::new(false);
        let output = cli.run(&["新", case.name]);
        assert_eq!(output.status.code(), Some(1), "CLI name={:?}", case.name);
        assert!(output.stdout.is_empty(), "CLI name={:?}", case.name);
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains(&format!("invalid input for name: {}", case.reason)),
            "CLI name={:?}, stderr={}",
            case.name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            cli.stored_project_names().is_empty(),
            "CLI name={:?}",
            case.name
        );

        let yaml = TestStorageDir::new();
        let project_yaml = yaml.write_project(&format!(
            "project:\n  name: \"{}\"\n",
            yaml_escape(case.name)
        ));
        let yaml_error = loaded_yaml_names(&yaml).unwrap_err();
        let diagnostic = error_chain(yaml_error.as_ref());
        assert!(
            diagnostic.contains(project_yaml.to_str().unwrap()),
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

fn assert_success(output: &Output, boundary: &str) {
    assert!(
        output.status.success(),
        "{boundary} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn loaded_yaml_names(storage: &TestStorageDir) -> Result<Vec<String>, Box<dyn Error>> {
    let mut repository = TaskRepository::new(storage.path.to_str().unwrap());
    repository.load()?;
    repository
        .get_all_projects()
        .into_iter()
        .map(|task| task.get_name().map_err(|error| error.into()))
        .collect()
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
