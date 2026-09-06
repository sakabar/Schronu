use std::{collections::VecDeque, fmt};

use crate::{CompleteSessionRequest, ListTasksRequest, RecordSessionRequest};

const MAX_HISTORY_ENTRIES: usize = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Bootstrap,
    ListTasks,
    AutoSession,
    AddSession,
    DiscardSession,
    RecordSession,
    CompleteSession,
    CompleteSessionWithoutRecording,
    ConfirmRepositoryCheck,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Success,
    Failure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerActionInvocation {
    Bootstrap,
    ListTasks(ListTasksRequest),
    AutoSession,
    RecordSession(RecordSessionRequest),
    CompleteSession(CompleteSessionRequest),
}

impl ServerActionInvocation {
    pub fn operation(&self) -> Operation {
        match self {
            Self::Bootstrap => Operation::Bootstrap,
            Self::ListTasks(_) => Operation::ListTasks,
            Self::AutoSession => Operation::AutoSession,
            Self::RecordSession(_) => Operation::RecordSession,
            Self::CompleteSession(request) if request.record_elapsed_seconds => {
                Operation::CompleteSession
            }
            Self::CompleteSession(_) => Operation::CompleteSessionWithoutRecording,
        }
    }

    pub fn task_id(&self) -> Option<&str> {
        match self {
            Self::RecordSession(request) => Some(&request.task_id),
            Self::CompleteSession(request) => Some(&request.task_id),
            Self::Bootstrap | Self::ListTasks(_) | Self::AutoSession => None,
        }
    }
}

impl fmt::Display for ServerActionInvocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bootstrap => formatter.write_str("bootstrap()"),
            Self::ListTasks(request) => {
                write!(formatter, "list_tasks(logical_date: {:?})", request.logical_date)
            }
            Self::AutoSession => formatter.write_str("auto_session()"),
            Self::RecordSession(request) => write!(
                formatter,
                "record_session(task_id: {:?}, started_at_epoch_ms: {}, expected_actual_work_seconds: {})",
                request.task_id, request.started_at_epoch_ms, request.expected_actual_work_seconds
            ),
            Self::CompleteSession(request) => write!(
                formatter,
                "complete_session(task_id: {:?}, started_at_epoch_ms: {}, expected_actual_work_seconds: {}, record_elapsed_seconds: {})",
                request.task_id,
                request.started_at_epoch_ms,
                request.expected_actual_work_seconds,
                request.record_elapsed_seconds
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationHistoryEntry {
    pub occurred_at_epoch_ms: i64,
    pub invocation: ServerActionInvocation,
    pub outcome: Outcome,
    pub summary: String,
}

pub fn push_history(history: &mut VecDeque<OperationHistoryEntry>, entry: OperationHistoryEntry) {
    if history.len() == MAX_HISTORY_ENTRIES {
        history.pop_front();
    }
    history.push_back(entry);
}
