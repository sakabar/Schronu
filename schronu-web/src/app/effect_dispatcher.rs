use crate::client::state::{ClientEffect, ClientState, ServerFailure};
use crate::client::work_sessions::KeyValueStorage;
use crate::{
    CompleteSessionResponse, ListTasksRequest, RecordSessionRequest, RecordSessionResult,
    ScheduledTaskRow, ServerSnapshot, SessionTask, WebError, WebSuccess,
};
use dioxus::prelude::ServerFnError;

pub(crate) trait WebGateway {
    async fn bootstrap(&self) -> Result<Result<ServerSnapshot, WebError>, ServerFnError>;

    async fn list_tasks(
        &self,
        request: ListTasksRequest,
    ) -> Result<Result<WebSuccess<Vec<ScheduledTaskRow>>, WebError>, ServerFnError>;

    async fn auto_session(
        &self,
    ) -> Result<Result<WebSuccess<Option<SessionTask>>, WebError>, ServerFnError>;

    async fn record_session(
        &self,
        request: RecordSessionRequest,
    ) -> Result<Result<WebSuccess<RecordSessionResult>, WebError>, ServerFnError>;

    async fn complete_session(
        &self,
        request: RecordSessionRequest,
    ) -> Result<Result<CompleteSessionResponse, WebError>, ServerFnError>;
}

pub(crate) struct ServerFunctionGateway;

impl WebGateway for ServerFunctionGateway {
    async fn bootstrap(&self) -> Result<Result<ServerSnapshot, WebError>, ServerFnError> {
        super::bootstrap().await
    }

    async fn list_tasks(
        &self,
        request: ListTasksRequest,
    ) -> Result<Result<WebSuccess<Vec<ScheduledTaskRow>>, WebError>, ServerFnError> {
        super::list_tasks(request).await
    }

    async fn auto_session(
        &self,
    ) -> Result<Result<WebSuccess<Option<SessionTask>>, WebError>, ServerFnError> {
        super::auto_session().await
    }

    async fn record_session(
        &self,
        request: RecordSessionRequest,
    ) -> Result<Result<WebSuccess<RecordSessionResult>, WebError>, ServerFnError> {
        super::record_session(request).await
    }

    async fn complete_session(
        &self,
        request: RecordSessionRequest,
    ) -> Result<Result<CompleteSessionResponse, WebError>, ServerFnError> {
        super::complete_session(request).await
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ClientResponse {
    Bootstrap {
        request_id: u64,
        result: Result<ServerSnapshot, ServerFailure>,
    },
    ListTasks {
        request_id: u64,
        requested_date: String,
        result: Result<WebSuccess<Vec<ScheduledTaskRow>>, ServerFailure>,
    },
    AutoSession {
        request_id: u64,
        result: Result<WebSuccess<Option<SessionTask>>, ServerFailure>,
    },
    RecordSession {
        request_id: u64,
        result: Result<WebSuccess<RecordSessionResult>, ServerFailure>,
    },
    CompleteSession {
        request_id: u64,
        result: Result<CompleteSessionResponse, ServerFailure>,
    },
}

pub(crate) async fn execute_effect<G: WebGateway>(
    gateway: &G,
    effect: ClientEffect,
) -> Option<ClientResponse> {
    match effect {
        ClientEffect::None => None,
        ClientEffect::Bootstrap { request_id } => Some(ClientResponse::Bootstrap {
            request_id,
            result: normalize_endpoint_result(gateway.bootstrap().await),
        }),
        ClientEffect::ListTasks {
            request_id,
            request,
        } => {
            let requested_date = request.logical_date.clone();
            Some(ClientResponse::ListTasks {
                request_id,
                requested_date,
                result: normalize_endpoint_result(gateway.list_tasks(request).await),
            })
        }
        ClientEffect::AutoSession { request_id } => Some(ClientResponse::AutoSession {
            request_id,
            result: normalize_endpoint_result(gateway.auto_session().await),
        }),
        ClientEffect::RecordSession {
            request_id,
            request,
        } => Some(ClientResponse::RecordSession {
            request_id,
            result: normalize_endpoint_result(gateway.record_session(request).await),
        }),
        ClientEffect::CompleteSession {
            request_id,
            request,
        } => Some(ClientResponse::CompleteSession {
            request_id,
            result: normalize_endpoint_result(gateway.complete_session(request).await),
        }),
    }
}

pub(crate) fn apply_response<S: KeyValueStorage>(
    state: &mut ClientState,
    storage: &S,
    response: ClientResponse,
) -> ClientEffect {
    match response {
        ClientResponse::Bootstrap { request_id, result } => {
            state.apply_bootstrap_result(request_id, result)
        }
        ClientResponse::ListTasks {
            request_id,
            requested_date,
            result,
        } => state.apply_list_result(request_id, &requested_date, result),
        ClientResponse::AutoSession { request_id, result } => {
            state.apply_auto_session_result(storage, request_id, result)
        }
        ClientResponse::RecordSession { request_id, result } => {
            state.apply_record_result(storage, request_id, result)
        }
        ClientResponse::CompleteSession { request_id, result } => {
            state.apply_complete_result(storage, request_id, result)
        }
    }
}

pub(crate) fn normalize_endpoint_result<T>(
    result: Result<Result<T, WebError>, ServerFnError>,
) -> Result<T, ServerFailure> {
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(ServerFailure::Operation(error)),
        Err(_) => Err(ServerFailure::Transport("transport".to_owned())),
    }
}
