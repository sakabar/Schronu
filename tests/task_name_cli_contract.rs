use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[path = "task_name_contract_support/cli.rs"]
mod cli_support;

use cli_support::CliFixture;

#[test]
fn 非対話cliはosのtask名argvを原文のまま保存する() {
    let task_names = [
        "内部  連続 空白 日本語 'single' \"double\" C:\\temp",
        "  前後 空白 日本語 '引用' \"二重\" C:\\path  ",
    ];

    for task_name in task_names {
        let fixture = CliFixture::new(true);

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
fn 非対話cliは単一argvのcommand全文を暗黙分割しない() {
    let fixture = CliFixture::new(true);
    let storage_before = persistent_storage_bytes_excluding_process_lock(&fixture);

    let output = fixture.run(&["新 legacy-name"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        persistent_storage_bytes_excluding_process_lock(&fixture),
        storage_before
    );
}

#[test]
fn 非対話cliはcontrol名を入力errorにしてstorageを変更しない() {
    for task_name in ["ESC\u{1b}name", "tab\tname"] {
        let fixture = CliFixture::new(true);
        let storage_before = persistent_storage_bytes_excluding_process_lock(&fixture);

        let output = fixture.run(&["新", task_name]);

        assert_eq!(output.status.code(), Some(1), "task_name={task_name:?}");
        assert!(output.stdout.is_empty(), "task_name={task_name:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(
            stderr,
            "[Error] 操作エラー: invalid input for name: must not contain control characters\n",
            "task_name={task_name:?}"
        );
        assert_eq!(
            persistent_storage_bytes_excluding_process_lock(&fixture),
            storage_before,
            "task_name={task_name:?}"
        );
    }
}

#[test]
fn 非対話cliのnul名はos境界で拒否されstorageを変更しない() {
    let fixture = CliFixture::new(true);
    let storage_before = persistent_storage_bytes_excluding_process_lock(&fixture);

    let error = fixture.command(&["新", "NUL\0name"]).output().unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        persistent_storage_bytes_excluding_process_lock(&fixture),
        storage_before
    );
}

fn persistent_storage_bytes_excluding_process_lock(
    fixture: &CliFixture,
) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect_directory_bytes(&fixture.storage, &fixture.storage, &mut files);
    files.remove(Path::new(".lock"));
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
        } else if file_type.is_file() {
            files.insert(
                path.strip_prefix(storage).unwrap().to_path_buf(),
                fs::read(path).unwrap(),
            );
        }
    }
}
