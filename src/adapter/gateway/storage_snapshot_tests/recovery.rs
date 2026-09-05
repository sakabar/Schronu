use crate::adapter::gateway::storage_snapshot::create_snapshot_at;
use crate::adapter::gateway::storage_transaction::{
    prepare, FileSystemStorageTransactionIo, WriteRequest,
};
use chrono::TimeZone;
use std::sync::Arc;

#[test]
fn snapshotはmarkerなしtransactionを破棄して旧snapshotを収録する() {
    let root = TestDirectory::new("recovery-uncommitted");
    let storage = root.child("source");
    let destination = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let (_, project_yaml) = create_saved_repository(&storage, now);
    let old_project = fs::read(&project_yaml).unwrap();
    let old_revision = fs::read(storage.join(".revision")).unwrap();
    let staged_project = String::from_utf8(old_project.clone())
        .unwrap()
        .replace("snapshot-project", "uncommitted-project")
        .into_bytes();
    let prepared = prepare(
        Arc::new(FileSystemStorageTransactionIo),
        &storage,
        Uuid::new_v4(),
        &[WriteRequest {
            target_path: &project_yaml,
            bytes: &staged_project,
        }],
    )
    .unwrap();
    drop(prepared);
    let active = storage.join(".schronu-transactions/.active");
    assert!(active.is_dir());

    let summary = create_snapshot_at(&storage, &destination, now).unwrap();

    assert!(!active.exists());
    assert_eq!(fs::read(&project_yaml).unwrap(), old_project);
    assert_eq!(fs::read(storage.join(".revision")).unwrap(), old_revision);
    assert_eq!(
        fs::read(destination.join("storage").join(project_yaml.strip_prefix(&storage).unwrap()))
            .unwrap(),
        old_project
    );
    assert_eq!(
        fs::read(destination.join("storage/.revision")).unwrap(),
        old_revision
    );
    assert_eq!(
        summary.revision(),
        Some(Uuid::parse_str(String::from_utf8(old_revision).unwrap().trim()).unwrap())
    );
}

#[test]
fn snapshotはmarker済みtransactionをroll_forwardして新snapshotを収録する() {
    let root = TestDirectory::new("recovery-committed");
    let storage = root.child("source");
    let destination = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let (_, project_yaml) = create_saved_repository(&storage, now);
    let new_project = fs::read_to_string(&project_yaml)
        .unwrap()
        .replace("snapshot-project", "recovered-project")
        .into_bytes();
    let new_revision = Uuid::new_v4();
    let io = Arc::new(RecordingIo::new(vec![FaultRule {
        operation: RecordingOperation::Rename,
        path_matcher: PathMatcher::FileName("project.yaml"),
        occurrence: 1,
        error_kind: std::io::ErrorKind::Other,
        error_message: "injected interruption after commit marker",
    }]));
    let prepared = prepare(
        io,
        &storage,
        new_revision,
        &[WriteRequest {
            target_path: &project_yaml,
            bytes: &new_project,
        }],
    )
    .unwrap();
    prepared.commit().unwrap_err();
    let active = storage.join(".schronu-transactions/.active");
    assert!(active.join("commit").is_file());

    let summary = create_snapshot_at(&storage, &destination, now).unwrap();

    assert!(!active.exists());
    assert_eq!(summary.revision(), Some(new_revision));
    assert_eq!(fs::read(&project_yaml).unwrap(), new_project);
    assert_eq!(
        fs::read(destination.join("storage").join(project_yaml.strip_prefix(&storage).unwrap()))
            .unwrap(),
        new_project
    );
    assert_eq!(
        fs::read_to_string(destination.join("storage/.revision")).unwrap(),
        format!("{new_revision}\n")
    );
}

#[test]
fn snapshotはrecovery後の不正yamlをstrict_loadで拒否して公開しない() {
    let root = TestDirectory::new("recovery-strict-load");
    let storage = root.child("source");
    let destination = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let (_, project_yaml) = create_saved_repository(&storage, now);
    let invalid_project = b"project: [";
    let new_revision = Uuid::new_v4();
    let io = Arc::new(RecordingIo::new(vec![FaultRule {
        operation: RecordingOperation::Rename,
        path_matcher: PathMatcher::FileName("project.yaml"),
        occurrence: 1,
        error_kind: std::io::ErrorKind::Other,
        error_message: "injected interruption after commit marker",
    }]));
    let prepared = prepare(
        io,
        &storage,
        new_revision,
        &[WriteRequest {
            target_path: &project_yaml,
            bytes: invalid_project,
        }],
    )
    .unwrap();
    prepared.commit().unwrap_err();

    let error = create_snapshot_at(&storage, &destination, now).unwrap_err();

    assert_eq!(error.path(), storage);
    assert!(!destination.exists());
    assert_eq!(fs::read(&project_yaml).unwrap(), invalid_project);
    assert_eq!(
        fs::read_to_string(storage.join(".revision")).unwrap(),
        format!("{new_revision}\n")
    );
    assert!(!storage.join(".schronu-transactions/.active").exists());
}
