use super::diagnostics::is_read_operation;
use super::*;
use crate::client::safety_state::{FixedMutationKind, FixedMutationRequest};
use crate::{
    CompleteSessionRequest, DiscardSessionRequest, RecordSessionRequest, RecordSessionResult,
    RetryAdvice, SessionTask, WebSuccess,
};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MutationKind {
    Record,
    Complete,
    CompleteWithoutRecording,
    Discard,
}

impl MutationKind {
    fn fixed_kind(self) -> FixedMutationKind {
        match self {
            Self::Record => FixedMutationKind::Record,
            Self::Complete => FixedMutationKind::Complete,
            Self::CompleteWithoutRecording => FixedMutationKind::CompleteWithoutRecording,
            Self::Discard => FixedMutationKind::Discard,
        }
    }
}

pub(super) struct PendingMutation {
    pub(super) invocation: ServerActionInvocation,
    pub(super) ended_at_epoch_ms: i64,
}

#[derive(Clone)]
pub(crate) struct CompletionConflict {
    pub(crate) original_request: CompleteSessionRequest,
    pub(crate) ended_at_epoch_ms: i64,
    pub(crate) current_actual_work_seconds: i64,
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
    pub(super) completion_conflicts: HashMap<String, CompletionConflict>,
}

impl SessionState {
    pub(super) fn new(
        work_sessions: WorkSessionsState,
        mutation_safety: MutationSafetyState,
    ) -> Self {
        let committed_blocked_task_ids = mutation_safety.committed_task_ids().clone();
        let uncertain_stopped_at_epoch_ms = mutation_safety.unresolved_ended_at_epoch_ms();
        Self {
            work_sessions,
            in_flight_task_ids: HashSet::new(),
            manual_check_blocked_task_ids: HashSet::new(),
            committed_blocked_task_ids,
            committed_actual_work_seconds: HashMap::new(),
            uncertain_stopped_at_epoch_ms,
            mutation_globally_blocked: mutation_safety.mutation_blocked(),
            mutation_safety,
            next_mutation_request_id: 1,
            pending_mutations: HashMap::new(),
            completion_conflicts: HashMap::new(),
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

    pub fn begin_discard_session<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
    ) -> ClientEffect {
        self.begin_mutation(storage, task_id, MutationKind::Discard)
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
            self.sessions.committed_blocked_task_ids.clear();
            self.sessions.committed_actual_work_seconds.clear();
        }
        let uncertain_task_ids = self
            .sessions
            .uncertain_stopped_at_epoch_ms
            .keys()
            .cloned()
            .collect::<HashSet<_>>();
        let result = self
            .sessions
            .mutation_safety
            .disarm_retaining_requests(storage, &uncertain_task_ids);
        if result.is_ok() {
            self.sessions.mutation_globally_blocked = false;
            self.sessions.uncertain_stopped_at_epoch_ms =
                self.sessions.mutation_safety.unresolved_ended_at_epoch_ms();
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
        let discard_can_exit_task_scoped_block = kind == MutationKind::Discard;
        if self.sessions.mutation_globally_blocked
            || self.sessions.in_flight_task_ids.contains(task_id)
            || (!discard_can_exit_task_scoped_block
                && (self
                    .sessions
                    .manual_check_blocked_task_ids
                    .contains(task_id)
                    || self.sessions.completion_conflicts.contains_key(task_id)))
            || self.sessions.committed_blocked_task_ids.contains(task_id)
        {
            return ClientEffect::None;
        }
        if self
            .sessions
            .mutation_safety
            .fixed_request_kind(task_id)
            .is_some_and(|fixed| fixed != kind.fixed_kind())
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
        let task_name_at_start = session.task_name.clone();
        let started_at_epoch_ms = session.started_at_epoch_ms;
        let expected_actual_work_seconds = session.actual_work_seconds_at_start;
        let request_id = self.sessions.next_mutation_request_id;
        let Some(next_request_id) = request_id.checked_add(1) else {
            return ClientEffect::None;
        };
        let ended_at_epoch_ms = self.tick_now_epoch_ms;
        let candidate = match kind {
            MutationKind::Record => FixedMutationRequest::Record(RecordSessionRequest {
                task_id: request_task_id,
                started_at_epoch_ms,
                ended_at_epoch_ms: Some(ended_at_epoch_ms),
                expected_actual_work_seconds,
            }),
            MutationKind::Complete | MutationKind::CompleteWithoutRecording => {
                FixedMutationRequest::Complete(CompleteSessionRequest {
                    task_id: request_task_id,
                    started_at_epoch_ms,
                    ended_at_epoch_ms: Some(ended_at_epoch_ms),
                    expected_actual_work_seconds,
                    record_elapsed_seconds: kind == MutationKind::Complete,
                    discard_event_id: (kind == MutationKind::CompleteWithoutRecording)
                        .then(|| Uuid::new_v4().to_string()),
                    task_name_at_start: Some(task_name_at_start),
                })
            }
            MutationKind::Discard => FixedMutationRequest::Discard(DiscardSessionRequest {
                event_id: Uuid::new_v4().to_string(),
                task_id: request_task_id,
                task_name_at_start,
                started_at_epoch_ms,
                ended_at_epoch_ms,
            }),
        };
        let fixed_request = match self
            .sessions
            .mutation_safety
            .arm_request(storage, task_id, candidate)
        {
            Ok(request) => request,
            Err(_) => {
                self.record_local_result(Some(task_id), false);
                return ClientEffect::None;
            }
        };
        self.sessions.next_mutation_request_id = next_request_id;
        self.sessions.in_flight_task_ids.insert(task_id.to_owned());
        let ended_at_epoch_ms = fixed_request
            .ended_at_epoch_ms()
            .expect("stored mutation request must contain its fixed end time");
        let effect = match fixed_request {
            FixedMutationRequest::Record(request) => ClientEffect::RecordSession {
                request_id,
                request,
            },
            FixedMutationRequest::Complete(request) => ClientEffect::CompleteSession {
                request_id,
                request,
            },
            FixedMutationRequest::Discard(request) => ClientEffect::DiscardSession {
                request_id,
                request,
            },
        };
        let invocation = match &effect {
            ClientEffect::RecordSession { request, .. } => {
                ServerActionInvocation::RecordSession(request.clone())
            }
            ClientEffect::CompleteSession { request, .. } => {
                ServerActionInvocation::CompleteSession(request.clone())
            }
            ClientEffect::DiscardSession { request, .. } => {
                ServerActionInvocation::DiscardSession(request.clone())
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

    fn take_pending_discard(&mut self, request_id: u64) -> Option<PendingMutation> {
        let pending = self.sessions.pending_mutations.get(&request_id)?;
        let ServerActionInvocation::DiscardSession(_) = &pending.invocation else {
            return None;
        };
        self.sessions.pending_mutations.remove(&request_id)
    }

    pub fn apply_discard_result<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        request_id: u64,
        result: Result<ServerSnapshot, ServerFailure>,
    ) -> ClientEffect {
        let Some(pending) = self.take_pending_discard(request_id) else {
            return ClientEffect::None;
        };
        let task_id = pending
            .invocation
            .task_id()
            .expect("discard invocation must have task id")
            .to_owned();
        self.sessions.in_flight_task_ids.remove(&task_id);
        match result {
            Ok(snapshot) => {
                self.sessions.completion_conflicts.remove(&task_id);
                self.clear_task_error_after_discard(&task_id);
                let follow_up = self.apply_mutation_snapshot_and_request_list(snapshot);
                self.finish_committed_mutation(storage, &task_id, pending.invocation, None);
                self.finish_mutation_safety(storage, false);
                follow_up
            }
            Err(error) => {
                let keep_safety = keeps_safety_marker(&error);
                if keep_safety {
                    self.sessions
                        .uncertain_stopped_at_epoch_ms
                        .insert(task_id.clone(), pending.ended_at_epoch_ms);
                }
                self.finish_failed_mutation(&task_id, pending.invocation, error);
                self.finish_mutation_safety(storage, keep_safety);
                ClientEffect::None
            }
        }
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
                let follow_up = self.apply_mutation_snapshot_and_request_list(success.snapshot);
                self.finish_committed_mutation(
                    storage,
                    &task_id,
                    invocation,
                    Some(success.data.actual_work_seconds),
                );
                self.finish_mutation_safety(storage, false);
                return follow_up;
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
                self.sessions.completion_conflicts.remove(&task_id);
                self.clear_superseded_completion_error(&task_id);
                let follow_up = self.apply_mutation_snapshot_and_request_list(snapshot);
                self.finish_committed_mutation(storage, &task_id, invocation, None);
                self.finish_mutation_safety(storage, false);
                return follow_up;
            }
            Err(error) => {
                let keep_safety = keeps_safety_marker(&error);
                if keep_safety {
                    self.sessions
                        .uncertain_stopped_at_epoch_ms
                        .insert(task_id.clone(), ended_at_epoch_ms);
                }
                if let Some(current_actual_work_seconds) = completion_conflict_actual(&error) {
                    self.retain_completion_conflict(
                        &task_id,
                        &invocation,
                        ended_at_epoch_ms,
                        current_actual_work_seconds,
                    );
                    self.clear_superseded_completion_error(&task_id);
                    self.record_server(invocation, Outcome::Failure, "server操作に失敗しました。");
                } else {
                    self.sessions.completion_conflicts.remove(&task_id);
                    self.finish_failed_mutation(&task_id, invocation, error);
                }
                self.finish_mutation_safety(storage, keep_safety);
            }
        }
        ClientEffect::None
    }

    pub fn confirm_completion_conflict<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
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
        let Some(conflict) = self.sessions.completion_conflicts.get(task_id).cloned() else {
            return ClientEffect::None;
        };
        let mut request = conflict.original_request;
        request.expected_actual_work_seconds = conflict.current_actual_work_seconds;
        self.enqueue_completion(storage, request)
    }

