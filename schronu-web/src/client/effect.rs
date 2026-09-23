use crate::{
    CompleteSessionRequest, DeferTaskRequest, ListAllTasksRequest, ListTasksRequest,
    RecordSessionRequest,
};

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
    ListAllTasks {
        request_id: u64,
        request: ListAllTasksRequest,
    },
    AutoSession {
        request_id: u64,
    },
    DeferTask {
        request_id: u64,
        request: DeferTaskRequest,
    },
    RecordSession {
        request_id: u64,
        request: RecordSessionRequest,
    },
    CompleteSession {
        request_id: u64,
        request: CompleteSessionRequest,
    },
}
