use super::io::TRANSACTION_LOCK_FILE_NAME;
use super::*;
use crate::adapter::gateway::storage_transaction_test_support::*;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Barrier;

include!("storage_transaction_tests/support.rs");

mod manifest {
    use super::*;

    include!("storage_transaction_tests/manifest.rs");
}

mod prepare {
    use super::*;

    include!("storage_transaction_tests/prepare.rs");
}

mod commit {
    use super::*;

    include!("storage_transaction_tests/commit.rs");
}

mod recovery {
    use super::*;

    include!("storage_transaction_tests/recovery.rs");
}

mod delete {
    use super::*;

    include!("storage_transaction_tests/delete.rs");
}

mod security {
    use super::*;

    include!("storage_transaction_tests/security.rs");
}
