use super::*;
use crate::client::date_buttons::logical_date_buttons;
use crate::{RecordSessionResult, RetryAdvice, WebSuccess};

impl ClientState {
    pub fn apply_bootstrap_result(
        &mut self,
        request_id: u64,
        result: Result<ServerSnapshot, ServerFailure>,
    ) -> ClientEffect {
        if !consume_latest(&mut self.latest_bootstrap_request_id, request_id) {
            self.record_stale_response(Operation::Bootstrap, result.is_ok());
            return ClientEffect::None;
        }
        match result {
            Ok(snapshot) => {
                if self.apply_snapshot(snapshot).is_none() {
                    return ClientEffect::None;
                }
                self.record_server(
                    Operation::Bootstrap,
                    None,
                    Outcome::Success,
                    "更新しました。",
                );
            }
            Err(error) => self.record_server_failure(Operation::Bootstrap, None, error),
        }
        ClientEffect::None
    }

    pub fn apply_list_result(
        &mut self,
        request_id: u64,
        requested_date: &str,
        result: Result<WebSuccess<Vec<ScheduledTaskRow>>, ServerFailure>,
    ) -> ClientEffect {
        if !consume_latest(&mut self.latest_list_request_id, request_id) {
            self.record_stale_response(Operation::ListTasks, result.is_ok());
            return ClientEffect::None;
        }
        match result {
            Ok(success) => {
                let Some(logical_date_changed) = self.apply_snapshot(success.snapshot) else {
                    return ClientEffect::None;
                };
                if !logical_date_changed {
                    self.selected_logical_date = Some(requested_date.to_owned());
                    self.scheduled_rows = success.data;
                }
                self.record_server(
                    Operation::ListTasks,
                    None,
                    Outcome::Success,
                    "一覧を更新しました。",
                );
            }
            Err(error) => self.record_server_failure(Operation::ListTasks, None, error),
        }
        ClientEffect::None
    }

