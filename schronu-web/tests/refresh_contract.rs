use schronu_web::{
    RefreshState, RefreshTrigger, TodayTextQuery, TodayWorkerHandle, REFRESH_INTERVAL,
};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[test]
fn initial_refresh_stores_the_first_successful_text() {
    let mut state = RefreshState::new();

    assert!(state.begin_refresh(RefreshTrigger::Initial));
    assert!(state.is_refreshing());
    state.complete_refresh(Ok("initial text".to_owned()));

    assert_eq!(state.text(), Some("initial text"));
    assert_eq!(state.error(), None);
    assert!(!state.is_refreshing());
}

#[test]
fn manual_refresh_replaces_the_previous_text() {
    let mut state = state_with_text("before");

    assert!(state.begin_refresh(RefreshTrigger::Manual));
    state.complete_refresh(Ok("after".to_owned()));

    assert_eq!(state.text(), Some("after"));
}

#[test]
fn interval_refresh_uses_a_sixty_second_tick() {
    assert_eq!(REFRESH_INTERVAL, Duration::from_secs(60));

    let mut state = state_with_text("before interval");
    assert!(state.begin_refresh(RefreshTrigger::Interval));
    state.complete_refresh(Ok("after interval".to_owned()));

    assert_eq!(state.text(), Some("after interval"));
}

#[test]
fn refresh_requests_do_not_overlap() {
    let mut state = RefreshState::new();

    assert!(state.begin_refresh(RefreshTrigger::Initial));
    assert!(!state.begin_refresh(RefreshTrigger::Manual));
    assert!(!state.begin_refresh(RefreshTrigger::Interval));
}

#[test]
fn failed_refresh_retains_the_previous_text_and_can_be_retried() {
    let mut state = state_with_text("last success");

    assert!(state.begin_refresh(RefreshTrigger::Interval));
    state.complete_refresh(Err("storage is locked".to_owned()));

    assert_eq!(state.text(), Some("last success"));
    assert_eq!(state.error(), Some("storage is locked"));
    assert!(!state.is_refreshing());

    assert!(state.begin_refresh(RefreshTrigger::Manual));
    state.complete_refresh(Ok("recovered".to_owned()));
    assert_eq!(state.text(), Some("recovered"));
    assert_eq!(state.error(), None);
}

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

    assert_eq!(worker.request(), Ok("today text".to_owned()));
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

fn state_with_text(text: &str) -> RefreshState {
    let mut state = RefreshState::new();
    assert!(state.begin_refresh(RefreshTrigger::Initial));
    state.complete_refresh(Ok(text.to_owned()));
    state
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
