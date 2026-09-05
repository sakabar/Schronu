use crate::adapter::gateway::storage_snapshot::create_snapshot_at;
use crate::adapter::gateway::storage_lock::{LockMode, StorageLock};
use chrono::TimeZone;

#[test]
fn snapshotはlock下の全永続dataとpermissionを保存して予約領域を除外する() {
    let root = TestDirectory::new("create");
    let storage = root.child("source");
    let destination = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let (_, project_yaml) = create_saved_repository(&storage, now);
    let project_directory = project_yaml.parent().unwrap();
    fs::write(project_directory.join("notes.bin"), b"persistent-note").unwrap();
    fs::write(
        project_directory.join(format!(".project.yaml.{}.tmp", Uuid::new_v4())),
        b"live-temporary",
    )
    .unwrap();
    fs::create_dir_all(project_directory.join("markdown/empty")).unwrap();
    fs::write(storage.join(".orphan.tmp"), b"temporary").unwrap();
    fs::create_dir_all(storage.join(".schronu-transactions/unused")).unwrap();
    fs::write(
        storage.join(".schronu-transactions/unused/material"),
        b"staging",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&project_yaml, fs::Permissions::from_mode(0o640)).unwrap();
    }

    let summary = create_snapshot_at(&storage, &destination, now).unwrap();

    assert_eq!(summary.file_count(), 3);
    assert!(summary.revision().is_some());
    let payload = destination.join("storage");
    let relative_project = project_yaml.strip_prefix(&storage).unwrap();
    assert_eq!(
        fs::read(payload.join(relative_project)).unwrap(),
        fs::read(&project_yaml).unwrap()
    );
    assert_eq!(
        fs::read(payload.join(relative_project.parent().unwrap()).join("notes.bin")).unwrap(),
        b"persistent-note"
    );
    assert!(payload
        .join(relative_project.parent().unwrap())
        .join("markdown/empty")
        .is_dir());
    assert!(!payload.join(".lock").exists());
    assert!(!payload.join(".orphan.tmp").exists());
    assert!(!payload.join(".schronu-transactions").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(payload.join(relative_project))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
    let manifest = decode_manifest(
        &destination.join("manifest.json"),
        &fs::read(destination.join("manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.revision, summary.revision());
    assert_eq!(manifest.tool_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest.files.len(), 3);
}

#[test]
fn snapshotはsave用lockとの競合中にstorageを読まない() {
    let root = TestDirectory::new("create-lock-contention");
    let storage = root.child("source");
    let destination = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    create_saved_repository(&storage, now);
    let save_lock = StorageLock::acquire(&storage, LockMode::Cli).unwrap();

    let error = create_snapshot_at(&storage, &destination, now).unwrap_err();

    assert_eq!(error.path(), storage.join(".lock"));
    assert!(!destination.exists());
    drop(save_lock);

    let summary = create_snapshot_at(&storage, &destination, now).unwrap();
    assert_eq!(summary.file_count(), 2);
}
