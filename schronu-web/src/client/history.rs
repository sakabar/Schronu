use std::collections::VecDeque;

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
pub enum Locality {
    Local,
    Server,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Success,
    Failure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationHistoryEntry {
    pub occurred_at_epoch_ms: i64,
    pub operation: Operation,
    pub task_id: Option<String>,
    pub locality: Locality,
    pub outcome: Outcome,
    pub summary: String,
}

pub fn push_history(history: &mut VecDeque<OperationHistoryEntry>, entry: OperationHistoryEntry) {
    if history.len() == MAX_HISTORY_ENTRIES {
        history.pop_front();
    }
    history.push_back(entry);
}
