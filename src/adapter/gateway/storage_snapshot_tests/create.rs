use crate::adapter::gateway::storage_lock::{LockMode, StorageLock};
use crate::adapter::gateway::storage_snapshot::{
    create_snapshot_after_parent_open, create_snapshot_at, finalize_publication,
};
use crate::adapter::gateway::storage_snapshot::io::rename_no_replace;
use crate::adapter::gateway::storage_snapshot::io::SnapshotIo;
use chrono::TimeZone;
use std::sync::atomic::{AtomicUsize, Ordering};

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
fn snapshot公開renameは競合して作成されたdestinationを置換しない() {
    let root = TestDirectory::new("create-no-replace");
    let staging = root.child(".snapshot.tmp-staged");
    let destination = root.child("snapshot");
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join("staged"), b"snapshot").unwrap();
    fs::create_dir(&destination).unwrap();

    rename_no_replace(&staging, &destination).unwrap_err();

    assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
    assert_eq!(fs::read(staging.join("staged")).unwrap(), b"snapshot");
}

struct FirstSyncFailureIo {
    sync_count: AtomicUsize,
}

impl SnapshotIo for FirstSyncFailureIo {
    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        let count = self.sync_count.fetch_add(1, Ordering::SeqCst);
        if count == 0 {
            Err(std::io::Error::other("injected parent sync failure"))
        } else {
            fs::File::open(path)?.sync_all()
        }
    }
}

#[test]
fn snapshot公開後のparent_sync失敗はdestinationをrollbackする() {
    let root = TestDirectory::new("create-parent-sync-failure");
    let destination = root.child("snapshot");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("manifest.json"), b"published").unwrap();
    let io = FirstSyncFailureIo {
        sync_count: AtomicUsize::new(0),
    };

    finalize_publication(&io, &destination).unwrap_err();

    assert!(!destination.exists());
    assert_eq!(io.sync_count.load(Ordering::SeqCst), 2);
}
