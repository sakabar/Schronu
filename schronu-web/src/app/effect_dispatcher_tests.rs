#![cfg(feature = "server")]

use super::effect_dispatcher::{
    execute_effect, normalize_endpoint_result, ClientResponse, WebGateway,
};
use crate::client::state::{ClientEffect, ServerFailure};
use crate::{
    CompleteSessionRequest, CompleteSessionResponse, ListTasksRequest, RecordSessionRequest,
    RecordSessionResult, RetryAdvice, ScheduledTaskRow, ServerSnapshot, SessionTask, WebError,
    WebSuccess,
};
use dioxus::prelude::ServerFnError;
use std::cell::RefCell;

#[test]
fn 五effectはrequest_idとpayloadを保持して各endpointを1回だけ呼ぶ() {
    let gateway = FakeGateway::default();
    let request = RecordSessionRequest {
        task_id: "task".to_owned(),
        started_at_epoch_ms: 1,
        expected_actual_work_seconds: 2,
    };
    let complete_request = CompleteSessionRequest {
        task_id: request.task_id.clone(),
        started_at_epoch_ms: request.started_at_epoch_ms,
        expected_actual_work_seconds: request.expected_actual_work_seconds,
        record_elapsed_seconds: true,
    };
    let responses = futures::executor::block_on(async {
        vec![
            execute_effect(&gateway, ClientEffect::Bootstrap { request_id: 10 }).await,
            execute_effect(
                &gateway,
                ClientEffect::ListTasks {
                    request_id: 11,
                    request: ListTasksRequest {
                        logical_date: "2026-09-05".to_owned(),
                    },
                },
            )
            .await,
            execute_effect(&gateway, ClientEffect::AutoSession { request_id: 12 }).await,
            execute_effect(
                &gateway,
                ClientEffect::RecordSession {
                    request_id: 13,
                    request: request.clone(),
                },
            )
            .await,
            execute_effect(
                &gateway,
                ClientEffect::CompleteSession {
                    request_id: 14,
                    request: complete_request,
                },
            )
            .await,
        ]
    });

    assert!(matches!(
        responses[0],
        Some(ClientResponse::Bootstrap { request_id: 10, .. })
    ));
    assert!(matches!(
        &responses[1],
        Some(ClientResponse::ListTasks { request_id: 11, requested_date, .. })
            if requested_date == "2026-09-05"
    ));
    assert!(matches!(
        responses[2],
        Some(ClientResponse::AutoSession { request_id: 12, .. })
    ));
    assert!(matches!(
        responses[3],
        Some(ClientResponse::RecordSession { request_id: 13, .. })
    ));
    assert!(matches!(
        responses[4],
        Some(ClientResponse::CompleteSession { request_id: 14, .. })
    ));
    assert_eq!(
        gateway.calls.borrow().as_slice(),
        [
            "bootstrap",
            "list:2026-09-05",
            "auto",
            "record:task",
            "complete:task"
        ]
    );
    assert_eq!(
        futures::executor::block_on(execute_effect(&gateway, ClientEffect::None)),
        None
    );
    assert_eq!(gateway.calls.borrow().len(), 5);
}

#[test]
fn endpointのouter失敗はsafe_transportへ変換しinner_errorは保持する() {
    let operation_error = WebError {
        code: "sentinel".to_owned(),
        message: "safe".to_owned(),
        retry_advice: RetryAdvice::ManualCheck,
    };
    assert_eq!(
        normalize_endpoint_result::<ServerSnapshot>(Ok(Err(operation_error.clone()))),
        Err(ServerFailure::Operation(operation_error))
    );
    assert_eq!(
        normalize_endpoint_result::<ServerSnapshot>(Err(ServerFnError::new(
            "secret transport detail"
        ))),
        Err(ServerFailure::Transport("transport".to_owned()))
    );
}

#[derive(Default)]
struct FakeGateway {
    calls: RefCell<Vec<String>>,
}

impl WebGateway for FakeGateway {
    async fn bootstrap(&self) -> Result<Result<ServerSnapshot, WebError>, ServerFnError> {
        self.calls.borrow_mut().push("bootstrap".to_owned());
        Ok(Ok(snapshot()))
    }

    async fn list_tasks(
        &self,
        request: ListTasksRequest,
    ) -> Result<Result<WebSuccess<Vec<ScheduledTaskRow>>, WebError>, ServerFnError> {
        self.calls
            .borrow_mut()
            .push(format!("list:{}", request.logical_date));
        Ok(Ok(WebSuccess {
            snapshot: snapshot(),
            data: Vec::new(),
        }))
    }

    async fn auto_session(
        &self,
    ) -> Result<Result<WebSuccess<Option<SessionTask>>, WebError>, ServerFnError> {
        self.calls.borrow_mut().push("auto".to_owned());
        Ok(Ok(WebSuccess {
            snapshot: snapshot(),
            data: None,
        }))
    }

    async fn record_session(
        &self,
        request: RecordSessionRequest,
    ) -> Result<Result<WebSuccess<RecordSessionResult>, WebError>, ServerFnError> {
        self.calls
            .borrow_mut()
            .push(format!("record:{}", request.task_id));
        Ok(Ok(WebSuccess {
            snapshot: snapshot(),
            data: RecordSessionResult {
                actual_work_seconds: 2,
            },
        }))
    }

    async fn complete_session(
        &self,
        request: CompleteSessionRequest,
    ) -> Result<Result<CompleteSessionResponse, WebError>, ServerFnError> {
        self.calls
            .borrow_mut()
            .push(format!("complete:{}", request.task_id));
        Ok(Ok(snapshot()))
    }
}

fn snapshot() -> ServerSnapshot {
    ServerSnapshot {
        observed_at_epoch_ms: 1,
        logical_date: "2026-09-05".to_owned(),
        buffer_seconds: 2,
    }
}
