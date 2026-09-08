#![cfg(feature = "server")]

use std::sync::{Arc, Mutex};

use super::carry_lock_view::{accepts_long_press_pointer, CarryLockBar, CarryLockViewModel};
use super::long_press_controller::{LongPressScheduler, LongPressSchedulerHandle};
use super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
use crate::client::carry_lock::CarryLockMode;
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;

#[derive(Clone)]
struct RootProps {
    model: CarryLockViewModel,
    events: Arc<Mutex<Vec<&'static str>>>,
    scheduler: LongPressSchedulerHandle,
}

#[derive(Clone, Copy)]
struct DeferredScheduler;

impl LongPressScheduler for DeferredScheduler {
    fn schedule(&self, _delay_millis: u32, _callback: Box<dyn FnOnce()>) {}
}

fn deferred_scheduler() -> LongPressSchedulerHandle {
    LongPressSchedulerHandle::new(DeferredScheduler)
}

fn root(props: RootProps) -> Element {
    let enable_events = Arc::clone(&props.events);
    let arm_events = Arc::clone(&props.events);
    let relock_events = Arc::clone(&props.events);
    let disable_events = Arc::clone(&props.events);
    rsx! {
        CarryLockBar {
            model: props.model,
            scheduler: props.scheduler,
            on_enable: move |_| enable_events.lock().unwrap().push("enable"),
            on_arm: move |_| arm_events.lock().unwrap().push("arm"),
            on_relock: move |_| relock_events.lock().unwrap().push("relock"),
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
            scheduler: deferred_scheduler(),
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
    assert!(locked.contains("1.2秒長押しで15秒間操作可能"), "{locked}");

    let armed = render(CarryLockViewModel::new(
        CarryLockMode::ArmedUntil(17_000),
        2_001,
    ));
    assert!(armed.contains("carry-lock-bar is-armed"), "{armed}");
    assert!(armed.contains("操作可能"), "{armed}");
    assert!(armed.contains("残り15秒"), "{armed}");
}

#[test]
fn 長押し操作はtoggleではない通常buttonのaria意味を持つ() {
    let html = render(CarryLockViewModel::new(CarryLockMode::Locked, 0));
    let class_position = html.find("class=\"carry-lock-hold\"").unwrap();
    let button_start = html[..class_position].rfind("<button").unwrap();
    let button_end = class_position + html[class_position..].find('>').unwrap();
    let opening_tag = &html[button_start..button_end];

    assert!(!opening_tag.contains("aria-pressed"), "{opening_tag}");
    assert!(
        opening_tag.contains("aria-label=\"1.2秒長押しで15秒間操作可能\""),
        "{opening_tag}"
    );
}

#[test]
fn live_regionはmode遷移だけを通知しarmed残秒を含まない() {
    for (mode, expected) in [
        (CarryLockMode::Normal, "通常モード"),
        (CarryLockMode::Locked, "操作ロック中"),
        (CarryLockMode::ArmedUntil(15_000), "操作可能"),
    ] {
        let html = render(CarryLockViewModel::new(mode, 0));
        assert_eq!(html.matches("aria-live=\"polite\"").count(), 1, "{html}");
        let live_start = html.find("class=\"carry-lock-live-status\"").unwrap();
        let live_end = live_start + html[live_start..].find("</span>").unwrap();
        let live_region = &html[live_start..live_end];
        assert!(live_region.contains(expected), "{live_region}");
        assert!(!live_region.contains("残り"), "{live_region}");
    }
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
fn lockedの状態文言は長押しbutton内に集約する() {
    let html = render(CarryLockViewModel::new(CarryLockMode::Locked, 0));
    let class_position = html.find("class=\"carry-lock-hold").unwrap();
    let button_start = html[..class_position].rfind("<button").unwrap();
    let button_end = class_position + html[class_position..].find("</button>").unwrap();
    let button = &html[button_start..button_end];

    assert!(button.contains("操作ロック中"), "{button}");
    assert!(button.contains("1.2秒長押しで15秒間操作可能"), "{button}");
    assert!(!html.contains("class=\"carry-lock-status\""), "{html}");

    let details_position = html.find("class=\"carry-lock-details\"").unwrap();
    assert!(button_end < details_position, "{html}");
}

#[test]
fn normalの1tapと解除確認の確定だけがcallbackを送る() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut normal = VirtualDom::new_with_props(
        root,
        RootProps {
            model: CarryLockViewModel::new(CarryLockMode::Normal, 0),
            events: Arc::clone(&events),
            scheduler: deferred_scheduler(),
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
            scheduler: deferred_scheduler(),
        },
    );
    let ids = rebuild_with_click_listeners(&mut locked);
    assert_eq!(ids.len(), 1, "解除確定以外をclickで解除してはならない");
    dispatch_click(&locked, ids[0]);
    assert_eq!(*events.lock().unwrap(), ["disable"]);
}

#[test]
fn armedは独立した今すぐlock_buttonでcallbackを1回送る() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut armed = VirtualDom::new_with_props(
        root,
        RootProps {
            model: CarryLockViewModel::new(CarryLockMode::ArmedUntil(17_000), 2_000),
            events: Arc::clone(&events),
            scheduler: deferred_scheduler(),
        },
    );

    let ids = rebuild_with_click_listeners(&mut armed);

    assert_eq!(ids.len(), 2, "永続解除と即時再lockのみclick可能にする");
    let html = dioxus::ssr::render(&armed);
    assert!(html.contains("今すぐロック"), "{html}");
    dispatch_click(&armed, ids[0]);
    assert_eq!(*events.lock().unwrap(), ["relock"]);
}

#[test]
fn pointer長押しは主pointerの許可されたcontactだけで開始する() {
    assert!(accepts_long_press_pointer(
        "mouse",
        true,
        Some(MouseButton::Primary)
    ));
    for button in [
        MouseButton::Secondary,
        MouseButton::Auxiliary,
        MouseButton::Fourth,
        MouseButton::Fifth,
        MouseButton::Unknown,
    ] {
        assert!(!accepts_long_press_pointer("mouse", true, Some(button)));
    }
    assert!(!accepts_long_press_pointer("mouse", true, None));
    assert!(accepts_long_press_pointer("touch", true, None));
    assert!(accepts_long_press_pointer("pen", true, None));
    assert!(!accepts_long_press_pointer(
        "touch",
        false,
        Some(MouseButton::Primary)
    ));
    assert!(!accepts_long_press_pointer(
        "unknown",
        true,
        Some(MouseButton::Primary)
    ));
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
        ".carry-lock-relock",
        "touch-action: pan-y;",
    ] {
        assert!(css.contains(fragment), "missing {fragment}");
    }
}

