use super::work_sessions::{KeyValueStorage, StorageError};
use crate::{CompleteSessionRequest, DiscardSessionRequest, RecordSessionRequest};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub const MUTATION_SAFETY_STORAGE_KEY: &str = "schronu_web.mutation_safety.v1";
const STORAGE_VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FixedMutationKind {
    Record,
    Complete,
    CompleteWithoutRecording,
    Discard,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "operation", content = "request", rename_all = "snake_case")]
pub(crate) enum FixedMutationRequest {
    Record(RecordSessionRequest),
    Complete(CompleteSessionRequest),
    Discard(DiscardSessionRequest),
}

impl FixedMutationRequest {
    pub(crate) fn kind(&self) -> FixedMutationKind {
        match self {
            Self::Record(_) => FixedMutationKind::Record,
            Self::Complete(request) if request.record_elapsed_seconds => {
                FixedMutationKind::Complete
            }
            Self::Complete(_) => FixedMutationKind::CompleteWithoutRecording,
            Self::Discard(_) => FixedMutationKind::Discard,
        }
    }

    pub(crate) fn task_id(&self) -> &str {
        match self {
            Self::Record(request) => &request.task_id,
            Self::Complete(request) => &request.task_id,
            Self::Discard(request) => &request.task_id,
        }
    }

    pub(crate) fn ended_at_epoch_ms(&self) -> Option<i64> {
        match self {
            Self::Record(request) => request.ended_at_epoch_ms,
            Self::Complete(request) => request.ended_at_epoch_ms,
            Self::Discard(request) => Some(request.ended_at_epoch_ms),
        }
    }

    fn is_valid_marker(&self, task_id: &str) -> bool {
        if self.task_id() != task_id || Uuid::parse_str(task_id).is_err() {
            return false;
        }
        match self {
            Self::Record(request) => {
                request.expected_actual_work_seconds >= 0
                    && valid_interval(request.started_at_epoch_ms, request.ended_at_epoch_ms)
            }
            Self::Complete(request) if !request.record_elapsed_seconds => {
                request.expected_actual_work_seconds >= 0
                    && valid_interval(request.started_at_epoch_ms, request.ended_at_epoch_ms)
                    && request
                        .discard_event_id
                        .as_deref()
                        .is_some_and(|event_id| Uuid::parse_str(event_id).is_ok())
                    && request
                        .task_name_at_start
                        .as_deref()
                        .is_some_and(|task_name| !task_name.trim().is_empty())
            }
            Self::Complete(request) => {
                request.expected_actual_work_seconds >= 0
                    && valid_interval(request.started_at_epoch_ms, request.ended_at_epoch_ms)
            }
            Self::Discard(request) => {
                Uuid::parse_str(&request.event_id).is_ok()
                    && !request.task_name_at_start.trim().is_empty()
                    && valid_interval(request.started_at_epoch_ms, Some(request.ended_at_epoch_ms))
            }
        }
    }
}

