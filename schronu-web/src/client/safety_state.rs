use super::work_sessions::{KeyValueStorage, StorageError};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub const MUTATION_SAFETY_STORAGE_KEY: &str = "schronu_web.mutation_safety.v1";
const STORAGE_VERSION: u64 = 1;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MutationSafetyState {
    mutation_blocked: bool,
    committed_task_ids: HashSet<String>,
    discard_event_ids: HashMap<String, String>,
}

#[derive(Deserialize, Serialize)]
struct StoredMutationSafety {
    version: u64,
    mutation_blocked: bool,
    #[serde(default)]
    committed_task_ids: Vec<String>,
    #[serde(default)]
    discard_event_ids: HashMap<String, String>,
}

pub fn load_mutation_safety<S: KeyValueStorage>(
    storage: &S,
) -> Result<MutationSafetyState, StorageError> {
    let Some(raw) = storage.get(MUTATION_SAFETY_STORAGE_KEY)? else {
        return Ok(MutationSafetyState::default());
    };
    let stored = serde_json::from_str::<StoredMutationSafety>(&raw).ok();
    let Some(stored) = stored.filter(|value| value.version == STORAGE_VERSION) else {
        return Ok(MutationSafetyState::blocked());
    };
    Ok(MutationSafetyState {
        mutation_blocked: stored.mutation_blocked,
        committed_task_ids: stored.committed_task_ids.into_iter().collect(),
        discard_event_ids: stored.discard_event_ids,
    })
}

impl MutationSafetyState {
    pub(crate) fn blocked() -> Self {
        Self {
            mutation_blocked: true,
            committed_task_ids: HashSet::new(),
            discard_event_ids: HashMap::new(),
        }
    }

    pub fn mutation_blocked(&self) -> bool {
        self.mutation_blocked
    }

    pub(crate) fn committed_task_ids(&self) -> &HashSet<String> {
        &self.committed_task_ids
    }

    pub fn arm<S: KeyValueStorage>(&mut self, storage: &S) -> Result<(), StorageError> {
        let stored = StoredMutationSafety {
            version: STORAGE_VERSION,
            mutation_blocked: true,
            committed_task_ids: self.committed_task_ids.iter().cloned().collect(),
            discard_event_ids: self.discard_event_ids.clone(),
        };
        let serialized = serde_json::to_string(&stored).map_err(|_| StorageError::WriteFailed)?;
        storage.set(MUTATION_SAFETY_STORAGE_KEY, &serialized)?;
        self.mutation_blocked = true;
        Ok(())
    }

    pub fn disarm<S: KeyValueStorage>(&mut self, storage: &S) -> Result<(), StorageError> {
        let stored = StoredMutationSafety {
            version: STORAGE_VERSION,
            mutation_blocked: false,
            committed_task_ids: Vec::new(),
            discard_event_ids: HashMap::new(),
        };
        let serialized = serde_json::to_string(&stored).map_err(|_| StorageError::WriteFailed)?;
        storage.set(MUTATION_SAFETY_STORAGE_KEY, &serialized)?;
        self.mutation_blocked = false;
        self.committed_task_ids.clear();
        self.discard_event_ids.clear();
        Ok(())
    }

    pub fn disarm_preserving_discard_events<S: KeyValueStorage>(
        &mut self,
        storage: &S,
    ) -> Result<(), StorageError> {
        let stored = StoredMutationSafety {
            version: STORAGE_VERSION,
            mutation_blocked: false,
            committed_task_ids: Vec::new(),
            discard_event_ids: self.discard_event_ids.clone(),
        };
        let serialized = serde_json::to_string(&stored).map_err(|_| StorageError::WriteFailed)?;
        storage.set(MUTATION_SAFETY_STORAGE_KEY, &serialized)?;
        self.mutation_blocked = false;
        self.committed_task_ids.clear();
        Ok(())
    }

    pub fn arm_discard<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
        requested_event_id: Option<&str>,
    ) -> Result<String, StorageError> {
        let event_id = requested_event_id
            .map(str::to_owned)
            .or_else(|| self.discard_event_ids.get(task_id).cloned())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut discard_event_ids = self.discard_event_ids.clone();
        discard_event_ids.insert(task_id.to_owned(), event_id.clone());
        let stored = StoredMutationSafety {
            version: STORAGE_VERSION,
            mutation_blocked: true,
            committed_task_ids: self.committed_task_ids.iter().cloned().collect(),
            discard_event_ids: discard_event_ids.clone(),
        };
        let serialized = serde_json::to_string(&stored).map_err(|_| StorageError::WriteFailed)?;
        storage.set(MUTATION_SAFETY_STORAGE_KEY, &serialized)?;
        self.mutation_blocked = true;
        self.discard_event_ids = discard_event_ids;
        Ok(event_id)
    }

    pub fn mark_committed<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
    ) -> Result<(), StorageError> {
        let mut committed_task_ids = self.committed_task_ids.clone();
        committed_task_ids.insert(task_id.to_owned());
        let stored = StoredMutationSafety {
            version: STORAGE_VERSION,
            mutation_blocked: true,
            committed_task_ids: committed_task_ids.iter().cloned().collect(),
            discard_event_ids: self.discard_event_ids.clone(),
        };
        let serialized = serde_json::to_string(&stored).map_err(|_| StorageError::WriteFailed)?;
        storage.set(MUTATION_SAFETY_STORAGE_KEY, &serialized)?;
        self.mutation_blocked = true;
        self.committed_task_ids = committed_task_ids;
        Ok(())
    }
}
