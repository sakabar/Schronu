use crate::{
    CompleteSessionRequest, CreateTaskRequest, DeferTaskRequest, ListAllTasksRequest,
    ListCompletedTasksRequest, ListTasksRequest, RecordSessionRequest,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Bootstrap,
    ListTasks,
    ListCompletedTasks,
    ListAllTasks,
    LoadBand,
    AutoSession,
    CreateTask,
    AddSession,
    DeferTask,
    DiscardSession,
    RecordSession,
    CompleteWorkSlice,
    CompleteSession,
    CompleteSessionWithoutRecording,
    ConfirmRepositoryCheck,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerActionInvocation {
    Bootstrap,
    ListTasks(ListTasksRequest),
    ListCompletedTasks(ListCompletedTasksRequest),
    ListAllTasks(ListAllTasksRequest),
    LoadBand,
    AutoSession,
    CreateTask(CreateTaskRequest),
    DeferTask(DeferTaskRequest),
    RecordSession(RecordSessionRequest),
    CompleteWorkSlice(RecordSessionRequest),
    CompleteSession(CompleteSessionRequest),
}

impl ServerActionInvocation {
    pub fn operation(&self) -> Operation {
        match self {
            Self::Bootstrap => Operation::Bootstrap,
            Self::ListTasks(_) => Operation::ListTasks,
            Self::ListCompletedTasks(_) => Operation::ListCompletedTasks,
            Self::ListAllTasks(_) => Operation::ListAllTasks,
            Self::LoadBand => Operation::LoadBand,
            Self::AutoSession => Operation::AutoSession,
            Self::CreateTask(_) => Operation::CreateTask,
            Self::DeferTask(_) => Operation::DeferTask,
            Self::RecordSession(_) => Operation::RecordSession,
            Self::CompleteWorkSlice(_) => Operation::CompleteWorkSlice,
            Self::CompleteSession(request) if request.record_elapsed_seconds => {
                Operation::CompleteSession
            }
            Self::CompleteSession(_) => Operation::CompleteSessionWithoutRecording,
        }
    }

    pub fn task_id(&self) -> Option<&str> {
        match self {
            Self::RecordSession(request) => Some(&request.task_id),
            Self::CompleteWorkSlice(request) => Some(&request.task_id),
            Self::CompleteSession(request) => Some(&request.task_id),
            Self::DeferTask(request) => Some(&request.task_id),
            Self::Bootstrap
            | Self::ListTasks(_)
            | Self::ListCompletedTasks(_)
            | Self::ListAllTasks(_)
            | Self::LoadBand
            | Self::AutoSession
            | Self::CreateTask(_) => None,
        }
    }
}
