use super::carry_lock_view::{CarryLockBar, CarryLockViewModel};
use super::component::{
    BufferPanel, DisplayTimezone, NavigationTabs, TimeDisplayMode, TIME_DISPLAY_MODE,
};
use super::component_models::{browser_monotonic_now_ms, browser_now_epoch_ms};
use super::load_view::LoadView;
use super::long_press_browser::BrowserLongPressScheduler;
use super::long_press_controller::LongPressSchedulerHandle;
use super::session_view::{OfflineSessionDetails, SessionAction, SessionActionKind, SessionView};
use super::task_name::display_task_name;
use crate::client::carry_lock::{load_carry_lock, CarryLockMode};
use crate::client::local_time::JAPAN_UTC_OFFSET_MINUTES;
use crate::client::offline::{format_at_offset, format_jst, OfflineState};
use crate::client::state::ActiveTab;
use crate::client::time_model::session_timing;
use crate::client::view_projection::project_session_card;
use crate::client::work_sessions::BrowserLocalStorage;
use crate::OfflineOperationKind;
use dioxus::prelude::*;
use std::collections::HashMap;
use std::rc::Rc;

mod schedule;
use schedule::Schedule;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = crypto, js_name = randomUUID)]
    fn random_operation_id() -> Result<String, JsValue>;
}

enum Action {
    Start(String),
    Finish(String, OfflineOperationKind),
    Restart(String),
    Release(String),
    DiscardConflict(String),
}

fn apply(state: &mut OfflineState, action: Action, now: i64) -> Result<(), &'static str> {
    match action {
        Action::Start(id) => state.start(&id, now),
        Action::Finish(id, kind) => {
            let operation_id =
                random_operation_id().map_err(|_| "操作IDを生成できませんでした。")?;
            state.finish(&id, kind, now, operation_id)
        }
        Action::Restart(id) => state.restart(&id, now),
        Action::Release(id) => {
            state.release(&id);
            Ok(())
        }
        Action::DiscardConflict(id) => {
            state.discard_conflict(&id);
            Ok(())
        }
    }
}

