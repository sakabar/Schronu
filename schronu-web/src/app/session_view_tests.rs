#![cfg(feature = "server")]

use std::sync::{Arc, Mutex};

use super::session_view::{SessionAction, SessionActionKind, SessionCardViewModel, SessionView};
use super::view_test_support::{
    dispatch_click, rebuild_with_click_listeners, render_with_click_listeners,
};
use dioxus::dioxus_core::ElementId;
use dioxus::dioxus_core::ScopeId;
use dioxus::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
struct RootProps {
    sessions: Vec<SessionCardViewModel>,
    global_blocked: bool,
    events: Arc<Mutex<Vec<String>>>,
}

fn test_root(props: RootProps) -> Element {
    let auto_events = Arc::clone(&props.events);
    let action_events = Arc::clone(&props.events);
    let cancel_events = Arc::clone(&props.events);
    rsx! {
        SessionView {
            sessions: props.sessions,
            global_blocked: props.global_blocked,
            mutations_locked: false,
            on_auto_session: move |_| auto_events.lock().unwrap().push("auto".to_owned()),
            on_action: move |action: SessionAction| action_events
                .lock()
                .unwrap()
                .push(format!("{}:{:?}", action.task_id, action.kind)),
            on_cancel_authorization: move |_| cancel_events.lock().unwrap().push("relock".to_owned()),
        }
    }
}

#[derive(Clone)]
struct AutoRootProps {
    in_flight: bool,
    events: Arc<Mutex<Vec<String>>>,
}

fn auto_root(props: AutoRootProps) -> Element {
    rsx! {
        SessionView {
            sessions: Vec::new(),
            global_blocked: true,
            mutations_locked: false,
            auto_session_in_flight: props.in_flight,
            on_auto_session: move |_| props.events.lock().unwrap().push("auto".to_owned()),
            on_action: move |_: SessionAction| {},
        }
    }
}

#[test]
fn carry_lockはauto_sessionと四操作をすべて無効化する() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut empty = VirtualDom::new_with_props(
        locked_root,
        RootProps {
            sessions: Vec::new(),
            global_blocked: false,
            events: Arc::clone(&events),
        },
    );
    let empty_ids = rebuild_with_click_listeners(&mut empty);
    let empty_html = dioxus::ssr::render(&empty);
    assert!(empty_html.contains("disabled"), "{empty_html}");
    for id in empty_ids {
        dispatch_click(&empty, id);
    }

    let mut session = VirtualDom::new_with_props(
        locked_root,
        RootProps {
            sessions: vec![card("locked")],
            global_blocked: false,
            events: Arc::clone(&events),
        },
    );
    let ids = rebuild_with_click_listeners(&mut session);
    assert_eq!(dioxus::ssr::render(&session).matches("disabled").count(), 4);
    for id in ids {
        dispatch_click(&session, id);
    }
    assert!(events.lock().unwrap().is_empty());
}

fn locked_root(props: RootProps) -> Element {
    let auto_events = Arc::clone(&props.events);
    let action_events = Arc::clone(&props.events);
    rsx! {
        SessionView {
            sessions: props.sessions,
            global_blocked: props.global_blocked,
            mutations_locked: true,
            on_auto_session: move |_| auto_events.lock().unwrap().push("auto".to_owned()),
            on_action: move |action: SessionAction| action_events.lock().unwrap().push(format!("{}:{:?}", action.task_id, action.kind)),
        }
    }
}

#[derive(Clone)]
struct ReactiveLockRootProps {
    locked: Arc<AtomicBool>,
}

fn reactive_lock_root(props: ReactiveLockRootProps) -> Element {
    rsx! {
        SessionView {
            sessions: vec![card("reactive-lock")],
            global_blocked: false,
            mutations_locked: props.locked.load(Ordering::SeqCst),
            on_auto_session: move |_| {},
            on_action: move |_: SessionAction| {},
        }
    }
}

#[test]
fn falseからtrueへ再描画されたcarry_lock_propは完了確認を閉じる() {
    let locked = Arc::new(AtomicBool::new(false));
    let mut dom = VirtualDom::new_with_props(
        reactive_lock_root,
        ReactiveLockRootProps {
            locked: Arc::clone(&locked),
        },
    );
    let action_ids = rebuild_with_click_listeners(&mut dom)
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    dispatch_click(&dom, action_ids[2]);
    render_with_click_listeners(&mut dom);
    assert!(dioxus::ssr::render(&dom).contains("タスクを完了しますか?"));

    locked.store(true, Ordering::SeqCst);
    dom.mark_dirty(ScopeId::APP);
    render_with_click_listeners(&mut dom);
    render_with_click_listeners(&mut dom);

    let html = dioxus::ssr::render(&dom);
    assert!(!html.contains("タスクを完了しますか?"), "{html}");
    assert_eq!(html.matches("disabled").count(), 4, "{html}");
}

