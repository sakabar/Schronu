use crate::{ListTasksRequest, RecordSessionRequest};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientEffect {
    None,
    Bootstrap,
    ListTasks(ListTasksRequest),
    AutoSession,
    RecordSession {
        request_id: u64,
        request: RecordSessionRequest,
    },
    CompleteSession {
        request_id: u64,
        request: RecordSessionRequest,
    },
}
