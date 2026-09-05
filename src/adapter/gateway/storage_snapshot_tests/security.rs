use crate::adapter::gateway::storage_content_integrity::content_digest;
use crate::adapter::gateway::storage_snapshot::{create_snapshot_at, verify_snapshot};
use chrono::TimeZone;

fn update_manifest_file(snapshot: &Path, relative: &Path, bytes: &[u8]) {
    let manifest_path = snapshot.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let relative = relative.to_str().unwrap();
    let entry = manifest["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["path"] == relative)
        .unwrap();
    entry["content_length"] = serde_json::json!(bytes.len());
    entry["content_digest"] = serde_json::json!(content_digest(bytes));
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
}

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
    update_manifest_file(&snapshot, relative, invalid);

    verify_snapshot(&snapshot).unwrap_err();
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
    fs::create_dir(snapshot.join("storage/duplicate")).unwrap();
    fs::write(snapshot.join("storage").join(&duplicate_relative), &bytes).unwrap();

    let manifest_path = snapshot.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["directories"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"path": "duplicate", "mode": 493}));
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

    verify_snapshot(&snapshot).unwrap_err();
}