#[component]
pub(super) fn OfflineView(
    state: OfflineState,
    busy: bool,
    on_change: EventHandler<OfflineState>,
    on_sync: EventHandler<()>,
) -> Element {
    let timezone = use_signal(DisplayTimezone::restore);
    let zone = timezone.read().zone_label().to_owned();
    let offset_at = use_callback(move |epoch: i64| timezone.read().offset_at(epoch));
    let mut tab = use_signal(|| {
        if state.sessions.is_empty() {
            ActiveTab::List
        } else {
            ActiveTab::Session
        }
    });
    let mut selected_date = use_signal(|| state.bundle.snapshot.logical_date.clone());
    let mut filter = use_signal(String::new);
    let mut today = use_signal(|| OfflineState::current_logical_date(browser_now_epoch_ms()));
    let mut local_error = use_signal(|| None::<&'static str>);
    let mut lock = use_signal(|| load_carry_lock(&BrowserLocalStorage));
    let scheduler = use_hook(|| LongPressSchedulerHandle::new(BrowserLongPressScheduler));
    let mut observed_monotonic = use_signal(browser_monotonic_now_ms);
    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(1_000).await;
            let day = OfflineState::current_logical_date(browser_now_epoch_ms());
            if day != *today.peek() {
                today.set(day);
            }
            if matches!(lock.peek().mode(), CarryLockMode::ArmedUntil(_)) {
                let now = browser_monotonic_now_ms();
                lock.write().observe_monotonic_time(now);
                observed_monotonic.set(now);
            }
        }
    });
    let count = state.sessions.len();
    let mut previous_count = use_signal(|| count);
    use_effect(use_reactive((&count,), move |(count,)| {
        let previous = *previous_count.peek();
        if count > previous {
            filter.set(String::new());
            if previous == 0 {
                lock.write().relock();
            }
            tab.set(ActiveTab::Session);
        } else if previous > 0 && count == 0 && *tab.peek() == ActiveTab::Session {
            tab.set(ActiveTab::List);
        }
        previous_count.set(count);
    }));
    let lock_model = CarryLockViewModel::new(lock.read().mode(), observed_monotonic());
    let lock_warning = lock.read().warning().map(str::to_owned);
    let blocked = busy || lock_model.mutations_locked();
    let authorize = use_callback(move |()| {
        if busy {
            return false;
        }
        let now = browser_monotonic_now_ms();
        let mut guard = lock.write();
        guard.observe_monotonic_time(now);
        guard.authorize_mutation(now)
    });
    let state = Rc::new(state);
    let for_actions = state.clone();
    let on_action = use_callback(move |action: Action| {
        if !authorize.call(()) {
            return;
        }
        let now = browser_now_epoch_ms();
        let mut candidate = (*for_actions).clone();
        match apply(&mut candidate, action, now) {
            Ok(()) => {
                local_error.set(None);
                on_change.call(candidate);
            }
            Err(message) => local_error.set(Some(message)),
        }
    });
    let first_date = state.bundle.snapshot.logical_date.clone();
    let last_date = state
        .bundle
        .days
        .keys()
        .next_back()
        .cloned()
        .unwrap_or_default();
    let saved = format_jst(state.bundle.snapshot.observed_at_epoch_ms, "%m/%d %H:%M");
    let pending = state.pending.len();
    let current_date = if state.bundle.days.contains_key(&selected_date()) {
        selected_date()
    } else {
        first_date.clone()
    };
    rsx! {
        main { class: "shell offline-shell", id: "schronu-web-ready",
            if tab() == ActiveTab::Session {
                header { class: "offline-status",
                    div {
                        strong { "オフライン利用" }
                        span { "保存時点 {saved} JST・未送信{pending}件" }
                    }
                    button { class: "primary-action", r#type: "button", disabled: blocked,
                        onclick: move |_| if authorize.call(()) { on_sync.call(()) },
                        if busy { "処理中…" } else { "同期" }
                    }
                }
            }
            if let Some(error) = local_error() { p { class: "error", role: "alert", "{error}" } }
            if let Some(error) = lock_warning { p { class: "error", role: "alert", "{error}" } }
            if !state.conflicts.is_empty() {
                ConflictList { state: state.clone(), blocked, on_action }
            }
            if tab() == ActiveTab::Session {
                CarryLockBar {
                    model: lock_model, scheduler: scheduler.clone(),
                    on_enable: move |_| lock.write().enable(&BrowserLocalStorage),
                    on_arm: move |_| lock.write().arm(browser_monotonic_now_ms()),
                    on_relock: move |_| lock.write().relock(),
                    on_disable: move |_| lock.write().disable(&BrowserLocalStorage),
                }
                Sessions { state: state.clone(), zone: zone.clone(), offset_at, blocked, on_action }
            } else if tab() == ActiveTab::List {
                Schedule { state: state.clone(), date: current_date, filter, timezone, today: today(), blocked, on_action,
                    on_select: move |date| selected_date.set(date),
                }
            } else {
                LoadView {
                    rows: state.bundle.load.band_days.clone(),
                    routine_load_report: Some(state.bundle.load.routine_load.clone()),
                    observed_at_epoch_ms: Some(state.bundle.snapshot.observed_at_epoch_ms),
                    loading: false, error: None, offline_last_schedule_date: last_date.clone(),
                    on_refresh: move |_| {},
                    on_select_date: move |date| { selected_date.set(date); tab.set(ActiveTab::List); },
                }
            }
            NavigationTabs { active_tab: tab(), create_disabled: true,
                on_switch: move |next| if next != ActiveTab::Create { tab.set(next); }
            }
        }
    }
}

