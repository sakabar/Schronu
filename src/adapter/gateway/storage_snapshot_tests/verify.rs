use crate::adapter::gateway::storage_snapshot::{
    create_snapshot_at, verify_snapshot, verify_snapshot_with_limits, SnapshotLimitKind,
    SnapshotResourceLimits,
};
use chrono::TimeZone;
use walkdir::WalkDir;

#[test]
fn snapshot_verify_resource_limitは境界を許可し超過をtyped拒否する() {
    let root = TestDirectory::new("verify-resource-limits");
    let storage = root.child("source");
    let snapshot = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let (_, project_yaml) = create_saved_repository(&storage, now);
    fs::remove_file(storage.join(".revision")).unwrap();
    fs::remove_dir_all(project_yaml.parent().unwrap().join("markdown")).unwrap();
    let relative_project = project_yaml.strip_prefix(&storage).unwrap().to_path_buf();
    create_snapshot_at(&storage, &snapshot, now).unwrap();
    fs::remove_dir_all(&storage).unwrap();
    let manifest_bytes = fs::metadata(snapshot.join("manifest.json")).unwrap().len();
    let payload = snapshot.join("storage");
    let entries = WalkDir::new(&payload)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.depth() > 0)
        .collect::<Vec<_>>();
    let files = entries
        .iter()
        .filter(|entry| entry.file_type().is_file())
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
    let logical_paths = entries
        .iter()
        .map(|entry| entry.path().strip_prefix(&payload).unwrap())
        .collect::<Vec<_>>();
    let path_bytes = logical_paths
        .iter()
        .map(|path| path.to_str().unwrap().len())
        .max()
        .unwrap();
    let depth = logical_paths
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

    verify_snapshot_with_limits(&snapshot, exact).unwrap();

    for (limits, expected) in [
        (
            SnapshotResourceLimits::new(
                manifest_bytes - 1,
                file_count,
                file_bytes,
                total_bytes,
                path_bytes,
                depth,
            ),
            SnapshotLimitKind::ManifestBytes,
        ),
        (
            SnapshotResourceLimits::new(
                manifest_bytes,
                file_count - 1,
                file_bytes,
                total_bytes,
                path_bytes,
                depth,
            ),
            SnapshotLimitKind::FileCount,
        ),
        (
            SnapshotResourceLimits::new(
                manifest_bytes,
                file_count,
                file_bytes - 1,
                total_bytes,
                path_bytes,
                depth,
            ),
            SnapshotLimitKind::FileBytes,
        ),
        (
            SnapshotResourceLimits::new(
                manifest_bytes,
                file_count,
                file_bytes,
                total_bytes - 1,
                path_bytes,
                depth,
            ),
            SnapshotLimitKind::PayloadBytes,
        ),
        (
            SnapshotResourceLimits::new(
                manifest_bytes,
                file_count,
                file_bytes,
                total_bytes,
                path_bytes - 1,
                depth,
            ),
            SnapshotLimitKind::PathBytes,
        ),
        (
            SnapshotResourceLimits::new(
                manifest_bytes,
                file_count,
                file_bytes,
                total_bytes,
                path_bytes,
                depth - 1,
            ),
            SnapshotLimitKind::PathDepth,
        ),
    ] {
        let error = verify_snapshot_with_limits(&snapshot, limits).unwrap_err();
        assert_eq!(error.limit_kind(), Some(expected), "{error}");
        assert_eq!(
            error.observed_value(),
            error.limit_value().and_then(|limit| limit.checked_add(1)),
            "{error}"
        );
        if expected == SnapshotLimitKind::ManifestBytes {
            assert_eq!(error.limit_path(), None, "{error}");
            assert_eq!(error.path(), snapshot.join("manifest.json"), "{error}");
        } else {
            let expected_path = relative_project.clone();
            assert_eq!(error.limit_path(), Some(expected_path.as_path()), "{error}");
            assert_eq!(error.path(), payload.join(expected_path), "{error}");
        }
    }
}

