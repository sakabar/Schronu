use crate::client::offline::OfflineState;
use crate::client::work_sessions::WorkSession;
use crate::{web_error_codes as code, OfflineOperation, WebError};
use std::collections::HashMap;

pub(super) enum PushError {
    TaskConflict(String),
    Unavailable(String),
}

impl From<WebError> for PushError {
    fn from(error: WebError) -> Self {
        match error.code.as_str() {
            code::TASK_NOT_FOUND
            | code::TASK_ALREADY_COMPLETED
            | code::ACTUAL_WORK_CONFLICT
            | code::TASK_NOT_COMPLETABLE
            | code::INVALID_INPUT
            | code::ARITHMETIC_OVERFLOW
            | code::OFFLINE_CONFLICT => Self::TaskConflict(error.message),
            _ => Self::Unavailable(error.message),
        }
    }
}

pub(super) trait SyncIo {
    async fn push(&mut self, operation: OfflineOperation) -> Result<(), PushError>;
    async fn checkpoint(&mut self, state: &OfflineState) -> Result<(), String>;
}

/// Persist each acknowledgement before sending the next operation. A lost reply leaves
/// the original operation ID available for replay, including after a browser restart.
pub(super) async fn send_pending(
    state: &mut OfflineState,
    io: &mut impl SyncIo,
) -> Result<usize, String> {
    let mut sent = 0;
    for operation in state.pending.clone() {
        if state.conflicts.contains_key(&operation.task_id) {
            continue;
        }
        match io.push(operation.clone()).await {
            Ok(()) => {
                state
                    .pending
                    .retain(|queued| queued.operation_id != operation.operation_id);
                sent += 1;
            }
            Err(PushError::TaskConflict(reason)) => {
                state.conflicts.insert(operation.task_id, reason);
            }
            Err(PushError::Unavailable(reason)) => return Err(reason),
        }
        io.checkpoint(state).await?;
    }
    Ok(sent)
}

/// Keep independent sessions added in another tab. A repeated handoff after a crash
/// produces the same result, including when the localStorage write already succeeded.
pub(super) fn merge_sessions(
    original: &[WorkSession],
    offline: &[WorkSession],
    online: &[WorkSession],
) -> Result<Vec<WorkSession>, String> {
    let by_id = |sessions: &[WorkSession]| -> HashMap<String, WorkSession> {
        sessions
            .iter()
            .map(|s| (s.task_id.clone(), s.clone()))
            .collect()
    };
    let original = by_id(original);
    let current = by_id(online);
    let mut result = Vec::new();
    for session in offline {
        let base = original.get(&session.task_id);
        let live = current.get(&session.task_id);
        if live == base || live == Some(session) {
            result.push(session.clone());
        } else if Some(session) == base {
            result.extend(live.cloned());
        } else if live.is_some() {
            return Err(format!("「{}」を別のタブでも計測中です。片方のセッションを終了してから再度同期してください。", session.task_name));
        } else {
            result.push(session.clone());
        }
    }
    for session in online {
        if original.get(&session.task_id) != Some(session)
            && !offline.iter().any(|s| s.task_id == session.task_id)
        {
            result.push(session.clone());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LoadData, OfflineBundle, OfflineOperationKind, RoutineLoadReport, ServerSnapshot};
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    fn session(id: &str, start: i64) -> WorkSession {
        WorkSession {
            task_id: id.into(),
            task_name: id.into(),
            started_at_epoch_ms: start,
            actual_work_seconds_at_start: 0,
            estimated_work_seconds_at_start: 600,
            estimate_success_percentage: None,
            buffer_credit_start_epoch_ms: None,
            start_time_epoch_ms: None,
            pending_until_epoch_ms: None,
        }
    }

    #[test]
    fn 引き継ぎは他のタブの計測を維持し再実行できる() {
        let base = vec![session("ended", 1), session("running", 2)];
        let offline = vec![session("running", 2), session("new", 3)];
        let mut online = base.clone();
        online.push(session("other", 4));
        let merged = merge_sessions(&base, &offline, &online).unwrap();
        assert_eq!(
            merged,
            [
                session("running", 2),
                session("new", 3),
                session("other", 4)
            ]
        );
        assert_eq!(merge_sessions(&base, &offline, &merged).unwrap(), merged);
        assert!(merge_sessions(&[], &[session("same", 3)], &[session("same", 4)]).is_err());
    }

    fn operation(task: &str, id: &str) -> OfflineOperation {
        OfflineOperation {
            operation_id: id.into(),
            task_id: task.into(),
            kind: OfflineOperationKind::Record,
            started_at_epoch_ms: 0,
            ended_at_epoch_ms: 1000,
            expected_actual_work_seconds: 0,
            expected_estimated_work_seconds: None,
        }
    }

    struct Fake {
        replies: VecDeque<Result<(), PushError>>,
        sent: Vec<String>,
        checkpoints: Vec<OfflineState>,
    }

    impl SyncIo for Fake {
        async fn push(&mut self, op: OfflineOperation) -> Result<(), PushError> {
            self.sent.push(op.operation_id);
            self.replies.pop_front().unwrap()
        }
        async fn checkpoint(&mut self, state: &OfflineState) -> Result<(), String> {
            self.checkpoints.push(state.clone());
            Ok(())
        }
    }

    #[test]
    fn 競合は同一タスクだけ止め通信断後は未確認の操作から再送する() {
        let bundle = OfflineBundle {
            snapshot: ServerSnapshot {
                observed_at_epoch_ms: 0,
                logical_date: "2026-01-01".into(),
                buffer_seconds: 0,
            },
            days: BTreeMap::new(),
            splittable_task_ids: BTreeSet::new(),
            load: LoadData {
                band_days: vec![],
                routine_load: RoutineLoadReport {
                    start_date: "2026-01-01".into(),
                    end_date: "2026-01-28".into(),
                    horizon_day_count: 28,
                    full_day_available_seconds_by_date: BTreeMap::new(),
                    forecast_by_date: BTreeMap::new(),
                    rows: vec![],
                },
            },
        };
        let mut state = OfflineState::new(bundle, vec![]);
        state.pending = vec![
            operation("a", "1"),
            operation("a", "2"),
            operation("b", "3"),
            operation("c", "4"),
        ];
        let mut io = Fake {
            replies: [
                Err(PushError::TaskConflict("競合".into())),
                Ok(()),
                Err(PushError::Unavailable("通信断".into())),
            ]
            .into(),
            sent: vec![],
            checkpoints: vec![],
        };
        assert!(futures::executor::block_on(send_pending(&mut state, &mut io)).is_err());
        assert_eq!(io.sent, ["1", "3", "4"]);
        let mut restored = io.checkpoints.pop().unwrap();
        io.replies.push_back(Ok(()));
        assert_eq!(
            futures::executor::block_on(send_pending(&mut restored, &mut io)),
            Ok(1)
        );
        assert_eq!(io.sent, ["1", "3", "4", "4"]);
        assert_eq!(restored.pending, [operation("a", "1"), operation("a", "2")]);
        assert_eq!(restored.conflicts["a"], "競合");
    }
}
