use schronu_web::{
    web_error_codes, CompleteSessionRequest, CompleteSessionResponse, ListTasksRequest,
    RecordSessionRequest, RecordSessionResult, RetryAdvice, ScheduledTaskRow, ServerSnapshot,
    SessionTask, WebError, WebOperations, WebSuccess, WebWorkerHandle,
};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

const STACK_WORKLOAD_CHILD: &str = "SCHRONU_WEB_STACK_WORKLOAD_CHILD";
const STACK_FRAME_BYTES: usize = 4 * 1024;
const STACK_DEPTH: usize = 3 * 1024;

#[test]
fn workerは5操作を送信順に専用threadで実行してpayloadを保持する() {
    let caller_thread = thread::current().id();
    let events = Arc::new(Mutex::new(Vec::new()));
    let factory_events = Arc::clone(&events);
    let worker = WebWorkerHandle::spawn(move || {
        factory_events
            .lock()
            .expect("event log must be writable")
            .push(Event::Factory(thread::current().id()));
        RecordingOperations {
            events: factory_events,
        }
    });
    let request = RecordSessionRequest {
        task_id: "task-1".to_owned(),
        started_at_epoch_ms: 123,
        expected_actual_work_seconds: 456,
    };
    let complete_request = CompleteSessionRequest {
        task_id: request.task_id.clone(),
        started_at_epoch_ms: request.started_at_epoch_ms,
        expected_actual_work_seconds: request.expected_actual_work_seconds,
        record_elapsed_seconds: true,
    };

    futures::executor::block_on(async {
        assert_eq!(worker.bootstrap().await, Ok(snapshot(1)));
        assert_eq!(
            worker
                .list_tasks(ListTasksRequest {
                    logical_date: "2026-09-05".to_owned(),
                })
                .await,
            Ok(WebSuccess {
                snapshot: snapshot(2),
                data: Vec::new(),
            })
        );
        assert_eq!(
            worker.auto_session().await,
            Ok(WebSuccess {
                snapshot: snapshot(3),
                data: Some(task()),
            })
        );
        assert_eq!(
            worker.record_session(request.clone()).await,
            Ok(WebSuccess {
                snapshot: snapshot(4),
                data: RecordSessionResult {
                    actual_work_seconds: 456,
                },
            })
        );
        assert_eq!(
            worker.complete_session(complete_request).await,
            Ok(snapshot(5))
        );
    });

    let events = events.lock().expect("event log must be readable");
    assert!(matches!(events[0], Event::Factory(id) if id != caller_thread));
    assert_eq!(
        &events[1..],
        &[
            Event::Bootstrap,
            Event::List("2026-09-05".to_owned()),
            Event::Auto,
            Event::Record(123, 456),
            Event::Complete(123, 456),
        ]
    );
}

#[test]
fn worker停止時だけworker_unavailableをretry可能として返す() {
    let worker = WebWorkerHandle::spawn(|| PanickingOperations);

    let error = futures::executor::block_on(worker.bootstrap())
        .expect_err("stopped worker must return an error");

    assert_eq!(error.code, web_error_codes::WORKER_UNAVAILABLE);
    assert_eq!(error.retry_advice, RetryAdvice::Retry);
}

