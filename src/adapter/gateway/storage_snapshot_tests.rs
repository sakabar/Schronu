use super::storage_snapshot::manifest::{
    decode_manifest, encode_manifest, DigestDescriptor, DirectoryEntry, FileEntry, SnapshotManifest,
};
use chrono::{FixedOffset, TimeZone};
use std::path::PathBuf;
use uuid::Uuid;

include!("storage_snapshot_tests/support.rs");

mod manifest {
    use super::*;

    include!("storage_snapshot_tests/manifest.rs");
}

mod create {
    use super::*;

    include!("storage_snapshot_tests/create.rs");
}

mod recovery {
    use super::*;
    use crate::adapter::gateway::storage_transaction_test_support::*;

    include!("storage_snapshot_tests/recovery.rs");
}