    pub fn apply_auto_session_result<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        request_id: u64,
        result: Result<WebSuccess<Option<SessionTask>>, ServerFailure>,
    ) -> ClientEffect {
        if !consume_latest(&mut self.latest_auto_request_id, request_id) {
            self.record_stale_response(Operation::AutoSession, result.is_ok());
            return ClientEffect::None;
        }
        match result {
            Ok(success) => {
                if self.apply_snapshot(success.snapshot).is_none() {
                    return ClientEffect::None;
                }
                self.auto_session_empty = success.data.is_none();
                self.record_server(
                    Operation::AutoSession,
                    None,
                    Outcome::Success,
                    "自動選定が完了しました。",
                );
                if let Some(task) = success.data {
                    self.add_session(storage, &task);
                }
            }
            Err(error) => self.record_server_failure(Operation::AutoSession, None, error),
        }
        ClientEffect::None
    }

    pub fn apply_record_result<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        request_id: u64,
        result: Result<WebSuccess<RecordSessionResult>, ServerFailure>,
    ) -> ClientEffect {
        let Some(task_id) = self.take_pending_mutation(request_id, MutationKind::Record) else {
            return ClientEffect::None;
        };
        self.in_flight_task_ids.remove(&task_id);
        match result {
            Ok(success) => {
                let _ = self.apply_snapshot(success.snapshot);
                self.finish_committed_mutation(
                    storage,
                    &task_id,
                    Operation::RecordSession,
                    Some(success.data.actual_work_seconds),
                );
            }
            Err(error) => self.finish_failed_mutation(&task_id, Operation::RecordSession, error),
        }
        ClientEffect::None
    }

    pub fn apply_complete_result<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        request_id: u64,
        result: Result<ServerSnapshot, ServerFailure>,
    ) -> ClientEffect {
        let Some(task_id) = self.take_pending_mutation(request_id, MutationKind::Complete) else {
            return ClientEffect::None;
        };
        self.in_flight_task_ids.remove(&task_id);
        match result {
            Ok(snapshot) => {
                let _ = self.apply_snapshot(snapshot);
                self.finish_committed_mutation(storage, &task_id, Operation::CompleteSession, None);
            }
            Err(error) => self.finish_failed_mutation(&task_id, Operation::CompleteSession, error),
        }
        ClientEffect::None
    }

    fn apply_snapshot(&mut self, snapshot: ServerSnapshot) -> Option<bool> {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|current| current.observed_at_epoch_ms > snapshot.observed_at_epoch_ms)
        {
            return None;
        }
        let changed = self
            .snapshot
            .as_ref()
            .is_none_or(|current| current.logical_date != snapshot.logical_date);
        if changed {
            self.date_buttons = logical_date_buttons(&snapshot.logical_date).unwrap_or_default();
            self.selected_logical_date = None;
            self.scheduled_rows.clear();
        }
        self.snapshot = Some(snapshot);
        Some(changed)
    }

    fn finish_failed_mutation(
        &mut self,
        task_id: &str,
        operation: Operation,
        error: ServerFailure,
    ) {
        if matches!(
            &error,
            ServerFailure::Operation(WebError { code, .. })
                if code == crate::web_error_codes::REPOSITORY_STATE_UNCERTAIN
        ) {
            self.mutation_globally_blocked = true;
        }
        if matches!(
            &error,
            ServerFailure::Operation(WebError {
                retry_advice: RetryAdvice::ManualCheck,
                ..
            })
        ) {
            self.manual_check_blocked_task_ids
                .insert(task_id.to_owned());
        }
        self.record_server_failure(operation, Some(task_id), error);
    }

    fn finish_committed_mutation<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
        operation: Operation,
        actual_work_seconds: Option<i64>,
    ) {
        let candidate = self
            .sessions()
            .iter()
            .filter(|session| session.task_id != task_id)
            .cloned()
            .collect();
        self.record_server(
            operation,
            Some(task_id),
            Outcome::Success,
            "server操作が完了しました。",
        );
        match self.work_sessions.replace_sessions(storage, candidate) {
            Ok(()) => {
                self.manual_check_blocked_task_ids.remove(task_id);
                self.record_local_result(Operation::DiscardSession, Some(task_id), true);
            }
            Err(_) => {
                self.committed_blocked_task_ids.insert(task_id.to_owned());
                if let Some(actual) = actual_work_seconds {
                    self.committed_actual_work_seconds
                        .insert(task_id.to_owned(), actual);
                }
                self.record_local_result(Operation::DiscardSession, Some(task_id), false);
                self.display_error = Some(DisplayError::LocalStorage {
                    committed_on_server: true,
                });
            }
        }
    }

    pub(super) fn record_local_result(
        &mut self,
        operation: Operation,
        task_id: Option<&str>,
        succeeded: bool,
    ) {
        let (outcome, summary) = if succeeded {
            self.display_error = None;
            (Outcome::Success, "localStorageを更新しました。")
        } else {
            self.display_error = Some(DisplayError::LocalStorage {
                committed_on_server: false,
            });
            (Outcome::Failure, "localStorage更新に失敗しました。")
        };
        self.record_history(operation, task_id, Locality::Local, outcome, summary);
    }

    fn record_server_failure(
        &mut self,
        operation: Operation,
        task_id: Option<&str>,
        error: ServerFailure,
    ) {
        self.display_error = Some(match error {
            ServerFailure::Operation(error) => DisplayError::Operation(error),
            ServerFailure::Transport(_) => DisplayError::Transport,
        });
        self.record_server(
            operation,
            task_id,
            Outcome::Failure,
            "server操作に失敗しました。",
        );
    }

    fn record_server(
        &mut self,
        operation: Operation,
        task_id: Option<&str>,
        outcome: Outcome,
        summary: &str,
    ) {
        if outcome == Outcome::Success {
            self.display_error = None;
        }
        self.record_history(operation, task_id, Locality::Server, outcome, summary);
    }

    fn record_stale_response(&mut self, operation: Operation, succeeded: bool) {
        self.record_history(
            operation,
            None,
            Locality::Server,
            if succeeded {
                Outcome::Success
            } else {
                Outcome::Failure
            },
            "古い応答を表示へ適用せず受信しました。",
        );
    }

    fn record_history(
        &mut self,
        operation: Operation,
        task_id: Option<&str>,
        locality: Locality,
        outcome: Outcome,
        summary: &str,
    ) {
        super::super::history::push_history(
            &mut self.history,
            OperationHistoryEntry {
                occurred_at_epoch_ms: self.tick_now_epoch_ms,
                operation,
                task_id: task_id.map(ToOwned::to_owned),
                locality,
                outcome,
                summary: summary.to_owned(),
            },
        );
    }
}

fn consume_latest(latest_request_id: &mut Option<u64>, request_id: u64) -> bool {
    if *latest_request_id != Some(request_id) {
        return false;
    }
    *latest_request_id = None;
    true
}
