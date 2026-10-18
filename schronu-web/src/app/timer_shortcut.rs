#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
use crate::client::{state::ClientState, time_model::session_timing};

#[cfg_attr(
    not(any(test, all(feature = "web", target_arch = "wasm32"))),
    allow(dead_code)
)]
pub(super) fn supports_timer_shortcut(platform: Option<&str>) -> bool {
    platform.is_some_and(|platform| {
        platform.starts_with("Mac") || matches!(platform, "iPad" | "iPhone")
    })
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub(super) fn browser_supports_timer_shortcut() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let navigator = window.navigator();
    supports_timer_shortcut(navigator.platform().ok().as_deref())
}

#[cfg_attr(
    not(any(test, all(feature = "web", target_arch = "wasm32"))),
    allow(dead_code)
)]
pub(super) fn timer_enabled(
    session: &crate::client::view_projection::SessionCardViewModel,
    global_blocked: bool,
    mutations_locked: bool,
    server_busy: bool,
) -> bool {
    session.remaining_seconds > 0
        && !global_blocked
        && !mutations_locked
        && !server_busy
        && !session.in_flight
        && !session.server_committed
        && !session.manual_check_blocked
        && session.completion_conflict.is_none()
}

#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
pub(super) fn open_timer_shortcut(
    state: &mut ClientState,
    supported: bool,
    server_busy: bool,
    task_id: &str,
    now_epoch_ms: i64,
    monotonic_now_ms: u64,
    open: impl FnOnce(&str) -> Result<(), String>,
) -> Option<Result<(), String>> {
    state.observe_carry_lock_time(monotonic_now_ms);
    if !supported || server_busy || state.active_tab() != crate::client::state::ActiveTab::Session {
        return None;
    }
    if !state.authorize_carry_lock_mutation(monotonic_now_ms) {
        return None;
    }
    let session = state
        .sessions()
        .iter()
        .find(|session| session.task_id == task_id)?;
    let remaining = session_timing(
        session.started_at_epoch_ms,
        session.estimated_work_seconds_at_start,
        session.actual_work_seconds_at_start,
        now_epoch_ms,
    )
    .remaining_seconds;
    let mut card = crate::client::view_projection::project_session_cards(state, 0)
        .into_iter()
        .find(|card| card.task_id == task_id)?;
    card.remaining_seconds = remaining;
    if !timer_enabled(
        &card,
        state.mutation_globally_blocked(),
        state.carry_lock_locked(),
        server_busy,
    ) {
        return None;
    }
    let url = format!("shortcuts://run-shortcut?name=TimerForSchronu&input=text&text={remaining}");
    Some(open(&url))
}
