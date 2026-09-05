use crate::adapter::gateway::storage_snapshot::{create_snapshot_at, verify_snapshot};
use chrono::TimeZone;

#[test]
fn snapshot_verifyはdigestが一致する不正yamlをstrict拒否する() {
    let root = TestDirectory::new("security-invalid-yaml");
    let storage = root.child("source");
    let snapshot = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let (_, project_yaml) = create_saved_repository(&storage, now);
    let relative = project_yaml.strip_prefix(&storage).unwrap();
    create_snapshot_at(&storage, &snapshot, now).unwrap();
    let invalid = b"project: [";
    fs::write(snapshot.join("storage").join(relative), invalid).unwrap();
    update_snapshot_manifest_file(&snapshot, relative, invalid);

    let error = verify_snapshot(&snapshot).unwrap_err().to_string();
    assert!(error.contains("RepositoryLoad"), "{error}");
    assert!(error.contains("project.yaml"), "{error}");
}

#[test]
fn snapshot_verifyはdigestが一致する重複task_uuidをstrict拒否する() {
    let root = TestDirectory::new("security-duplicate-uuid");
    let storage = root.child("source");
    let snapshot = root.child("snapshot");
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let (_, project_yaml) = create_saved_repository(&storage, now);
    create_snapshot_at(&storage, &snapshot, now).unwrap();
    let source_relative = project_yaml.strip_prefix(&storage).unwrap();
    let duplicate_relative = PathBuf::from("duplicate/project.yaml");
    let bytes = fs::read(snapshot.join("storage").join(source_relative)).unwrap();
    let duplicate_directory = snapshot.join("storage/duplicate");
    fs::create_dir(&duplicate_directory).unwrap();
    fs::write(snapshot.join("storage").join(&duplicate_relative), &bytes).unwrap();

    let manifest_path = snapshot.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["directories"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "path": "duplicate",
            "mode": permission_mode(&duplicate_directory),
        }));
    let mut file_entry = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == source_relative.to_str().unwrap())
        .unwrap()
        .clone();
    file_entry["path"] = serde_json::json!(duplicate_relative);
    manifest["files"].as_array_mut().unwrap().push(file_entry);
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let error = verify_snapshot(&snapshot).unwrap_err().to_string();
    assert!(error.contains("RepositoryLoad"), "{error}");
    assert!(error.contains("duplicate task ID"), "{error}");
}

#[cfg(unix)]
fn permission_mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn permission_mode(_path: &Path) -> Option<u32> {
    None
}
