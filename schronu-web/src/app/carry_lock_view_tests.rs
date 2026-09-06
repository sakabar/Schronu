#![cfg(feature = "server")]

use std::sync::{Arc, Mutex};

use super::carry_lock_view::{CarryLockBar, CarryLockViewModel, LongPressSource, LongPressTracker};
use super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
use crate::client::carry_lock::CarryLockMode;
use dioxus::prelude::*;

#[derive(Clone)]
struct RootProps {
    model: CarryLockViewModel,
    events: Arc<Mutex<Vec<&'static str>>>,
}

fn root(props: RootProps) -> Element {
    let enable_events = Arc::clone(&props.events);
    let arm_events = Arc::clone(&props.events);
    let disable_events = Arc::clone(&props.events);
    rsx! {
        CarryLockBar {
            model: props.model,
            on_enable: move |_| enable_events.lock().unwrap().push("enable"),
            on_arm: move |_| arm_events.lock().unwrap().push("arm"),
            on_disable: move |_| disable_events.lock().unwrap().push("disable"),
        }
    }
}

fn render(model: CarryLockViewModel) -> String {
    let mut dom = VirtualDom::new_with_props(
        root,
        RootProps {
            model,
            events: Arc::new(Mutex::new(Vec::new())),
        },
    );
    dom.rebuild_in_place();
    dioxus::ssr::render(&dom)
}

#[test]
fn lock_barはmodeごとの状態とaccessibility契約を表示する() {
    let normal = render(CarryLockViewModel::new(CarryLockMode::Normal, 0));
    assert!(normal.contains("carry-lock-bar is-normal"), "{normal}");
    assert!(normal.contains("持ち歩きロック"), "{normal}");
    assert!(normal.contains("aria-pressed=\"false\""), "{normal}");

    let locked = render(CarryLockViewModel::new(CarryLockMode::Locked, 0));
    assert!(locked.contains("carry-lock-bar is-locked"), "{locked}");
    assert!(locked.contains("操作ロック中"), "{locked}");
    assert!(locked.contains("1.2秒長押しで1操作許可"), "{locked}");
    assert!(locked.contains("aria-pressed=\"true\""), "{locked}");
    assert!(locked.contains("aria-live=\"polite\""), "{locked}");

    let armed = render(CarryLockViewModel::new(
        CarryLockMode::ArmedUntil(17_000),
        2_001,
    ));
    assert!(armed.contains("carry-lock-bar is-armed"), "{armed}");
    assert!(armed.contains("1操作可能"), "{armed}");
    assert!(armed.contains("残り15秒"), "{armed}");
}

#[test]
fn lockedだけが長押し領域と確認付き永続解除を表示する() {
    let locked = render(CarryLockViewModel::new(CarryLockMode::Locked, 0));
    assert!(locked.contains("class=\"carry-lock-hold"), "{locked}");
    assert!(locked.contains("touch-action: pan-y"), "{locked}");
    assert!(locked.contains("<details"), "{locked}");
    assert!(locked.contains("通常モードへ戻す"), "{locked}");
    assert!(locked.contains("持ち歩きロックを解除しますか?"), "{locked}");

    let normal = render(CarryLockViewModel::new(CarryLockMode::Normal, 0));
    assert!(!normal.contains("carry-lock-hold"), "{normal}");
    assert!(
        !normal.contains("持ち歩きロックを解除しますか?"),
        "{normal}"
    );
}

#[test]
fn normalの1tapと解除確認の確定だけがcallbackを送る() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut normal = VirtualDom::new_with_props(
        root,
        RootProps {
            model: CarryLockViewModel::new(CarryLockMode::Normal, 0),
            events: Arc::clone(&events),
        },
    );
    let ids = rebuild_with_click_listeners(&mut normal);
    assert_eq!(ids.len(), 1);
    dispatch_click(&normal, ids[0]);
    assert_eq!(*events.lock().unwrap(), ["enable"]);

    events.lock().unwrap().clear();
    let mut locked = VirtualDom::new_with_props(
        root,
        RootProps {
            model: CarryLockViewModel::new(CarryLockMode::Locked, 0),
            events: Arc::clone(&events),
        },
    );
    let ids = rebuild_with_click_listeners(&mut locked);
    assert_eq!(ids.len(), 1, "解除確定以外をclickで解除してはならない");
    dispatch_click(&locked, ids[0]);
    assert_eq!(*events.lock().unwrap(), ["disable"]);
}

#[test]
fn pointer長押しは成立時に一度だけ発火し短押しとcancelを拒否する() {
    let mut tracker = LongPressTracker::default();

    let short = tracker.begin(LongPressSource::Pointer);
    tracker.cancel(LongPressSource::Pointer);
    assert!(!tracker.complete(short));

    let cancelled = tracker.begin(LongPressSource::Pointer);
    tracker.cancel(LongPressSource::Pointer);
    assert!(!tracker.complete(cancelled));

    let completed = tracker.begin(LongPressSource::Pointer);
    assert!(tracker.complete(completed));
    assert!(!tracker.complete(completed), "同じtimerは1回だけ成立する");
}

#[test]
fn keyboard長押しはspaceとenterだけを受け付けkeyupでcancelする() {
    let mut tracker = LongPressTracker::default();
    assert!(tracker.begin_keyboard("Escape", false).is_none());

    let space = tracker.begin_keyboard(" ", false).unwrap();
    assert!(tracker.begin_keyboard(" ", true).is_none());
    tracker.cancel(LongPressSource::Keyboard);
    assert!(!tracker.complete(space));

    let enter = tracker.begin_keyboard("Enter", false).unwrap();
    assert!(tracker.complete(enter));
    assert!(!tracker.complete(enter));
}

#[test]
fn lock_bar_cssはstickyとsafe_areaと状態feedbackを持つ() {
    let css = include_str!("../../assets/main.css");
    for fragment in [
        ".carry-lock-bar",
        "position: sticky;",
        "env(safe-area-inset-top)",
        ".carry-lock-bar.is-armed",
        ".carry-lock-hold.is-pressing",
        "touch-action: pan-y;",
    ] {
        assert!(css.contains(fragment), "missing {fragment}");
    }
}
