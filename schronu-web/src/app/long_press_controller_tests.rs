use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::long_press_controller::{
    LongPressController, LongPressScheduler, LongPressSchedulerHandle, LongPressSource,
    LongPressTracker, LONG_PRESS_MILLIS,
};
use super::view_test_support::{
    dispatch_platform_event, rebuild_with_event_listeners, render_with_click_listeners,
};
use crate::client::carry_lock::CarryLockMode;
use dioxus::html::SerializedPointerData;
use dioxus::prelude::*;

type ScheduledCallback = Box<dyn FnOnce()>;

#[derive(Clone, Default)]
struct FakeScheduler {
    scheduled: Rc<RefCell<Vec<(u32, ScheduledCallback)>>>,
}

impl FakeScheduler {
    fn run_next(&self) -> bool {
        let task = self.scheduled.borrow_mut().pop();
        if let Some((_, callback)) = task {
            callback();
            true
        } else {
            false
        }
    }
}

impl LongPressScheduler for FakeScheduler {
    fn schedule(&self, delay_millis: u32, callback: ScheduledCallback) {
        self.scheduled.borrow_mut().push((delay_millis, callback));
    }
}

fn controller(
    scheduler: FakeScheduler,
    holding: Rc<RefCell<Vec<bool>>>,
    arm_count: Rc<Cell<u8>>,
) -> LongPressController<FakeScheduler> {
    LongPressController::new(
        scheduler,
        move |active| holding.borrow_mut().push(active),
        move || arm_count.set(arm_count.get().saturating_add(1)),
    )
}

#[test]
fn componentが使うcontrollerは1200ms後に一度だけarmする() {
    let scheduler = FakeScheduler::default();
    let holding = Rc::new(RefCell::new(Vec::new()));
    let arm_count = Rc::new(Cell::new(0));
    let controller = controller(
        scheduler.clone(),
        Rc::clone(&holding),
        Rc::clone(&arm_count),
    );

    controller.start(LongPressSource::Pointer);
    assert_eq!(holding.borrow().as_slice(), [true]);
    assert_eq!(scheduler.scheduled.borrow()[0].0, LONG_PRESS_MILLIS);
    assert_eq!(arm_count.get(), 0);

    assert!(scheduler.run_next());
    assert_eq!(holding.borrow().as_slice(), [true, false]);
    assert_eq!(arm_count.get(), 1);
    assert!(!scheduler.run_next());
    assert_eq!(arm_count.get(), 1);
}

#[test]
fn pointer_cancelとscroll_cancelはstale_timerのarmを拒否する() {
    for cancel in [
        LongPressController::cancel_pointer,
        LongPressController::cancel_for_scroll,
    ] {
        let scheduler = FakeScheduler::default();
        let holding = Rc::new(RefCell::new(Vec::new()));
        let arm_count = Rc::new(Cell::new(0));
        let controller = controller(
            scheduler.clone(),
            Rc::clone(&holding),
            Rc::clone(&arm_count),
        );

        controller.start(LongPressSource::Pointer);
        cancel(&controller);
        assert_eq!(holding.borrow().as_slice(), [true, false]);
        assert!(scheduler.run_next());
        assert_eq!(arm_count.get(), 0);
    }
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
fn window_scrollはactive長押しをcancelしてstale_timerを拒否する() {
    let mut tracker = LongPressTracker::default();
    let pointer_timer = tracker.begin(LongPressSource::Pointer);

    assert!(tracker.cancel_all());

    assert!(!tracker.complete(pointer_timer));
    assert!(!tracker.cancel_all(), "非active時のscrollは状態更新しない");
    let keyboard_timer = tracker.begin_keyboard("Enter", false).unwrap();
    assert!(tracker.cancel_all());
    assert!(!tracker.complete(keyboard_timer));
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

#[derive(Clone)]
struct ComponentRootProps {
    scheduler: FakeScheduler,
    arm_count: Rc<Cell<u8>>,
}

fn component_root(props: ComponentRootProps) -> Element {
    let arm_count = Rc::clone(&props.arm_count);
    rsx! {
        super::carry_lock_view::CarryLockBar {
            model: super::carry_lock_view::CarryLockViewModel::new(CarryLockMode::Locked, 0),
            scheduler: LongPressSchedulerHandle::new(props.scheduler),
            on_enable: move |_| {},
            on_arm: move |_| arm_count.set(arm_count.get().saturating_add(1)),
            on_disable: move |_| {},
        }
    }
}

fn primary_mouse_pointer() -> SerializedPointerData {
    serde_json::from_value(serde_json::json!({
        "alt_key": false,
        "button": 0,
        "buttons": 1,
        "client_x": 0.0,
        "client_y": 0.0,
        "ctrl_key": false,
        "meta_key": false,
        "offset_x": 0.0,
        "offset_y": 0.0,
        "page_x": 0.0,
        "page_y": 0.0,
        "screen_x": 0.0,
        "screen_y": 0.0,
        "shift_key": false,
        "pointer_id": 1,
        "width": 1.0,
        "height": 1.0,
        "pressure": 0.5,
        "tangential_pressure": 0.0,
        "tilt_x": 0,
        "tilt_y": 0,
        "twist": 0,
        "pointer_type": "mouse",
        "is_primary": true
    }))
    .unwrap()
}

#[test]
fn pointerdown_handlerは同じcontrollerとschedulerを経てarmする() {
    let scheduler = FakeScheduler::default();
    let arm_count = Rc::new(Cell::new(0));
    let mut dom = VirtualDom::new_with_props(
        component_root,
        ComponentRootProps {
            scheduler: scheduler.clone(),
            arm_count: Rc::clone(&arm_count),
        },
    );
    let pointerdown_ids = rebuild_with_event_listeners(&mut dom, "pointerdown");
    assert_eq!(pointerdown_ids.len(), 1);

    dispatch_platform_event(
        &dom,
        "pointerdown",
        pointerdown_ids[0],
        Box::new(primary_mouse_pointer()),
    );
    render_with_click_listeners(&mut dom);
    assert!(dioxus::ssr::render(&dom).contains("carry-lock-hold is-pressing"));
    assert_eq!(scheduler.scheduled.borrow()[0].0, LONG_PRESS_MILLIS);
    assert_eq!(arm_count.get(), 0);

    assert!(scheduler.run_next());
    render_with_click_listeners(&mut dom);
    assert_eq!(arm_count.get(), 1);
    assert!(!dioxus::ssr::render(&dom).contains("carry-lock-hold is-pressing"));

    dispatch_platform_event(
        &dom,
        "pointerdown",
        pointerdown_ids[0],
        Box::new(primary_mouse_pointer()),
    );
    dispatch_platform_event(
        &dom,
        "pointerup",
        pointerdown_ids[0],
        Box::new(primary_mouse_pointer()),
    );
    assert!(scheduler.run_next());
    render_with_click_listeners(&mut dom);
    assert_eq!(arm_count.get(), 1, "pointerup後のstale timerはarmしない");
}
