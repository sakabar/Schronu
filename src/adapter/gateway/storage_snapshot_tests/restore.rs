use crate::adapter::gateway::storage_snapshot::{
    create_snapshot_at, restore_snapshot, restore_snapshot_after_parent_open,
    restore_snapshot_with_failure, SnapshotFailurePoint,
};
use chrono::TimeZone;

#[test]
fn snapshot_restoreはsource非依存で別directoryへ全永続dataを復元する() {
    let root = TestDirectory::new("restore");
    let source = root.child("source");
    let snapshot = root.child("snapshot");
    let destination = root.child("restored");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let (_, project_yaml) = create_saved_repository(&source, now);
    let relative_project = project_yaml.strip_prefix(&source).unwrap().to_path_buf();
    fs::write(project_yaml.parent().unwrap().join("notes.bin"), b"note").unwrap();
    let empty_directory = project_yaml.parent().unwrap().join("markdown/empty");
    fs::create_dir_all(&empty_directory).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&project_yaml, fs::Permissions::from_mode(0o640)).unwrap();
        fs::set_permissions(&empty_directory, fs::Permissions::from_mode(0o710)).unwrap();
        fs::set_permissions(source.join(".revision"), fs::Permissions::from_mode(0o600)).unwrap();
    }
    let expected_revision_bytes = fs::read(source.join(".revision")).unwrap();
    let expected_revision = create_snapshot_at(&source, &snapshot, now)
        .unwrap()
        .revision();
    fs::remove_dir_all(&source).unwrap();

    let summary = restore_snapshot(&snapshot, &destination).unwrap();

    assert_eq!(summary.revision(), expected_revision);
    assert_eq!(summary.file_count(), 3);
    assert!(destination.join(&relative_project).is_file());
    assert_eq!(
        fs::read(destination.join(relative_project.parent().unwrap()).join("notes.bin")).unwrap(),
        b"note"
    );
    assert!(destination
        .join(relative_project.parent().unwrap())
        .join("markdown/empty")
        .is_dir());
    assert_eq!(
        fs::read(destination.join(".revision")).unwrap(),
        expected_revision_bytes
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(destination.join(&relative_project))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
        assert_eq!(
            fs::metadata(
                destination
                    .join(relative_project.parent().unwrap())
                    .join("markdown/empty")
            )
            .unwrap()
            .permissions()
            .mode()
                & 0o777,
            0o710
        );
        assert_eq!(
            fs::metadata(destination.join(".revision"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let mut repository = TaskRepository::new(destination.to_str().unwrap());
    repository.load().unwrap();
}

#[test]
fn snapshot_restoreは既存destinationを変更せず拒否する() {
    let root = TestDirectory::new("restore-existing");
    let source = root.child("source");
    let snapshot = root.child("snapshot");
    let destination = root.child("restored");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    create_saved_repository(&source, now);
    create_snapshot_at(&source, &snapshot, now).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("existing"), b"preserve").unwrap();

    restore_snapshot(&snapshot, &destination).unwrap_err();

    assert_eq!(fs::read(destination.join("existing")).unwrap(), b"preserve");
}

#[test]
fn snapshot_restoreは検証後に差し替えられた親directoryへ書き込まない() {
    let root = TestDirectory::new("restore-parent-swap");
    let source = root.child("source");
    let snapshot = root.child("snapshot");
    let parent = root.child("parent");
    let original_parent = root.child("original-parent");
    let destination = parent.join("restored");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    create_saved_repository(&source, now);
    create_snapshot_at(&source, &snapshot, now).unwrap();
    fs::create_dir(&parent).unwrap();

    restore_snapshot_after_parent_open(&snapshot, &destination, || {
        fs::rename(&parent, &original_parent).unwrap();
        fs::create_dir(&parent).unwrap();
        fs::write(parent.join("sentinel"), b"preserve").unwrap();
    })
    .unwrap();

    assert!(original_parent.join("restored/.revision").is_file());
    assert!(!parent.join("restored").exists());
    assert_eq!(fs::read(parent.join("sentinel")).unwrap(), b"preserve");
}

#[test]
fn snapshot_restore失敗はdestinationもstagingも公開しない() {
    for point in [
        SnapshotFailurePoint::StrictValidation,
        SnapshotFailurePoint::Copy,
        SnapshotFailurePoint::Permission,
        SnapshotFailurePoint::FileSync,
        SnapshotFailurePoint::DirectorySync,
        SnapshotFailurePoint::Rename,
        SnapshotFailurePoint::ParentSync,
    ] {
        let root = TestDirectory::new(&format!("restore-atomic-{point:?}"));
        let source = root.child("source");
        let snapshot = root.child("snapshot");
        let destination = root.child("restored");
        let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
        create_saved_repository(&source, now);
        create_snapshot_at(&source, &snapshot, now).unwrap();

        restore_snapshot_with_failure(&snapshot, &destination, point).unwrap_err();

        assert!(!destination.exists(), "{point:?}");
        assert!(
            fs::read_dir(&root.path).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".restored.tmp-")),
            "{point:?}"
        );
    }
}