#[test]
fn workerは32mib_stackを要する操作を完了する() {
    if std::env::var_os(STACK_WORKLOAD_CHILD).is_some() {
        assert!(std::env::var_os("RUST_MIN_STACK").is_none());
        let worker = WebWorkerHandle::spawn(|| StackWorkloadOperations);

        assert_eq!(
            futures::executor::block_on(worker.bootstrap()),
            Ok(snapshot(1))
        );
        return;
    }

    let output = Command::new(std::env::current_exe().expect("test executable must be available"))
        .arg("--exact")
        .arg("workerは32mib_stackを要する操作を完了する")
        .arg("--nocapture")
        .env(STACK_WORKLOAD_CHILD, "1")
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("stack workload child process must start");

    assert!(
        output.status.success(),
        "stack workload child failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[derive(Debug, PartialEq)]
enum Event {
    Factory(thread::ThreadId),
    Bootstrap,
    List(String),
    Auto,
    Record(i64, i64),
    Complete(i64, i64),
}

struct RecordingOperations {
    events: Arc<Mutex<Vec<Event>>>,
}

struct PanickingOperations;

struct StackWorkloadOperations;

impl WebOperations for StackWorkloadOperations {
    fn bootstrap(&mut self) -> Result<ServerSnapshot, WebError> {
        let checksum = consume_stack(STACK_DEPTH);
        std::hint::black_box(checksum);
        Ok(snapshot(1))
    }

    fn list_tasks(
        &mut self,
        _request: ListTasksRequest,
    ) -> Result<WebSuccess<Vec<ScheduledTaskRow>>, WebError> {
        unreachable!()
    }

    fn auto_session(&mut self) -> Result<WebSuccess<Option<SessionTask>>, WebError> {
        unreachable!()
    }

    fn record_session(
        &mut self,
        _request: RecordSessionRequest,
    ) -> Result<WebSuccess<RecordSessionResult>, WebError> {
        unreachable!()
    }

    fn complete_session(
        &mut self,
        _request: CompleteSessionRequest,
    ) -> Result<CompleteSessionResponse, WebError> {
        unreachable!()
    }
}

impl WebOperations for PanickingOperations {
    fn bootstrap(&mut self) -> Result<ServerSnapshot, WebError> {
        panic!("injected worker stop")
    }

    fn list_tasks(
        &mut self,
        _request: ListTasksRequest,
    ) -> Result<WebSuccess<Vec<ScheduledTaskRow>>, WebError> {
        unreachable!()
    }

    fn auto_session(&mut self) -> Result<WebSuccess<Option<SessionTask>>, WebError> {
        unreachable!()
    }

    fn record_session(
        &mut self,
        _request: RecordSessionRequest,
    ) -> Result<WebSuccess<RecordSessionResult>, WebError> {
        unreachable!()
    }

    fn complete_session(
        &mut self,
        _request: CompleteSessionRequest,
    ) -> Result<CompleteSessionResponse, WebError> {
        unreachable!()
    }
}

impl WebOperations for RecordingOperations {
    fn bootstrap(&mut self) -> Result<ServerSnapshot, WebError> {
        self.events.lock().unwrap().push(Event::Bootstrap);
        Ok(snapshot(1))
    }

    fn list_tasks(
        &mut self,
        request: ListTasksRequest,
    ) -> Result<WebSuccess<Vec<ScheduledTaskRow>>, WebError> {
        self.events
            .lock()
            .unwrap()
            .push(Event::List(request.logical_date));
        Ok(WebSuccess {
            snapshot: snapshot(2),
            data: Vec::new(),
        })
    }

    fn auto_session(&mut self) -> Result<WebSuccess<Option<SessionTask>>, WebError> {
        self.events.lock().unwrap().push(Event::Auto);
        Ok(WebSuccess {
            snapshot: snapshot(3),
            data: Some(task()),
        })
    }

    fn record_session(
        &mut self,
        request: RecordSessionRequest,
    ) -> Result<WebSuccess<RecordSessionResult>, WebError> {
        self.events.lock().unwrap().push(Event::Record(
            request.started_at_epoch_ms,
            request.expected_actual_work_seconds,
        ));
        Ok(WebSuccess {
            snapshot: snapshot(4),
            data: RecordSessionResult {
                actual_work_seconds: 456,
            },
        })
    }

    fn complete_session(
        &mut self,
        request: CompleteSessionRequest,
    ) -> Result<CompleteSessionResponse, WebError> {
        self.events.lock().unwrap().push(Event::Complete(
            request.started_at_epoch_ms,
            request.expected_actual_work_seconds,
        ));
        Ok(snapshot(5))
    }
}

fn snapshot(observed_at_epoch_ms: i64) -> ServerSnapshot {
    ServerSnapshot {
        observed_at_epoch_ms,
        logical_date: "2026-09-05".to_owned(),
        buffer_seconds: 60,
    }
}

fn task() -> SessionTask {
    SessionTask {
        task_id: "task-1".to_owned(),
        task_name: "task".to_owned(),
        estimated_work_seconds: 900,
        actual_work_seconds: 456,
    }
}

#[inline(never)]
fn consume_stack(depth: usize) -> usize {
    let frame = [depth as u8; STACK_FRAME_BYTES];
    std::hint::black_box(&frame);
    let nested = if depth == 0 {
        0
    } else {
        consume_stack(depth - 1)
    };
    std::hint::black_box(&frame);
    nested.wrapping_add(frame[depth % STACK_FRAME_BYTES] as usize)
}
