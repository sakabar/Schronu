use crate::adapter::gateway::storage_lock::{LockMode, StorageLock};
use crate::adapter::gateway::storage_snapshot::{
    create_snapshot_at, restore_current_snapshot_at, verify_snapshot,
};
use chrono::TimeZone;

#[test]
fn current_restoreはpre_backupを公開してsnapshotをfresh_revisionで置換する() {
    let root = TestDirectory::new("current-restore");
    let current = root.child("current");
    let source = root.child("source");
    let snapshot = root.child("snapshot");
    let pre_backup = root.child("pre-backup");
    let now = Local.with_ymd_and_hms(2026, 9, 6, 12, 0, 0).unwrap();
    let (_, current_project) = create_saved_repository(&current, now);
    let old_revision = fs::read_to_string(current.join(".revision")).unwrap();
    fs::write(current_project.parent().unwrap().join("obsolete.bin"), b"old").unwrap();
    fs::create_dir_all(current.join("obsolete/empty")).unwrap();
    let (_, source_project) = create_saved_repository(&source, now);
    fs::write(source_project.parent().unwrap().join("restored.bin"), b"new").unwrap();
    let snapshot_revision = create_snapshot_at(&source, &snapshot, now)
        .unwrap()
        .revision()
        .unwrap();
    let storage_lock = StorageLock::acquire(&current, LockMode::Cli).unwrap();

    let summary = restore_current_snapshot_at(
        &current,
        &snapshot,
        &pre_backup,
        now,
        &storage_lock,
    )
    .unwrap();

    let fresh_revision = summary.revision().unwrap();
    assert_ne!(fresh_revision, snapshot_revision);
    assert_ne!(format!("{fresh_revision}\n"), old_revision);
    assert_eq!(
        fs::read_to_string(current.join(".revision")).unwrap(),
        format!("{fresh_revision}\n")
    );
    assert!(!current.join("obsolete").exists());
    assert!(!current_project.parent().unwrap().join("obsolete.bin").exists());
    assert!(fs::read_dir(&current)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry.path().join("restored.bin").is_file()));
    verify_snapshot(&pre_backup).unwrap();
    assert!(pre_backup.join("storage/obsolete/empty").is_dir());
    assert!(pre_backup
        .join("storage")
        .join(current_project.strip_prefix(&current).unwrap())
        .is_file());
}
