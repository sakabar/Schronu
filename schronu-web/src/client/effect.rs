use crate::{ListTasksRequest, RecordSessionRequest};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientEffect {
    None,
    Bootstrap,
    ListTasks(ListTasksRequest),
    AutoSession,
    RecordSession(RecordSessionRequest),
    CompleteSession(RecordSessionRequest),
}
