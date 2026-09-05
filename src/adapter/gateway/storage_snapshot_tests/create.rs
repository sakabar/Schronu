use crate::adapter::gateway::storage_lock::{LockMode, StorageLock};
use crate::adapter::gateway::storage_snapshot::{
    create_snapshot_after_parent_open, create_snapshot_at, create_snapshot_before_publish,
    create_snapshot_with_failure, create_snapshot_with_failure_observation, SnapshotFailurePoint,
};
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
    fs::write(project_directory.join("notes.tmp"), b"persistent-tmp").unwrap();
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

    assert_eq!(summary.file_count(), 5);
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
    assert_eq!(
        fs::read(payload.join(relative_project.parent().unwrap()).join("notes.tmp")).unwrap(),
        b"persistent-tmp"
    );
    assert!(payload
        .join(relative_project.parent().unwrap())
        .join("markdown/empty")
        .is_dir());
    assert!(!payload.join(".lock").exists());
    assert_eq!(fs::read(payload.join(".orphan.tmp")).unwrap(), b"temporary");
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
    assert_eq!(manifest.files.len(), 5);
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

#[test]
fn snapshot作成は検証後に差し替えられた親directoryへ書き込まない() {
    let root = TestDirectory::new("create-parent-swap");
    let storage = root.child("source");
    let parent = root.child("parent");
    let original_parent = root.child("original-parent");
    let destination = parent.join("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    create_saved_repository(&storage, now);
    fs::create_dir(&parent).unwrap();

    create_snapshot_after_parent_open(&storage, &destination, now, || {
        fs::rename(&parent, &original_parent).unwrap();
        fs::create_dir(&parent).unwrap();
        fs::write(parent.join("sentinel"), b"preserve").unwrap();
    })
    .unwrap();

    assert!(original_parent.join("snapshot/manifest.json").is_file());
    assert!(!parent.join("snapshot").exists());
    assert_eq!(fs::read(parent.join("sentinel")).unwrap(), b"preserve");
}

#[test]
fn snapshot作成はsource配下へ移動された親directoryを拒否する() {
    let root = TestDirectory::new("create-parent-moved-into-source");
    let storage = root.child("source");
    let parent = root.child("parent");
    let moved_parent = storage.join("moved-parent");
    let destination = parent.join("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    create_saved_repository(&storage, now);
    fs::create_dir(&parent).unwrap();

    create_snapshot_after_parent_open(&storage, &destination, now, || {
        fs::rename(&parent, &moved_parent).unwrap();
    })
    .unwrap_err();

    assert!(!moved_parent.join("snapshot").exists());
    assert!(fs::read_dir(&moved_parent).unwrap().next().is_none());
}

#[test]
fn snapshot作成は差し替えられたstaging_directoryを公開しない() {
    let root = TestDirectory::new("create-staging-swap");
    let storage = root.child("source");
    let destination = root.child("snapshot");
    let displaced = root.child("displaced-staging");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    create_saved_repository(&storage, now);

    create_snapshot_before_publish(&storage, &destination, now, || {
        let staging = fs::read_dir(&root.path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".snapshot.tmp-")
            })
            .unwrap();
        fs::rename(&staging, &displaced).unwrap();
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("foreign"), b"preserve").unwrap();
    })
    .unwrap_err();

    assert!(!destination.exists());
    assert!(displaced.join("manifest.json").is_file());
    let foreign = fs::read_dir(&root.path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.join("foreign").is_file())
        .unwrap();
    assert_eq!(fs::read(foreign.join("foreign")).unwrap(), b"preserve");
}

#[test]
fn snapshot作成失敗はdestinationもstagingも公開しない() {
    for point in [
        SnapshotFailurePoint::Read,
        SnapshotFailurePoint::Write,
        SnapshotFailurePoint::Permission,
        SnapshotFailurePoint::FileSync,
        SnapshotFailurePoint::DirectorySync,
        SnapshotFailurePoint::Rename,
        SnapshotFailurePoint::ParentSync,
    ] {
        let root = TestDirectory::new(&format!("create-atomic-{point:?}"));
        let storage = root.child("source");
        let destination = root.child("snapshot");
        let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
        create_saved_repository(&storage, now);

        create_snapshot_with_failure(&storage, &destination, now, point).unwrap_err();

        assert!(!destination.exists(), "{point:?}");
        assert!(
            fs::read_dir(&root.path).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".snapshot.tmp-")),
            "{point:?}"
        );
    }
}

#[test]
fn snapshot公開renameは競合destinationを置換しない() {
    let root = TestDirectory::new("create-no-replace-product-path");
    let storage = root.child("source");
    let destination = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    create_saved_repository(&storage, now);

    create_snapshot_before_publish(&storage, &destination, now, || {
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("sentinel"), b"preserve").unwrap();
    })
    .unwrap_err();

    assert_eq!(fs::read(destination.join("sentinel")).unwrap(), b"preserve");
    assert!(
        fs::read_dir(&root.path).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".snapshot.tmp-"))
    );
}

#[test]
fn snapshot公開後のparent_sync失敗はrollback後にparentを再syncする() {
    let root = TestDirectory::new("create-parent-resync");
    let storage = root.child("source");
    let destination = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    create_saved_repository(&storage, now);

    let (result, sync_count) = create_snapshot_with_failure_observation(
        &storage,
        &destination,
        now,
        SnapshotFailurePoint::ParentSync,
    );

    result.unwrap_err();
    assert!(!destination.exists());
    assert_eq!(sync_count, 2);
}
