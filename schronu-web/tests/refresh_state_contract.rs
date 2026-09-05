use schronu_web::{RefreshState, RefreshTrigger, REFRESH_INTERVAL};
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

fn state_with_text(text: &str) -> RefreshState {
    let mut state = RefreshState::new();
    assert!(state.begin_refresh(RefreshTrigger::Initial));
    state.complete_refresh(Ok(text.to_owned()));
    state
}
