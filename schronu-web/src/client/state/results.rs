use super::*;
use crate::client::date_buttons::logical_date_buttons;
use crate::{RecordSessionResult, RetryAdvice, WebSuccess};

impl ClientState {
    pub fn apply_bootstrap_result(
        &mut self,
        result: Result<ServerSnapshot, ServerFailure>,
    ) -> ClientEffect {
        match result {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
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
        requested_date: &str,
        result: Result<WebSuccess<Vec<ScheduledTaskRow>>, ServerFailure>,
    ) -> ClientEffect {
        match result {
            Ok(success) => {
                let logical_date_changed = self.apply_snapshot(success.snapshot);
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
        result: Result<WebSuccess<Option<SessionTask>>, ServerFailure>,
    ) -> ClientEffect {
        match result {
            Ok(success) => {
                self.apply_snapshot(success.snapshot);
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
                self.apply_snapshot(success.snapshot);
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
                self.apply_snapshot(snapshot);
                self.finish_committed_mutation(storage, &task_id, Operation::CompleteSession, None);
            }
            Err(error) => self.finish_failed_mutation(&task_id, Operation::CompleteSession, error),
        }
        ClientEffect::None
    }

    fn apply_snapshot(&mut self, snapshot: ServerSnapshot) -> bool {
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
        changed
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
        match self.work_sessions.replace_sessions(storage, candidate) {
            Ok(()) => {
                self.manual_check_blocked_task_ids.remove(task_id);
                self.record_server(
                    operation,
                    Some(task_id),
                    Outcome::Success,
                    "操作が完了しました。",
                );
            }
            Err(_) => {
                self.committed_blocked_task_ids.insert(task_id.to_owned());
                if let Some(actual) = actual_work_seconds {
                    self.committed_actual_work_seconds
                        .insert(task_id.to_owned(), actual);
                }
                self.display_error = Some(DisplayError {
                    message: "serverでは保存済みですが、localStorageの更新に失敗しました。再送せず状態を確認してください。".to_owned(),
                    retryable: false,
                });
                self.record_server(
                    operation,
                    Some(task_id),
                    Outcome::Failure,
                    "server保存後にlocalStorage更新へ失敗しました。",
                );
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
            (Outcome::Success, "localStorageを更新しました。")
        } else {
            self.display_error = Some(DisplayError {
                message: "localStorageを更新できませんでした。".to_owned(),
                retryable: false,
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
            ServerFailure::Operation(error) => DisplayError {
                message: error.message,
                retryable: error.retry_advice == RetryAdvice::Retry,
            },
            ServerFailure::Transport(_) => DisplayError {
                message: "通信に失敗しました。時間をおいて再試行してください。".to_owned(),
                retryable: true,
            },
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
        self.record_history(operation, task_id, Locality::Server, outcome, summary);
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
