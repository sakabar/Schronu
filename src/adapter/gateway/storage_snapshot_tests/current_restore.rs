use crate::adapter::gateway::storage_lock::{LockMode, StorageLock};
use crate::adapter::gateway::storage_snapshot::{
    create_snapshot_at, restore_current_snapshot_at,
    restore_current_snapshot_at_with_transaction_io, verify_snapshot,
};
use chrono::TimeZone;
use crate::adapter::gateway::storage_transaction_test_support::{
    FaultRule, PathMatcher, RecordingIo, RecordingOperation,
};
use std::sync::Arc;

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

#[test]
fn current_restoreはtransaction_prepare失敗時にcurrentを変更しない() {
    let root = TestDirectory::new("current-restore-prepare-failure");
    let current = root.child("current");
    let source = root.child("source");
    let snapshot = root.child("snapshot");
    let pre_backup = root.child("pre-backup");
    let now = Local.with_ymd_and_hms(2026, 9, 6, 13, 0, 0).unwrap();
    let (_, current_project) = create_saved_repository(&current, now);
    let current_project_bytes = fs::read(&current_project).unwrap();
    let current_revision = fs::read(current.join(".revision")).unwrap();
    create_saved_repository(&source, now);
    create_snapshot_at(&source, &snapshot, now).unwrap();
    let storage_lock = StorageLock::acquire(&current, LockMode::Cli).unwrap();
    let io = Arc::new(RecordingIo::new(vec![FaultRule {
        operation: RecordingOperation::CreateFile,
        path_matcher: PathMatcher::FileName("0"),
        occurrence: 1,
        error_kind: std::io::ErrorKind::Other,
        error_message: "injected current restore prepare failure",
    }]));

    let error = restore_current_snapshot_at_with_transaction_io(
        &current,
        &snapshot,
        &pre_backup,
        now,
        &storage_lock,
        io,
    )
    .unwrap_err();

    assert_eq!(error.path(), current);
    assert!(
        error
            .to_string()
            .contains("injected current restore prepare failure"),
        "{error}"
    );
    assert_eq!(fs::read(current_project).unwrap(), current_project_bytes);
    assert_eq!(fs::read(current.join(".revision")).unwrap(), current_revision);
    verify_snapshot(pre_backup).unwrap();
}
