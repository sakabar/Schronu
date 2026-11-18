use super::carry_lock_view::{CarryLockBar, CarryLockViewModel};
use super::component::{BufferPanel, DisplayTimezone, TimeDisplayMode, TIME_DISPLAY_MODE};
use super::component_models::{browser_monotonic_now_ms, browser_now_epoch_ms};
use super::load_view::LoadView;
use super::long_press_browser::BrowserLongPressScheduler;
use super::long_press_controller::LongPressSchedulerHandle;
use super::task_name::display_task_name;
use crate::client::carry_lock::{load_carry_lock, CarryLockMode};
use crate::client::offline::{format_at_offset, format_jst, OfflineState};
use crate::client::time_model::{format_hh_mm_ss, session_timing};
use crate::client::view_projection::task_name_matches;
use crate::client::work_sessions::{BrowserLocalStorage, WorkSession};
use crate::{OfflineOperationKind, TaskDisplayKind};
use dioxus::prelude::*;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = crypto, js_name = randomUUID)]
    fn random_operation_id() -> Result<String, JsValue>;
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Sessions,
    Schedule,
    Load,
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
    let timezone = use_hook(|| Rc::new(DisplayTimezone::restore()));
    let zone = timezone.zone_label().to_owned();
    let offset_at = use_callback(move |epoch: i64| timezone.offset_at(epoch));
    let mut tab = use_signal(|| {
        if state.sessions.is_empty() {
            Tab::Schedule
        } else {
            Tab::Sessions
        }
    });
    let mut selected_date = use_signal(|| state.bundle.snapshot.logical_date.clone());
    let mut filter = use_signal(String::new);
    let mut local_error = use_signal(|| None::<&'static str>);
    let mut lock = use_signal(|| load_carry_lock(&BrowserLocalStorage));
    let scheduler = use_hook(|| LongPressSchedulerHandle::new(BrowserLongPressScheduler));
    let mut observed_monotonic = use_signal(browser_monotonic_now_ms);
    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(1_000).await;
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
            if previous == 0 {
                lock.write().relock();
            }
            tab.set(Tab::Sessions);
        } else if previous > 0 && count == 0 && *tab.peek() == Tab::Sessions {
            tab.set(Tab::Schedule);
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
    let day_count = state.bundle.days.len();
    let count_slots = state.bundle.days.values().map(Vec::len).sum::<usize>();
    let current_date = if state.bundle.days.contains_key(&selected_date()) {
        selected_date()
    } else {
        first_date.clone()
    };
    rsx! {
        main { class: "shell offline-shell", id: "schronu-web-ready",
            if tab() == Tab::Sessions {
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
            if tab() == Tab::Sessions {
                CarryLockBar {
                    model: lock_model, scheduler: scheduler.clone(),
                    on_enable: move |_| lock.write().enable(&BrowserLocalStorage),
                    on_arm: move |_| lock.write().arm(browser_monotonic_now_ms()),
                    on_relock: move |_| lock.write().relock(),
                    on_disable: move |_| lock.write().disable(&BrowserLocalStorage),
                }
                Sessions { state: state.clone(), zone: zone.clone(), offset_at, blocked, on_action }
            } else if tab() == Tab::Schedule {
                section { class: "task-list-view",
                    p { class: "offline-period", "準備完了・{first_date} 〜 {last_date} ({day_count}日分・予定{count_slots}行)" }
                    nav { class: "date-pills", aria_label: "保存した予定日",
                        for date in state.bundle.days.keys() {
                            {
                                let date = date.clone();
                                let selected = date == current_date;
                                let label = date.get(5..).unwrap_or(&date).replace('-', "/");
                                rsx! { button { key: "{date}", r#type: "button", aria_label: date.clone(),
                                    class: if selected { "date-pill is-selected" } else { "date-pill" },
                                    aria_pressed: selected, onclick: move |_| selected_date.set(date.clone()), "{label}" } }
                            }
                        }
                    }
                    label { class: "task-name-filter",
                        input { class: "task-name-filter-input", r#type: "search", aria_label: "タスク名で検索",
                            placeholder: "タスク名で検索", value: filter(), oninput: move |event| filter.set(event.value()) }
                    }
                    Schedule { state: state.clone(), date: current_date, filter: filter(), zone: zone.clone(), offset_at, blocked, on_action }
                }
            } else {
                LoadView {
                    rows: state.bundle.load.band_days.clone(),
                    routine_load_report: Some(state.bundle.load.routine_load.clone()),
                    observed_at_epoch_ms: Some(state.bundle.snapshot.observed_at_epoch_ms),
                    loading: false, error: None, offline_last_schedule_date: last_date.clone(),
                    on_refresh: move |_| {},
                    on_select_date: move |date| { selected_date.set(date); tab.set(Tab::Schedule); },
                }
            }
            nav { class: "tabs offline-tabs", aria_label: "表示切替",
                for (kind, label) in [(Tab::Sessions, "セッション"), (Tab::Schedule, "予定"), (Tab::Load, "負荷")] {
                    button { key: "{label}", r#type: "button", class: if tab() == kind { "tab-button is-selected" } else { "tab-button" },
                        aria_pressed: tab() == kind, onclick: move |_| tab.set(kind), "{label}" }
                }
            }
        }
    }
}

#[component]
fn Schedule(
    state: Rc<OfflineState>,
    date: String,
    filter: String,
    zone: String,
    offset_at: Callback<i64, Option<i32>>,
    blocked: bool,
    on_action: EventHandler<Action>,
) -> Element {
    let rows = state
        .bundle
        .days
        .get(&date)
        .into_iter()
        .flatten()
        .filter(|r| task_name_matches(&filter, &r.task.task_name))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return rsx! { p { class: "load-status", if filter.trim().is_empty() { "この日の予定はありません。" } else { "一致するタスクはありません。" } } };
    }
    rsx! {
        div { class: "task-table-scroll",
            table { class: "task-table offline-schedule",
                thead { tr { th { "操作" } th { "予定 ({zone})" } th { "タスク / 実績・見積" } } }
                tbody {
                    for row in rows {
                        {
                            let id = row.task.task_id.clone().unwrap_or_default();
                            let active = state.sessions.iter().any(|s| s.task_id == id);
                            let projected = row.occurrence.is_projected();
                            let complete = !projected && state.completed(&id);
                            let waiting = !projected && (state.conflicts.contains_key(&id) || state.awaiting_refresh(&id));
                            let name = display_task_name(row.display_task_name.as_deref().unwrap_or(&row.task.task_name));
                            let start = format_at_offset(row.schedule_start_epoch_ms, offset_at.call(row.schedule_start_epoch_ms), "%H:%M");
                            let end = format_at_offset(row.schedule_end_epoch_ms, offset_at.call(row.schedule_end_epoch_ms), "%H:%M");
                            let actual = format_hh_mm_ss(i128::from(row.task.actual_work_seconds));
                            let estimate = format_hh_mm_ss(i128::from(row.task.estimated_work_seconds));
                            let kind = match row.task_display_kind {
                                TaskDisplayKind::Fixed => "task-kind-fixed",
                                TaskDisplayKind::Repetitive => "task-kind-repetitive",
                                TaskDisplayKind::NonRepetitive => "task-kind-non-repetitive",
                            };
                            rsx! { tr {
                                td {
                                    if active { "計測中" } else if complete { "完了" } else if waiting { "要確認" } else if projected { "見込み" }
                                    else if state.can_start(row) {
                                        button { class: "session-start", aria_label: "{name}の計測を開始", disabled: blocked, r#type: "button",
                                            onclick: move |_| on_action.call(Action::Start(id.clone())), "＋" }
                                    }
                                }
                                td { "{start}–{end}" }
                                td { span { class: kind, "{name}" } small { "{actual} / {estimate}" } }
                            } }
                        }
                    }
                }
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
    rsx! {
        if TIME_DISPLAY_MODE != TimeDisplayMode::Hidden {
            p { class: "offline-period", if state.prior_day_estimate(epoch) { "前日以前の推定 ({date})" } else { "{date}の推定" } }
            BufferPanel { value: Some(state.display_buffer_seconds(epoch)) }
        }
        if state.sessions.is_empty() {
            p { class: "load-status", "予定から作業するタスクを選択してください。" }
        }
        section { class: "session-list",
            for session in &state.sessions {
                OfflineSessionCard { key: "{session.task_id}", session: session.clone(), now: epoch, zone: zone.clone(), offset_at,
                    split: state.bundle.splittable_task_ids.contains(&session.task_id),
                    waiting: state.conflicts.contains_key(&session.task_id) || state.awaiting_refresh(&session.task_id),
                    blocked, on_action,
                }
            }
        }
    }
}

#[component]
fn OfflineSessionCard(
    session: WorkSession,
    now: i64,
    zone: String,
    offset_at: Callback<i64, Option<i32>>,
    split: bool,
    waiting: bool,
    blocked: bool,
    on_action: EventHandler<Action>,
) -> Element {
    let t = session_timing(
        session.started_at_epoch_ms,
        session.estimated_work_seconds_at_start,
        session.actual_work_seconds_at_start,
        now,
    );
    let elapsed = format_hh_mm_ss(i128::from(t.elapsed_seconds));
    let remaining = format_hh_mm_ss(t.remaining_seconds.abs());
    let name = display_task_name(&session.task_name);
    let started = format_at_offset(
        session.started_at_epoch_ms,
        offset_at.call(session.started_at_epoch_ms),
        "%m/%d %H:%M",
    );
    let finished = |kind| {
        let id = session.task_id.clone();
        move |_| on_action.call(Action::Finish(id.clone(), kind))
    };
    let release = session.task_id.clone();
    let restart = session.task_id.clone();
    rsx! {
        article { class: "session-card",
            h3 { "{name}" }
            p { "開始 {started} {zone}・経過 {elapsed}" }
            p { class: "session-countdown",
                if t.remaining_seconds < 0 { "超過 " } else { "残り " }
                strong { "{remaining}" }
            }
            div { class: "session-actions offline-session-actions",
                button { class: "session-action-record", r#type: "button", disabled: blocked || waiting, onclick: finished(OfflineOperationKind::Record), "記録して解除" }
                button { class: "session-action-work-slice", r#type: "button", disabled: blocked || waiting || !split || t.elapsed_seconds == 0, onclick: finished(OfflineOperationKind::Split), "記録して分割" }
                button { class: "session-action-complete", r#type: "button", disabled: blocked || waiting, onclick: finished(OfflineOperationKind::Complete), "記録して完了" }
                button { class: "session-action-restart", r#type: "button", disabled: blocked || waiting, onclick: move |_| on_action.call(Action::Restart(restart.clone())), "計測を破棄して再開" }
                button { class: "session-action-discard", r#type: "button", disabled: blocked, onclick: move |_| on_action.call(Action::Release(release.clone())), "計測を破棄して解除" }
            }
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
                    let name = display_task_name(state.recorded_task_name(&id).unwrap_or(&id));
                    let choose = id.clone();
                    let discard = id.clone();
                    rsx! { article { key: "{id}",
                        strong { "要確認・{name}" }
                        p { "{reason}" }
                        ul {
                            for op in state.pending.iter().filter(|op| op.task_id == id) {
                                {
                                    let action = match op.kind {
                                        OfflineOperationKind::Record => "記録して解除", OfflineOperationKind::Split => "記録して分割", OfflineOperationKind::Complete => "記録して完了",
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
