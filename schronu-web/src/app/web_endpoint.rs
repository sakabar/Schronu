#[cfg(test)]
mod tests {
    use super::{
        dispatch_auto_session, dispatch_bootstrap, dispatch_complete_session, dispatch_list_tasks,
        dispatch_record_session, WebOperationResult,
    };
    use crate::{
        CompleteSessionResponse, ListTasksRequest, RecordSessionRequest, RecordSessionResult,
        ScheduledTaskRow, ServerSnapshot, SessionTask, WebError, WebOperations, WebSuccess,
        WebWorkerHandle,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn 五endpoint境界はworkerへ各1回dispatchしてoperation_errorを内側に保つ() {
        let calls = Arc::new(AtomicUsize::new(0));
        let worker_calls = Arc::clone(&calls);
        let worker = WebWorkerHandle::spawn(move || CountingOperations {
            calls: worker_calls,
        });
        let request = RecordSessionRequest {
            task_id: "task".to_owned(),
            started_at_epoch_ms: 1,
            expected_actual_work_seconds: 2,
        };

        futures::executor::block_on(async {
            let _: WebOperationResult<ServerSnapshot> = dispatch_bootstrap(worker.clone()).await;
            let _: WebOperationResult<WebSuccess<Vec<ScheduledTaskRow>>> = dispatch_list_tasks(
                worker.clone(),
                ListTasksRequest {
                    logical_date: "2026-09-05".to_owned(),
                },
            )
            .await;
            let _: WebOperationResult<WebSuccess<Option<SessionTask>>> =
                dispatch_auto_session(worker.clone()).await;
            let _: WebOperationResult<WebSuccess<RecordSessionResult>> =
                dispatch_record_session(worker.clone(), request.clone()).await;
            let completed: WebOperationResult<CompleteSessionResponse> =
                dispatch_complete_session(worker, request).await;
            assert_eq!(completed, Ok(snapshot()));
        });

        assert_eq!(calls.load(Ordering::SeqCst), 5);
    }

    struct CountingOperations {
        calls: Arc<AtomicUsize>,
    }

    impl CountingOperations {
        fn count(&self) {
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl WebOperations for CountingOperations {
        fn bootstrap(&mut self) -> Result<ServerSnapshot, WebError> {
            self.count();
            Ok(snapshot())
        }

        fn list_tasks(
            &mut self,
            _request: ListTasksRequest,
        ) -> Result<WebSuccess<Vec<ScheduledTaskRow>>, WebError> {
            self.count();
            Ok(WebSuccess {
                snapshot: snapshot(),
                data: Vec::new(),
            })
        }

        fn auto_session(&mut self) -> Result<WebSuccess<Option<SessionTask>>, WebError> {
            self.count();
            Ok(WebSuccess {
                snapshot: snapshot(),
                data: None,
            })
        }

        fn record_session(
            &mut self,
            _request: RecordSessionRequest,
        ) -> Result<WebSuccess<RecordSessionResult>, WebError> {
            self.count();
            Ok(WebSuccess {
                snapshot: snapshot(),
                data: RecordSessionResult {
                    actual_work_seconds: 2,
                },
            })
        }

        fn complete_session(
            &mut self,
            _request: RecordSessionRequest,
        ) -> Result<CompleteSessionResponse, WebError> {
            self.count();
            Ok(snapshot())
        }
    }

    fn snapshot() -> ServerSnapshot {
        ServerSnapshot {
            observed_at_epoch_ms: 1,
            logical_date: "2026-09-05".to_owned(),
            buffer_seconds: 2,
        }
    }
}