fn render(sessions: Vec<SessionCardViewModel>, global_blocked: bool) -> (String, Vec<String>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, _) = build_dom(sessions, global_blocked, Arc::clone(&events));
    let html = dioxus::ssr::render(&dom);
    let rendered_events = events.lock().unwrap().clone();
    (html, rendered_events)
}

fn build_dom(
    sessions: Vec<SessionCardViewModel>,
    global_blocked: bool,
    events: Arc<Mutex<Vec<String>>>,
) -> (VirtualDom, Vec<ElementId>) {
    let mut dom = VirtualDom::new_with_props(
        test_root,
        RootProps {
            sessions,
            global_blocked,
            events,
        },
    );
    let listener_ids = rebuild_with_click_listeners(&mut dom);
    (dom, listener_ids)
}

fn card(task_id: &str) -> SessionCardViewModel {
    SessionCardViewModel {
        task_id: task_id.to_owned(),
        task_name: "コピーをせん".to_owned(),
        started_at_hh_mm: "11:25".to_owned(),
        completion_hh_mm: Some("11:28".to_owned()),
        progress_percent: Some(133),
        normal_bar_percent: 100,
        overrun_bar_percent: 33,
        remaining_seconds: -3,
        in_flight: false,
        manual_check_blocked: false,
        server_committed: false,
    }
}

#[test]
fn empty_session_view_only_offers_auto_session_without_firing_callbacks() {
    let (html, events) = render(Vec::new(), false);

    assert!(html.contains("自動セッション"));
    assert!(!html.contains("session-card"));
    assert!(events.is_empty());
}

#[test]
fn session_card_renders_time_progress_overrun_and_four_typed_actions() {
    let (html, events) = render(vec![card("task-1")], false);

    for text in [
        "コピーをせん",
        "11:25",
        "11:28",
        "133%",
        "00:03",
        "破棄して解除",
        "記録して解除",
        "計測を破棄して完了",
        "記録して完了",
    ] {
        assert!(html.contains(text), "missing {text}: {html}");
    }
    assert!(!html.contains("自動セッション"));
    assert!(html.contains("session-progress-normal"));
    assert!(html.contains("role=\"progressbar\""), "{html}");
    assert!(html.contains("aria-label=\"コピーをせんの進捗\""), "{html}");
    assert!(html.contains("aria-valuemin=\"0\""), "{html}");
    assert!(html.contains("aria-valuemax=\"100\""), "{html}");
    assert!(html.contains("aria-valuenow=\"100\""), "{html}");
    assert!(html.contains("aria-valuetext=\"133%\""), "{html}");
    assert!(html.contains("width:100%"));
    assert!(html.contains("session-progress-overrun"));
    assert!(html.contains("width:33%"));
    assert!(html.contains("session-remaining is-overrun"));
    for label in [
        "破棄して解除",
        "記録して解除",
        "計測を破棄して完了",
        "記録して完了",
    ] {
        assert!(
            html.contains(&format!("aria-label=\"コピーをせん: {label}\"")),
            "{html}"
        );
    }
    assert!(events.is_empty());
}

#[test]
fn session_cardは開始と完了予定と残り超過を同じ時刻行に表示する() {
    let mut active = card("active");
    active.started_at_hh_mm = "12:43".to_owned();
    active.completion_hh_mm = Some("13:43".to_owned());
    active.remaining_seconds = 58 * 60 + 4;
    let mut overrun = card("overrun");
    overrun.remaining_seconds = -3;

    let (html, _) = render(vec![active, overrun], false);
    let timing_start = html
        .find("class=\"session-timing\"")
        .expect("session card must have one timing container");
    let timing_end = html[timing_start..]
        .find("</div>")
        .map(|offset| timing_start + offset)
        .expect("timing container must close");
    let timing = &html[timing_start..timing_end];

    for text in ["12:43", "→", "13:43", "58:04"] {
        assert!(timing.contains(text), "missing {text} in {timing}");
    }
    for label in [
        "aria-label=\"開始時刻 12:43\"",
        "aria-label=\"完了予定時刻 13:43\"",
        "aria-label=\"残り時間 58:04\"",
    ] {
        assert!(timing.contains(label), "missing {label} in {timing}");
    }
    assert!(timing.matches("<time").count() >= 2, "{timing}");
    assert!(html.contains("aria-label=\"超過時間 00:03\""), "{html}");
    assert!(
        html.contains("class=\"session-remaining is-overrun\""),
        "{html}"
    );
}

