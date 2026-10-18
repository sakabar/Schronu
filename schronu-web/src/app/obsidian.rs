#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
use super::session_links::SessionLinks;
#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
use crate::client::state::{ActiveTab, ClientState};

#[cfg_attr(
    not(any(test, all(feature = "web", target_arch = "wasm32"))),
    allow(dead_code)
)]
pub(super) fn supports_obsidian(platform: Option<&str>, max_touch_points: Option<i32>) -> bool {
    platform.is_some_and(|platform| platform.starts_with("Mac"))
        && max_touch_points.is_some_and(|points| (0..=1).contains(&points))
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub(super) fn browser_supports_obsidian() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let navigator = window.navigator();
    let platform = navigator.platform().ok();
    // Reflect preserves an unavailable/invalid property instead of treating it as zero.
    let touch_points = js_sys::Reflect::get(navigator.as_ref(), &"maxTouchPoints".into())
        .ok()
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite() && value.fract() == 0.0 && (0.0..=1.0).contains(value))
        .map(|value| value as i32);
    supports_obsidian(platform.as_deref(), touch_points)
}

#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
pub(super) fn open_obsidian(
    state: &ClientState,
    links: &SessionLinks,
    supported: bool,
    server_busy: bool,
    task_id: &str,
    open: impl FnOnce(&str) -> Result<(), String>,
) -> Option<Result<(), String>> {
    if !supported
        || server_busy
        || state.active_tab() != ActiveTab::Session
        || !state
            .sessions()
            .iter()
            .any(|session| session.task_id == task_id)
    {
        return None;
    }
    Some(open(links.url_for(task_id)?))
}

#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
pub(super) fn refresh_requests(
    links: &mut SessionLinks,
    state: Option<&ClientState>,
    supported: bool,
) -> Vec<(String, u64)> {
    let task_ids: Vec<_> = state
        .filter(|_| supported)
        .map(|state| {
            state
                .sessions()
                .iter()
                .map(|session| session.task_id.clone())
                .collect()
        })
        .unwrap_or_default();
    let active = supported && state.is_some_and(|state| state.active_tab() == ActiveTab::Session);
    links.reconcile(active, &task_ids)
}