#[component]
fn Sessions(
    state: Rc<OfflineState>,
    zone: String,
    offset_at: Callback<i64, Option<i32>>,
    blocked: bool,
    on_action: EventHandler<Action>,
) -> Element {
    let mut now = use_signal(browser_now_epoch_ms);
    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(1_000).await;
            now.set(browser_now_epoch_ms());
        }
    });
    let epoch = now();
    let date = &state.bundle.snapshot.logical_date;
    let mut details = HashMap::new();
    let cards = state
        .sessions
        .iter()
        .map(|session| {
            let id = &session.task_id;
            let timing = session_timing(
                session.started_at_epoch_ms,
                session.estimated_work_seconds_at_start,
                session.actual_work_seconds_at_start,
                epoch,
            );
            let start_date = format_at_offset(
                session.started_at_epoch_ms,
                offset_at.call(session.started_at_epoch_ms),
                "%m/%d",
            );
            details.insert(
                id.clone(),
                OfflineSessionDetails {
                    start_date_and_zone: format!("{start_date} {zone}"),
                    elapsed_seconds: timing.elapsed_seconds,
                    can_split: state.bundle.splittable_task_ids.contains(id)
                        && timing.elapsed_seconds > 0,
                },
            );
            let mut card = project_session_card(
                session,
                timing,
                epoch,
                false,
                &|ms| offset_at.call(ms),
                &|_| Some(JAPAN_UTC_OFFSET_MINUTES),
            );
            card.manual_check_blocked = state.needs_review(id);
            card
        })
        .collect();
    rsx! {
        if TIME_DISPLAY_MODE != TimeDisplayMode::Hidden {
            p { class: "offline-period", if state.prior_day_estimate(epoch) { "前日以前の推定 ({date})" } else { "{date}の推定" } }
            BufferPanel { value: Some(state.display_buffer_seconds(epoch)) }
        }
        SessionView {
            sessions: cards, offline_sessions: details, global_blocked: blocked, mutations_locked: blocked,
            on_auto_session: move |_| {},
            on_action: move |SessionAction { task_id, kind }| {
                let action = match kind {
                    SessionActionKind::RestartWithoutRecording => Action::Restart(task_id),
                    SessionActionKind::Discard => Action::Release(task_id),
                    SessionActionKind::Record => Action::Finish(task_id, OfflineOperationKind::Record),
                    SessionActionKind::WorkSlice => Action::Finish(task_id, OfflineOperationKind::Split),
                    SessionActionKind::Complete => Action::Finish(task_id, OfflineOperationKind::Complete),
                    SessionActionKind::CompleteWithoutRecording => Action::Finish(task_id, OfflineOperationKind::CompleteWithoutRecording),
                    SessionActionKind::ResumeCompletionConflict | SessionActionKind::ConfirmCompletionConflict => return,
                };
                on_action.call(action);
            },
        }
    }
}

#[component]
fn ConflictList(
    state: Rc<OfflineState>,
    blocked: bool,
    on_action: EventHandler<Action>,
) -> Element {
    let mut confirming = use_signal(|| None::<String>);
    rsx! {
        section { class: "offline-conflicts", aria_label: "同期で要確認となった記録",
            for (id, reason) in &state.conflicts {
                {
                    let id = id.clone();
                    let name = display_task_name(state.review_task_name(&id).unwrap_or(&id));
                    let choose = id.clone();
                    let discard = id.clone();
                    rsx! { article { key: "{id}",
                        strong { "要確認・{name}" }
                        p { "{reason}" }
                        ul {
                            for op in state.pending.iter().filter(|op| op.task_id == id) {
                                {
                                    let action = match op.kind {
                                        OfflineOperationKind::Record => "記録して解除",
                                        OfflineOperationKind::Split => "記録して分割",
                                        OfflineOperationKind::Complete => "記録して完了",
                                        OfflineOperationKind::CompleteWithoutRecording => "計測を破棄して完了",
                                    };
                                    let from = format_jst(op.started_at_epoch_ms, "%m/%d %H:%M:%S");
                                    let until = format_jst(op.ended_at_epoch_ms, "%m/%d %H:%M:%S");
                                    rsx! { li { key: "{op.operation_id}", "{action}・{from} ～ {until} JST" } }
                                }
                            }
                        }
                        if confirming().as_ref() == Some(&id) {
                            p { "このタスクの未送信記録をすべて端末から削除しますか?" }
                            button { r#type: "button", onclick: move |_| confirming.set(None), "キャンセル" }
                            button { r#type: "button", disabled: blocked,
                                onclick: move |_| { confirming.set(None); on_action.call(Action::DiscardConflict(discard.clone())); }, "削除を確定" }
                        } else {
                            button { r#type: "button", disabled: blocked, onclick: move |_| confirming.set(Some(choose.clone())), "手動で確認した記録を削除" }
                        }
                    } }
                }
            }
        }
    }
}