    pub fn resume_completion_conflict<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
    ) -> ClientEffect {
        if self.sessions.mutation_globally_blocked
            || self.sessions.in_flight_task_ids.contains(task_id)
            || !self.sessions.pending_mutations.is_empty()
            || self
                .sessions
                .manual_check_blocked_task_ids
                .contains(task_id)
            || self.sessions.committed_blocked_task_ids.contains(task_id)
        {
            return ClientEffect::None;
        }
        let Some(conflict) = self.sessions.completion_conflicts.get(task_id).cloned() else {
            return ClientEffect::None;
        };
        let measured_milliseconds = (i128::from(conflict.ended_at_epoch_ms)
            - i128::from(conflict.original_request.started_at_epoch_ms))
        .max(0);
        let Some(started_at_epoch_ms) = i64::try_from(measured_milliseconds)
            .ok()
            .and_then(|elapsed| self.tick_now_epoch_ms.checked_sub(elapsed))
        else {
            self.record_local_result(Some(task_id), false);
            return ClientEffect::None;
        };
        let mut candidate = self.sessions().to_vec();
        let Some(session) = candidate
            .iter_mut()
            .find(|session| session.task_id == task_id)
        else {
            return ClientEffect::None;
        };
        session.started_at_epoch_ms = started_at_epoch_ms;
        session.actual_work_seconds_at_start = conflict.current_actual_work_seconds;
        let mut persisted_work_sessions = self.sessions.work_sessions.clone();
        if persisted_work_sessions
            .replace_sessions(storage, candidate)
            .is_err()
        {
            self.record_local_result(Some(task_id), false);
            return ClientEffect::None;
        }
        if self.sessions.mutation_safety.disarm(storage).is_err() {
            self.sessions.mutation_globally_blocked = true;
            self.record_local_result(Some(task_id), false);
            return ClientEffect::None;
        }
        self.sessions.work_sessions = persisted_work_sessions;
        self.sessions.completion_conflicts.remove(task_id);
        self.sessions.manual_check_blocked_task_ids.remove(task_id);
        self.sessions.uncertain_stopped_at_epoch_ms.remove(task_id);
        self.clear_superseded_completion_error(task_id);
        self.record_local_result(Some(task_id), true);
        ClientEffect::None
    }

    fn enqueue_completion<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        request: CompleteSessionRequest,
    ) -> ClientEffect {
        let request_id = self.sessions.next_mutation_request_id;
        let Some(next_request_id) = request_id.checked_add(1) else {
            return ClientEffect::None;
        };
        let task_id = request.task_id.clone();
        let requested_kind = FixedMutationRequest::Complete(request.clone()).kind();
        if self
            .sessions
            .mutation_safety
            .fixed_request_kind(&task_id)
            .is_some_and(|fixed| fixed != requested_kind)
        {
            return ClientEffect::None;
        }
        let request = match self.sessions.mutation_safety.arm_request(
            storage,
            &task_id,
            FixedMutationRequest::Complete(request),
        ) {
            Ok(FixedMutationRequest::Complete(request)) => request,
            Ok(_) => unreachable!("matching completion kind must retain a completion request"),
            Err(_) => {
                self.record_local_result(Some(&task_id), false);
                return ClientEffect::None;
            }
        };
        let ended_at_epoch_ms = request
            .ended_at_epoch_ms
            .expect("stored completion request must contain its fixed end time");
        let invocation = ServerActionInvocation::CompleteSession(request.clone());
        self.sessions.next_mutation_request_id = next_request_id;
        self.sessions.in_flight_task_ids.insert(task_id);
        self.sessions.pending_mutations.insert(
            request_id,
            PendingMutation {
                invocation,
                ended_at_epoch_ms,
            },
        );
        ClientEffect::CompleteSession {
            request_id,
            request,
        }
    }

    fn retain_completion_conflict(
        &mut self,
        task_id: &str,
        invocation: &ServerActionInvocation,
        ended_at_epoch_ms: i64,
        current_actual_work_seconds: i64,
    ) {
        if let Some(conflict) = self.sessions.completion_conflicts.get_mut(task_id) {
            conflict.current_actual_work_seconds = current_actual_work_seconds;
            return;
        }
        let ServerActionInvocation::CompleteSession(original_request) = invocation else {
            return;
        };
        self.sessions.completion_conflicts.insert(
            task_id.to_owned(),
            CompletionConflict {
                original_request: original_request.clone(),
                ended_at_epoch_ms,
                current_actual_work_seconds,
            },
        );
    }

    fn finish_failed_mutation(
        &mut self,
        task_id: &str,
        invocation: ServerActionInvocation,
        error: ServerFailure,
    ) {
        if !keeps_safety_marker(&error) {
            self.sessions.uncertain_stopped_at_epoch_ms.remove(task_id);
        }
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
            self.sessions.mutation_globally_blocked = true;
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
        self.sessions.uncertain_stopped_at_epoch_ms.remove(task_id);
        self.record_server(invocation, Outcome::Success, "server操作が完了しました。");
        if self
            .sessions
            .mutation_safety
            .mark_committed(storage, task_id)
            .is_err()
        {
            self.keep_committed_session(task_id, actual_work_seconds);
            return;
        }
        let candidate = self
            .sessions()
            .iter()
            .filter(|session| session.task_id != task_id)
            .cloned()
            .collect();
        match self
            .sessions
            .work_sessions
            .replace_sessions(storage, candidate)
        {
            Ok(()) => {
                self.sessions.manual_check_blocked_task_ids.remove(task_id);
                self.sessions.uncertain_stopped_at_epoch_ms.remove(task_id);
                self.record_local_result(Some(task_id), true);
            }
            Err(_) => {
                self.keep_committed_session(task_id, actual_work_seconds);
            }
        }
    }

    fn keep_committed_session(&mut self, task_id: &str, actual_work_seconds: Option<i64>) {
        self.sessions
            .committed_blocked_task_ids
            .insert(task_id.to_owned());
        self.sessions.mutation_globally_blocked = true;
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

fn completion_conflict_actual(error: &ServerFailure) -> Option<i64> {
    match error {
        ServerFailure::Operation(WebError {
            code,
            current_actual_work_seconds: Some(current_actual_work_seconds),
            ..
        }) if code == crate::web_error_codes::ACTUAL_WORK_CONFLICT
            && *current_actual_work_seconds >= 0 =>
        {
            Some(*current_actual_work_seconds)
        }
        _ => None,
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