#[test]
fn session操作は意味別classと狭幅1列layoutを持つ() {
    let (html, _) = render(vec![card("task-1")], false);
    for class in [
        "session-action-discard",
        "session-action-record",
        "session-action-complete-without-recording",
        "session-action-complete",
    ] {
        assert!(html.contains(&format!("class=\"{class}\"")), "{html}");
    }

    let css = include_str!("../../assets/main.css");
    assert!(!css.contains(".session-actions button:nth-child"));
    for (selector, declaration) in [
        (".session-action-discard", "color: var(--muted);"),
        (".session-action-record", "color: var(--blue-dark);"),
        (
            ".session-action-complete-without-recording",
            "color: var(--red);",
        ),
        (".session-action-complete", "color: var(--green-dark);"),
    ] {
        assert!(css_rule_body(css, selector).contains(declaration));
    }
    let narrow_layout = css
        .split_once("@media (max-width: 34rem)")
        .expect("narrow viewport rule must exist")
        .1;
    assert!(
        css_rule_body(narrow_layout, ".session-actions").contains("grid-template-columns: 1fr;")
    );
}

fn css_rule_body<'a>(css: &'a str, selector: &str) -> &'a str {
    let (_, after_selector) = css
        .split_once(selector)
        .unwrap_or_else(|| panic!("CSS selector must exist: {selector}"));
    let (_, after_opening_brace) = after_selector
        .split_once('{')
        .unwrap_or_else(|| panic!("CSS rule must open a block: {selector}"));
    after_opening_brace
        .split_once('}')
        .unwrap_or_else(|| panic!("CSS rule must close a block: {selector}"))
        .0
}

#[test]
fn unavailable_completion_and_progress_render_placeholders() {
    let mut unavailable = card("task-1");
    unavailable.completion_hh_mm = None;
    unavailable.progress_percent = None;
    unavailable.normal_bar_percent = 0;
    unavailable.overrun_bar_percent = 0;

    let (html, _) = render(vec![unavailable], false);

    assert!(html.contains("--:--"));
    assert!(html.contains("--%"));
    assert!(
        html.contains("aria-valuetext=\"進捗を計算できません\""),
        "{html}"
    );
    assert!(!html.contains("aria-valuenow"), "{html}");
}

#[test]
fn each_block_reason_disables_only_the_affected_session_actions() {
    for blocked_card in [
        SessionCardViewModel {
            in_flight: true,
            ..card("blocked")
        },
        SessionCardViewModel {
            server_committed: true,
            ..card("blocked")
        },
    ] {
        let (html, _) = render(vec![blocked_card, card("active")], false);
        assert_eq!(html.matches("disabled").count(), 4, "{html}");
    }

    let manually_blocked = SessionCardViewModel {
        manual_check_blocked: true,
        ..card("blocked")
    };
    let (html, _) = render(vec![manually_blocked, card("active")], false);
    assert_eq!(html.matches("disabled").count(), 3, "{html}");

    let (globally_blocked, _) = render(vec![card("one"), card("two")], true);
    assert_eq!(globally_blocked.matches("disabled").count(), 6);
}

#[test]
fn action_kind_is_a_closed_typed_contract() {
    assert_ne!(SessionActionKind::Discard, SessionActionKind::Record);
    assert_ne!(SessionActionKind::Record, SessionActionKind::Complete);
    assert_ne!(
        SessionActionKind::Complete,
        SessionActionKind::CompleteWithoutRecording
    );
}

#[test]
fn 計測破棄完了はcard内で確認し確定時だけtyped_callbackを送る() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (mut cancel_dom, action_ids) = build_dom(vec![card("task-a")], false, Arc::clone(&events));
    assert_eq!(action_ids.len(), 4);
    let action_ids: Vec<_> = action_ids.into_iter().rev().collect();

    dispatch_click(&cancel_dom, action_ids[2]);
    assert!(events.lock().unwrap().is_empty());
    let confirm_ids = render_with_click_listeners(&mut cancel_dom);
    let html = dioxus::ssr::render(&cancel_dom);
    assert!(html.contains("このセッションの計測時間は記録されません。タスクを完了しますか?"));
    assert!(html.contains("class=\"session-discard-completion-confirmation\""));
    assert_eq!(confirm_ids.len(), 2);
    dispatch_click(&cancel_dom, confirm_ids[0]);
    render_with_click_listeners(&mut cancel_dom);
    assert!(!dioxus::ssr::render(&cancel_dom).contains("タスクを完了しますか?"));
    assert_eq!(*events.lock().unwrap(), ["relock"]);

    events.lock().unwrap().clear();
    let (mut confirm_dom, action_ids) = build_dom(vec![card("task-b")], false, Arc::clone(&events));
    let action_ids: Vec<_> = action_ids.into_iter().rev().collect();
    dispatch_click(&confirm_dom, action_ids[2]);
    let confirm_ids = render_with_click_listeners(&mut confirm_dom);
    dispatch_click(&confirm_dom, confirm_ids[1]);
    assert_eq!(*events.lock().unwrap(), ["task-b:CompleteWithoutRecording"]);
}

