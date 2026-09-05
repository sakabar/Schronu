#[test]
fn snapshot_manifest_v1はschemaとfield順を固定する() {
    let manifest = SnapshotManifest {
        format_version: 1,
        tool_version: "0.1.0".to_string(),
        created_at: FixedOffset::east_opt(9 * 60 * 60)
            .unwrap()
            .with_ymd_and_hms(2026, 9, 5, 12, 0, 0)
            .unwrap(),
        revision: Some(Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap()),
        digest: DigestDescriptor {
            algorithm: "fnv1a64".to_string(),
            version: 1,
        },
        directories: vec![DirectoryEntry {
            path: PathBuf::from("20260905-project-11111111-2222-3333-4444-555555555555"),
            mode: Some(0o755),
        }],
        files: vec![FileEntry {
            path: PathBuf::from(
                "20260905-project-11111111-2222-3333-4444-555555555555/project.yaml",
            ),
            mode: Some(0o640),
            content_length: 12,
            content_digest: "fnv1a64:0123456789abcdef".to_string(),
        }],
    };

    assert_eq!(
        encode_manifest(&manifest).unwrap(),
        br#"{"format_version":1,"tool_version":"0.1.0","created_at":"2026-09-05T12:00:00+09:00","revision":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","digest":{"algorithm":"fnv1a64","version":1},"directories":[{"path":"20260905-project-11111111-2222-3333-4444-555555555555","mode":493}],"files":[{"path":"20260905-project-11111111-2222-3333-4444-555555555555/project.yaml","mode":416,"content_length":12,"content_digest":"fnv1a64:0123456789abcdef"}]}"#
    );
}

#[test]
fn snapshot_manifest_v1はrevisionなしをnullで保持する() {
    let manifest = SnapshotManifest {
        format_version: 1,
        tool_version: "0.1.0".to_string(),
        created_at: FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 9, 5, 3, 0, 0)
            .unwrap(),
        revision: None,
        digest: DigestDescriptor {
            algorithm: "fnv1a64".to_string(),
            version: 1,
        },
        directories: Vec::new(),
        files: Vec::new(),
    };

    let encoded = encode_manifest(&manifest).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    assert!(value["revision"].is_null());
}

#[test]
fn snapshot_manifest_v1はdirectoryとfileをpath順にencodeする() {
    let manifest = SnapshotManifest {
        format_version: 1,
        tool_version: "0.1.0".to_string(),
        created_at: FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 9, 5, 3, 0, 0)
            .unwrap(),
        revision: None,
        digest: DigestDescriptor {
            algorithm: "fnv1a64".to_string(),
            version: 1,
        },
        directories: vec![
            DirectoryEntry {
                path: PathBuf::from("z"),
                mode: None,
            },
            DirectoryEntry {
                path: PathBuf::from("a"),
                mode: None,
            },
        ],
        files: vec![
            FileEntry {
                path: PathBuf::from("z/file"),
                mode: None,
                content_length: 0,
                content_digest: "fnv1a64:cbf29ce484222325".to_string(),
            },
            FileEntry {
                path: PathBuf::from("a/file"),
                mode: None,
                content_length: 0,
                content_digest: "fnv1a64:cbf29ce484222325".to_string(),
            },
        ],
    };

    let encoded: serde_json::Value =
        serde_json::from_slice(&encode_manifest(&manifest).unwrap()).unwrap();
    assert_eq!(encoded["directories"][0]["path"], "a");
    assert_eq!(encoded["directories"][1]["path"], "z");
    assert_eq!(encoded["files"][0]["path"], "a/file");
    assert_eq!(encoded["files"][1]["path"], "z/file");
}