#[test]
fn 即時再lock_buttonは44pxの操作高と横並びを維持する() {
    let css = include_str!("../../assets/main.css");
    let armed_rule = css_block(css, ".carry-lock-bar.is-armed");
    assert!(armed_rule.contains("flex-wrap: nowrap;"), "{armed_rule}");

    let relock_rule = css_block(css, ".carry-lock-relock");
    assert!(
        relock_rule.contains("min-height: max(2.75rem, 44px);"),
        "{relock_rule}"
    );
}

#[test]
fn lock_bar_cssは長押しbuttonの操作高を保ちmobileでも縦積みにしない() {
    let css = include_str!("../../assets/main.css");
    let hold_rule = css_block(css, ".carry-lock-hold");
    assert!(hold_rule.contains("display: flex;"), "{hold_rule}");
    assert!(
        !hold_rule.contains("flex-direction: column;"),
        "長押しbuttonは横並びを維持する: {hold_rule}"
    );
    assert!(
        hold_rule.contains("min-height: max(2.75rem, 44px);"),
        "{hold_rule}"
    );

    let locked_rule = css_block(css, ".carry-lock-bar.is-locked");
    assert!(locked_rule.contains("flex-wrap: wrap;"), "{locked_rule}");

    let details_rule = css_block(css, ".carry-lock-bar.is-locked .carry-lock-details");
    assert!(details_rule.contains("flex-basis: 100%;"), "{details_rule}");

    let mobile_rule = css_block(css, "@media (max-width: 34rem)");
    assert!(
        !mobile_rule.contains(".carry-lock-bar"),
        "mobile規則でcarry-lock-barを縦積みにしてはならない: {mobile_rule}"
    );
}

fn css_block<'a>(css: &'a str, selector: &str) -> &'a str {
    let header = format!("{selector} {{");
    let header_start = css
        .find(&header)
        .unwrap_or_else(|| panic!("missing CSS block `{selector}`"));
    let opening_brace = header_start + header.len() - 1;
    let mut depth = 0_u32;

    for (offset, character) in css[opening_brace..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &css[opening_brace + 1..opening_brace + offset];
                }
            }
            _ => {}
        }
    }

    panic!("unclosed CSS block `{selector}`");
}
