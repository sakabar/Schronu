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
    discard_event_ended_at_epoch_ms: HashMap<String, i64>,
}

#[derive(Deserialize, Serialize)]
struct StoredMutationSafety {
    version: u64,
    mutation_blocked: bool,
    #[serde(default)]
    committed_task_ids: Vec<String>,
    #[serde(default)]
    discard_event_ids: HashMap<String, String>,
    #[serde(default)]
    discard_event_ended_at_epoch_ms: HashMap<String, i64>,
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
        discard_event_ended_at_epoch_ms: stored.discard_event_ended_at_epoch_ms,
    })
}

impl MutationSafetyState {
    pub(crate) fn blocked() -> Self {
        Self {
            mutation_blocked: true,
            committed_task_ids: HashSet::new(),
            discard_event_ids: HashMap::new(),
            discard_event_ended_at_epoch_ms: HashMap::new(),
        }
    }

    pub fn mutation_blocked(&self) -> bool {
        self.mutation_blocked
    }

    pub(crate) fn committed_task_ids(&self) -> &HashSet<String> {
        &self.committed_task_ids
    }

    pub(crate) fn unresolved_discard_ended_at_epoch_ms(&self) -> HashMap<String, i64> {
        self.discard_event_ended_at_epoch_ms
            .iter()
            .filter(|(task_id, _)| !self.committed_task_ids.contains(*task_id))
            .map(|(task_id, ended_at)| (task_id.clone(), *ended_at))
            .collect()
    }

    pub fn arm<S: KeyValueStorage>(&mut self, storage: &S) -> Result<(), StorageError> {
        let stored = StoredMutationSafety {
            version: STORAGE_VERSION,
            mutation_blocked: true,
            committed_task_ids: self.committed_task_ids.iter().cloned().collect(),
            discard_event_ids: self.discard_event_ids.clone(),
            discard_event_ended_at_epoch_ms: self.discard_event_ended_at_epoch_ms.clone(),
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
            discard_event_ended_at_epoch_ms: HashMap::new(),
        };
        let serialized = serde_json::to_string(&stored).map_err(|_| StorageError::WriteFailed)?;
        storage.set(MUTATION_SAFETY_STORAGE_KEY, &serialized)?;
        self.mutation_blocked = false;
        self.committed_task_ids.clear();
        self.discard_event_ids.clear();
        self.discard_event_ended_at_epoch_ms.clear();
        Ok(())
    }

    pub fn disarm_retaining_discard_events<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        retained_task_ids: &HashSet<String>,
    ) -> Result<(), StorageError> {
        let discard_event_ids = self
            .discard_event_ids
            .iter()
            .filter(|(task_id, _)| retained_task_ids.contains(*task_id))
            .map(|(task_id, event_id)| (task_id.clone(), event_id.clone()))
            .collect::<HashMap<_, _>>();
        let discard_event_ended_at_epoch_ms = self
            .discard_event_ended_at_epoch_ms
            .iter()
            .filter(|(task_id, _)| retained_task_ids.contains(*task_id))
            .map(|(task_id, ended_at)| (task_id.clone(), *ended_at))
            .collect::<HashMap<_, _>>();
        let stored = StoredMutationSafety {
            version: STORAGE_VERSION,
            mutation_blocked: false,
            committed_task_ids: Vec::new(),
            discard_event_ids: discard_event_ids.clone(),
            discard_event_ended_at_epoch_ms: discard_event_ended_at_epoch_ms.clone(),
        };
        let serialized = serde_json::to_string(&stored).map_err(|_| StorageError::WriteFailed)?;
        storage.set(MUTATION_SAFETY_STORAGE_KEY, &serialized)?;
        self.mutation_blocked = false;
        self.committed_task_ids.clear();
        self.discard_event_ids = discard_event_ids;
        self.discard_event_ended_at_epoch_ms = discard_event_ended_at_epoch_ms;
        Ok(())
    }

    pub fn arm_discard<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
        requested_event_id: Option<&str>,
        requested_ended_at_epoch_ms: i64,
    ) -> Result<(String, i64), StorageError> {
        let event_id = requested_event_id
            .map(str::to_owned)
            .or_else(|| self.discard_event_ids.get(task_id).cloned())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut discard_event_ids = self.discard_event_ids.clone();
        discard_event_ids.insert(task_id.to_owned(), event_id.clone());
        let ended_at_epoch_ms = self
            .discard_event_ended_at_epoch_ms
            .get(task_id)
            .copied()
            .unwrap_or(requested_ended_at_epoch_ms);
        let mut discard_event_ended_at_epoch_ms = self.discard_event_ended_at_epoch_ms.clone();
        discard_event_ended_at_epoch_ms.insert(task_id.to_owned(), ended_at_epoch_ms);
        let stored = StoredMutationSafety {
            version: STORAGE_VERSION,
            mutation_blocked: true,
            committed_task_ids: self.committed_task_ids.iter().cloned().collect(),
            discard_event_ids: discard_event_ids.clone(),
            discard_event_ended_at_epoch_ms: discard_event_ended_at_epoch_ms.clone(),
        };
        let serialized = serde_json::to_string(&stored).map_err(|_| StorageError::WriteFailed)?;
        storage.set(MUTATION_SAFETY_STORAGE_KEY, &serialized)?;
        self.mutation_blocked = true;
        self.discard_event_ids = discard_event_ids;
        self.discard_event_ended_at_epoch_ms = discard_event_ended_at_epoch_ms;
        Ok((event_id, ended_at_epoch_ms))
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
            discard_event_ended_at_epoch_ms: self.discard_event_ended_at_epoch_ms.clone(),
        };
        let serialized = serde_json::to_string(&stored).map_err(|_| StorageError::WriteFailed)?;
        storage.set(MUTATION_SAFETY_STORAGE_KEY, &serialized)?;
        self.mutation_blocked = true;
        self.committed_task_ids = committed_task_ids;
        Ok(())
    }
}
