use super::work_sessions::{KeyValueStorage, StorageError};
use serde::{Deserialize, Serialize};

pub const MUTATION_SAFETY_STORAGE_KEY: &str = "schronu_web.mutation_safety.v1";
const STORAGE_VERSION: u64 = 1;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MutationSafetyState {
    mutation_blocked: bool,
}

#[derive(Deserialize, Serialize)]
struct StoredMutationSafety {
    version: u64,
    mutation_blocked: bool,
}

pub fn load_mutation_safety<S: KeyValueStorage>(
    storage: &S,
) -> Result<MutationSafetyState, StorageError> {
    let Some(raw) = storage.get(MUTATION_SAFETY_STORAGE_KEY)? else {
        return Ok(MutationSafetyState::default());
    };
    let stored = serde_json::from_str::<StoredMutationSafety>(&raw).ok();
    Ok(MutationSafetyState {
        mutation_blocked: stored
            .filter(|value| value.version == STORAGE_VERSION)
            .is_none_or(|value| value.mutation_blocked),
    })
}

impl MutationSafetyState {
    pub fn mutation_blocked(&self) -> bool {
        self.mutation_blocked
    }

    pub fn block_mutations<S: KeyValueStorage>(&mut self, storage: &S) {
        self.mutation_blocked = true;
        let stored = StoredMutationSafety {
            version: STORAGE_VERSION,
            mutation_blocked: true,
        };
        if let Ok(serialized) = serde_json::to_string(&stored) {
            let _ = storage.set(MUTATION_SAFETY_STORAGE_KEY, &serialized);
        }
    }
}
