use super::storage_snapshot::manifest::{
    decode_manifest, encode_manifest_with_limits, DigestDescriptor, DirectoryEntry, FileEntry,
    SnapshotManifest,
};
use chrono::{FixedOffset, TimeZone};
use std::path::PathBuf;
use uuid::Uuid;

fn encode_manifest(
    manifest: &SnapshotManifest,
) -> Result<Vec<u8>, super::storage_snapshot::SnapshotError> {
    encode_manifest_with_limits(
        std::path::Path::new("manifest.json"),
        manifest,
        super::storage_snapshot::SnapshotResourceLimits::new(
            u64::MAX,
            usize::MAX,
            u64::MAX,
            u64::MAX,
            usize::MAX,
            usize::MAX,
        ),
    )
}

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

mod verify {
    use super::*;

    include!("storage_snapshot_tests/verify.rs");
}

mod security {
    use super::*;

    include!("storage_snapshot_tests/security.rs");
}

mod restore {
    use super::*;

    include!("storage_snapshot_tests/restore.rs");
}
