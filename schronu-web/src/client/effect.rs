use crate::{ListTasksRequest, RecordSessionRequest};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientEffect {
    None,
    Bootstrap {
        request_id: u64,
    },
    ListTasks {
        request_id: u64,
        request: ListTasksRequest,
    },
    AutoSession {
        request_id: u64,
    },
    RecordSession {
        request_id: u64,
        request: RecordSessionRequest,
    },
    CompleteSession {
        request_id: u64,
        request: RecordSessionRequest,
    },
}