fn valid_interval(started_at_epoch_ms: i64, ended_at_epoch_ms: Option<i64>) -> bool {
    let Some(ended_at_epoch_ms) = ended_at_epoch_ms else {
        return false;
    };
    DateTime::<Utc>::from_timestamp_millis(started_at_epoch_ms).is_some()
        && DateTime::<Utc>::from_timestamp_millis(ended_at_epoch_ms).is_some()
        && started_at_epoch_ms <= ended_at_epoch_ms
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MutationSafetyState {
    mutation_blocked: bool,
    committed_task_ids: HashSet<String>,
    fixed_requests: HashMap<String, FixedMutationRequest>,
}

#[derive(Deserialize, Serialize)]
struct StoredMutationSafety {
    version: u64,
    mutation_blocked: bool,
    #[serde(default)]
    committed_task_ids: Vec<String>,
    #[serde(default)]
    fixed_requests: HashMap<String, FixedMutationRequest>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    discard_event_ids: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
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
    let legacy_discard_marker =
        !stored.discard_event_ids.is_empty() || !stored.discard_event_ended_at_epoch_ms.is_empty();
    let invalid_fixed_request = stored
        .fixed_requests
        .iter()
        .any(|(task_id, request)| !request.is_valid_marker(task_id));
    let committed_task_ids = stored
        .committed_task_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let invalid_committed_task_ids = (!stored.mutation_blocked && !committed_task_ids.is_empty())
        || committed_task_ids.len() != stored.committed_task_ids.len()
        || committed_task_ids
            .iter()
            .any(|task_id| !stored.fixed_requests.contains_key(task_id));
    let invalid_marker =
        legacy_discard_marker || invalid_fixed_request || invalid_committed_task_ids;
    Ok(MutationSafetyState {
        mutation_blocked: stored.mutation_blocked || invalid_marker,
        committed_task_ids: if invalid_marker {
            HashSet::new()
        } else {
            committed_task_ids
        },
        fixed_requests: if invalid_marker {
            HashMap::new()
        } else {
            stored.fixed_requests
        },
    })
}

impl MutationSafetyState {
    pub(crate) fn blocked() -> Self {
        Self {
            mutation_blocked: true,
            committed_task_ids: HashSet::new(),
            fixed_requests: HashMap::new(),
        }
    }

    pub fn mutation_blocked(&self) -> bool {
        self.mutation_blocked
    }

    pub(crate) fn committed_task_ids(&self) -> &HashSet<String> {
        &self.committed_task_ids
    }

    pub(crate) fn fixed_request_kind(&self, task_id: &str) -> Option<FixedMutationKind> {
        self.fixed_requests
            .get(task_id)
            .map(FixedMutationRequest::kind)
    }

    pub(crate) fn unresolved_ended_at_epoch_ms(&self) -> HashMap<String, i64> {
        self.fixed_requests
            .iter()
            .filter(|(task_id, _)| !self.committed_task_ids.contains(*task_id))
            .filter_map(|(task_id, request)| {
                request
                    .ended_at_epoch_ms()
                    .map(|ended_at| (task_id.clone(), ended_at))
            })
            .collect()
    }

    pub(crate) fn arm_request<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
        candidate: FixedMutationRequest,
    ) -> Result<FixedMutationRequest, StorageError> {
        let request = self
            .fixed_requests
            .get(task_id)
            .cloned()
            .unwrap_or(candidate);
        let mut fixed_requests = self.fixed_requests.clone();
        fixed_requests.insert(task_id.to_owned(), request.clone());
        self.store(storage, true, &self.committed_task_ids, &fixed_requests)?;
        self.mutation_blocked = true;
        self.fixed_requests = fixed_requests;
        Ok(request)
    }

    pub fn disarm<S: KeyValueStorage>(&mut self, storage: &S) -> Result<(), StorageError> {
        self.store(storage, false, &HashSet::new(), &HashMap::new())?;
        self.mutation_blocked = false;
        self.committed_task_ids.clear();
        self.fixed_requests.clear();
        Ok(())
    }

    pub fn disarm_retaining_requests<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        retained_task_ids: &HashSet<String>,
    ) -> Result<(), StorageError> {
        let fixed_requests = self
            .fixed_requests
            .iter()
            .filter(|(task_id, _)| retained_task_ids.contains(*task_id))
            .map(|(task_id, request)| (task_id.clone(), request.clone()))
            .collect::<HashMap<_, _>>();
        self.store(storage, false, &HashSet::new(), &fixed_requests)?;
        self.mutation_blocked = false;
        self.committed_task_ids.clear();
        self.fixed_requests = fixed_requests;
        Ok(())
    }

    pub fn mark_committed<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
    ) -> Result<(), StorageError> {
        let mut committed_task_ids = self.committed_task_ids.clone();
        committed_task_ids.insert(task_id.to_owned());
        self.store(storage, true, &committed_task_ids, &self.fixed_requests)?;
        self.mutation_blocked = true;
        self.committed_task_ids = committed_task_ids;
        Ok(())
    }

    fn store<S: KeyValueStorage>(
        &self,
        storage: &S,
        mutation_blocked: bool,
        committed_task_ids: &HashSet<String>,
        fixed_requests: &HashMap<String, FixedMutationRequest>,
    ) -> Result<(), StorageError> {
        let stored = StoredMutationSafety {
            version: STORAGE_VERSION,
            mutation_blocked,
            committed_task_ids: committed_task_ids.iter().cloned().collect(),
            fixed_requests: fixed_requests.clone(),
            discard_event_ids: HashMap::new(),
            discard_event_ended_at_epoch_ms: HashMap::new(),
        };
        let serialized = serde_json::to_string(&stored).map_err(|_| StorageError::WriteFailed)?;
        storage.set(MUTATION_SAFETY_STORAGE_KEY, &serialized)
    }
}
