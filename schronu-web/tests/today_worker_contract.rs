use schronu_web::{TodayTextQuery, TodayWorkerHandle};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

const STACK_WORKLOAD_CHILD: &str = "SCHRONU_WEB_STACK_WORKLOAD_CHILD";
const STACK_FRAME_BYTES: usize = 4 * 1024;
const STACK_DEPTH: usize = 3 * 1024;

#[test]
fn worker_constructs_and_runs_the_query_on_its_dedicated_thread() {
    let caller_thread = thread::current().id();
    let construction_thread = Arc::new(Mutex::new(None));
    let query_thread = Arc::new(Mutex::new(None));
    let recorded_construction_thread = Arc::clone(&construction_thread);
    let recorded_query_thread = Arc::clone(&query_thread);

    let worker = TodayWorkerHandle::spawn(move || {
        *recorded_construction_thread
            .lock()
            .expect("thread record must be writable") = Some(thread::current().id());
        FixedQuery {
            query_thread: recorded_query_thread,
        }
    });

    assert_eq!(
        futures::executor::block_on(worker.request_async()),
        Ok("today text".to_owned())
    );
    assert_ne!(
        *construction_thread
            .lock()
            .expect("construction thread record must be readable"),
        Some(caller_thread)
    );
    assert_ne!(
        *query_thread.lock().expect("thread record must be readable"),
        Some(caller_thread)
    );
}

#[test]
fn worker_handles_today_text_stack_workload() {
    if std::env::var_os(STACK_WORKLOAD_CHILD).is_some() {
        assert!(std::env::var_os("RUST_MIN_STACK").is_none());
        let worker = TodayWorkerHandle::spawn(|| StackWorkloadQuery);

        assert_eq!(
            futures::executor::block_on(worker.request_async()),
            Ok("stack workload completed".to_owned())
        );
        return;
    }

    let output = Command::new(std::env::current_exe().expect("test executable must be available"))
        .arg("--exact")
        .arg("worker_handles_today_text_stack_workload")
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

struct FixedQuery {
    query_thread: Arc<Mutex<Option<thread::ThreadId>>>,
}

impl TodayTextQuery for FixedQuery {
    fn today_text(&mut self) -> Result<String, String> {
        *self
            .query_thread
            .lock()
            .expect("query thread record must be writable") = Some(thread::current().id());
        Ok("today text".to_owned())
    }
}

struct StackWorkloadQuery;

impl TodayTextQuery for StackWorkloadQuery {
    fn today_text(&mut self) -> Result<String, String> {
        let checksum = consume_stack(STACK_DEPTH);
        std::hint::black_box(checksum);
        Ok("stack workload completed".to_owned())
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
