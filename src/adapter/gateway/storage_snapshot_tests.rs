use super::storage_snapshot::manifest::{
    decode_manifest, encode_manifest, DigestDescriptor, DirectoryEntry, FileEntry, SnapshotManifest,
};
use chrono::{FixedOffset, TimeZone};
use std::path::PathBuf;
use uuid::Uuid;

mod manifest {
    use super::*;

    include!("storage_snapshot_tests/manifest.rs");
}