#[test]
fn snapshot_verifyはmanifest_decode前のdirectory_captureをmanifest_budgetで制限する() {
    let root = TestDirectory::new("verify-directory-budget");
    let storage = root.child("source");
    let snapshot = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    create_saved_repository(&storage, now);
    create_snapshot_at(&storage, &snapshot, now).unwrap();
    fs::remove_dir_all(&storage).unwrap();

    let manifest_bytes = fs::metadata(snapshot.join("manifest.json")).unwrap().len();
    let limits = SnapshotResourceLimits::new(
        manifest_bytes,
        10_000,
        64 * 1024 * 1024,
        256 * 1024 * 1024,
        4_096,
        64,
    );
    let payload = snapshot.join("storage");
    for index in 0..=(manifest_bytes / 24) {
        fs::create_dir(payload.join(format!("extra-{index}"))).unwrap();
    }

    let error = verify_snapshot_with_limits(&snapshot, limits).unwrap_err();

    assert_eq!(error.limit_kind(), Some(SnapshotLimitKind::ManifestBytes));
    assert_eq!(error.limit_value(), Some(manifest_bytes));
    assert!(error.observed_value().unwrap() > manifest_bytes);
    assert!(error.limit_path().is_some());
    assert!(error.path().starts_with(&payload));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn snapshot_verifyのdirectory_captureは実encode長境界を許可し1byte超過をtyped拒否する() {
    use std::os::unix::fs::PermissionsExt;

    let root = TestDirectory::new("verify-directory-exact-budget");
    let storage = root.child("source");
    let snapshot = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    create_saved_repository(&storage, now);
    create_snapshot_at(&storage, &snapshot, now).unwrap();
    fs::remove_dir_all(&storage).unwrap();
    let payload = snapshot.join("storage");
    fs::remove_dir_all(&payload).unwrap();
    fs::create_dir(&payload).unwrap();
    let expected_relative = PathBuf::from("a".repeat(200))
        .join("b".repeat(200))
        .join("c".repeat(200))
        .join("d".repeat(200));
    fs::create_dir_all(payload.join(&expected_relative)).unwrap();

    let entries = WalkDir::new(&payload)
        .min_depth(1)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_dir())
        .map(|entry| DirectoryEntry {
            path: entry.path().strip_prefix(&payload).unwrap().to_path_buf(),
            mode: Some(entry.metadata().unwrap().permissions().mode() & 0o7777),
        })
        .collect::<Vec<_>>();
    let exact_bytes = entries
        .iter()
        .map(|entry| serde_json::to_vec(entry).unwrap().len() as u64)
        .sum::<u64>()
        + entries.len().saturating_sub(1) as u64;
    let manifest_bytes = fs::metadata(snapshot.join("manifest.json")).unwrap().len();
    assert!(exact_bytes > manifest_bytes);
    let exact = SnapshotResourceLimits::new(
        exact_bytes,
        10_000,
        64 * 1024 * 1024,
        256 * 1024 * 1024,
        4_096,
        64,
    );

    let non_limit_error = verify_snapshot_with_limits(&snapshot, exact).unwrap_err();
    assert_eq!(non_limit_error.limit_kind(), None, "{non_limit_error}");

    let error = verify_snapshot_with_limits(
        &snapshot,
        SnapshotResourceLimits::new(
            exact_bytes - 1,
            10_000,
            64 * 1024 * 1024,
            256 * 1024 * 1024,
            4_096,
            64,
        ),
    )
    .unwrap_err();
    assert_eq!(error.limit_kind(), Some(SnapshotLimitKind::ManifestBytes));
    assert_eq!(error.limit_value(), Some(exact_bytes - 1));
    assert_eq!(error.observed_value(), Some(exact_bytes));
    assert_eq!(error.limit_path(), Some(expected_relative.as_path()));
    assert_eq!(error.path(), payload.join(&expected_relative));
}

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
