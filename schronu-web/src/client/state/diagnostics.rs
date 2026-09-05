use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisplayError {
    Operation {
        error: WebError,
        operation: Operation,
        task_id: Option<String>,
    },
    Transport {
        operation: Operation,
        task_id: Option<String>,
    },
    LocalStorage {
        committed_on_server: bool,
        task_id: Option<String>,
    },
}

impl DisplayError {
    pub fn message(&self) -> &str {
        match self {
            Self::Operation { error, .. } => &error.message,
            Self::Transport { operation, .. }
                if is_read_operation(*operation) =>
            {
                "通信に失敗しました。時間をおいて再試行してください。"
            }
            Self::Transport { .. } => {
                "通信結果を確認できません。repositoryの状態を手動確認してください。"
            }
            Self::LocalStorage {
                committed_on_server: true,
                ..
            } => "serverでは保存済みですが、localStorageの更新に失敗しました。再送せず状態を確認してください。",
            Self::LocalStorage {
                committed_on_server: false,
                ..
            } => "localStorageを更新できませんでした。",
        }
    }

    pub fn retryable(&self) -> bool {
        match self {
            Self::Transport { operation, .. } => is_read_operation(*operation),
            Self::Operation { error, .. } => error.retry_advice == crate::RetryAdvice::Retry,
            Self::LocalStorage { .. } => false,
        }
    }

    pub(super) fn is_resolved_by_server_success(
        &self,
        operation: Operation,
        task_id: Option<&str>,
    ) -> bool {
        matches!(
            self,
            Self::Transport {
                operation: error_operation,
                task_id: error_task_id,
            } if *error_operation == operation && error_task_id.as_deref() == task_id
        ) || matches!(
            self,
            Self::Operation {
                error,
                operation: error_operation,
                task_id: error_task_id,
            } if *error_operation == operation
                && error_task_id.as_deref() == task_id
                && (error.retry_advice == crate::RetryAdvice::Retry
                    || (error_task_id.is_none()
                        && error.code
                            != crate::web_error_codes::REPOSITORY_STATE_UNCERTAIN))
        )
    }

    pub(super) fn is_resolved_by_local_success(&self) -> bool {
        matches!(
            self,
            Self::LocalStorage {
                committed_on_server: false,
                ..
            }
        )
    }

    pub(super) fn is_resolved_by_discard(&self, task_id: &str) -> bool {
        matches!(
            self,
            Self::Operation {
                error: WebError {
                    code,
                    retry_advice: crate::RetryAdvice::ManualCheck,
                    ..
                },
                operation: _,
                task_id: Some(error_task_id),
            } if error_task_id == task_id
                && code != crate::web_error_codes::REPOSITORY_STATE_UNCERTAIN
        )
    }
}

pub(super) fn is_read_operation(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::Bootstrap | Operation::ListTasks | Operation::AutoSession
    )
}

pub(super) struct DiagnosticsState {
    pub(super) display_error: Option<DisplayError>,
    pub(super) history: VecDeque<OperationHistoryEntry>,
}

impl DiagnosticsState {
    pub(super) fn new() -> Self {
        Self {
            display_error: None,
            history: VecDeque::new(),
        }
    }
}

impl ClientState {
    pub(super) fn record_local_result(
        &mut self,
        operation: Operation,
        task_id: Option<&str>,
        succeeded: bool,
    ) {
        let (outcome, summary) = if succeeded {
            if self
                .diagnostics
                .display_error
                .as_ref()
                .is_some_and(DisplayError::is_resolved_by_local_success)
            {
                self.diagnostics.display_error = None;
            }
            (Outcome::Success, "localStorageを更新しました。")
        } else {
            self.diagnostics.display_error = Some(DisplayError::LocalStorage {
                committed_on_server: false,
                task_id: task_id.map(ToOwned::to_owned),
            });
            (Outcome::Failure, "localStorage更新に失敗しました。")
        };
        self.record_history(operation, task_id, Locality::Local, outcome, summary);
    }

    pub(super) fn record_server_failure(
        &mut self,
        operation: Operation,
        task_id: Option<&str>,
        error: ServerFailure,
    ) {
        self.diagnostics.display_error = Some(match error {
            ServerFailure::Operation(error) => DisplayError::Operation {
                error,
                operation,
                task_id: task_id.map(ToOwned::to_owned),
            },
            ServerFailure::Transport(_) => DisplayError::Transport {
                operation,
                task_id: task_id.map(ToOwned::to_owned),
            },
        });
        self.record_server(
            operation,
            task_id,
            Outcome::Failure,
            "server操作に失敗しました。",
        );
    }

    pub(super) fn record_server(
        &mut self,
        operation: Operation,
        task_id: Option<&str>,
        outcome: Outcome,
        summary: &str,
    ) {
        if outcome == Outcome::Success
            && self
                .diagnostics
                .display_error
                .as_ref()
                .is_some_and(|error| error.is_resolved_by_server_success(operation, task_id))
        {
            self.diagnostics.display_error = None;
        }
        self.record_history(operation, task_id, Locality::Server, outcome, summary);
    }

    pub(super) fn record_stale_response(&mut self, operation: Operation, succeeded: bool) {
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
            &mut self.diagnostics.history,
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
