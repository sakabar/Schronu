use crate::{
    web_error_codes, CompleteSessionRequest, CompleteSessionResponse, ListTasksRequest,
    RecordSessionRequest, RecordSessionResult, RetryAdvice, ScheduledTaskRow, ServerSnapshot,
    SessionTask, WebError, WebSuccess,
};
use std::sync::mpsc;
use std::thread;
use tokio::sync::oneshot;

const WEB_WORKER_STACK_SIZE_BYTES: usize = 32 * 1024 * 1024;

pub trait WebOperations: 'static {
    fn bootstrap(&mut self) -> Result<ServerSnapshot, WebError>;
    fn list_tasks(
        &mut self,
        request: ListTasksRequest,
    ) -> Result<WebSuccess<Vec<ScheduledTaskRow>>, WebError>;
    fn auto_session(&mut self) -> Result<WebSuccess<Option<SessionTask>>, WebError>;
    fn record_session(
        &mut self,
        request: RecordSessionRequest,
    ) -> Result<WebSuccess<RecordSessionResult>, WebError>;
    fn complete_session(
        &mut self,
        request: CompleteSessionRequest,
    ) -> Result<CompleteSessionResponse, WebError>;
}

#[derive(Clone, Debug)]
pub struct WebWorkerHandle {
    commands: mpsc::Sender<WebWorkerCommand>,
}

enum WebWorkerCommand {
    Bootstrap {
        response: oneshot::Sender<Result<ServerSnapshot, WebError>>,
    },
    ListTasks {
        request: ListTasksRequest,
        response: oneshot::Sender<Result<WebSuccess<Vec<ScheduledTaskRow>>, WebError>>,
    },
    AutoSession {
        response: oneshot::Sender<Result<WebSuccess<Option<SessionTask>>, WebError>>,
    },
    RecordSession {
        request: RecordSessionRequest,
        response: oneshot::Sender<Result<WebSuccess<RecordSessionResult>, WebError>>,
    },
    CompleteSession {
        request: CompleteSessionRequest,
        response: oneshot::Sender<Result<CompleteSessionResponse, WebError>>,
    },
}

impl WebWorkerHandle {
    pub fn spawn<F, O>(factory: F) -> Self
    where
        F: FnOnce() -> O + Send + 'static,
        O: WebOperations,
    {
        let (commands, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("schronu-web-operations".to_owned())
            .stack_size(WEB_WORKER_STACK_SIZE_BYTES)
            .spawn(move || run_worker(factory(), receiver))
            .expect("Web worker thread must start");
        Self { commands }
    }

    pub async fn bootstrap(&self) -> Result<ServerSnapshot, WebError> {
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(WebWorkerCommand::Bootstrap { response })
            .map_err(|_| unavailable_error())?;
        receiver.await.map_err(|_| unavailable_error())?
    }

    pub async fn list_tasks(
        &self,
        request: ListTasksRequest,
    ) -> Result<WebSuccess<Vec<ScheduledTaskRow>>, WebError> {
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(WebWorkerCommand::ListTasks { request, response })
            .map_err(|_| unavailable_error())?;
        receiver.await.map_err(|_| unavailable_error())?
    }

    pub async fn auto_session(&self) -> Result<WebSuccess<Option<SessionTask>>, WebError> {
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(WebWorkerCommand::AutoSession { response })
            .map_err(|_| unavailable_error())?;
        receiver.await.map_err(|_| unavailable_error())?
    }

    pub async fn record_session(
        &self,
        request: RecordSessionRequest,
    ) -> Result<WebSuccess<RecordSessionResult>, WebError> {
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(WebWorkerCommand::RecordSession { request, response })
            .map_err(|_| unavailable_error())?;
        receiver.await.map_err(|_| unavailable_error())?
    }

    pub async fn complete_session(
        &self,
        request: CompleteSessionRequest,
    ) -> Result<CompleteSessionResponse, WebError> {
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(WebWorkerCommand::CompleteSession { request, response })
            .map_err(|_| unavailable_error())?;
        receiver.await.map_err(|_| unavailable_error())?
    }
}

fn run_worker<O: WebOperations>(mut operations: O, receiver: mpsc::Receiver<WebWorkerCommand>) {
    while let Ok(command) = receiver.recv() {
        match command {
            WebWorkerCommand::Bootstrap { response } => {
                let _ = response.send(operations.bootstrap());
            }
            WebWorkerCommand::ListTasks { request, response } => {
                let _ = response.send(operations.list_tasks(request));
            }
            WebWorkerCommand::AutoSession { response } => {
                let _ = response.send(operations.auto_session());
            }
            WebWorkerCommand::RecordSession { request, response } => {
                let _ = response.send(operations.record_session(request));
            }
            WebWorkerCommand::CompleteSession { request, response } => {
                let _ = response.send(operations.complete_session(request));
            }
        }
    }
}

fn unavailable_error() -> WebError {
    WebError {
        code: web_error_codes::WORKER_UNAVAILABLE.to_owned(),
        message: "Web操作を処理できません。時間をおいて再試行してください。".to_owned(),
        retry_advice: RetryAdvice::Retry,
    }
}
