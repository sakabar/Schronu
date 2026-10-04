use crate::{
    CompleteSessionRequest, DeferTaskRequest, ListAllTasksRequest, ListCompletedTasksRequest,
    ListTasksRequest, RecordSessionRequest,
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
    ListCompletedTasks {
        request_id: u64,
        request: ListCompletedTasksRequest,
    },
    ListAllTasks {
        request_id: u64,
        request: ListAllTasksRequest,
    },
    LoadBand {
        request_id: u64,
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