#[test]
fn 計測破棄完了の確認状態は選択したcardだけに保持する() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (mut dom, action_ids) = build_dom(
        vec![card("task-a"), card("task-b")],
        false,
        Arc::clone(&events),
    );
    let action_ids: Vec<_> = action_ids.into_iter().rev().collect();

    dispatch_click(&dom, action_ids[2]);
    render_with_click_listeners(&mut dom);
    let html = dioxus::ssr::render(&dom);

    assert_eq!(html.matches("タスクを完了しますか?").count(), 1, "{html}");
    assert_eq!(
        html.matches("class=\"session-actions\"").count(),
        1,
        "{html}"
    );
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn enabled_buttons_dispatch_the_exact_typed_callback_once() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (auto_dom, auto_ids) = build_dom(Vec::new(), false, Arc::clone(&events));
    assert_eq!(auto_ids.len(), 1);
    dispatch_click(&auto_dom, auto_ids[0]);
    assert_eq!(*events.lock().unwrap(), ["auto"]);

    events.lock().unwrap().clear();
    let (action_dom, action_ids) = build_dom(vec![card("task-a")], false, Arc::clone(&events));
    assert_eq!(action_ids.len(), 4);
    for (element_id, expected) in action_ids.into_iter().rev().zip([
        Some("task-a:Discard"),
        Some("task-a:Record"),
        None,
        Some("task-a:Complete"),
    ]) {
        dispatch_click(&action_dom, element_id);
        if let Some(expected) = expected {
            assert_eq!(events.lock().unwrap().last().unwrap(), expected);
        }
    }
    assert_eq!(
        *events.lock().unwrap(),
        ["task-a:Discard", "task-a:Record", "task-a:Complete"]
    );

    events.lock().unwrap().clear();
    let (other_dom, other_ids) = build_dom(vec![card("task-b")], false, Arc::clone(&events));
    dispatch_click(&other_dom, *other_ids.last().unwrap());
    assert_eq!(*events.lock().unwrap(), ["task-b:Discard"]);
}

#[test]
fn safety_blocks_keep_read_and_local_discard_available() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (auto_dom, auto_ids) = build_dom(Vec::new(), true, Arc::clone(&events));
    for element_id in auto_ids {
        dispatch_click(&auto_dom, element_id);
    }
    assert_eq!(*events.lock().unwrap(), ["auto"]);

    events.lock().unwrap().clear();
    let manually_blocked = SessionCardViewModel {
        manual_check_blocked: true,
        ..card("blocked")
    };
    let (action_dom, action_ids) = build_dom(vec![manually_blocked], true, Arc::clone(&events));
    for element_id in action_ids {
        dispatch_click(&action_dom, element_id);
    }
    assert_eq!(*events.lock().unwrap(), ["blocked:Discard"]);
}

#[test]
fn in_flight_and_committed_sessions_reject_every_action_callback() {
    for blocked in [
        SessionCardViewModel {
            in_flight: true,
            ..card("blocked")
        },
        SessionCardViewModel {
            server_committed: true,
            ..card("blocked")
        },
    ] {
        let events = Arc::new(Mutex::new(Vec::new()));
        let (action_dom, action_ids) = build_dom(vec![blocked], false, Arc::clone(&events));
        for element_id in action_ids {
            dispatch_click(&action_dom, element_id);
        }

        assert!(events.lock().unwrap().is_empty());
    }
}

#[test]
fn auto_session_is_disabled_only_while_its_own_request_is_in_flight() {
    for (in_flight, expected) in [(false, ["auto"].as_slice()), (true, [].as_slice())] {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut dom = VirtualDom::new_with_props(
            auto_root,
            AutoRootProps {
                in_flight,
                events: Arc::clone(&events),
            },
        );
        let listener_ids = rebuild_with_click_listeners(&mut dom);
        assert_eq!(listener_ids.len(), 1);
        let html = dioxus::ssr::render(&dom);
        assert_eq!(html.contains("disabled"), in_flight, "{html}");
        dispatch_click(&dom, listener_ids[0]);
        assert_eq!(*events.lock().unwrap(), expected);
    }
}
