use chrono::{Local, Timelike};
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;
use schronu::entity::task::TaskHandle;
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
            "schronu-storage-backup-cli-contract-{}",
            Uuid::new_v4().hyphenated()
        ));
        let storage = root.join("storage");
        fs::create_dir_all(&storage).unwrap();
        let config = root.join("schronu.yaml");
        fs::write(&config, "busy_time_slots_yaml_path: unused.yaml\n").unwrap();

        let now = Local::now().with_nanosecond(0).unwrap();
        let task = TaskHandle::with_identity("backup CLI対象", Uuid::new_v4(), now).unwrap();
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

    fn child(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for CliFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn backup_cliは整合snapshotを作成してsummaryをstdoutへ表示する() {
    let fixture = CliFixture::seeded();
    let snapshot = fixture.child("snapshot");

    let output = fixture.run(&["backup", snapshot.to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert!(snapshot.join("manifest.json").is_file());
    assert!(snapshot.join("storage/.revision").is_file());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .starts_with(&format!("backup: OK {} revision=", snapshot.display())));
}

#[test]
fn backup_cliは引数不足と余分な引数をusage付きで拒否する() {
    let fixture = CliFixture::seeded();
    let snapshot = fixture.child("snapshot");

    for args in [
        vec!["backup"],
        vec!["backup", snapshot.to_str().unwrap(), "extra"],
    ] {
        let output = fixture.run(&args);

        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("コマンド: backup"), "{stderr}");
        assert!(stderr.contains("使い方: backup <snapshot_dir>"), "{stderr}");
        assert!(!snapshot.exists());
    }
}

#[test]
fn backup_cliはsnapshot_errorのpathと段階と原因をstderrへ保持する() {
    let fixture = CliFixture::seeded();
    let snapshot = fixture.child("existing");
    fs::create_dir(&snapshot).unwrap();

    let output = fixture.run(&["backup", snapshot.to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("storage snapshot Validate failed"),
        "{stderr}"
    );
    assert!(stderr.contains(snapshot.to_str().unwrap()), "{stderr}");
    assert!(
        stderr.contains("snapshot destination must not exist"),
        "{stderr}"
    );
    assert!(Path::new(&snapshot).is_dir());
}

#[test]
fn backup_cliは不正current_storageをsnapshot_repository_load_errorとして返す() {
    let fixture = CliFixture::seeded();
    let snapshot = fixture.child("snapshot");
    let project_yaml = find_project_yaml(&fixture.storage);
    fs::write(&project_yaml, "project: [").unwrap();

    let output = fixture.run(&["backup", snapshot.to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("storage snapshot RepositoryLoad failed"),
        "{stderr}"
    );
    assert!(
        stderr.contains(fixture.storage.to_str().unwrap()),
        "{stderr}"
    );
    assert!(stderr.contains(project_yaml.to_str().unwrap()), "{stderr}");
    assert!(stderr.contains("while parsing a node"), "{stderr}");
    assert!(!snapshot.exists());
}

#[test]
fn backup_verify_cliはcurrent_storageへ依存せずsnapshotを検証する() {
    let fixture = CliFixture::seeded();
    let snapshot = fixture.child("snapshot");
    let backup = fixture.run(&["backup", snapshot.to_str().unwrap()]);
    assert_eq!(backup.status.code(), Some(0));
    fs::remove_dir_all(&fixture.storage).unwrap();

    let output = fixture.run(&["backup", "verify", snapshot.to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .starts_with(&format!("backup verify: OK {} revision=", snapshot.display())));
}

#[test]
fn backup_verify_cliは引数不足と余分な引数をusage付きで拒否する() {
    let fixture = CliFixture::seeded();
    let snapshot = fixture.child("snapshot");

    for args in [
        vec!["backup", "verify"],
        vec!["backup", "verify", snapshot.to_str().unwrap(), "extra"],
    ] {
        let output = fixture.run(&args);

        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("コマンド: backup verify"), "{stderr}");
        assert!(
            stderr.contains("使い方: backup verify <snapshot_dir>"),
            "{stderr}"
        );
    }
}

#[test]
fn backup_verify_cliはsnapshot_errorのpathと段階と原因をstderrへ保持する() {
    let fixture = CliFixture::seeded();
    let snapshot = fixture.child("snapshot");
    let backup = fixture.run(&["backup", snapshot.to_str().unwrap()]);
    assert_eq!(backup.status.code(), Some(0));
    let manifest = snapshot.join("manifest.json");
    fs::write(&manifest, "{").unwrap();

    let output = fixture.run(&["backup", "verify", snapshot.to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("storage snapshot DecodeManifest failed"),
        "{stderr}"
    );
    assert!(stderr.contains(manifest.to_str().unwrap()), "{stderr}");
    assert!(stderr.contains("EOF while parsing"), "{stderr}");
}

fn find_project_yaml(storage: &Path) -> PathBuf {
    fs::read_dir(storage)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("project.yaml"))
        .find(|path| path.is_file())
        .unwrap()
}
