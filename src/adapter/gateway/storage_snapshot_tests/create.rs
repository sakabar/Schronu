use crate::adapter::gateway::storage_lock::{LockMode, StorageLock};
use crate::adapter::gateway::storage_snapshot::{
    create_snapshot_after_capture, create_snapshot_after_parent_open, create_snapshot_at, create_snapshot_before_publish,
    create_snapshot_with_failure, create_snapshot_with_failure_observation,
    create_snapshot_with_limits, SnapshotFailurePoint, SnapshotLimitKind,
    SnapshotResourceLimits,
};
use chrono::TimeZone;
use std::error::Error;
use walkdir::WalkDir;

#[test]
fn snapshot作成resource_limitは境界を許可し超過をtyped拒否する() {
    let root = TestDirectory::new("create-resource-limits");
    let storage = root.child("source");
    let baseline = root.child("baseline");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let (_, project_yaml) = create_saved_repository(&storage, now);
    fs::remove_file(storage.join(".revision")).unwrap();
    fs::remove_dir_all(project_yaml.parent().unwrap().join("markdown")).unwrap();
    let relative_project = project_yaml.strip_prefix(&storage).unwrap();
    create_snapshot_at(&storage, &baseline, now).unwrap();

    let manifest_bytes = fs::metadata(baseline.join("manifest.json")).unwrap().len();
    let files = WalkDir::new(&storage)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && entry.file_name() != ".lock")
        .collect::<Vec<_>>();
    let file_count = files.len();
    let file_bytes = files
        .iter()
        .map(|entry| entry.metadata().unwrap().len())
        .max()
        .unwrap();
    let total_bytes = files
        .iter()
        .map(|entry| entry.metadata().unwrap().len())
        .sum::<u64>();
    let paths = files
        .iter()
        .map(|entry| entry.path().strip_prefix(&storage).unwrap())
        .collect::<Vec<_>>();
    let path_bytes = paths
        .iter()
        .map(|path| path.to_str().unwrap().len())
        .max()
        .unwrap();
    let depth = paths
        .iter()
        .map(|path| path.components().count())
        .max()
        .unwrap();
    let exact = SnapshotResourceLimits::new(
        manifest_bytes,
        file_count,
        file_bytes,
        total_bytes,
        path_bytes,
        depth,
    );

    create_snapshot_with_limits(&storage, &root.child("exact"), now, exact).unwrap();

    for (name, limits, expected) in [
        (
            "manifest",
            exact.with_manifest_bytes(manifest_bytes - 1),
            SnapshotLimitKind::ManifestBytes,
        ),
        (
            "file-count",
            exact.with_file_count(file_count - 1),
            SnapshotLimitKind::FileCount,
        ),
        (
            "file-bytes",
            exact.with_file_bytes(file_bytes - 1),
            SnapshotLimitKind::FileBytes,
        ),
        (
            "total-bytes",
            exact.with_total_bytes(total_bytes - 1),
            SnapshotLimitKind::PayloadBytes,
        ),
        (
            "path-bytes",
            exact.with_path_bytes(path_bytes - 1),
            SnapshotLimitKind::PathBytes,
        ),
        (
            "depth",
            exact.with_depth(depth - 1),
            SnapshotLimitKind::PathDepth,
        ),
    ] {
        let error = create_snapshot_with_limits(&storage, &root.child(name), now, limits)
            .unwrap_err();
        assert_eq!(error.limit_kind(), Some(expected), "{name}: {error}");
        assert_eq!(
            error.observed_value(),
            error.limit_value().and_then(|limit| limit.checked_add(1)),
            "{name}: {error}"
        );
        if expected == SnapshotLimitKind::ManifestBytes {
            assert_eq!(error.limit_path(), None, "{name}: {error}");
            assert_eq!(error.path().file_name().unwrap(), "manifest.json", "{name}: {error}");
            let staging = error.path().parent().unwrap();
            assert_eq!(staging.parent(), Some(root.path.as_path()), "{name}: {error}");
            let staging_name = staging.file_name().unwrap().to_str().unwrap();
            let staging_id = staging_name.strip_prefix(".manifest.tmp-").unwrap();
            assert_eq!(Uuid::parse_str(staging_id).unwrap().hyphenated().to_string(), staging_id);
        } else {
            let expected_path = relative_project.to_path_buf();
            assert_eq!(error.path(), storage.join(&expected_path), "{name}: {error}");
            assert_eq!(error.limit_path(), Some(expected_path.as_path()), "{name}: {error}");
        }
    }
}

#[test]
fn snapshot作成はcapture後の非協調file差し替えを公開しない() {
    let root = TestDirectory::new("create-capture-swap");
    let storage = root.child("source");
    let destination = root.child("snapshot");
    let displaced = root.child("captured-project.yaml");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let (_, project_yaml) = create_saved_repository(&storage, now);
    let replacement = fs::read_to_string(&project_yaml)
        .unwrap()
        .replace("snapshot-project", "replacement-project");

    create_snapshot_after_capture(&storage, &destination, now, || {
        fs::rename(&project_yaml, &displaced).unwrap();
        fs::write(&project_yaml, replacement).unwrap();
    })
    .unwrap_err();

    assert!(!destination.exists());
}

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

    let error = create_snapshot_before_publish(&storage, &destination, now, || {
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
    let mut source_chain = Vec::new();
    let mut source = error.source();
    while let Some(current) = source {
        source_chain.push(current.to_string());
        source = current.source();
    }
    let display = error.to_string();

    assert_eq!(error.path(), destination);
    assert!(display.contains("staging directory was replaced before publication"));
    assert!(display.contains("cleanup failed"));
    assert!(display.contains("published destination was replaced before rollback"));
    assert!(source_chain
        .iter()
        .any(|source| source.contains("published destination was replaced before rollback")));
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

        let error = create_snapshot_with_failure(&storage, &destination, now, point)
            .unwrap_err()
            .to_string();
        let expected_operation = match point {
            SnapshotFailurePoint::FileSync
            | SnapshotFailurePoint::DirectorySync
            | SnapshotFailurePoint::ParentSync => "Sync",
            SnapshotFailurePoint::Read => "Read",
            SnapshotFailurePoint::Write
            | SnapshotFailurePoint::Permission
            | SnapshotFailurePoint::Rename => "Write",
            SnapshotFailurePoint::Copy => unreachable!(),
        };

        assert!(
            error.contains(&format!("snapshot {expected_operation} failed")),
            "{error}"
        );
        assert!(error.contains(&format!("injected {point:?} failure")), "{error}");
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

    let error = result.unwrap_err().to_string();
    assert!(error.contains("injected ParentSync failure"), "{error}");
    assert!(!error.contains("cleanup failed"), "{error}");
    assert!(!destination.exists());
    assert_eq!(sync_count, 2);
}
