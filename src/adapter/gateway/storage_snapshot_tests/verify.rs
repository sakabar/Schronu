use crate::adapter::gateway::storage_snapshot::{create_snapshot_at, verify_snapshot};
use chrono::TimeZone;

fn create_source_independent_snapshot(label: &str) -> (TestDirectory, PathBuf, PathBuf) {
    let root = TestDirectory::new(label);
    let storage = root.child("source");
    let snapshot = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let (_, project_yaml) = create_saved_repository(&storage, now);
    let relative_project = project_yaml.strip_prefix(&storage).unwrap().to_path_buf();
    create_snapshot_at(&storage, &snapshot, now).unwrap();
    fs::remove_dir_all(&storage).unwrap();
    (root, snapshot, relative_project)
}

#[test]
fn snapshot_verifyはsource削除後もmanifestと全payloadを検証する() {
    let (_root, snapshot, _) = create_source_independent_snapshot("verify-source-independent");

    let summary = verify_snapshot(&snapshot).unwrap();

    assert_eq!(summary.file_count(), 2);
    assert!(summary.revision().is_some());
}

#[test]
fn snapshot_verifyは欠落余剰長さ違いdigest違いを拒否する() {
    for corruption in ["missing", "extra", "length", "digest"] {
        let (_root, snapshot, relative_project) =
            create_source_independent_snapshot(&format!("verify-{corruption}"));
        let project = snapshot.join("storage").join(relative_project);
        match corruption {
            "missing" => fs::remove_file(&project).unwrap(),
            "extra" => fs::write(snapshot.join("storage/extra.bin"), b"extra").unwrap(),
            "length" => fs::write(&project, b"short").unwrap(),
            "digest" => {
                let mut bytes = fs::read(&project).unwrap();
                bytes[0] ^= 1;
                fs::write(&project, bytes).unwrap();
            }
            _ => unreachable!(),
        }

        let error = verify_snapshot(&snapshot).unwrap_err();

        assert!(
            error.path().starts_with(&snapshot),
            "corruption {corruption}: {error}"
        );
    }
}

#[test]
fn snapshot_verifyはmanifestとpayloadのrevision不一致を拒否する() {
    let (_root, snapshot, _) = create_source_independent_snapshot("verify-revision-mismatch");
    let manifest_path = snapshot.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["revision"] = serde_json::json!(Uuid::new_v4());
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let error = verify_snapshot(&snapshot).unwrap_err();

    assert_eq!(error.path(), snapshot.join("storage/.revision"));
}

#[cfg(unix)]
#[test]
fn snapshot_verifyはmanifest_payload_file_storage_symlinkを拒否する() {
    use std::os::unix::fs::symlink;

    for target in ["manifest", "payload-file", "storage"] {
        let (root, snapshot, relative_project) =
            create_source_independent_snapshot(&format!("verify-symlink-{target}"));
        let outside = root.child("outside");
        fs::create_dir(&outside).unwrap();
        let outside_file = outside.join("data");
        fs::write(&outside_file, b"outside").unwrap();
        match target {
            "manifest" => {
                fs::remove_file(snapshot.join("manifest.json")).unwrap();
                symlink(&outside_file, snapshot.join("manifest.json")).unwrap();
            }
            "payload-file" => {
                let project = snapshot.join("storage").join(relative_project);
                fs::remove_file(&project).unwrap();
                symlink(&outside_file, project).unwrap();
            }
            "storage" => {
                fs::remove_dir_all(snapshot.join("storage")).unwrap();
                symlink(&outside, snapshot.join("storage")).unwrap();
            }
            _ => unreachable!(),
        }

        verify_snapshot(&snapshot).unwrap_err();

        assert_eq!(fs::read(&outside_file).unwrap(), b"outside");
    }
}

#[cfg(unix)]
#[test]
fn snapshot_verifyはwriterなしfifoをblockせず拒否する() {
    use std::os::unix::ffi::OsStrExt;

    let (_root, snapshot, _) = create_source_independent_snapshot("verify-fifo");
    let fifo = snapshot.join("storage/unexpected-fifo");
    let fifo_path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: fifo_path is a live CString and mkfifo does not retain its pointer.
    assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);

    verify_snapshot(&snapshot).unwrap_err();
}
