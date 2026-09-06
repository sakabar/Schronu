use super::diagnostics::is_read_operation;
use super::*;
use crate::{
    CompleteSessionRequest, RecordSessionRequest, RecordSessionResult, RetryAdvice, SessionTask,
    WebSuccess,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MutationKind {
    Record,
    Complete,
    CompleteWithoutRecording,
}

pub(super) struct PendingMutation {
    pub(super) invocation: ServerActionInvocation,
    pub(super) ended_at_epoch_ms: i64,
}

pub(super) struct SessionState {
    pub(super) work_sessions: WorkSessionsState,
    pub(super) in_flight_task_ids: HashSet<String>,
    pub(super) manual_check_blocked_task_ids: HashSet<String>,
    pub(super) committed_blocked_task_ids: HashSet<String>,
    pub(super) committed_actual_work_seconds: HashMap<String, i64>,
    pub(super) uncertain_stopped_at_epoch_ms: HashMap<String, i64>,
    pub(super) mutation_globally_blocked: bool,
    pub(super) mutation_safety: MutationSafetyState,
    pub(super) next_mutation_request_id: u64,
    pub(super) pending_mutations: HashMap<u64, PendingMutation>,
}

impl SessionState {
    pub(super) fn new(
        work_sessions: WorkSessionsState,
        mutation_safety: MutationSafetyState,
    ) -> Self {
        let committed_blocked_task_ids = mutation_safety.committed_task_ids().clone();
        Self {
            work_sessions,
            in_flight_task_ids: HashSet::new(),
            manual_check_blocked_task_ids: HashSet::new(),
            committed_blocked_task_ids,
            committed_actual_work_seconds: HashMap::new(),
            uncertain_stopped_at_epoch_ms: HashMap::new(),
            mutation_globally_blocked: mutation_safety.mutation_blocked(),
            mutation_safety,
            next_mutation_request_id: 1,
            pending_mutations: HashMap::new(),
        }
    }
}

impl ClientState {
    pub fn add_session_from_row<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        row: &ScheduledTaskRow,
    ) -> ClientEffect {
        self.add_session_from_list_task(storage, &row.task, row.is_leaf)
    }

    pub(crate) fn add_session_from_list_task<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task: &SessionTask,
        is_leaf: bool,
    ) -> ClientEffect {
        if !is_leaf {
            return ClientEffect::None;
        }
        self.add_session(storage, task);
        ClientEffect::None
    }

    pub fn discard_session<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
    ) -> ClientEffect {
        if self.sessions.in_flight_task_ids.contains(task_id)
            || self.sessions.committed_blocked_task_ids.contains(task_id)
        {
            return ClientEffect::None;
        }
        let candidate: Vec<_> = self
            .sessions()
            .iter()
            .filter(|session| session.task_id != task_id)
            .cloned()
            .collect();
        let found = candidate.len() != self.sessions().len();
        if !found {
            return ClientEffect::None;
        }
        let result = self
            .sessions
            .work_sessions
            .replace_sessions(storage, candidate);
        if result.is_ok() {
            self.restored_session_task_ids.remove(task_id);
            self.sessions.manual_check_blocked_task_ids.remove(task_id);
            self.sessions.uncertain_stopped_at_epoch_ms.remove(task_id);
            if self
                .diagnostics
                .display_error
                .as_ref()
                .is_some_and(|error| error.is_resolved_by_discard(task_id))
            {
                self.diagnostics.display_error = None;
            }
        }
        self.record_local_result(Some(task_id), result.is_ok());
        ClientEffect::None
    }

    pub fn begin_record_session<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
    ) -> ClientEffect {
        self.begin_mutation(storage, task_id, MutationKind::Record)
    }

    pub fn begin_complete_session<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
    ) -> ClientEffect {
        self.begin_mutation(storage, task_id, MutationKind::Complete)
    }

    pub fn begin_complete_session_without_recording<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
    ) -> ClientEffect {
        self.begin_mutation(storage, task_id, MutationKind::CompleteWithoutRecording)
    }

    pub fn confirm_repository_checked<S: KeyValueStorage>(&mut self, storage: &S) -> ClientEffect {
        if !self.can_confirm_repository_checked() {
            return ClientEffect::None;
        }
        if !self.sessions.committed_blocked_task_ids.is_empty() {
            let candidate = self
                .sessions()
                .iter()
                .filter(|session| {
                    !self
                        .sessions
                        .committed_blocked_task_ids
                        .contains(&session.task_id)
                })
                .cloned()
                .collect();
            if self
                .sessions
                .work_sessions
                .replace_sessions(storage, candidate)
                .is_err()
            {
                let task_id = self
                    .sessions
                    .committed_blocked_task_ids
                    .iter()
                    .next()
                    .cloned();
                self.record_local_result(task_id.as_deref(), false);
                self.diagnostics.display_error = Some(DisplayError::LocalStorage {
                    committed_on_server: true,
                    task_id,
                });
                return ClientEffect::None;
            }
            self.restored_session_task_ids
                .retain(|task_id| !self.sessions.committed_blocked_task_ids.contains(task_id));
            self.sessions.committed_blocked_task_ids.clear();
            self.sessions.committed_actual_work_seconds.clear();
        }
        let result = self.sessions.mutation_safety.disarm(storage);
        if result.is_ok() {
            self.sessions.mutation_globally_blocked = false;
            self.sessions.uncertain_stopped_at_epoch_ms.clear();
            if matches!(
                &self.diagnostics.display_error,
                Some(DisplayError::Operation {
                    error: WebError { code, .. },
                    ..
                }) if code == crate::web_error_codes::REPOSITORY_STATE_UNCERTAIN
            ) || matches!(
                &self.diagnostics.display_error,
                Some(DisplayError::LocalStorage {
                    committed_on_server: true,
                    ..
                })
            ) || matches!(
                &self.diagnostics.display_error,
                Some(DisplayError::Transport { operation, .. })
                    if !is_read_operation(*operation)
            ) {
                self.diagnostics.display_error = None;
            }
        }
        self.record_local_result(None, result.is_ok());
        ClientEffect::None
    }

    pub(super) fn add_session<S: KeyValueStorage>(&mut self, storage: &S, task: &SessionTask) {
        if self
            .sessions()
            .iter()
            .any(|session| session.task_id == task.task_id)
        {
            return;
        }
        let mut candidate = self.sessions().to_vec();
        candidate.push(WorkSession {
            task_id: task.task_id.clone(),
            task_name: task.task_name.clone(),
            started_at_epoch_ms: self.tick_now_epoch_ms,
            estimated_work_seconds_at_start: task.estimated_work_seconds,
            actual_work_seconds_at_start: task.actual_work_seconds,
        });
        let result = self
            .sessions
            .work_sessions
            .replace_sessions(storage, candidate);
        self.record_local_result(Some(&task.task_id), result.is_ok());
    }

    fn begin_mutation<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
        kind: MutationKind,
    ) -> ClientEffect {
        if self.sessions.mutation_globally_blocked
            || self.sessions.in_flight_task_ids.contains(task_id)
            || self
                .sessions
                .manual_check_blocked_task_ids
                .contains(task_id)
            || self.sessions.committed_blocked_task_ids.contains(task_id)
        {
            return ClientEffect::None;
        }
        let Some(session) = self
            .sessions()
            .iter()
            .find(|session| session.task_id == task_id)
        else {
            return ClientEffect::None;
        };
        let request_task_id = session.task_id.clone();
        let started_at_epoch_ms = session.started_at_epoch_ms;
        let expected_actual_work_seconds = session.actual_work_seconds_at_start;
        let request_id = self.sessions.next_mutation_request_id;
        let Some(next_request_id) = request_id.checked_add(1) else {
            return ClientEffect::None;
        };
        if self.sessions.pending_mutations.is_empty()
            && self.sessions.mutation_safety.arm(storage).is_err()
        {
            self.record_local_result(Some(task_id), false);
            return ClientEffect::None;
        }
        self.sessions.next_mutation_request_id = next_request_id;
        self.sessions.in_flight_task_ids.insert(task_id.to_owned());
        let ended_at_epoch_ms = self.tick_now_epoch_ms;
        let effect = match kind {
            MutationKind::Complete | MutationKind::CompleteWithoutRecording => {
                let request = CompleteSessionRequest {
                    task_id: request_task_id,
                    started_at_epoch_ms,
                    ended_at_epoch_ms: Some(ended_at_epoch_ms),
                    expected_actual_work_seconds,
                    record_elapsed_seconds: kind == MutationKind::Complete,
                };
                ClientEffect::CompleteSession {
                    request_id,
                    request,
                }
            }
            MutationKind::Record => {
                let request = RecordSessionRequest {
                    task_id: request_task_id,
                    started_at_epoch_ms,
                    ended_at_epoch_ms: Some(ended_at_epoch_ms),
                    expected_actual_work_seconds,
                };
                ClientEffect::RecordSession {
                    request_id,
                    request,
                }
            }
        };
        let invocation = match &effect {
            ClientEffect::RecordSession { request, .. } => {
                ServerActionInvocation::RecordSession(request.clone())
            }
            ClientEffect::CompleteSession { request, .. } => {
                ServerActionInvocation::CompleteSession(request.clone())
            }
            _ => unreachable!("mutation must produce a mutation effect"),
        };
        self.sessions.pending_mutations.insert(
            request_id,
            PendingMutation {
                invocation,
                ended_at_epoch_ms,
            },
        );
        effect
    }

    fn take_pending_record(&mut self, request_id: u64) -> Option<PendingMutation> {
        let pending = self.sessions.pending_mutations.get(&request_id)?;
        let ServerActionInvocation::RecordSession(_) = &pending.invocation else {
            return None;
        };
        self.sessions.pending_mutations.remove(&request_id)
    }

    fn take_pending_completion(&mut self, request_id: u64) -> Option<PendingMutation> {
        let pending = self.sessions.pending_mutations.get(&request_id)?;
        let ServerActionInvocation::CompleteSession(_) = &pending.invocation else {
            return None;
        };
        self.sessions.pending_mutations.remove(&request_id)
    }

    pub fn apply_record_result<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        request_id: u64,
        result: Result<WebSuccess<RecordSessionResult>, ServerFailure>,
    ) -> ClientEffect {
        let Some(pending) = self.take_pending_record(request_id) else {
            return ClientEffect::None;
        };
        let PendingMutation {
            invocation,
            ended_at_epoch_ms,
        } = pending;
        let task_id = invocation
            .task_id()
            .expect("record invocation must have a task id")
            .to_owned();
        self.sessions.in_flight_task_ids.remove(&task_id);
        match result {
            Ok(success) => {
                let _ = self.apply_snapshot(success.snapshot);
                self.finish_committed_mutation(
                    storage,
                    &task_id,
                    invocation,
                    Some(success.data.actual_work_seconds),
                );
                self.finish_mutation_safety(storage, false);
            }
            Err(error) => {
                let keep_safety = keeps_safety_marker(&error);
                if keep_safety {
                    self.sessions
                        .uncertain_stopped_at_epoch_ms
                        .insert(task_id.clone(), ended_at_epoch_ms);
                }
                self.finish_failed_mutation(&task_id, invocation, error);
                self.finish_mutation_safety(storage, keep_safety);
            }
        }
        ClientEffect::None
    }

    pub fn apply_complete_result<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        request_id: u64,
        result: Result<ServerSnapshot, ServerFailure>,
    ) -> ClientEffect {
        let Some(pending) = self.take_pending_completion(request_id) else {
            return ClientEffect::None;
        };
        let PendingMutation {
            invocation,
            ended_at_epoch_ms,
        } = pending;
        let task_id = invocation
            .task_id()
            .expect("complete invocation must have a task id")
            .to_owned();
        self.sessions.in_flight_task_ids.remove(&task_id);
        match result {
            Ok(snapshot) => {
                self.apply_successful_completion_to_read_state(snapshot, &task_id);
                self.finish_committed_mutation(storage, &task_id, invocation, None);
                self.finish_mutation_safety(storage, false);
            }
            Err(error) => {
                let keep_safety = keeps_safety_marker(&error);
                if keep_safety {
                    self.sessions
                        .uncertain_stopped_at_epoch_ms
                        .insert(task_id.clone(), ended_at_epoch_ms);
                }
                self.finish_failed_mutation(&task_id, invocation, error);
                self.finish_mutation_safety(storage, keep_safety);
            }
        }
        ClientEffect::None
    }

    fn finish_failed_mutation(
        &mut self,
        task_id: &str,
        invocation: ServerActionInvocation,
        error: ServerFailure,
    ) {
        if matches!(&error, ServerFailure::Transport(_))
            || matches!(
                &error,
                ServerFailure::Operation(WebError { code, .. })
                    if code == crate::web_error_codes::REPOSITORY_STATE_UNCERTAIN
            )
        {
            self.sessions.mutation_globally_blocked = true;
        }
        if matches!(
            &error,
            ServerFailure::Operation(WebError {
                code,
                retry_advice: RetryAdvice::ManualCheck,
                ..
            }) if code != crate::web_error_codes::REPOSITORY_STATE_UNCERTAIN
        ) {
            self.sessions
                .manual_check_blocked_task_ids
                .insert(task_id.to_owned());
        }
        self.record_server_failure(invocation, error);
    }

    fn finish_mutation_safety<S: KeyValueStorage>(&mut self, storage: &S, keep_armed: bool) {
        if keep_armed
            || !self.sessions.pending_mutations.is_empty()
            || self.sessions.mutation_globally_blocked
        {
            return;
        }
        if !self.sessions.committed_blocked_task_ids.is_empty() {
            self.sessions.mutation_globally_blocked = true;
            return;
        }
        if self.sessions.mutation_safety.disarm(storage).is_err()
            && self.sessions.committed_blocked_task_ids.is_empty()
        {
            self.record_local_result(None, false);
        }
    }

    fn finish_committed_mutation<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
        invocation: ServerActionInvocation,
        actual_work_seconds: Option<i64>,
    ) {
        let candidate = self
            .sessions()
            .iter()
            .filter(|session| session.task_id != task_id)
            .cloned()
            .collect();
        self.record_server(invocation, Outcome::Success, "server操作が完了しました。");
        match self
            .sessions
            .work_sessions
            .replace_sessions(storage, candidate)
        {
            Ok(()) => {
                self.restored_session_task_ids.remove(task_id);
                self.sessions.manual_check_blocked_task_ids.remove(task_id);
                self.sessions.uncertain_stopped_at_epoch_ms.remove(task_id);
                self.record_local_result(Some(task_id), true);
            }
            Err(_) => {
                self.sessions
                    .committed_blocked_task_ids
                    .insert(task_id.to_owned());
                if self
                    .sessions
                    .mutation_safety
                    .mark_committed(storage, task_id)
                    .is_err()
                {
                    self.sessions.mutation_globally_blocked = true;
                }
                if let Some(actual) = actual_work_seconds {
                    self.sessions
                        .committed_actual_work_seconds
                        .insert(task_id.to_owned(), actual);
                }
                self.record_local_result(Some(task_id), false);
                self.diagnostics.display_error = Some(DisplayError::LocalStorage {
                    committed_on_server: true,
                    task_id: Some(task_id.to_owned()),
                });
            }
        }
    }
}

fn keeps_safety_marker(error: &ServerFailure) -> bool {
    matches!(error, ServerFailure::Transport(_))
        || matches!(
            error,
            ServerFailure::Operation(WebError { code, .. })
                if code == crate::web_error_codes::REPOSITORY_STATE_UNCERTAIN
        )
}
